#!/usr/bin/env python3
"""Read-only, symmetric memory instrumentation of EXPORTED egglog source trees."""
from pathlib import Path
import sys
root=Path(sys.argv[1])
current=sys.argv[2]=='current'
def edit(path,old,new):
 p=root/path;s=p.read_text();assert old in s,(path,old[:80]);p.write_text(s.replace(old,new,1))
def append(path,text):
 p=root/path;p.write_text(p.read_text()+'\n'+text+'\n')
fields='Vec<(&\'static str, usize)>'
shared='Vec<(usize, &\'static str, usize)>'
tuple_ty=f'({fields}, {shared})'
edit('core-relations/src/table_spec.rs','pub trait Table: Any + Send + Sync {',f'pub trait Table: Any + Send + Sync {{\n    fn audit_memory(&self) -> {tuple_ty} {{ (vec![("supported",0)],vec![]) }}\n')
append('core-relations/src/row_buffer/mod.rs','impl RowBuffer { pub(crate) fn audit_bytes(&self) -> usize { self.data.capacity() * std::mem::size_of::<std::cell::Cell<Value>>() } }')
append('core-relations/src/table/sharded_hash_table.rs','impl<T> ShardedHashTable<T> { pub(crate) fn audit_bytes(&self)->usize { self.shards.capacity()*std::mem::size_of::<HashTable<T>>() + self.shards.iter().map(|s|s.allocation_size()).sum::<usize>() } }')
append('union-find/src/lib.rs','impl<V> UnionFind<V> { pub fn audit_bytes(&self)->usize { self.parents.capacity()*std::mem::size_of::<V>() } }')
append('concurrency/src/resettable_oncelock.rs','impl<T> ResettableOnceLock<T> { /// Exclusive access: observes even an invalidated cache without refreshing it.\n pub fn audit_storage(&mut self)->&T { unsafe { &*self.data.get() } } }')
append('core-relations/src/hash_index/mod.rs',r'''
impl SubsetBuffer {
 fn audit_bytes(&self)->usize {
  self.buf.capacity()*std::mem::size_of::<RowId>() + self.free_list.data.iter().map(|v|v.capacity()*std::mem::size_of::<BufferIndex>()).sum::<usize>()
 }
}
impl Index<ColumnIndex> {
 pub(crate) fn audit_bytes(&self)->usize {
  self.key.capacity()*std::mem::size_of::<ColumnId>() + self.table.shards.capacity()*std::mem::size_of::<ColumnIndexShard>()
   + self.table.shards.iter().map(|(_,s)|s.table.allocation_size()+s.subsets.audit_bytes()).sum::<usize>()
 }
}
impl Index<TupleIndex> {
 pub(crate) fn audit_bytes(&self)->usize {
  self.key.capacity()*std::mem::size_of::<ColumnId>() + self.table.shards.capacity()*std::mem::size_of::<TupleIndexShard>()
   + self.table.shards.iter().map(|(_,s)|s.table.hash.allocation_size()+s.table.keys.audit_bytes()+s.subsets.audit_bytes()).sum::<usize>()
 }
}
impl<K:Clone+std::hash::Hash+Eq,I:Clone> IndexCatalog<K,I> {
 pub(crate) fn audit_each(&mut self, mut f: impl FnMut(&mut I)) -> usize {
  let data=self.data.as_mut_ref(); let bytes=data.capacity()*std::mem::size_of::<(K,I)>();
  for (_,v) in data { f(v); } bytes
 }
}
''')
virtual=r'''
 if let Some(s)=self.virtual_stats() {
  m.push(("virtual_descriptors_bytes",s.descriptor_bytes));
  m.push(("virtual_direct_bytes",s.direct_bytes - self.data.data.len()*self.n_columns*std::mem::size_of::<Value>()));
  m.push(("virtual_metadata_bytes",s.metadata_bytes));
  m.push(("virtual_headers_bytes",s.local_header_bytes));
  m.push(("virtual_rows",s.virtual_rows));m.push(("direct_base_rows",s.direct_base_rows));
  let bh=s.binding_headers.into_iter().collect::<std::collections::BTreeMap<_,_>>();
  let th=s.template_headers.into_iter().collect::<std::collections::BTreeMap<_,_>>();
  for (id,b) in s.instances {shared.push((id,"bindings",b+bh.get(&id).copied().unwrap_or(0)));}
  for (id,b) in s.templates {shared.push((id,"templates",b+th.get(&id).copied().unwrap_or(0)));}
 }
''' if current else ''
physical='self.physical_len()' if current else 'self.data.data.len()'
stale='self.stale_count()' if current else 'self.data.stale_rows'
method=f'''
 fn audit_memory(&self) -> {tuple_ty} {{
  let mut m=vec![("supported",1),("physical_rows",{physical}),("stale_rows",{stale}),("physical_columns",self.n_columns),
   ("row_buffer_bytes",self.data.data.audit_bytes()),("hashcons_bytes",self.hash.audit_bytes()),
   ("offsets_bytes",self.offsets.capacity()*std::mem::size_of::<(Value,RowId)>()),
   ("rebuild_columns_bytes",self.to_rebuild.capacity()*std::mem::size_of::<ColumnId>()),
   ("rebuild_index_bytes",self.rebuild_index.audit_bytes()),("table_struct_bytes",std::mem::size_of::<Self>())];
  let mut shared=vec![];
  {virtual}
  (m,shared)
 }}
'''
edit('core-relations/src/table/mod.rs','impl Table for SortedWritesTable {','impl Table for SortedWritesTable {'+method)
edit('core-relations/src/uf/mod.rs','impl Table for DisplacedTable {',f'''impl Table for DisplacedTable {{
 fn audit_memory(&self)->{tuple_ty} {{
  (vec![("supported",1),("uf_parents_bytes",self.uf.audit_bytes()),("displaced_rows_bytes",self.displaced.capacity()*std::mem::size_of::<(Value,Value)>()),("uf_lookup_bytes",self.lookup_table.allocation_size()),("table_struct_bytes",std::mem::size_of::<Self>())],vec![])
 }}
''')
append('core-relations/src/free_join/mod.rs',f'''
impl Database {{
 pub fn audit_tables(&mut self)->Vec<(String,{fields},{shared})> {{
  let mut result=vec![];
  for (id,t) in self.tables.iter_mut() {{
   let (mut m,shared)=t.table.audit_memory();m.push(("live_rows",t.table.len()));
   let mut skipped=0;let mut column=0;let mut tuple=0;
   let cc=t.column_indexes.audit_each(|v| {{
    if let Some(v)=Arc::get_mut(v) {{column+=std::mem::size_of_val(v)+2*std::mem::size_of::<usize>()+v.audit_storage().audit_bytes();}}
    else {{skipped+=1;}}
   }});
   let tc=t.indexes.audit_each(|v| {{
    if let Some(v)=Arc::get_mut(v) {{tuple+=std::mem::size_of_val(v)+2*std::mem::size_of::<usize>()+v.audit_storage().audit_bytes();}}
    else {{skipped+=1;}}
   }});
   m.push(("column_indexes_bytes",column));m.push(("tuple_indexes_bytes",tuple));m.push(("index_catalog_bytes",cc+tc));m.push(("uninspected_shared_indexes",skipped));
   result.push((t.name.as_ref().map(|s|s.to_string()).unwrap_or_else(||format!("internal:{{id:?}}")),m,shared));
  }} result
 }}
}}
''')
append('egglog-bridge/src/lib.rs',f'impl EGraph {{ pub fn audit_tables(&mut self)->Vec<(String,{fields},{shared})> {{ self.db.audit_tables() }} }}')
append('src/lib.rs',f'impl EGraph {{ pub fn audit_tables(&mut self)->Vec<(String,{fields},{shared})> {{ self.backend.audit_tables() }} }}')
