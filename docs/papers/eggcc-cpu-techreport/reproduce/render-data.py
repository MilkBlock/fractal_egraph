from pathlib import Path
import json,math,statistics as st,csv,xml.sax.saxutils as xml
r=Path(__file__).resolve().parents[1]; raw=list(map(json.loads,(r/'suite.jsonl').read_text().splitlines()))
data={}; summary=[]; tasks=[]; archived=[]
for z in raw:data.setdefault(z['case'],{})[z['pass']]=z
names=sorted(data)
def ratio(v):return v['native_ms']/v['cell_ms']
def row(label,a):
 q=[ratio(x) for x in a];return [label,str(len(a)),str(sum(x>1 for x in q)),f'{st.geometric_mean(q):.3f}×',f'{sum(x["native_ms"] for x in a)/sum(x["cell_ms"] for x in a):.3f}×']
for p in (1,2,3):
 a=[d[p] for d in data.values() if p in d and all(k in d[p] for k in ('cell_ms','native_ms'))]
 if a:summary.append(row('Pass '+str(p),a))
valid=[n for n in names if all(p in data[n] and all(k in data[n][p] for k in ('cell_ms','native_ms')) for p in (1,2,3))]
totals={n:{k:sum(data[n][p][k] for p in (1,2,3)) for k in ('native_ms','cell_ms','prod_sched_ms')} for n in valid}
if totals:summary.append(row('Three-pass sum',list(totals.values())))
cap=json.load(open(r/'evidence/manifest.json'))
for c in cap['cases']:
 n=c['source'].split('/benchmarks/passing/')[1];ix=int(c['directory'][:3]);d=data.get(n,{})
 f=lambda v:('—' if v is None else f'{v:,.1f}')
 tasks.append([str(ix),n.replace('linear-algebra','LA'),*[f(d.get(p,{}).get(k)) for p in (1,2,3) for k in ('native_ms','cell_ms')],f'{ratio(totals[n]):.2f}×' if n in totals else '—'])
 archived.append([str(ix),n.replace('linear-algebra','LA'),*[f(d.get(p,{}).get('prod_sched_ms')) for p in (1,2,3)],f(sum(d[p]['prod_extract_ms'] for p in d)),f(sum(d[p]['prod_exec_ms'] for p in d))])
unique=sorted({k for z in raw for k in z})
with open(r/'benchmark.csv','w') as f:
 wr=csv.DictWriter(f,fieldnames=unique,lineterminator="\n");wr.writeheader();wr.writerows(sorted(raw,key=lambda z:(z['case'],z['pass'])))
out={'summary':summary,'tasks':tasks,'archived':archived,'n':len(valid),'total':len(raw)}
if valid:
 s=sorted(valid,key=lambda n:ratio(totals[n]));out['tails']=[[n,f'{ratio(totals[n]):.3f}×',f'{totals[n]["native_ms"]:,.1f}',f'{totals[n]["cell_ms"]:,.1f}'] for n in s[:3]+s[-3:][::-1]]
 out['native_s']=round(sum(x['native_ms'] for x in totals.values())/1000,3);out['cell_s']=round(sum(x['cell_ms'] for x in totals.values())/1000,3)
 out['prep_s']=round(sum(z.get('prepare_s',0) for z in raw),1)
pair=[]
if (r/'paired.json').exists():
 for n in ('032-sum-bits','064-fenwick_tree','095-raytrace'):
  a=[z for z in json.load(open(r/'paired.json')) if z['directory']==n and z['block']]
  if a:
   x,y=[st.median(v[k] for v in a) for k in ['native_ms','cell_ms']]
   pair.append([n[4:],str(len(a)),f'{x:,.1f}',f'{y:,.1f}',f'{x/y:.3f}×',str(sum(z['cell_ms']<z['native_ms'] for z in a))])
production=[]
for p in (1,2,3):
 allrows=[x for x in raw if x['pass']==p]
 both=[x for x in allrows if 'cell_ms'in x and 'native_ms'in x]
 if allrows:production.append(['Pass '+str(p),*[f'{sum(x[k] for x in arr)/1000:,.2f}' for k,arr in [('prod_sched_ms',allrows),('prod_extract_ms',allrows),('native_ms',both),('cell_ms',both)]]])
out['production']=production
out['pairs']=pair
if not valid:out.update(tails=[],native_s=0,cell_s=0,prep_s=0)
(r/'report-data.json').write_text(json.dumps(out,indent=2))
w,h=800,223
svg=[f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect width="100%" height="100%" fill="#f7fafb"/>']
if valid:
 val=[ratio(totals[n]) for n in s];lo=min(-0.5,min(map(math.log2,val)));hi=max(0.5,max(map(math.log2,val)));y=lambda z:170-(math.log2(z)-lo)/(hi-lo)*140
 for t in (0.5,0.75,1,1.25,1.5,2,3):
  if y(t)<12 or y(t)>181:continue
  svg+= [f'<line x1="53" x2="787" y1="{y(t)}" y2="{y(t)}" stroke="{"#586a72" if t==1 else "#dce5e9"}"/><text x="3" y="{y(t)+5}" fill="#26424a" font-size="14" font-family="Helvetica">{t}×</text>']
 for i,v in enumerate(val):
  x=55+730*(i+.5)/len(val);svg.append(f'<line x1="{x}" x2="{x}" y1="{y(1)}" y2="{y(v)}" stroke="{"#187b70" if v>=1 else "#b35737"}" stroke-width="{max(2,660/len(val))}"/>')
 svg.append('<text x="55" y="205" fill="#425f68" font-size="15" font-family="Helvetica">Each bar is one workload, sorted by native / Cell time for three captured schedules. Log scale.</text>')
(r/'figures/suite.svg').write_text(''.join(svg)+'</svg>')
