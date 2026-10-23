#pragma once
#include "greedy.h"
#include <algorithm>
#include <cstdio>
#include <memory>
#include <vector>

namespace extraction_markers {
// A cost marker records EXACT paid e-class contributions. It is not a scalar
// local optimum: the bag survives so cross-Cell DAG sharing remains visible.
struct BagStats {
    size_t merges=0, input_entries=0, output_entries=0, max_entries=0;
    ~BagStats() {
        if (merges) std::fprintf(stderr,"TIGER_COST_BAG {\"merges\":%zu,\"input_entries\":%zu,\"output_entries\":%zu,\"max_entries\":%zu}\n",merges,input_entries,output_entries,max_entries);
    }
};
inline BagStats &bag_stats() { static BagStats stats; return stats; }

struct CostBag {
    std::vector<std::pair<EClassId,Cost>> entries;
    Cost total=0;
};
struct MarkerCost {
    Cost sum;
    std::shared_ptr<const CostBag> bag;
    explicit MarkerCost(Cost v) : sum(v) {}
};
inline bool operator<(const MarkerCost &a,const MarkerCost &b) { return a.sum < b.sum; }

inline MarkerCost &operator+=(MarkerCost &a,const std::pair<EClassId,MarkerCost> &child) {
    static const CostBag empty;
    const auto &left=a.bag ? *a.bag : empty;
    const auto &right=child.second.bag ? *child.second.bag : empty;
    // Tiger's overhead is child.sum minus the contributions already in its bag.
    // Preserve uint64 modulo arithmetic, including INF and overflow behavior.
    Cost overhead=child.second.sum-right.total;
    auto merged=std::make_shared<CostBag>(); auto &out=merged->entries;
    out.reserve(left.entries.size()+right.entries.size()+1);
    size_t i=0,j=0;
    while (i<left.entries.size() || j<right.entries.size()) {
        if (j==right.entries.size() || (i<left.entries.size() && left.entries[i].first<right.entries[j].first))
            out.push_back(left.entries[i++]);
        else if (i==left.entries.size() || right.entries[j].first<left.entries[i].first)
            out.push_back(right.entries[j++]);
        else { out.emplace_back(left.entries[i].first,std::min(left.entries[i].second,right.entries[j].second));++i;++j; }
    }
    auto pos=std::lower_bound(out.begin(),out.end(),child.first,[](const auto &v,EClassId key){return v.first<key;});
    if (pos!=out.end() && pos->first==child.first) pos->second=std::min(pos->second,overhead);
    else out.insert(pos,{child.first,overhead});
    for (const auto &v:out) merged->total+=v.second;
    a.sum+=merged->total-left.total;
    auto &s=bag_stats();++s.merges;s.input_entries+=left.entries.size()+right.entries.size();
    s.output_entries+=out.size();s.max_entries=std::max(s.max_entries,out.size());
    a.bag=std::move(merged);
    return a;
}
} // namespace extraction_markers
