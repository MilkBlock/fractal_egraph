// Exact observed normalization coverage, not subgraph containment or match counts.
const key=xs=>JSON.stringify([...new Set(xs||[])].sort((a,b)=>a-b));
export function ripenCoverage(catalog,cs,limit=100){
 if(!catalog||!Array.isArray(cs?.units))return {status:'missing_cs_context',rows:[]};
 const known=new Set(),byMembers=new Map();
 for(const u of cs.units){
  if(u.source_composition!=null||!Number.isInteger(u.coarse_layer))continue;
  known.add(u.coarse_layer);const k=key([...(u.coarse||[]),...(u.smooth||[])]);
  if(!byMembers.has(k))byMembers.set(k,new Set());byMembers.get(k).add(u.coarse_layer);
 }
 const groups=new Map();for(const g of catalog.state_groups||[])groups.set(g.saturated_rule_composition,{state:g.saturated_rule_composition,layers:new Set(),entries:new Set(),triggers:0,attributed:0});
 let unattributed=0;
 for(const t of catalog.triggers||[]){
  const id=t.saturated_rule_composition;if(!Number.isInteger(id))continue;
  if(!groups.has(id))groups.set(id,{state:id,layers:new Set(),entries:new Set(),triggers:0,attributed:0});
  const g=groups.get(id);g.triggers++;
  const members=t.binding_origin?.members;
  const layers=Array.isArray(members)?byMembers.get(key(members)):null;
  if(!layers){unattributed++;continue;}
  g.attributed++;g.entries.add(t.entry||'');for(const id of layers)g.layers.add(id);
 }
 const all=[...groups.values()].map(g=>({state:g.state,layer_ids:[...g.layers].sort((a,b)=>a-b),count:g.layers.size,distinct_entries:g.entries.size,triggers:g.triggers,attributed_triggers:g.attributed})).sort((a,b)=>b.count-a.count||a.state-b.state);
 const verified=new Set(all.flatMap(r=>r.layer_ids)),covered=new Set();
 const rows=all.slice(0,Math.min(100,Math.max(0,limit))).map((r,i)=>{
  for(const id of r.layer_ids)covered.add(id);
  return {...r,rank:i+1,cumulative:covered.size,coverage:known.size?covered.size/known.size:null};
 });
 return {schema:'ripen-cs-coverage/v1',status:'ok',known_layers:known.size,known_layer_ids:[...known].sort((a,b)=>a-b),verified_layers:verified.size,not_verified_layers:known.size-verified.size,total_states:all.length,unattributed_triggers:unattributed,rows,
  definition:'Exact saturation equality for whole CS member sets; deduplicate original coarse_layer IDs across versions and triggers. Combined components do not inherit equivalence. Population includes only recorded base CS descriptors, not every possible layer.'};
}
export function coverageCsv(d){return 'rank,state,distinct_cs_layers,distinct_entry_programs,cumulative_cs_layers,cumulative_fraction,layer_ids\n'+d.rows.map(r=>[r.rank,'C'+r.state,r.count,r.distinct_entries,r.cumulative,r.coverage??'',`"${r.layer_ids.join(' ')}"`].join(',')).join('\n')+'\n';}
export function coverageSvg(d){
 const W=1000,H=350,L=65,R=75,T=50,B=65,pw=W-L-R,ph=H-T-B;
 const rows=d.rows||[],n=Math.max(rows.length,1),max=Math.max(1,...rows.map(r=>r.count)),step=pw/n;
 let s=`<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${W} ${H}" role="img" aria-label="Top 100 ripen 图的已验证 CS layer 覆盖" style="width:100%;background:white;color:#162b3a;font:13px sans-serif"><title>精确饱和共享覆盖：按不同 CS layer 数排名</title><rect width="100%" height="100%" fill="white"/><text x="${L}" y="20" fill="#176da5">蓝柱：每图不同 CS layer 数</text><text x="510" y="20" fill="#b85d08">橙线：累计去重 / 全部已记录 CS layer</text>`;
 for(let i=0;i<=5;i++){const y=T+ph-i*ph/5;s+=`<path d="M${L} ${y}H${W-R}" stroke="#e2e8ee"/><text x="${L-9}" y="${y+4}" text-anchor="end">${(max*i/5).toFixed(max<5?1:0)}</text><text x="${W-R+9}" y="${y+4}">${i*20}%</text>`;}
 const points=[];
 for(const r of rows){const x=L+(r.rank-1)*step,y=T+ph-r.count/max*ph,cx=x+step/2,cy=T+ph-(r.coverage||0)*ph;points.push(`${cx},${cy}`);
 s+=`<g data-state="${r.state}" tabindex="0" role="button" aria-label="C${r.state}，${r.count} 个 CS layer"><title>C${r.state}: ${r.count} layers; ${r.distinct_entries} distinct entries; cumulative ${r.cumulative}/${d.known_layers}</title><rect x="${x+step*.13}" y="${y}" width="${step*.74}" height="${Math.max(1,T+ph-y)}" fill="#2678a9"/>`;
 if(n<=20)s+=`<text x="${cx}" y="${Math.max(T+12,y-7)}" text-anchor="middle">${r.count}</text>`;
 if(n<=20||r.rank===1||r.rank%10===0)s+=`<text x="${cx}" y="${H-B+20}" text-anchor="middle">${r.rank} · C${r.state}</text>`;s+='</g>';
 }
 s+=`<polyline points="${points.join(' ')}" fill="none" stroke="#d77819" stroke-width="3" pointer-events="none"/>`;
 for(const p of points){const[x,y]=p.split(',');s+=`<circle cx="${x}" cy="${y}" r="3" fill="#d77819" pointer-events="none"/>`;}
 s+=`<text x="${W/2}" y="${H-15}" text-anchor="middle">排名 · 实际 ${rows.length} / ${d.total_states||0} 个饱和图（最多 100）</text></svg>`;return s;
}
export function renderCoverage(host,d,onSelect){
 host.replaceChildren();const title=document.createElement('h3');title.textContent='Ripen Top 100 · CS layer 归一化覆盖';host.append(title);
 const note=document.createElement('p');host.append(note);
 if(d.status!=='ok'){note.textContent='缺少同次运行的 CS 描述，不能用 Trigger 数冒充 layer 覆盖率。';return;}
 const last=d.rows.at(-1);note.textContent=`已验证 ${d.verified_layers}/${d.known_layers} 个已记录 CS layer；Top ${d.rows.length} 累计 ${last?.cumulative||0}（${((last?.coverage||0)*100).toFixed(1)}%）。${d.not_verified_layers} 个尚未验证，不表示不等价。`;
 const explanation=document.createElement('p');explanation.textContent='只计历史完整入口的精确饱和共享，不代表最新 layer 版本已饱和或 tier0 已压缩；同一 layer 的历史版本和重复 Trigger 去重。组合包含关系不计为判等，未归属基础 CS layer 的 Trigger：'+d.unattributed_triggers+'。柱上可点击查看对应饱和图。';host.append(explanation);
 const plot=document.createElement('div');plot.innerHTML=coverageSvg(d);host.append(plot);
 for(const g of plot.querySelectorAll('[data-state]')){g.style.cursor='pointer';g.onclick=()=>onSelect(Number(g.dataset.state));g.onkeydown=e=>{if(e.key==='Enter'||e.key===' '){e.preventDefault();g.onclick();}};}
 const controls=document.createElement('div');host.append(controls);
 for(const [label,name,type,body] of [['下载 SVG','ripen-top100.svg','image/svg+xml',coverageSvg(d)],['下载 CSV','ripen-top100.csv','text/csv',coverageCsv(d)],['下载证据 JSON','ripen-top100.json','application/json',JSON.stringify(d,null,2)]]){
  const b=document.createElement('button');b.textContent=label;b.onclick=()=>{const url=URL.createObjectURL(new Blob([body],{type}));const a=document.createElement('a');a.href=url;a.download=name;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);};controls.append(b);
 }
 const details=document.createElement('details');const sum=document.createElement('summary');sum.textContent='逐图查看 layer ID 与不同入口数';details.append(sum);const pre=document.createElement('pre');pre.textContent=JSON.stringify(d.rows,null,2);details.append(pre);host.append(details);
}
