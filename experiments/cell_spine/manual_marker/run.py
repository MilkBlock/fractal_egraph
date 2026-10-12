#!/usr/bin/env python3
"""Explicit edits to frozen generated .egg files, before changing the compiler.

No candidate search, solver changes or full graph comparison. Emit each edited
library and its textual diff; compare native counts in a separate process and
time each variant once. Recipes below are deliberately specific to these files.
"""
import argparse
import copy
import difflib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[3]
sys.path[:0]=[str(ROOT),str(ROOT/'tools/bench')]
from math_virtual import measure
from tools.cell_spine.compiler import ast,digest
from tools.cell_spine.markers import render
from tools.cell_spine.terms import text,walk
from tools.cell_spine.takeover import var
from tools.cell_spine.pricing import cost

RUNNER=ROOT/'target/release/examples/native_math_baseline'


def marker_fact(f):
    return f['kind']=='fact' and f['expr'].get('head','').startswith('Spine-Summary-')


def manual_lhs(case,rules):
    result=copy.deepcopy(rules);edits=[]
    for r in result:
        before=render(r)
        if case=='long_spine_graft' and r['name']=='retire_0':
            assert len(r['body'])==9
            r['body']=[f for f in r['body'] if marker_fact(f)]
            reason='The large marker carries the exact child root; both active markers stay in the LHS. Seven existential decoder reads are unused.'
        elif case=='assoc' and r['name']=='retire_0':
            assert len(r['body'])==4
            # Keep outer Plus: it binds which child marker may be retired.
            r['body']=[f for f in r['body'] if not (f['kind']=='eq' and text(f['args'][1])=='(Spine-Cano-Plus p0 p1)')]
            reason='The child marker guarantees a Plus at its root; only its outer connection must still be read.'
        elif case=='assoc' and r['name']=='retire_1':
            assert len(r['body'])==4
            r['body']=[f for f in r['body'] if marker_fact(f)]
            reason='Both markers already bind the same root; the owner guarantees the two-node seed.'
        elif case=='eggcc_if' and r['name']=='retire_2':
            assert len(r['body'])==3 and text(r['body'][-1]['args'][1])=='(Spine-Cano-Single p0)'
            r['body']=r['body'][:-1]
            reason='Summary-2 directly carries Single(p0) as its anchor; the repeated constructor check contributes no binding.'
        else:continue
        edits.append(dict(rule=r['name'],before=before,after=render(r),reason=reason))
    return result,edits


def manual_rhs_if(rules):
    result=copy.deepcopy(rules);edits=[]
    recipes={
        'impl_6_5_cover_7':{'(Spine-Cano-Get marker_value_45 0)':'cover_0_r3','(Spine-Cano-Get marker_value_46 0)':'cover_1_r4'},
        'impl_7_5_cover_19':{'(Spine-Cano-Get marker_value_33 0)':'cover_0_r3','(Spine-Cano-Get marker_value_46 0)':'cover_1_r4'},
    }
    for r in result:
        if r['name'] not in recipes:continue
        before=render(r);mapping=recipes[r['name']]
        # These are existing Get(Single(p),0) results carried by the two owners.
        # The preceding Single unions stay intact. Do not erase a newly needed
        # target constructor or a source/target equality.
        if r['name']=='impl_6_5_cover_7':
            assert any(a['kind']=='union' and {text(x) for x in a['args']}=={'marker_value_45','(Spine-Cano-Single cover_0_p0)'} for a in r['head'])
        assert any(a['kind']=='union' and {text(x) for x in a['args']}=={'marker_value_46','(Spine-Cano-Single cover_1_p1)'} for a in r['head'])
        def replace(e):
            if text(e) in mapping:return var(mapping[text(e)])
            if e['kind']=='call':return dict(e,args=[replace(a) for a in e['args']])
            return e
        head=[];removed=0
        for a in r['head']:
            if a['kind']=='union':
                a=dict(a,args=[replace(e) for e in a['args']])
                if a['args'][0]==a['args'][1]:removed+=1;continue
            elif a['kind']=='expr':a=dict(a,expr=replace(a['expr']))
            head.append(a)
        assert removed==2
        r['head']=head
        edits.append(dict(rule=r['name'],before=before,after=render(r),reason='Reuse two source-marker Get results after keeping the Single alias unions; remove only the two resulting self unions.'))
    assert len(edits)==2
    return result,edits


def apply_rules(library,old,new):
    for before,after in zip(old,new):
        a,b=render(before),render(after)
        assert library.count(a)==1
        library=library.replace(a,b)
    return library


def marker_only_long(library,rules,exports):
    """One extra marker field replaces the target's canonical Extend node."""
    result=copy.deepcopy(rules);edits=[]
    for r in result:
        if r['name'] not in ('impl_1_0','impl_1_0_cover_1','retire_0'):continue
        before=render(r)
        for f in r['body']:
            if marker_fact(f) and f['expr']['head']=='Spine-Summary-1':f['expr']['args'].append(var('previous_next'))
        if r['name']!='retire_0':
            r['head']=[a for a in r['head'] if not any(e.get('head')=='Spine-Cano-Extend' for root in ([a['expr']] if a['kind']=='expr' else a['args']) for e in walk(root))]
            for a in r['head']:
                if a['kind']=='expr' and a['expr']['head']=='Spine-Summary-1':a['expr']['args'].append(var('p7'))
        edits.append(dict(rule=r['name'],before=before,after=render(r),reason='Carry next in Summary-1; no runtime reader requires a materialized canonical Extend. Keep external Extend reads and parent subsumption.'))
    library=apply_rules(library,rules,result)
    declaration='(constructor Spine-Summary-1 (Spine-Term Spine-Term) Spine-Cell)'
    assert library.count(declaration)==1
    library=library.replace(declaration,'(constructor Spine-Summary-1 (Spine-Term Spine-Term Spine-Term) Spine-Cell)')
    changed=0
    for r in exports:
        if not any(marker_fact(f) and f['expr']['head']=='Spine-Summary-1' for f in r['body']):continue
        edited=copy.deepcopy(r);before=render(r)
        for f in edited['body']:
            if marker_fact(f) and f['expr']['head']=='Spine-Summary-1':f['expr']['args'].append(var('p7'))
        edited['body']=[f for f in edited['body'] if not (f['kind']=='eq' and f['args'][1].get('head')=='Spine-Cano-Extend')]
        assert len(edited['body'])==len(r['body'])-1 and library.count(before)==1
        library=library.replace(before,render(edited));changed+=1
        edits.append(dict(rule='export Summary-1',before=before,after=render(edited),reason='Decode next from the marker field; export does not need canonical Extend either.'))
    assert changed==1
    return library,result,edits


def aggregate(rules):
    return {k:sum(cost(r)[k] for r in rules) for k in cost(rules[0])}


def reuse_input_spine(library,rules,folder):
    """This fixture's input already IS the chosen right spine. Borrow it."""
    changed=copy.deepcopy(rules);edits=[]
    for r in changed:
        if r['name']!='impl_0_0':continue
        before=render(r)
        r['head']=[a for a in r['head'] if a['kind']=='expr' and a['expr']['head']=='Spine-Summary-0']
        assert len(r['head'])==1
        library=library.replace(before,render(r))
        edits.append(dict(rule=r['name'],before=before,after=render(r),reason='Input is already the selected canonical right chain; its raw rows persist. Publish the owner without copying six nodes into Cano tables.'))
    parsed=ast_file_text(library,folder/'borrow-input-ast')
    for r in parsed['rules']:
        if r['ruleset']!='Spine-Export' or not any(marker_fact(f) for f in r['body']):continue
        edited=copy.deepcopy(r);before=render(r)
        for f in edited['body']:
            for root in (f['args'] if f['kind']=='eq' else [f['expr']]):
                for e in walk(root):
                    if e.get('head')=='Spine-Cano-Plus':e['head']='Spine-Plus'
        assert library.count(before)==1
        library=library.replace(before,render(edited))
        edits.append(dict(rule='export owner',before=before,after=render(edited),reason='Recover the seed slots from the existing input chain, with the owner marker still required.'))
    return library,changed,edits


def ast_file_text(program,prefix):
    prefix.with_suffix('.egg').write_text(program)
    return ast(prefix.with_suffix('.egg'),prefix.with_suffix('.json'))


def measure_variant(case,mode,program,rs,edits,lib,folder,base,original):
    vdir=folder/mode;vdir.mkdir();path=vdir/'library.egg';path.write_text(program)
    (vdir/'changes.diff').write_text(''.join(difflib.unified_diff(lib.splitlines(True),program.splitlines(True),fromfile='generated.egg',tofile=f'{mode}.egg')))
    (vdir/'edits.json').write_text(json.dumps(edits,indent=2)+'\n')
    ast(path,vdir/'library-ast.json')
    inspect=json.loads((base/case/'fresh-1000/joint_export.json').read_text())
    inspect.update(environment=str(path),counts=True)
    (vdir/'counts.json').write_text(json.dumps(inspect,indent=2)+'\n')
    shape_run=measure([RUNNER,'bench-probes',vdir/'counts.json'],vdir/'counts')
    v=shape_run['result']['verification'];assert v['check_mode']=='counts_only'
    expected=next(c['native_shape'] for c in original['cases'] if c['fixture']==case and c['label']=='fresh-1000')
    actual=v['restored'];counts_match=all(actual[k]==expected[k] for k in ('enodes','eclasses'))
    if not counts_match:raise ValueError(f'{case}/{mode}: restored size changed')
    timings={}
    for timed_mode in ('compact','export'):
        manifest=dict(inspect,counts=False)
        if timed_mode=='compact':manifest.pop('export',None)
        file=vdir/f'{timed_mode}.json';file.write_text(json.dumps(manifest,indent=2)+'\n')
        timings[timed_mode]=measure([RUNNER,'bench',file],vdir/f'time-{timed_mode}')
        assert timings[timed_mode]['result']['verification'] is None
    return dict(fixture=case,variant=mode,library_bytes=len(program.encode()),runtime_cost=aggregate(rs),edit_steps=len(edits),
        affected_runtime_rules=sum(render(r) not in lib for r in rs),
        size_check='counts_match_not_graph_equivalence',restored_enodes=actual['enodes'],restored_eclasses=actual['eclasses'],
        raw_enodes=v['raw_spine']['enodes'],canonical_enodes=v['canonical_spine']['enodes'],
        active_summaries=v['active_summaries'],subsumed_summaries=v['subsumed_summaries'],
        total_table_rows=v['native_total_rows_before_export'],probes=v['run_probe_counters'],rule_matches=v['rule_matches'],timings=timings)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--ablation-root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--skip-build',action='store_true')
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    if not a.skip_build:subprocess.run(['cargo','build','--offline','--release','--example','native_math_baseline'],cwd=ROOT,check=True)
    os.environ['RAYON_NUM_THREADS']='1';os.environ.pop('EGGLOG_JOIN_WORK_RULES',None)
    base=a.ablation_root.resolve();original=json.loads((base/'report.json').read_text())
    report=dict(schema='manual-marker-bench/v1',scope='Hand-edited generated libraries; count comparison only, no full graph equivalence check; no compiler/solver changes',
        commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),binary_sha256=digest(RUNNER),cases=[],native_reference={})
    for case in ('long_spine_graft','assoc','eggcc_if'):
        folder=out/case;folder.mkdir()
        oldbase=base/case/'joint';lib=(oldbase/'library.egg').read_text()
        assert digest(oldbase/'library.egg')==original['training'][case]['variants']['joint']['library_sha256']
        rules=json.loads((oldbase/'marker-runtime.json').read_text())['rules']
        lhs,changes=manual_lhs(case,rules);lhs_lib=apply_rules(lib,rules,lhs)
        variants={'base':(lib,rules,[]),'lhs_manual':(lhs_lib,lhs,changes)}
        if case=='eggcc_if':
            rhs,extra=manual_rhs_if(lhs)
            variants['lhs_rhs_manual']=(apply_rules(lhs_lib,lhs,rhs),rhs,changes+extra)
        if case=='long_spine_graft':
            parsed=ast(oldbase/'library.egg',folder/'original-ast.json')
            exports=[r for r in parsed['rules'] if r['ruleset']=='Spine-Export']
            compact,rs,extra=marker_only_long(lhs_lib,lhs,exports)
            variants['marker_only']=(compact,rs,changes+extra)
            borrowed,br,bextra=reuse_input_spine(compact,rs,folder)
            variants['reuse_input_spine']=(borrowed,br,changes+extra+bextra)
        for mode,(program,rs,edits) in variants.items():
            row=measure_variant(case,mode,program,rs,edits,lib,folder,base,original)
            report['cases'].append(row)
            (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
            print(case,mode,round(row['timings']['compact']['result']['run_seconds']*1000,3),row['canonical_enodes'],row['probes']['candidate_rows'],flush=True)
        native_dir=folder/'native';native_dir.mkdir()
        manifest=json.loads((base/case/'fresh-1000/native.json').read_text())
        (native_dir/'run.json').write_text(json.dumps(manifest,indent=2)+'\n')
        report['native_reference'][case]=measure([RUNNER,'bench',native_dir/'run.json'],native_dir/'time')
        (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(out/'report.json')


if __name__=='__main__':main()
