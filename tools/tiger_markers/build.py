"""Build an isolated, fail-closed overlay of the user's production Tiger."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

HERE = Path(__file__).resolve().parent
SOURCES = 'egraphin.cpp greedy.cpp json2egraphin.cpp main.cpp regionalize.cpp statewalkdp.cpp tiger.cpp toegglog.cpp debug.cpp ilp.cpp time_ilp.cpp'.split()


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def identity(checkout):
    checkout=Path(checkout)
    source = checkout / 'dag_in_context/src/tiger'
    inputs = {str(p.relative_to(checkout)): sha(p) for p in sorted(source.iterdir()) if p.suffix in ('.h', '.cpp')}
    inputs.update({str(p.relative_to(HERE)): sha(p) for p in (HERE/'build.py', HERE/'extraction_markers.h', HERE/'cost_bag.h', HERE/'costs.swift')})
    return inputs


def ensure(checkout, cache):
    key=hashlib.sha256(json.dumps(identity(checkout),sort_keys=True).encode()).hexdigest()
    return build(checkout,Path(cache)/key)


def build(checkout, destination):
    checkout, destination = Path(checkout), Path(destination)
    source = checkout / 'dag_in_context/src/tiger'
    inputs=identity(checkout)
    stamp = destination/'build.json'
    binary, worker = destination/'tiger', destination/'costs'
    if stamp.exists():
        old = json.loads(stamp.read_text())
        if old.get('inputs') == inputs and all(p.exists() and old.get(p.name) == sha(p) for p in (binary, worker)):
            return binary, worker
        raise ValueError('build directory has different inputs; choose a fresh directory')
    destination.mkdir(parents=True, exist_ok=False)
    copy = destination/'source'; copy.mkdir()
    for p in source.iterdir():
        if p.suffix in ('.cpp', '.h'): shutil.copyfile(p, copy/p.name)
    greedy = copy/'greedy.cpp'; text = greedy.read_text()
    hook = 'vector<Cost> eclass_cost = greedy_extract_estimate_all_eclasses_cost(g);'
    if text.count(hook) != 1: raise ValueError('production Tiger hook changed')
    begin = 'pair<vector<ENodeId>, vector<Cost> > greedy_extract_compute_eclasses_pick('
    end = 'vector<Cost> greedy_extract_estimate_all_eclasses_cost('
    if text.count(begin)!=1 or text.count(end)!=1:raise ValueError('production greedy hook changed')
    i,j=text.index(begin),text.index(end)
    original=text[i:j]
    generic='template<class StateCost>\n'+original.replace('greedy_extract_compute_eclasses_pick','marker_compute_picks').replace('SCost','StateCost')
    wrapper='''pair<vector<ENodeId>,vector<Cost>> greedy_extract_compute_eclasses_pick(const EGraph &g,const EClassId root=-1) {
    if (!extraction_markers::bags_enabled()) return marker_compute_picks<SCost>(g,root);
    auto result=marker_compute_picks<extraction_markers::MarkerCost>(g,root);
    const char *verify=std::getenv("TIGER_EXTRACTION_VERIFY");
    if (verify && std::strcmp(verify,"1")==0) {
        auto reference=marker_compute_picks<SCost>(g,root);
        if (result!=reference) throw std::runtime_error("marker greedy choices or costs differ from Tiger");
        std::fprintf(stderr,"TIGER_GREEDY_ORACLE {\\"classes\\":%zu,\\"root\\":%d,\\"passed\\":true}\\n",g.neclasses(),root);
    }
    return result;
}

'''
    text=text[:i]+generic+wrapper+text[j:]
    text='#include "cost_bag.h"\n'+text
    text = '#include "extraction_markers.h"\n' + text.replace(hook, '''auto estimate_start = std::chrono::steady_clock::now();
    ''' + hook + '''
    auto estimate_end = std::chrono::steady_clock::now();
    std::fprintf(stderr, "TIGER_COST_PHASE {\\"estimate_seconds\\":%.9f}\\n", std::chrono::duration<double>(estimate_end-estimate_start).count());
    if (extraction_markers::enabled()) return extraction_markers::run(g, eclass_cost, get_statewalk_enode_cost);''')
    tail = '    return statewalk_cost;'
    if text.count(tail) != 1: raise ValueError('production Tiger return changed')
    text = text.replace(tail, '''    std::fprintf(stderr, "TIGER_COST_PHASE {\\"native_candidate_seconds\\":%.9f}\\n", std::chrono::duration<double>(std::chrono::steady_clock::now()-estimate_end).count());
''' + tail)
    greedy.write_text(text)
    shutil.copyfile(HERE/'extraction_markers.h', copy/'extraction_markers.h')
    shutil.copyfile(HERE/'cost_bag.h', copy/'cost_bag.h')
    subprocess.run(['c++', '-std=c++17', '-O3', '-Wno-unused-result', *SOURCES, '-o', str(binary.resolve())], cwd=copy, check=True)
    subprocess.run(['swiftc', '-O', str(HERE/'costs.swift'), '-o', str(worker.resolve())], check=True)
    stamp.write_text(json.dumps(dict(inputs=inputs, tiger=sha(binary), costs=sha(worker),
        revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=checkout,text=True).strip()),indent=2)+'\n')
    return binary, worker
