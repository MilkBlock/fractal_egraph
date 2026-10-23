#pragma once
// Diagnostic-only trace of one complete production Statewalk region.
#include "egraphin.h"
#include <cstdlib>
#include <fstream>
#include <sstream>

namespace film_trace {
inline int function_index=-1,region_index=-1;
inline int wanted() { const char *s=std::getenv("STATEWALK_VIDEO_FUNCTION");return s?std::atoi(s):2; }
inline bool active() { return function_index==wanted() && region_index==0; }
inline std::ofstream &file() { static std::ofstream f(std::getenv("STATEWALK_VIDEO_TRACE"));return f; }
inline std::string quote(const std::string &s) {
    std::string result="\"";for(char c:s){if(c=='\\'||c=='\"')result+='\\';if(c=='\n')result+="\\n";else result+=c;}return result+'\"';
}
template<class T>std::string array(const std::vector<T> &xs){std::ostringstream o;o<<'[';for(size_t i=0;i<xs.size();i++){if(i)o<<',';o<<xs[i];}o<<']';return o.str();}
inline void event(const std::string &body){if(active())file()<<'{'<<body<<"}\n";}
inline void function(int i,const EGraph &g,EClassId root){
    function_index=i;region_index=-1;
    if(i!=wanted())return;
    file()<<"{\"event\":\"function\",\"index\":"<<i<<",\"root\":"<<root<<",\"heads\":[";
    bool first=true;for(auto &n:g.eclasses[root].enodes){for(auto ch:n.ch)for(auto &v:g.eclasses[ch].enodes){if(!first)file()<<',';first=false;file()<<quote(v.get_op());}}file()<<"]}\n";
}
inline void begin(const EGraph &g,EClassId root){
    ++region_index;if(!active())return;
    event("\"event\":\"region\",\"root\":"+std::to_string(root)+",\"classes\":"+std::to_string(g.neclasses()));
    for(size_t c=0;c<g.neclasses();c++)for(size_t n=0;n<g.eclasses[c].enodes.size();n++){
        const auto &e=g.eclasses[c].enodes[n];
        event("\"event\":\"node\",\"class\":"+std::to_string(c)+",\"node\":"+std::to_string(n)+",\"head\":"+quote(e.head)+",\"effectful\":"+(g.eclasses[c].isEffectful?"true":"false")+",\"children\":"+array(e.ch));
    }
}
inline void pop(int id,int pos,unsigned long long cost,size_t queue,bool stale){event("\"event\":\"pop\",\"id\":"+std::to_string(id)+",\"position\":"+std::to_string(pos)+",\"cost\":"+std::to_string(cost)+",\"queue_after_pop\":"+std::to_string(queue)+",\"stale\":"+(stale?"true":"false"));}
inline void candidate(int from,int target,int node,int missing,unsigned long long cost){event("\"event\":\"candidate\",\"from\":"+std::to_string(from)+",\"target\":"+std::to_string(target)+",\"node\":"+std::to_string(node)+",\"first_missing\":"+std::to_string(missing)+",\"incremental_estimate\":"+std::to_string(cost));}
inline void publish(int id,int prev,int target,int node,unsigned long long cost){event("\"event\":\"publish\",\"id\":"+std::to_string(id)+",\"prev\":"+std::to_string(prev)+",\"target\":"+std::to_string(target)+",\"node\":"+std::to_string(node)+",\"cost\":"+std::to_string(cost));}
}
