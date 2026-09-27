#!/usr/bin/env python3
"""Native round-boundary growth curves, no full match history. macOS/Linux ps RSS."""
import argparse, csv, hashlib, json, subprocess, sys
from pathlib import Path
from math_virtual import measure, library_inventory
ROOT=Path(__file__).resolve().parents[2]
def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',default='egglog/tests/math-microbenchmark.egg')
    p.add_argument('--library',required=True)
    p.add_argument('--output',required=True)
    a=p.parse_args(); out=(ROOT/a.output).resolve();out.mkdir(parents=True,exist_ok=False)
    src=(ROOT/a.source).resolve();lib=(ROOT/a.library).resolve()
    rows=[];runs={}
    for mode in ['native','virtual']:
        runs[mode]=measure([ROOT/'target/release/examples/math_growth',src,mode,lib],out/mode)
        points=[json.loads(s) for s in (out/f'{mode}.stdout').read_text().splitlines() if s.startswith('{')]
        rows.extend(points)
    n=[r for r in rows if r['mode']=='native'];v=[r for r in rows if r['mode']=='virtual']
    assert [(r['round'],r['cumulative_matches'],r['expanded_enodes'],r['expanded_eclasses']) for r in n]==[(r['round'],r['cumulative_matches'],r['expanded_enodes'],r['expanded_eclasses']) for r in v], 'growth mismatch'
    report=dict(source=str(src),source_sha256=hashlib.sha256(src.read_bytes()).hexdigest(),
        library=library_inventory(lib),runs=runs,samples=rows,
        notes=['Round-boundary samples, not per-match snapshots. Lines interpolate sampled points.',
               'Matches are native RunReport match counts; not distinct matches or committed writes.',
               'RSS sampled by ps before each graph scan; includes library and diagnostics. Not peak RSS.',
               'Expanded enodes are unique live native Math constructor rows including subsumed; eclasses are canonical Math result IDs, excluding primitives.',
               'Virtual mode packs existing rows after round 1 only, then runs normally. Offline Bake excluded.',
               'Matching counts and graph counts agree across modes; count agreement alone is not graph isomorphism.'])
    (out/'growth.json').write_text(json.dumps(report,indent=2)+'\n')
    with (out/'growth.csv').open('w') as f:
        w=csv.DictWriter(f,fieldnames=list(rows[0]));w.writeheader();w.writerows(rows)
    html='''<!doctype html><meta charset="utf-8"><title>Math microbenchmark growth</title>
<style>body{font:16px system-ui;max-width:1150px;margin:32px auto;color:#17263a}svg{width:100%;background:#f7f9fc;border-radius:12px}section{margin:25px 0}table{border-collapse:collapse;width:100%}td,th{padding:7px;border-bottom:1px solid #ddd;text-align:right}small{color:#526174}label{display:block;margin:12px}</style>
<h1>Math microbenchmark：随 match 增长</h1><p>蓝色：原生 egglog　橙色：虚拟事实存储。每轮结束采样；点间连线不代表逐 match 测量。</p>
<p id="info"></p><label><input id="log" type="checkbox">横轴使用 log₁₀(1 + 累计 match)，观察早期轮次</label><div id="charts"></div><h2>每轮实测</h2><div id="table"></div><h2>口径</h2><div id="notes"></div>
<script>const data=PAYLOAD;
info.textContent=`库 ${data.library.file_bytes.toLocaleString()} bytes · ${data.library.bodies} bodies · ${data.library.enclosures} enclosures · ${data.library.entries} entries。只在第 1 轮后转换已有事实。`;
notes.innerHTML=data.notes.map(x=>`<p><small>${x}</small></p>`).join('');
const fields=[['rss_bytes','当前 RSS（MiB）',1048576],['expanded_enodes','展开后去重 e-node',1],['expanded_eclasses','展开后 canonical e-class',1]];
function draw(){charts.innerHTML='';for(const [key,title,div] of fields){let W=1050,H=300,L=85,R=25,T=25,B=50;const tx=x=>log.checked?Math.log10(1+x):x;
let xmax=Math.max(...data.samples.map(r=>tx(r.cumulative_matches)))||1,ymax=Math.max(...data.samples.map(r=>r[key]/div))*1.08||1;
const x=r=>L+tx(r.cumulative_matches)/xmax*(W-L-R),y=r=>H-B-r[key]/div/ymax*(H-T-B);
let s=`<section><h2>${title}</h2><svg viewBox="0 0 ${W} ${H}" role="img" aria-label="${title}">`;
for(let i=0;i<=4;i++){let yy=H-B-i/4*(H-T-B),xx=L+i/4*(W-L-R);s+=`<path d="M${L},${yy}H${W-R}" stroke="#dce2eb"/><text x="${L-10}" y="${yy+4}" text-anchor="end" font-size="12">${(ymax*i/4).toFixed(div===1?0:1)}</text><text x="${xx}" y="${H-28}" text-anchor="middle" font-size="12">${Math.round(log.checked?10**(xmax*i/4)-1:xmax*i/4).toLocaleString()}</text>`;}
for(const [mode,color] of [['native','#2364c5'],['virtual','#e17a14']]){let pts=data.samples.filter(r=>r.mode===mode);s+=`<polyline points="${pts.map(r=>`${x(r)},${y(r)}`).join(' ')}" fill="none" stroke="${color}" stroke-width="2" ${mode==='virtual'?'stroke-dasharray="5 4"':''}/>`;for(const r of pts)s+=`<circle cx="${x(r)}" cy="${y(r)}" r="4" fill="${color}"><title>${mode}, round ${r.round}, matches ${r.cumulative_matches}, ${title}: ${(r[key]/div).toFixed(div===1?0:2)}</title></circle>`;}
s+=`<text x="${W/2}" y="${H-6}" text-anchor="middle" font-size="13">累计 native match 数</text></svg></section>`;charts.innerHTML+=s;}}
log.onchange=draw;draw();
table.innerHTML='<table><tr><th>模式</th><th>轮次</th><th>累计 match</th><th>RSS MiB</th><th>e-node</th><th>e-class</th><th>虚拟行</th></tr>'+data.samples.map(r=>`<tr><td>${r.mode}</td><td>${r.round}</td><td>${r.cumulative_matches}</td><td>${(r.rss_bytes/1048576).toFixed(2)}</td><td>${r.expanded_enodes}</td><td>${r.expanded_eclasses}</td><td>${r.virtual_rows}</td></tr>`).join('')+'</table>';
</script>'''
    (out/'index.html').write_text(html.replace('PAYLOAD',json.dumps(report).replace('<','\\u003c')))
    print(json.dumps({'output':str(out),'final': [n[-1],v[-1]],'library_bytes':report['library']['file_bytes']},indent=2))
if __name__=='__main__': main()
