"""Actual native CPU execution; one ordinary/native control per fixture."""
import argparse,copy,json,subprocess,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];sys.path.insert(0,str(ROOT))
from eggfuse.engine.cpu import execute,binary,compare_native

CASES={
'chain':(ROOT/'experiments/cpu_multistage/chain.egg').read_text(),
'finite-and-sat':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (run a 1) (fail (check (Q (P (N 0)))))
(run-schedule (saturate (run a) (run b))) (check (Q (P (N 0))))''',
'finite-repeat':'''(datatype E (N i64)) (ruleset s)
(rule ((= n (N x))) ((N (+ x 1))) :ruleset s)
(N 0) (run-schedule (repeat 5 (run s)))
(check (N 5)) (fail (check (N 6)))''',
'guards-and-branches':'''(datatype E (N i64) (P E i64) (Q E) (R E))
(ruleset a) (ruleset b) (ruleset c)
(rule ((= x (N n))) ((P x n)) :ruleset a)
(rule ((= p (P x n)) (> n 0)) ((Q p)) :ruleset b)
(rule ((= p (P x n)) (> n -2)) ((R p)) :ruleset c)
(N 0) (N 1) (run-schedule (saturate (run a) (run b) (run c)))
(check (Q (P (N 1) 1)) (R (P (N 0) 0))) (fail (check (Q (P (N 0) 0))))''',
'late-input':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (run-schedule (saturate (run a) (run b)))
(N 1) (run-schedule (saturate (run a) (run b)))
(check (Q (P (N 0))) (Q (P (N 1))))''',
'late-union':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (N 1) (run-schedule (saturate (run a) (run b)))
(union (N 0) (N 1)) (run-schedule (saturate (run a) (run b)))
(check (= (Q (P (N 0))) (Q (P (N 1)))))''',
'hidden':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (P (N 0)) (subsume (P (N 0)))
(run-schedule (saturate (run a) (run b))) (fail (check (Q (P (N 0)))))''',
'union':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((union x (P x))) :ruleset a)
(rule ((= p (P x)) (= p x)) ((Q p)) :ruleset b)
(N 0) (run-schedule (saturate (run a) (run b))) (check (Q (N 0)))''',
'hidden-union':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((union x (P x))) :ruleset a)
(rule ((= p (P x)) (= p x)) ((Q p)) :ruleset b)
(P (N 0)) (P (N 1)) (subsume (P (N 1)))
(run-schedule (saturate (run a) (run b))) (check (Q (N 0)))''',
'unrelated-existing-receipt':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N 0))) ((P x)) :ruleset a) (rule ((= p (P x))) ((Q p)) :ruleset b)
(P (N 0)) (Q (N 99)) (run-schedule (saturate (run a) (run b)))
(check (Q (P (N 0))))''',
'unit-functions':'''(datatype E (N i64)) (function P (E) Unit :merge old) (function Q (E) Unit :merge old)
(ruleset a) (ruleset b) (rule ((= x (N n))) ((set (P x) ())) :ruleset a)
(rule ((P x)) ((set (Q x) ())) :ruleset b)
(N 0) (N 1) (run-schedule (saturate (run a) (run b))) (check (Q (N 0)))''',
'mixed-closed-region':'''(datatype E (N i64) (P E) (Q E)) (function F () i64 :merge new)
(ruleset a) (ruleset b) (rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(rule ((= n (F)) (> n 0)) ((set (F) (- n 1))) :ruleset a)
(N 0) (set (F) 4) (run-schedule (saturate (run a) (run b)))
(check (= (F) 0) (Q (P (N 0))))''',
'mixed-observer-barrier':'''(datatype E (N i64) (P E) (Q E)) (function F () i64 :merge new)
(ruleset a) (ruleset b) (ruleset c)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= q (Q p))) ((set (F) 1)) :ruleset b)
(rule ((= p (P x))) ((Q p) (set (F) 2)) :ruleset c)
(N 0) (run-schedule (saturate (run a) (run b) (run c))) (check (= (F) 1))''',
'multi-binding':'''(datatype E (N i64) (P E E) (Q E E)) (relation Edge (E E))
(ruleset a) (ruleset b)
(rule ((Edge x y)) ((P x y)) :ruleset a)
(rule ((= p (P x y))) ((Q p y)) :ruleset b)
(Edge (N 1) (N 2)) (Edge (N 1) (N 3))
(run-schedule (saturate (run a) (run b)))
(check (Q (P (N 1) (N 2)) (N 2)) (Q (P (N 1) (N 3)) (N 3)))
(fail (check (Q (P (N 1) (N 2)) (N 3))))''',
'private-receipts':'''(datatype E (N i64) (P E) (Q E) (R E) (S E))
(ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p) (R p)) :ruleset b)
(N 0) (N 1) (run-schedule (saturate (run a) (run b)))
(check (Q (P (N 0))) (R (P (N 1))))
(extract (Q (P (N 0))))''',
'visibility-update':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(N 0) (run-schedule (saturate (run a) (run b)))
(subsume (P (N 0))) (delete (Q (P (N 0))))
(run-schedule (saturate (run a) (run b)))
(fail (check (Q (P (N 0)))))''',
'future-scalar-observer':'''(datatype E (N i64) (P E) (Q E))
(function Choice () i64 :merge new) (ruleset a) (ruleset b) (ruleset later)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(rule ((= q (Q p)) (= p (P x)) (= x (N n))) ((set (Choice) n)) :ruleset later)
(N 0) (N 1) (run-schedule (saturate (run a) (run b))) (run later 1)''',
'future-choice-barrier':'''(datatype E (N i64) (P E) (Q E))
(function Choice (E) E :merge new) (ruleset a) (ruleset b) (ruleset later)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(rule ((= q (Q p))) ((set (Choice p) q)) :ruleset later)
(N 0) (run-schedule (saturate (run a) (run b))) (run later 1)
(check (= (Choice (P (N 0))) (Q (P (N 0)))))''',
'custom-merge':'''(datatype E (N i64) (Pair E E)) (function F () E :merge (Pair old new))
(ruleset s) (rule ((= x (N n))) ((set (F) x)) :ruleset s)
(N 0) (N 1) (run-schedule (saturate (run s)))''',
'minmax':'''(datatype E (N i64) (P E)) (function Size (E) i64 :merge (min old new))
(ruleset s) (rule ((= x (N n))) ((P x) (set (Size x) (+ n 1))) :ruleset s)
(N 4) (run-schedule (saturate s)) (check (= (Size (N 4)) 5))''',
'receipt-progress':'''(datatype E (N i64) (P E) (Q E)) (ruleset a) (ruleset b)
(rule ((= x (N n))) ((P x)) :ruleset a)
(rule ((= p (P x))) ((Q p)) :ruleset b)
(Q (P (N 0))) (run-schedule (repeat 5 (saturate (run a) (run b))))''',
}

def main():
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True);p.add_argument('--cpu-bin',type=Path);a=p.parse_args();exe=a.cpu_bin.resolve() if a.cpu_bin else binary();a.output.mkdir(parents=True,exist_ok=False);reports={}
    for name,source in CASES.items():
        src=a.output/(name+'.egg');src.write_text(source);r=execute(src,a.output/name,exe,True);c=r['candidate']['cells'];reports[name]=dict(native=r['native']['inference_seconds'],cell=r['candidate']['inference_seconds'],proof=r['verification'],cells=c)
        if name=='private-receipts':
            assert c['private_receipt_tables']>0 and c['inline_successor_invocations']>0
            assert r['candidate']['source_outputs']==r['native']['source_outputs']
        if name=='future-scalar-observer':assert c['compiled_scopes']==0
        if name=='future-choice-barrier':
            assert c['compiled_scopes']==0 and 'order-sensitive' in c['rejected_scopes'][0]['reason']
        if name=='mixed-closed-region':assert c['inline_successor_invocations']>0
        if name=='mixed-observer-barrier':assert c['inline_successor_invocations']==0
        if name=='chain':assert c['inline_successor_invocations']==4 and c['whole_rule_queries_skipped']>0
        if name in ('custom-merge','minmax','finite-repeat'):assert c['inline_successor_invocations']==0
        if name=='hidden':assert c['inline_successor_invocations']==0
        if name=='hidden-union':assert c['hidden_union_scope_fallbacks']>0
        if name=='receipt-progress':assert len(r['candidate']['native_report']['iterations'])==len(r['native']['native_report']['iterations'])
        print(name,'passed',c['inline_successor_invocations'],c['whole_rule_queries_skipped'],flush=True)
    src=a.output/'chain.egg';subprocess.run([str(exe),'--source',str(src),'--plan',str(a.output/'chain/cpu-program.json'),'--output',str(a.output/'repeat')],check=True,stdout=subprocess.DEVNULL)
    assert json.loads((a.output/'repeat/state.json').read_text())==json.loads((a.output/'chain/cell/state.json').read_text())
    # Saved artifact is input-parametric: another process and new facts.
    pth=a.output/'chain/cpu-program.json';src=a.output/'unseen.egg';src.write_text(CASES['chain'].replace('(N 0) (N 1)','(N 0) (N 1) (N 99)'))
    reports['fresh-input']=execute(src,a.output/'fresh-input',exe,True,pth)
    # A malformed mapping cannot authorize an effect; a forged source is fatal.
    plan=json.loads(pth.read_text());plan['regions']['batch_program']['templates'][0]['steps'][0]['mapping'][-1]={'slot':0}
    forged=a.output/'bad-mapping.json';forged.write_text(json.dumps(plan));folder=a.output/'bad-mapping'
    subprocess.run([str(exe),'--source',str(a.output/'chain.egg'),'--plan',str(forged),'--output',str(folder)],check=True,stdout=subprocess.DEVNULL)
    assert json.loads((folder/'report.json').read_text())['cells']['rejected_templates']
    compare_native(json.loads((a.output/'chain/core.json').read_text()),json.loads((folder/'state.json').read_text()),json.loads((a.output/'chain/native/state.json').read_text()))
    plan['core']['rulesets'][0]['rules'][0]['name']='forged';forged.write_text(json.dumps(plan))
    r=subprocess.run([str(exe),'--source',str(a.output/'chain.egg'),'--plan',str(forged),'--output',str(a.output/'bad-source')],capture_output=True);assert r.returncode!=0
    (a.output/'results.json').write_text(json.dumps(reports,indent=2)+'\n')
if __name__=='__main__':main()
