#!/usr/bin/env python3
"""Native/adaptive/frozen actual tier0 runs. Equal rounds are not equal proof work."""
import argparse,json,sys,statistics,hashlib
from pathlib import Path
from math_virtual import ROOT,measure

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--output',required=True);p.add_argument('--runs',type=int,default=3);p.add_argument('--full',action='store_true');a=p.parse_args()
    out=(ROOT/a.output).resolve();out.mkdir(parents=True,exist_ok=False)
    binary=ROOT/'target/release/examples/adaptive_math';source=ROOT/'experiments/math_mini3/source.egg'
    report={'scope':'native positive Math rewrite runtime; sampling/serialization included in process wall and RSS. Equal rounds NOT equal inferred work. Frozen library learned only from rounds 1-2.', 'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),'runs':[],'full':[]}
    lib=None
    for i in range(a.runs):
        for mode in ['native','adaptive','frozen','warm']:
            dest=out/f'{mode}-{i}';cmd=[binary,'adaptive' if mode=='warm' else mode,source,dest]
            if mode in ('frozen','warm'):cmd+=[lib]
            sample=measure(cmd,out/f'{mode}-{i}-process');sample['mode']=mode;sample['repetition']=i;report['runs'].append(sample)
            if mode=='adaptive' and lib is None:lib=dest/'bake_lib_round2.json'
            (out/'measurements.json').write_text(json.dumps(report,indent=2)+'\n')
    report['library_bytes']=lib.stat().st_size;report['library']=str(lib)
    report['medians']={m:{k:statistics.median(r[k] for r in report['runs'] if r['mode']==m) for k in ['process_wall_seconds','peak_rss_bytes']} for m in ['native','adaptive','frozen','warm']}
    if a.full:
        for mode in ['native','adaptive','frozen','warm']:
            dest=out/f'full-{mode}';cmd=[binary,'adaptive' if mode=='warm' else mode,ROOT/'egglog/tests/math-microbenchmark.egg',dest]
            if mode in ('frozen','warm'):cmd+=[lib]
            try:sample=measure(cmd,out/f'full-{mode}-process')
            except RuntimeError as e:sample={'external_guard_stop':str(e),'progress':json.loads((dest/'progress.json').read_text()) if (dest/'progress.json').exists() else []}
            sample['mode']=mode;report['full'].append(sample)
            (out/'measurements.json').write_text(json.dumps(report,indent=2)+'\n')
    (out/'measurements.json').write_text(json.dumps(report,indent=2)+'\n')
    data=[]
    for r in report['runs']:
        if r['repetition']==0:
            for s in r['result']['samples']:data.append(dict(s,mode=r['mode']))
    full=[]
    for r in report['full']:
        for s in r.get('result',{}).get('samples',r.get('progress',[])):full.append(dict(s,mode=r['mode']))
    html='''<!doctype html><meta charset="utf-8"><title>Adaptive RipSat runtime</title><style>body{font:16px system-ui;max-width:1100px;margin:32px auto;color:#203047}svg{width:100%;background:#f4f7fb}section{margin:25px 0}td,th{padding:7px;border-bottom:1px solid #ddd;text-align:right}table{width:100%;border-collapse:collapse}</style><h1>RipSat：原生、在线学习、加载 round-2 库</h1><p>蓝色 native · 橙色 adaptive · 绿色 frozen · 紫色 warm（加载后继续学习）。相同轮数不代表相同推理量；宏会提前产生多步结果。</p><p id="summary"></p><select id="workload"><option value="mini">mini3</option><option value="full">完整 11 轮</option></select><label><input type="checkbox" id="log" checked>对数横轴 log₁₀(1+match)</label><div id="charts"></div><div id="table"></div><p>RSS 为每轮当前进程内存，包含诊断扫描影响。图用第一次运行；进程统计中位数见 measurements.json。未完成运行仅展示已提交采样点。</p><script>const mini=MINI,full=FULL,report=REPORT;
summary.textContent=`round-2 库 ${report.library_bytes} bytes。11 轮状态：`+report.full.map(r=>r.mode+': '+(r.external_guard_stop?'外部保护停止':'完成')).join('；');
function draw(){const rows=workload.value==='mini'?mini:full;charts.innerHTML='';if(!rows.length){charts.textContent='未运行此工作负载';return;}for(const [key,title,div] of [['rss_bytes','RSS MiB',1048576],['enodes','逻辑 e-node',1],['eclasses','canonical e-class',1],['library_bytes','库 KiB',1024],['virtual_rows','虚拟化唯一行',1]]){let W=1000,H=280,L=85,R=20,T=20,B=45;const val=r=>key==='library_bytes'?r[key]:key==='virtual_rows'?r.packing.virtual_rows:r.graph[key],tx=x=>log.checked?Math.log10(1+x):x;let xm=Math.max(...rows.map(r=>tx(r.cumulative_matches)))||1,ym=Math.max(...rows.map(r=>val(r)/div))*1.05||1;const x=r=>L+tx(r.cumulative_matches)/xm*(W-L-R),y=r=>H-B-val(r)/div/ym*(H-T-B);let s=`<section><h2>${title}</h2><svg viewBox="0 0 ${W} ${H}">`;for(let i=0;i<=4;i++){let yy=H-B-i/4*(H-T-B),xx=L+i/4*(W-L-R);s+=`<path d="M${L},${yy}H${W-R}" stroke="#d9e0e9"/><text x="${L-8}" y="${yy+4}" text-anchor="end" font-size="12">${(ym*i/4).toFixed(div===1?0:1)}</text><text x="${xx}" y="${H-23}" text-anchor="middle" font-size="12">${Math.round(log.checked?10**(xm*i/4)-1:xm*i/4)}</text>`;}for(const [m,c] of [['native','#2464c4'],['adaptive','#dc7614'],['frozen','#178368'],['warm','#904fb6']]){const pts=rows.filter(r=>r.mode===m);s+=`<polyline points="${pts.map(r=>`${x(r)},${y(r)}`).join(' ')}" stroke="${c}" fill="none" stroke-width="2"/>`;for(const r of pts)s+=`<circle cx="${x(r)}" cy="${y(r)}" r="4" fill="${c}"><title>${m} round ${r.round}, match ${r.cumulative_matches}, ${title}: ${val(r)/div}</title></circle>`;}s+=`<text x="500" y="275" text-anchor="middle">累计 native match（包括宏触发）</text></svg></section>`;charts.innerHTML+=s;}table.innerHTML='<table><tr><th>模式/轮次</th><th>source match</th><th>macro match</th><th>e-node</th><th>e-class</th><th>库主体数</th><th>虚拟行</th></tr>'+rows.map(r=>`<tr><td>${r.mode}/${r.round}</td><td>${r.source_matches}</td><td>${r.macro_matches}</td><td>${r.graph.enodes}</td><td>${r.graph.eclasses}</td><td>${r.bodies}</td><td>${r.packing.virtual_rows}</td></tr>`).join('')+'</table>';}
workload.onchange=draw;log.onchange=draw;draw();</script>'''
    for k,v in [('MINI',data),('FULL',full),('REPORT',{'library_bytes':report['library_bytes'],'full':[{'mode':r['mode'],'external_guard_stop':r.get('external_guard_stop')} for r in report['full']]})]:html=html.replace(k,json.dumps(v).replace('<','\\u003c'))
    (out/'index.html').write_text(html)
    print(json.dumps({'medians':report['medians'],'library_bytes':report['library_bytes'],'output':str(out)},indent=2))
if __name__=='__main__':main()
