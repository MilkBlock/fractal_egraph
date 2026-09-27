#ifndef PROF_H
#define PROF_H
#include <chrono>
#include <cstdio>
#include <map>
#include <string>
inline std::map<std::string,double>& prof_map(){ static std::map<std::string,double> m; return m; }
static std::chrono::steady_clock::time_point g_prof_t;
static inline void prof_begin(){ g_prof_t = std::chrono::steady_clock::now(); }
static inline void prof_end(const char* k){ auto e=std::chrono::steady_clock::now(); prof_map()[k]+=std::chrono::duration<double>(e-g_prof_t).count(); }
static inline void prof_dump(){
    for (auto &kv : prof_map()) std::fprintf(stderr, "PROF %-30s %10.4f s\n", kv.first.c_str(), kv.second);
}
#endif
