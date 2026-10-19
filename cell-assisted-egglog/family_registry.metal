// Sparse row -> list of observed (row, projection) instances. Metadata is
// [anchor row, projection, next]; slots are stable while live and recycled only
// between kernels. Revision counters survive recycling (no ABA certificates).
// registry: high-water, free count, need, tickets, live, retired,
//           new watches, new ready ports, surviving watches, surviving ports.
bool anchor_matches(device const uint* code,device uint* rows,uint cp,uint row){
 if(!R(row,1))return false;uint m=CELL_META+cp*6,q=code[m],a=code[m+1];
 uint v[S],lo[S],hi[S],b[S];for(uint i=0;i<S;i++)v[i]=lo[i]=hi[i]=b[i]=0;
 return bind_atom(code,rows,q,a,row,v,lo,hi,b);
}
kernel void family_cell_retire_count(CTX,uint lane [[thread_position_in_grid]]){
 if(lane>=cfg[2])return;uint row=packet[4+4*cfg[3]+lane*(1+RS)],prev=NO;
 uint id=rowCells[row];
 while(id!=NO){
  uint next=instanceMeta[id*3+2],cp=instanceMeta[id*3+1],base=id*CS;
  if(!anchor_matches(code,rows,cp,row)){
   if(prev==NO)rowCells[row]=next;else instanceMeta[prev*3+2]=next;
   if(cells[base+2]==NO || cells[base+6]==NO){atomic_fetch_or_explicit(counters+1,4,memory_order_relaxed);return;}
   cells[base]=0;cells[base+2]++;cells[base+6]++;instanceMeta[id*3]=NO;
   uint f=atomic_fetch_add_explicit(registry+1,1,memory_order_relaxed);freeCells[f]=id;
   atomic_fetch_sub_explicit(registry+4,1,memory_order_relaxed);atomic_fetch_add_explicit(registry+5,1,memory_order_relaxed);
  }else{if(CELL_EVENTS)dirty_cell(cells,dirty,state,cfg,id,0);prev=id;}
  id=next;
 }
 if(!R(row,1))return;uint table=R(row,0);
 for(uint a=code[ANCHOR_OFFSETS+table];a<code[ANCHOR_OFFSETS+table+1];a++){
  uint cp=code[ANCHOR_PLANS+a];
  if(cell_lookup(rowCells,instanceMeta,row,cp)==NO && anchor_matches(code,rows,cp,row))atomic_fetch_add_explicit(registry+2,1,memory_order_relaxed);
 }
}
kernel void family_cell_allocate(CTX,uint lane [[thread_position_in_grid]]){
 if(lane>=cfg[2])return;uint row=packet[4+4*cfg[3]+lane*(1+RS)];if(!R(row,1))return;
 uint table=R(row,0),oldSize=atomic_load_explicit(registry,memory_order_relaxed),freeCount=atomic_load_explicit(registry+1,memory_order_relaxed);
 for(uint a=code[ANCHOR_OFFSETS+table];a<code[ANCHOR_OFFSETS+table+1];a++){
  uint cp=code[ANCHOR_PLANS+a];if(cell_lookup(rowCells,instanceMeta,row,cp)!=NO || !anchor_matches(code,rows,cp,row))continue;
  uint ticket=atomic_fetch_add_explicit(registry+3,1,memory_order_relaxed),id=ticket<freeCount?freeCells[freeCount-1-ticket]:oldSize+ticket-freeCount;
  if(id>=cfg[22]){atomic_fetch_or_explicit(counters+1,32,memory_order_relaxed);return;}
  uint base=id*CS,revision=cells[base+2],portRevision=cells[base+6],queued=cells[base+3];
  if(revision==NO || portRevision==NO){atomic_fetch_or_explicit(counters+1,4,memory_order_relaxed);return;}
  for(uint j=0;j<CS;j++)cells[base+j]=0;
  cells[base+2]=revision+1;cells[base+6]=portRevision+1;cells[base+3]=queued;
  instanceMeta[id*3]=row;instanceMeta[id*3+1]=cp;instanceMeta[id*3+2]=rowCells[row];rowCells[row]=id;
  if(CELL_EVENTS)dirty_cell(cells,dirty,state,cfg,id,0);
 }
}
kernel void family_registry_finish(CTX,uint lane [[thread_position_in_grid]]){
 if(lane)return;device uint* r=(device uint*)registry;
 if(r[2]!=r[3]){atomic_fetch_or_explicit(counters+1,32,memory_order_relaxed);return;}
 uint freeCount=r[1],needed=r[2];r[0]+=needed>freeCount?needed-freeCount:0;r[1]=freeCount>needed?freeCount-needed:0;r[4]+=needed;
 if(r[0]!=r[1]+r[4])atomic_fetch_or_explicit(counters+1,32,memory_order_relaxed);
}
