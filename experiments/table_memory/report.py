#!/usr/bin/env python3
import argparse,csv,json
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--input',required=True);p.add_argument('--output',required=True);a=p.parse_args()
src=Path(a.input);out=Path(a.output);out.mkdir(parents=True,exist_ok=True)
raw={k:json.loads((src/f'{k}.json').read_text()) for k in ['pristine','full','r6']}
assert raw['pristine']['before']==raw['pristine']['after'],'observer changed pristine table accounting'
snaps={'pristine':raw['pristine']['after'],'current_native':raw['full']['before'],'current_round1':raw['full']['after'],'current_rich_native':raw['r6']['before'],'current_rich':raw['r6']['after']}
by={k:{t['name']:t for t in s['tables']} for k,s in snaps.items()}
for k,s in snaps.items():
 assert (s['math_enodes'],s['math_eclasses'])==(1047896,443832)
 assert not s['unmapped_functions']
 assert set(by[k])==set(by['pristine'])
 for t in s['tables']:
  assert t['metrics']['supported']==1 and t['metrics']['uninspected_shared_indexes']==0
  if t['schema'] and t['schema']['output_sort']=='Math':assert t['schema']['visible_rows']==by['pristine'][t['name']]['schema']['visible_rows']
rows=[]
for variant,s in snaps.items():
 for t in s['tables']:
  rows.append({'variant':variant,'table':t['name'],'visible_rows':t['schema']['visible_rows'] if t['schema'] else '', 'logical_value_bytes':t['schema']['visible_rows']*(len(t['schema']['input_sorts'])+1)*4 if t['schema'] else '',**t['metrics'],'shared_referenced_bytes':t['shared_referenced_bytes'],'local_known_bytes':t['local_known_bytes'],'inclusive_known_bytes':t['inclusive_known_bytes']})
cols=['variant','table','visible_rows']+sorted(set().union(*(r.keys() for r in rows))-{'variant','table','visible_rows'})
with (out/'tables.csv').open('w') as f:
 w=csv.DictWriter(f,fieldnames=cols,lineterminator="\n");w.writeheader();w.writerows(rows)
(out/'tables.json').write_text(json.dumps({'snapshots':snaps,'packing':{k:raw[k]['packing'] for k in ['full','r6']}},indent=2)+'\n')
order=sorted(by['pristine'],key=lambda n:by['pristine'][n]['inclusive_known_bytes'],reverse=True)
lines=['# 每张表的已计存储对比','',
'同一原始 11 轮 Math：**1,047,896 个可见 Math e-node / 443,832 个 e-class**。单位 MiB。',
'数值是直接读取的数据结构分配容量，**不是每表 RSS，也不是整个进程的完整 heap**。',
'本表的“含共享引用”将该表引用的 binding/template 块完整列入；这些块可能跨表共享，不能直接把各表相加。总计已做跨表去重。','',
'| 表 | 可见 Math 行数 | pristine | 当前未 pack | round-1 pack | 丰富库 pack |',
'|---|---:|---:|---:|---:|---:|']
for n in order:
 t=by['pristine'][n];visible=t['schema']['visible_rows'] if t['schema'] else '内部表'
 v=[by[k][n]['inclusive_known_bytes']/2**20 for k in ['pristine','current_native','current_round1','current_rich']]
 lines.append(f'| `{n}` | {visible:,} | '+ ' | '.join(f'{x:.3f}' for x in v)+' |' if isinstance(visible,int) else f'| `{n}` | {visible} | '+' | '.join(f'{x:.3f}' for x in v)+' |')
lines+=['| **去重后合计** | — | '+' | '.join(f"{snaps[k]['total_known_unique_bytes']/2**20:.3f}" for k in ['pristine','current_native','current_round1','current_rich'])+' |','',
'## 同进程 pack 前后','',
'不同原生运行的临时 ID、stale 行和哈希容量会波动。因此 pack 的归因用各自同一进程前后差，不能只用两次运行的绝对差。','',
'| 库 | pack 前 | pack 后 | 差值 |','|---|---:|---:|---:|']
for tag,before,after in [('round-1','current_native','current_round1'),('丰富库','current_rich_native','current_rich')]:
 b=snaps[before]['total_known_unique_bytes']/2**20;v=snaps[after]['total_known_unique_bytes']/2**20
 lines.append(f'| {tag} | {b:.3f} | {v:.3f} | {v-b:+.3f} |')
lines+=['','## Add / Mul 分项（含共享引用）','','| 表/分项 | pristine | 当前未 pack | round-1 pack | 丰富库 pack |','|---|---:|---:|---:|---:|']
for n in ['Add','Mul']:
 for label,keys in [('行存储（含虚拟描述、页头和共享引用）',['row_buffer_bytes','virtual_descriptors_bytes','virtual_direct_bytes','virtual_metadata_bytes','virtual_headers_bytes']),('hashcons',['hashcons_bytes']),('rebuild 索引',['rebuild_index_bytes']),('列索引',['column_indexes_bytes'])]:
  vs=[]
  for k in ['pristine','current_native','current_round1','current_rich']:
   t=by[k][n];v=sum(t['metrics'].get(f,0) for f in keys)+(t['shared_referenced_bytes'] if label.startswith('行存储') else 0);vs.append(v/2**20)
  lines.append(f'| {n} / {label} | '+' | '.join(f'{v:.3f}' for v in vs)+' |')
lines+=['','## 口径与复现','',
'- CSV 的 logical_value_bytes 是可见行的 Value 槽字节（两份源码的 Value 都是 u32），不包括时间戳等内部列，也不包括全局字符串/primitive 内容。',
'- 包含：row buffer 的容量、hashcons 哈希分配（含桶与控制字节）、offset 向量、rebuild/列/元组索引及其 row-ID 缓冲容量、表与索引头、虚拟描述与直接页、共享 binding/template。',
'- 所有表的观测接口均受支持；所有持久索引都成功检查，未跳过任何共享索引句柄。对失效缓存也只读其已分配数据，不刷新索引。',
'- 不包含：分配器 rounding/保留页、线程局部池、临时 Join 索引、规则/查询计划、全局 primitive/symbol interner、pending/provenance 辅助分配与动态 merge 闭包。不能把这份合计当作 RSS。',
'- `$uf` 是 union-find 内部表，不是 Math eclass 数。它的 displaced 行数可随并行执行变化；父数组、lookup hash 和 displaced 向量已计入。',
'- round-1 与丰富库额外共享池在 JSON 中单列，合计只计一次；CSV 提供每张表的本地成本和共享引用成本，避免重复计费。',
'- pristine 是不可变标签导出的源码，当前版是工作树源码副本；两者都只加了观测方法。没有修改原标签或原始 checkout，也没有改变推理、查询、存储算法。',
'- instrumentation 的源码与编译器、快照标识记录在 provenance.json；原始观测在 tables.json，所有字节分项在 tables.csv。','',
'实际看到：Add 的行存储变小，但查询新增的列索引抵消了一部分；Mul 的新增索引超过了行存储节省。表内长期驻留成本略降，不代表全图搜索的临时峰值或进程 RSS 也下降。','']
(out/'README.md').write_text('\n'.join(lines))
