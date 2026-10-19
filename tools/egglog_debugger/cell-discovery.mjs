// A report inspector only. It never infers missing matches from the source/graph.
export function discoverySummary(d) {
  if (!d?.contract) return {available:false};
  const counts={}, reasons={}, records=new Set();
  let known=true;
  for (const o of d.outcomes||[]) {
    const category=d.contract.outcomes?.[o.reason]?.category;
    known &&= !!category && Number.isInteger(o.record) && !records.has(o.record);
    records.add(o.record);
    counts[category]=(counts[category]||0)+1;
    reasons[o.reason]=(reasons[o.reason]||0)+1;
  }
  const same=(a,b)=>[...new Set([...Object.keys(a),...Object.keys(b||{})])].every(k=>(a[k]||0)===(b?.[k]||0));
  const complete=known && records.size===d.total_records && records.size===d.accounted_records
    && [...records].every(r=>r>=0&&r<d.total_records) && same(counts,d.counts) && same(reasons,d.by_reason);
  return {available:true,complete,counts,reasons,total:d.total_records,accounted:records.size};
}

const categories={'recognized':'已识别','missing-evidence':'缺少证据','outside-scope':'超出当前范围','incompatible':'条件不成立'};
const readLabels={
  verified:'读取与来源闭包已核对',
  'missing recorded producer':'没有记录到 producer；不能根据相同值补猜来源',
  'producer has no registered apply entry':'producer 没有已登记的 Cell 入口',
  'closure not certified':'来源闭包尚未认证，查看其 ripen 状态',
  'unresolved certified port frame':'来源的 RipenInput 无法唯一对应端口',
  'read row or port mapping not verified':'读取的行、版本或端口映射未通过验证',
};

export function renderDiscovery(host,d,{selectUnit,selectFamily}) {
  host.replaceChildren();
  const node=(tag,text,parent=host)=>{const n=document.createElement(tag);if(text!=null)n.textContent=text;parent.append(n);return n;};
  const summary=discoverySummary(d);
  const status=node('p',summary.available
    ? `${summary.complete?'✓':'核对失败：'} 已解释 ${summary.accounted}/${summary.total} 条已采集记录。`
    : '旧快照没有逐条诊断，请重新 analyze 或回放保存的历史；不能把缺少报告解释为没有重复。');
  status.dataset.role='discovery-accounting';
  if(!summary.available)return;
  node('p',d.contract.scope);
  node('p',d.contract.guarantee);
  node('p','未采集的匹配不在下表保证内。没有端口族，不等于没有重复；请先查看条件和缺失证据。');
  const guide=node('details');node('summary','如何设计 fact · 条件、正例与反例',guide);
  node('p',d.contract.example?.positive,guide);
  node('p',d.contract.example?.negative,guide);
  node('p',d.contract.environment,guide);
  const stages=node('ol',null,guide);
  for(const g of d.contract.gates)node('li',g,stages);
  node('p','没有结果时先检查首个未满足的条件，不自动增加轮次或修改规则。坐标可以随新证据移动；只对当前快照的有限交互成立。',guide);
  const filter=node('select');filter.setAttribute('aria-label','发现诊断筛选');
  for(const [value,label] of [['two','双读取交互'],['all','所有已采集记录'],...Object.entries(categories).map(([k,v])=>[k,`${v} (${summary.counts[k]||0})`])])filter.add(new Option(label,value));
  const tally=node('div');tally.dataset.role='discovery-reasons';
  const list=node('div');list.dataset.role='discovery-records';
  const nav=node('div'),prev=node('button','上一页',nav),pageLabel=node('span','',nav),next=node('button','下一页',nav);
  let page=0,reason=null;
  const size=20; // Display pagination only; no candidate cap or omitted outcomes.
  function button(text,parent,fn){const b=node('button',text,parent);b.onclick=fn;return b;}
  function unitButton(unit,text,parent){if(unit!=null)button(text,parent,()=>selectUnit(unit));}
  function render(){
    list.replaceChildren();tally.replaceChildren();
    for(const [key,count] of Object.entries(summary.reasons).sort((a,b)=>b[1]-a[1]||a[0].localeCompare(b[0]))) {
      const label=d.contract.outcomes[key]?.label||key;
      button(`${label} (${count})`,tally,()=>{reason=key;filter.value='all';page=0;render();});
    }
    if(reason)button('清除原因筛选',tally,()=>{reason=null;page=0;render();});
    const rows=(d.outcomes||[]).filter(o=>(!reason||o.reason===reason)&&(
      filter.value==='all'||filter.value==='two'&&o.read_count===2||d.contract.outcomes[o.reason]?.category===filter.value));
    const pages=Math.max(1,Math.ceil(rows.length/size));page=Math.min(page,pages-1);
    pageLabel.textContent=` 第 ${page+1}/${pages} 页，共 ${rows.length} 条（仅分页显示） `;
    prev.disabled=page===0;next.disabled=page+1===pages;
    if(!rows.length)node('p','本筛选下没有记录。没有双读取交互时，不能从独立 Cell 推断相邻关系。',list);
    for(const o of rows.slice(page*size,(page+1)*size)){
      const result=d.contract.outcomes[o.reason];
      const item=node('details',null,list);item.dataset.discoveryRecord=o.record;
      node('summary',`event ${o.event} · ${o.rule} · ${result?.label||o.reason}`,item);
      if(!result){node('p','未知诊断类型，不能解释为已通过。',item);continue;}
      node('p',result.next_experiment,item);
      const checks=node('ol',null,item);
      d.contract.gates.forEach((g,i)=>node('li',`${i<result.gate?'✓':i===result.gate?'✗':'未检查'} ${g}`,checks));
      for(const read of o.reads||[]){
        node('p',`读取槽 ${read.slot} · fact token ${read.fact_token} · producer record ${read.producer_record??'缺失'}：${readLabels[read.status]||read.status}`,item);
        unitButton(read.source_unit,`查看来源 Cell · unit ${read.source_unit}`,item);
      }
      unitButton(o.target_unit,`查看目标 Cell · unit ${o.target_unit}`,item);
      if(o.family!=null)button(`查看端口族 P${o.family}`,item,()=>selectFamily(o.family));
      const raw=node('details',null,item);node('summary','原始记录引用与条件差异',raw);node('pre',JSON.stringify(o,null,2),raw).style.whiteSpace='pre-wrap';
    }
  }
  filter.onchange=()=>{reason=null;page=0;render();};
  prev.onclick=()=>{page--;render();};next.onclick=()=>{page++;render();};
  render();
}
