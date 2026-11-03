const $=id=>document.getElementById(id);
const lists=['include','exclude','root_tables'];
const numbers=['min_alternatives','min_parents','min_facts','max_facts','min_classes','max_classes','min_ports','max_ports'];
const toggles=['repeated_arg','shared_ports'];
const control=k=>$('filter-'+k.replaceAll('_','-'));
export function readFilters(){
  const f={change:control('change').value,include_mode:control('include_mode').value};
  for(const k of lists)f[k]=[...new Set(control(k).value.trim().split(/[,，\s]+/).filter(Boolean))].sort();
  for(const k of numbers){
    const n=control(k);if(!n.checkValidity())throw Error('筛选数量必须是非负整数');
    f[k]=n.value===''?null:Number(n.value);
  }
  for(const k of toggles)f[k]=control(k).checked;
  for(const k of ['facts','classes','ports'])if(f['min_'+k]!=null&&f['max_'+k]!=null&&f['min_'+k]>f['max_'+k])throw Error('数量范围的下限不能超过上限');
  return f;
}
export function writeFilters(f={}){
  control('change').value=f.change||'any';control('include_mode').value=f.include_mode||'all';
  for(const k of lists)control(k).value=Array.isArray(f[k])?f[k].join(', '):f[k]||'';
  for(const k of numbers)control(k).value=f[k]??'';
  for(const k of toggles)control(k).checked=!!f[k];
}
export const presets={changes:{change:'changed'},equivalents:{min_alternatives:2,min_parents:2},boundary:{change:'changed',shared_ports:true}};
export function setTables(names){$('table-names').replaceChildren(...names.map(n=>new Option(n,n)));}
function text(tag,value,cls){const n=document.createElement(tag);n.textContent=value;if(cls)n.className=cls;return n;}
export function renderFilterReport(report,focusItem){
  const host=$('filter-report-body');host.replaceChildren();
  if(!report){$('filter-summary').textContent='尚未筛选';$('filter-summary').classList.remove('filter-warning');return;}
  const m=report.selected;
  const mismatch=m&&(!report.selected_eligible||report.selected_failures.length);
  $('filter-summary').textContent=`符合条件 ${report.eligible_roots.toLocaleString()} / ${report.base_roots.toLocaleString()} 个位置${mismatch?' · 当前图在筛选外':''} · 查看筛选依据`;
  $('filter-summary').classList.toggle('filter-warning',!!mismatch||report.eligible_roots===0);
  host.append(text('p',report.scope,'muted'));
  const steps=document.createElement('ol');
  steps.append(text('li',`Stage、sort 与采样位置：${report.base_roots} 个`));
  for(const step of report.steps)steps.append(text('li',`${step.label} → ${step.remaining} 个`,step.remaining===0?'filter-warning':''));
  host.append(steps);
  if(!m)return;
  if(!report.selected_eligible)host.append(text('p','固定根不属于当前抽样池；保留位置供前后对比，不自动换根。','filter-warning'));
  if(report.selected_failures.length)host.append(text('p','当前图未满足：'+report.selected_failures.join('；'),'filter-warning'));
  if(report.manual_view)host.append(text('p','已手动展开／折叠。以下是当前图的指标；候选计数仍按基础裁剪设置计算。','muted'));
  host.append(text('p',`当前图：${m.facts} 条事实 · ${m.classes} 个展开 eclass · ${m.ports} 个 RipenInput`));
  host.append(text('p','可见类型：'+(m.tables.join(', ')||'无')));
  const link=(label,key)=>{const b=text('button',label,'evidence-link');b.onclick=()=>focusItem(key);return b;};
  const witnesses=document.createElement('div');witnesses.className='filter-witnesses';
  for(const x of m.alternatives.filter(x=>x.enodes>1))witnesses.append(link(`e${x.class}：${x.enodes} 个等价 enode（展示 ${x.shown}）`,'c'+x.class));
  for(const x of m.sharing)witnesses.append(link(`${x.port?'RipenInput':'e'}${x.class}：${x.parents} 条事实 / ${x.references} 个参数位置引用`,'c'+x.class));
  for(const x of m.repeated_args)witnesses.append(link(`重复参数 ${x.positions.join(', ')} → e${x.class}`,x.row));
  for(const v of m.unions)witnesses.append(link(`union 涉及 e${v}`,'c'+v));
  host.append(witnesses);
}
