#!/usr/bin/env python3
import hashlib,json,random,statistics,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2];OUT=ROOT/'results/prefix_witness'
MODES=['source_only','global_rematch','rematch','witness']
def main():
 OUT.mkdir(parents=True,exist_ok=True);rows=[];binary=ROOT/'target/release/prefix_witness'
 for case,n,noise in [('fresh',128,0),('fresh',128,4096),('preexisting',128,0),('redundant',128,0),('inactive',128,4096)]:
  for repeat in range(5):
   modes=MODES.copy();random.Random(101+repeat).shuffle(modes)
   for mode in modes:
    p=subprocess.run([str(binary),mode,case,str(n),str(noise)],text=True,capture_output=True,check=True,timeout=60)
    x=json.loads(p.stdout);x['repeat']=repeat;rows.append(x)
    print(case,noise,mode,round(x['measured_ns']/1e6,3),x['recognized_prefixes'],x['global_prefixes'],flush=True)
 summary=[]
 for case,noise,mode in sorted({(x['case'],x['unrelated_roots'],x['mode'])for x in rows}):
  rr=[x for x in rows if(x['case'],x['unrelated_roots'],x['mode'])==(case,noise,mode)]
  x={k:statistics.median(r[k] for r in rr)for k in ['source_ns','recognize_ns','measured_ns','recognized_prefixes','global_prefixes','global_coverage','phase_peak_requested_bytes','phase_retained_delta_bytes']}
  x.update(case=case,noise=noise,mode=mode,min_ns=min(r['measured_ns']for r in rr),max_ns=max(r['measured_ns']for r in rr));summary.append(x)
 meta={'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'repetitions':5,'scope':'ablation 1: source step plus rooted-prefix recognition; no Zobrist history or blocks'}
 (OUT/'runs.json').write_text(json.dumps({'metadata':meta,'runs':rows},indent=2)+'\n');(OUT/'summary.json').write_text(json.dumps({'metadata':meta,'aggregate':summary},indent=2)+'\n')
 lines=['# Prefix witness ablation 1','','Actual local egglog, one source step. Prefix catalog: Add(p0,p1), Add(Mul(x,y),Neg(x)), and Add(Neg(x),Mul(x,y)). Prefix instances include canonical root IDs and ordered boundary bindings, including repeated x.','','|case|unrelated roots|mode|source + recognition ms|recognition ms|found|global catalog instances|','|---|---:|---|---:|---:|---:|---:|']
 for x in summary:lines.append(f"|{x['case']}|{x['noise']}|{x['mode']}|{x['measured_ns']/1e6:.3f}|{x['recognize_ns']/1e6:.3f}|{x['recognized_prefixes']}|{x['global_prefixes']}|")
 lines+=['','## Scope and controls','','- source_only runs the source rule without a detector; it is a timing floor, not a recognition algorithm. global_rematch runs native indexed prefix queries over the graph. rematch uses native queries anchored by a pre-established Target(root) relation, an intentionally strong baseline given the active roots. witness reuses source trace substitutions and verifies constructors by keyed lookup after the step.',
 '- Native detector queries are implemented as redundant union rules with trace output because the public API lacks a query-only substitution sink. We assert updated=false for each detector. This includes action/trace overhead, so it is not a lower bound for an optimized query-only implementation.',
 '- All detector rules and Target relations are prepared in every mode before timing. Setup costs are excluded. Measured time includes the actual source step and extra source tracing in witness mode, plus event handling, verification lookups and result-set allocation. It is not only the hash lookup cost.',
 '- Witness mode gives rule-derivation prefix instances, not all prefixes in the graph. The pure-swap fixtures have no extra alternatives in active classes, so the anchored native baseline and witness result sets are exactly equal. Global queries additionally find preexisting RHS-only regions and unrelated Add roots. A separate test demonstrates that another alternative in the same active class can also escape the witness.',
 '- Scope is a single unconditional, non-deleting E-sort rewrite with x and y retained by the existing logical trace. This does not provide generic physical LHS row witnesses or per-mutation provenance. Successful source matching certifies the LHS; keyed post-step lookups confirm the RHS exists, including the case where it already existed.',
 '- Five independent release processes per configuration, 100 runs total. Phase memory tracks allocation-request delta relative to the prepared graph, not whole-engine compression. No claim about production routing or memory reduction is made.',
 '- Future ablations: incremental prefix lifecycle across merges and alternative additions; then exact-set history versus counted history versus Zobrist history, with field/index/update costs included. Do not interpret equal history hashes as structural equality.','',
 '```sh','cargo test --bin prefix_witness','cargo build --release --bin prefix_witness','python3 experiments/prefix_witness/run.py','```']
 (OUT/'report.md').write_text('\n'.join(lines)+'\n')
if __name__=='__main__':main()
