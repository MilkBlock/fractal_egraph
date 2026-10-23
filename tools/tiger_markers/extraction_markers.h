#pragma once
// Extraction-specific markers are arithmetic recipes, NOT RipSat closure proofs.
// Preserve Tiger's unsigned integer arithmetic, candidate order and chosen DAG.
#include "greedy.h"
#include <algorithm>
#include <chrono>
#include <cerrno>
#include <csignal>
#include <cstdlib>
#include <cstring>
#include <fstream>
#include <map>
#include <set>
#include <pthread.h>
#include <tuple>
#include <stdexcept>
#include <sys/wait.h>
#include <unistd.h>

namespace extraction_markers {
using Word = unsigned long long;
static_assert(sizeof(Word) == 8, "Tiger costs must be uint64");
constexpr Word magic = 0x5449474552434f53ull;

struct Recipe {
    Word kind = 0, base = 0; // 0: sum, 1: If, 2: DoWhile
    std::vector<Word> ports;
    bool operator<(const Recipe &o) const {
        return std::tie(kind, base, ports) < std::tie(o.kind, o.base, o.ports);
    }
    Word eval(const std::vector<Cost> &x) const {
        if (kind == 2) return x[ports[0]] * 500ull;
        Word value = base;
        auto count = ports.size() - (kind == 1 ? 2 : 0);
        for (size_t i = 0; i < count; ++i) value += x[ports[i]];
        if (kind == 1) {
            auto a = x[ports[count]], b = x[ports[count + 1]];
            value += std::max(a, b) + (std::min(a, b) >> 2);
        }
        return value;
    }
};

struct Program {
    std::vector<Recipe> recipes;
    std::vector<std::pair<EClassId, ENodeId>> owners;
    std::vector<size_t> binding;
    // One subscription per distinct cost port/recipe; repeated ports still
    // occur in the recipe and therefore retain their multiplicity in the sum.
    std::vector<std::vector<size_t>> subscribers;
    size_t port_reads = 0;

    explicit Program(const EGraph &g) : subscribers(g.neclasses()) {
        std::map<Recipe, size_t> intern;
        for (EClassId c = 0; c < (EClassId)g.neclasses(); ++c) {
            if (!g.eclasses[c].isEffectful) continue;
            for (ENodeId n = 0; n < (ENodeId)g.eclasses[c].nenodes(); ++n) {
                const auto &node = g.eclasses[c].enodes[n];
                Recipe r;
                auto op = node.get_op();
                if (op == "DoWhile") {
                    if (node.ch.size() != 2) throw std::runtime_error("invalid DoWhile arity");
                    r.kind = 2; r.ports = {Word(node.ch[1])};
                } else {
                    r.base = get_enode_cost(node);
                    size_t count = node.ch.size();
                    if (op == "If") {
                        if (count != 4) throw std::runtime_error("invalid If arity");
                        r.kind = 1; count = 2;
                    }
                    for (size_t i = 0; i < count; ++i) {
                        auto ch = node.ch[i];
                        if (ch < 0 || size_t(ch) >= g.neclasses()) throw std::runtime_error("invalid child");
                        if (!g.eclasses[ch].isEffectful) r.ports.push_back(ch);
                    }
                    if (r.kind == 1) {
                        r.ports.push_back(node.ch[2]); r.ports.push_back(node.ch[3]);
                    }
                }
                for (auto ch : r.ports) if (ch >= g.neclasses()) throw std::runtime_error("invalid cost port");
                auto entry = intern.emplace(r, recipes.size());
                if (entry.second) {
                    recipes.push_back(r); port_reads += r.ports.size();
                    for (auto ch : std::set<Word>(r.ports.begin(), r.ports.end()))
                        subscribers[ch].push_back(entry.first->second);
                }
                owners.emplace_back(c, n); binding.push_back(entry.first->second);
            }
        }
    }

    std::vector<Cost> evaluate(const std::vector<Cost> &costs) const {
        if (costs.size() != subscribers.size()) throw std::runtime_error("cost shape changed");
        std::vector<Cost> result; result.reserve(recipes.size());
        for (auto &r : recipes) result.push_back(r.eval(costs));
        return result;
    }

    // Cost-only updates on the SAME frozen graph. Union, added alternatives or
    // effect classifications require recompilation, not reuse of this plan.
    size_t update(const std::vector<Cost> &old, const std::vector<Cost> &next,
                  std::vector<Cost> &values) const {
        if (old.size() != subscribers.size() || next.size() != old.size() || values.size() != recipes.size())
            throw std::runtime_error("cost update shape changed");
        std::set<size_t> dirty;
        for (size_t i = 0; i < next.size(); ++i) if (next[i] != old[i])
            dirty.insert(subscribers[i].begin(), subscribers[i].end());
        for (auto i : dirty) values[i] = recipes[i].eval(next);
        return dirty.size();
    }

    std::vector<Word> packet(const std::vector<Cost> &costs) const {
        if (costs.size() != subscribers.size()) throw std::runtime_error("cost shape changed");
        std::vector<Word> refs, headers;
        for (const auto &r : recipes) {
            headers.insert(headers.end(), {r.kind, r.base, Word(refs.size()), Word(r.ports.size())});
            refs.insert(refs.end(), r.ports.begin(), r.ports.end());
        }
        std::vector<Word> out{magic, Word(costs.size()), Word(recipes.size()), Word(refs.size())};
        out.insert(out.end(), costs.begin(), costs.end());
        out.insert(out.end(), headers.begin(), headers.end());
        out.insert(out.end(), refs.begin(), refs.end());
        return out;
    }

    std::vector<std::vector<Cost>> scatter(const EGraph &g, const std::vector<Cost> &values) const {
        if (values.size() != recipes.size()) throw std::runtime_error("marker answer shape changed");
        std::vector<std::vector<Cost>> result(g.neclasses());
        for (size_t c = 0; c < g.neclasses(); ++c) if (g.eclasses[c].isEffectful)
            result[c].resize(g.eclasses[c].nenodes());
        for (size_t i = 0; i < owners.size(); ++i)
            result[owners[i].first][owners[i].second] = values[binding[i]];
        return result;
    }
};

inline std::vector<Cost> device(const std::vector<Word> &input, size_t count) {
    const char *worker = std::getenv("TIGER_EXTRACTION_WORKER");
    if (!worker || !*worker) throw std::runtime_error("GPU extraction requires TIGER_EXTRACTION_WORKER");
    unsigned endian = 1;
    if (*reinterpret_cast<unsigned char *>(&endian) != 1) throw std::runtime_error("little endian required");
    int request[2], response[2];
    if (pipe(request)) throw std::runtime_error("request pipe failed");
    if (pipe(response)) { close(request[0]); close(request[1]); throw std::runtime_error("response pipe failed"); }
    pid_t pid = fork();
    if (pid == 0) {
        dup2(request[0], STDIN_FILENO); dup2(response[1], STDOUT_FILENO);
        close(request[0]); close(request[1]); close(response[0]); close(response[1]);
        execl(worker, worker, (char *)nullptr); _exit(127);
    }
    close(request[0]); close(response[1]);
    if (pid < 0) { close(request[1]); close(response[0]); throw std::runtime_error("worker fork failed"); }
    // Block SIGPIPE locally so a failed worker yields a checked error.
    sigset_t blocked, previous; sigemptyset(&blocked); sigaddset(&blocked, SIGPIPE);
    pthread_sigmask(SIG_BLOCK, &blocked, &previous);
    bool ok = true; size_t written = 0, bytes = input.size() * sizeof(Word);
    while (written < bytes) {
        auto n = write(request[1], reinterpret_cast<const char *>(input.data()) + written, bytes - written);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { ok = false; break; } written += n;
    }
    close(request[1]);
    // Consume only the signal generated by this write before restoring mask.
    if (!ok && !sigismember(&previous, SIGPIPE)) {
        sigset_t pending; sigpending(&pending);
        if (sigismember(&pending, SIGPIPE)) { int ignored; sigwait(&blocked, &ignored); }
    }
    pthread_sigmask(SIG_SETMASK, &previous, nullptr);
    std::vector<Word> output(count + 2); size_t received = 0; bytes = output.size() * sizeof(Word);
    while (received < bytes) {
        auto n = read(response[0], reinterpret_cast<char *>(output.data()) + received, bytes - received);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) { ok = false; break; } received += n;
    }
    char extra; auto trailing = read(response[0], &extra, 1); close(response[0]);
    int status; while (waitpid(pid, &status, 0) < 0) if (errno != EINTR) throw std::runtime_error("waitpid failed");
    if (!ok || trailing != 0 || !WIFEXITED(status) || WEXITSTATUS(status) != 0 || output[0] != magic || output[1] != count)
        throw std::runtime_error("invalid GPU extraction response");
    return std::vector<Cost>(output.begin() + 2, output.end());
}

inline bool enabled() {
    const char *mode = std::getenv("TIGER_EXTRACTION_MARKERS");
    if (!mode || !std::strcmp(mode,"off") || !std::strcmp(mode,"bag")) return false;
    if (std::strcmp(mode,"cpu") && std::strcmp(mode,"gpu")) throw std::runtime_error("unknown extraction marker mode");
    return true;
}

inline bool bags_enabled() {
    const char *mode = std::getenv("TIGER_EXTRACTION_MARKERS");
    return mode && std::strcmp(mode,"off") != 0;
}

template<class NativeCost>
std::vector<std::vector<Cost>> run(const EGraph &g, const std::vector<Cost> &costs, NativeCost native) {
    using Clock = std::chrono::steady_clock;
    auto start = Clock::now(); Program p(g); auto compiled = Clock::now();
    std::string mode = std::getenv("TIGER_EXTRACTION_MARKERS");
    std::vector<Word> packet; std::vector<Cost> values;
    if (mode == "cpu") values = p.evaluate(costs);
    else if (mode == "gpu") { packet = p.packet(costs); values = device(packet, p.recipes.size()); }
    else throw std::runtime_error("unknown TIGER_EXTRACTION_MARKERS mode");
    auto evaluated = Clock::now(); auto result = p.scatter(g, values); auto scattered = Clock::now();
    const char *verify = std::getenv("TIGER_EXTRACTION_VERIFY");
    bool checked = verify && std::strcmp(verify, "1") == 0;
    if (checked) for (const auto &owner : p.owners)
        if (result[owner.first][owner.second] != native(g, costs, g.eclasses[owner.first].enodes[owner.second]))
            throw std::runtime_error("marker cost differs from native Tiger");
    auto seconds = [](auto a, auto b) { return std::chrono::duration<double>(b-a).count(); };
    std::fprintf(stderr, "TIGER_MARKERS {\"mode\":\"%s\",\"owners\":%zu,\"recipes\":%zu,\"port_reads\":%zu,\"request_bytes\":%zu,\"compile_seconds\":%.9f,\"evaluate_seconds\":%.9f,\"scatter_seconds\":%.9f,\"verified_costs\":%zu}\n",
        mode.c_str(), p.owners.size(), p.recipes.size(), p.port_reads, packet.size()*8,
        seconds(start, compiled), seconds(compiled, evaluated), seconds(evaluated, scattered), checked ? p.owners.size() : 0);
    return result;
}
} // namespace extraction_markers
