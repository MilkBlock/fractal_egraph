#!/usr/bin/env python3
"""Compare complete saturated constructor graphs, not just extracted root answers."""
import argparse,datetime,hashlib,json,platform,random,re,statistics,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'results/rule_combine'
MODES=['original','observe','static','dynamic']

def main():
 p=argparse.ArgumentParser();p.add_argument('--repeats',type=int,default=3);a=p.parse_args();assert a.repeats>0
 OUT.mkdir(parents=True,exist_ok=True);binary=ROOT/'target/release/rule_combine'
 env={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'platform':platform.platform(),'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'repetitions':a.repeats,'scope':'real local egglog execution with optional runtime shortcut installation'}
 rows=[];catalog={};closures={}
 for case in ['chain','algebra','negative']:
  for count,waves in [(64,1),(256,4)]:
   key=f'{case}/{count}/{waves}'
   for repeat in range(a.repeats):
    modes=MODES.copy();random.Random(773+repeat).shuffle(modes)
    for mode in modes:
     r=subprocess.run(['/usr/bin/time','-l',str(binary),mode,case,str(count),str(waves)],cwd=ROOT,text=True,capture_output=True,check=True,timeout=60)
     x=json.loads(r.stdout);closure=x.pop('closure');candidates=x.pop('candidates')
     # Compare full exact canonical records and root labels before hashing for reports.
     if key in closures:assert closure==closures[key],(key,mode,'closure mismatch')
     else:closures[key]=closure
     x['closure_sha256']=hashlib.sha256(json.dumps(closure,sort_keys=True).encode()).hexdigest();x['constructor_nodes']=len(closure['nodes'])
     if candidates:catalog[case]=candidates
     m=re.search(r'(\d+)\s+maximum resident set size',r.stderr)
     x.update(repeat=repeat,count_per_wave=count,peak_rss_bytes=int(m.group(1)) if m else None)
     rows.append(x)
     print(key,mode,'rounds',x['rounds'],'enabled',len(x['enabled']),'inference_ms',round(x['inference_ns']/1e6,3),'total_ms',round(x['total_ns']/1e6,3),flush=True)
     (OUT/'runs.json').write_text(json.dumps({'environment':env,'runs':rows},indent=2)+'\n')
 summary=[]
 for case,count,waves,mode in sorted({(r['case'],r['count_per_wave'],r['waves'],r['mode']) for r in rows}):
  rr=[r for r in rows if(r['case'],r['count_per_wave'],r['waves'],r['mode'])==(case,count,waves,mode)]
  x={k:statistics.median(r[k] for r in rr) for k in ['rounds','constructor_nodes','retained_bytes','peak_bytes','peak_rss_bytes','preprocess_ns','inference_ns','discovery_ns','install_ns','total_ns','discovery_probes','observed_logical_matches']}
  x.update(case=case,count_per_wave=count,waves=waves,mode=mode,enabled=len(rr[0]['enabled']),total_min_ns=min(r['total_ns'] for r in rr),total_max_ns=max(r['total_ns'] for r in rr));summary.append(x)
 (OUT/'summary.json').write_text(json.dumps({'environment':env,'aggregate':summary,'exact_closure_comparisons':len(rows)-len(closures)},indent=2)+'\n')
 (OUT/'candidates.json').write_text(json.dumps(catalog,indent=2)+'\n')
 lines=['# Online rule composition experiment','','All modes execute the real local egglog engine. Source rules remain installed. Shortcuts are sound two-step compositions of unconditional constructor rewrites; composition is independent of observed trace frequency.','',
 '|case|roots/wave|waves|mode|rounds|enabled|inference ms|discovery ms|total ms|retained KiB|',
 '|---|---:|---:|---|---:|---:|---:|---:|---:|---:|']
 for x in summary:lines.append(f"|{x['case']}|{x['count_per_wave']}|{x['waves']}|{x['mode']}|{x['rounds']}|{x['enabled']}|{x['inference_ns']/1e6:.3f}|{x['discovery_ns']/1e6:.3f}|{x['total_ns']/1e6:.3f}|{x['retained_bytes']/1024:.2f}|")
 lines+=['','## Semantics and controls','',
 '- original: source rules only, no tracing or composition preprocessing. observe: preprocess and run the same bounded discovery, including virtual selection and stopping, but install nothing. static: install every preprocessed candidate before inference. dynamic: install a candidate after two observations, at most eight active shortcuts; stop tracing/discovery when every candidate or the active budget is installed.',
 '- Preprocessing uses the egglog expression parser, variables renamed apart, first-order unification with occurs checks, and explicit RHS overlap paths. Variable-only overlap positions are excluded. Canonical candidate variables preserve sharing across both sides and the intermediate witness. RHS variables must occur in the LHS. Candidate sides are limited to 32 AST nodes and the catalog to 64 entries.',
 '- Runtime discovery starts from Survived logical matches of source rules. It resolves the first RHS subterm in the current canonical graph, then checks the second LHS against that local class with consistent variable bindings. This is evidence of current applicability, not evidence that the first match committed that subterm. Guards, arbitrary actions, subsumption, primitive side effects and mixed-sort rules are not supported.',
 '- For this prototype a constructor-row index is rebuilt at each discovery round. At most 4096 surviving events and 128 distinct candidate/target pairs are probed per round; repeated candidate/target pairs are skipped. Each local match has a 256-visit budget and at most 16 substitutions. A missed discovery is safe because source rules remain available. Observed logical-match counts are only for traced phases, not comparable total matching work across modes.',
 '- chain is an intentionally synthetic eight-step unary chain; algebra combines distribution, multiplication by zero/one, addition with zero, and double negation; negative uses Add of two variables and triggers none of those source rules. Sources arrive in one or four waves; each wave is saturated before the next.',
 '- Every measured run is compared by complete canonical constructor records AND root labels across modes. Class names are least-size ground representatives, so differences in allocated IDs do not invalidate comparison. This covers the supported single E sort and its Var integer payload; not arbitrary user-defined egglog schemas.',
 '- Six tests cover occurs check, nonlinear patterns, chain and nested compositions, every generated candidate via actual first-rule/second-rule execution, and exact closure equality for three workloads across all modes.',
 '- Inference time includes tracing, discovery and dynamic compilation inside the loop. Static compilation is outside inference time but included in total and install time. Total additionally includes source preparation, engine initialization, input generation/parsing/insertion and preprocessing. Memory includes the engine, roots, candidate catalog, counters and activation records. Peak RSS also includes post-measurement closure verification.','',
 '```sh','cargo test --bin rule_combine','cargo build --release --bin rule_combine','python3 experiments/rule_combine/run.py','```']
 (OUT/'report.md').write_text('\n'.join(lines)+'\n')

if __name__=='__main__':main()
