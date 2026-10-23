#include "extraction_markers.h"
#include "cost_bag.h"
#include <cassert>
#include <iostream>
#include <random>
#include <unordered_map>

Cost get_statewalk_enode_cost(const EGraph &, const std::vector<Cost> &, const ENode &);

int main(int argc, char **argv) {
    using namespace extraction_markers;
    std::mt19937_64 rng(17);
    struct Reference { Cost sum; std::unordered_map<EClassId,Cost> bag; };
    std::vector<MarkerCost> markers; std::vector<Reference> refs;
    for (int i=0;i<64;++i) { auto x=rng();markers.emplace_back(x);refs.push_back({x,{}}); }
    for (int step=0;step<4000;++step) {
        auto i=rng()%64,j=rng()%64; EClassId key=rng()%24;
        auto value=markers[i];value+=std::make_pair(key,markers[j]);
        auto r=refs[i];Cost overhead=refs[j].sum;
        auto add=[&](EClassId k,Cost c) {
            auto p=r.bag.find(k);
            if (p==r.bag.end()) {r.bag.emplace(k,c);r.sum+=c;}
            else if(p->second>c){r.sum-=p->second-c;p->second=c;}
        };
        for (auto v:refs[j].bag) {overhead-=v.second;add(v.first,v.second);}
        add(key,overhead);assert(value.sum==r.sum);assert(value.bag->entries.size()==r.bag.size());
        for (auto v:value.bag->entries) assert(r.bag.at(v.first)==v.second);
        markers[i]=value;refs[i]=std::move(r);
    }
    EGraph g; g.eclasses.resize(128);
    std::vector<Cost> costs(g.neclasses());
    for (size_t i=0; i<costs.size(); ++i) {
        costs[i] = rng(); // includes unsigned overflow cases
        g.eclasses[i].isEffectful = i >= 64;
    }
    const char *ops[] = {"If", "DoWhile", "Print", "Bop", "Get", "Call"};
    for (int c=64; c<128; ++c) for (int j=0; j<100; ++j) {
        std::string op = ops[j%6];
        ENode n; n.head = "instance_" + std::to_string(c) + "###" + op; n.eclass = c;
        int arity = op == "If" ? 4 : op == "DoWhile" ? 2 : 3;
        for (int a=0; a<arity; ++a) n.ch.push_back((j+a)%128);
        // Ensure actual duplicate-child edges survive interning and summation.
        if (j%7 == 0 && arity >= 3) n.ch[1] = n.ch[0];
        g.eclasses[c].enodes.push_back(n);
    }
    Program p(g);
    assert(p.recipes.size() < p.owners.size());
    auto check = [&](const std::vector<Cost> &x, const std::vector<Cost> &values) {
        auto actual = p.scatter(g, values);
        for (const auto &o : p.owners)
            assert(actual[o.first][o.second] == get_statewalk_enode_cost(g, x, g.eclasses[o.first].enodes[o.second]));
    };
    auto values = p.evaluate(costs); check(costs, values);
    auto next = costs; next[0]=0; next[1]=~0ull; next[2]=~0ull-1;
    auto dirty = p.update(costs, next, values); assert(dirty > 0 && dirty < p.recipes.size()); check(next,values);
    assert(p.update(next, next, values) == 0);
    if (argc > 1) {
        setenv("TIGER_EXTRACTION_WORKER", argv[1], 1);
        auto gpu = device(p.packet(next), p.recipes.size()); assert(gpu == values); check(next,gpu);
        // A separate process/dispatch must produce exactly the same uint64 vector.
        assert(device(p.packet(next), p.recipes.size()) == gpu);
    }
    bool rejected=false;
    try { p.evaluate({}); } catch (const std::runtime_error &) { rejected=true; }
    assert(rejected);
    EGraph empty; Program e(empty); assert(e.evaluate({}).empty());
    if (argc > 1) assert(device(e.packet({}),0).empty());
    if (argc > 1) {
        setenv("TIGER_EXTRACTION_WORKER","/nonexistent/tiger-worker",1);bool rejected=false;
        try { device(p.packet(next),p.recipes.size()); } catch(const std::runtime_error &) {rejected=true;}
        assert(rejected);
    }
    std::cout << "passed: " << p.owners.size() << " native costs, " << p.recipes.size()
              << " shared recipes, " << dirty << " dirty updates; aliases, If, loop, overflow, deterministic GPU\n";
}
