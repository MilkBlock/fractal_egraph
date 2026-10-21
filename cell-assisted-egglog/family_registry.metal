// Sparse row -> list of observed (row, projection) instances. Metadata is
// [anchor row, projection, next]; slots are stable while live and recycled only
// between kernels. Revision counters survive recycling (no ABA certificates).
// registry: high-water, free count, need, tickets, live, retired,
//           new watches, new ready ports, surviving watches, surviving ports.
bool anchor_matches(device const uint* code,device uint* rows,uint cp,uint row){
 if(!R(row,1))return false;uint m=CELL_META+cp*6,q=code[m],a=code[m+1];
 uint v[S],lo[S],hi[S],b[S];for(uint i=0;i<S;i++)v[i]=lo[i]=hi[i]=b[i]=0;
 return bind_atom(code,rows,q,a,row,v,lo,hi,b,!CELL_CONNECTIONS);
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
 if(COMMIT_MARKERS || !R(row,1))return;uint table=R(row,0);
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
// Post-commit receipts only transport physical witnesses, never new facts.
// The completed patch/rebuild precedes this kernel. Validate the frozen native
// row generations and binding before making them available to the evaluator.
kernel void family_install_markers(CTX,uint lane [[thread_position_in_grid]]){
 if(lane>=cfg[30])return;uint at=cfg[29]+lane*(4+2*P),cp=packet[at],r=packet[at+1],generation=packet[at+2],count=packet[at+3];
 if(cp>=NC || r>=cfg[0] || count==0 || count>P){atomic_fetch_or_explicit(counters+1,32,memory_order_relaxed);return;}
 cell_work(counters,cp,CM_RECEIPT_HINTS);
 uint id=cell_lookup(rowCells,instanceMeta,r,cp),m=CELL_META+cp*6;
 if(id==NO || !R(r,1) || R(r,2)!=generation || count>code[m+2]){cell_work(counters,cp,CM_RECEIPT_REJECTED);return;}
 uint q=code[m],anchor=code[m+1],v[S],lo[S],hi[S],b[S];for(uint j=0;j<S;j++)v[j]=lo[j]=hi[j]=b[j]=0;
 if(!bind_atom(code,rows,q,anchor,r,v,lo,hi,b)){cell_work(counters,cp,CM_RECEIPT_REJECTED);return;}
 for(uint j=0;j<count;j++){
  uint row=packet[at+4+2*j],gen=packet[at+5+2*j],part=code[CELL_STEP+(cp*(P-1)+j)*2];
  if(row>=cfg[0] || !R(row,1) || R(row,2)!=gen || !bind_atom(code,rows,q,part,row,v,lo,hi,b)){cell_work(counters,cp,CM_RECEIPT_REJECTED);return;}
 }
 uint base=id*CS;bool changed=cells[base+1]!=generation;
 for(uint j=0;j<count;j++){
  uint save=base+CH+2*j;changed=changed || cells[save]!=packet[at+4+2*j] || cells[save+1]!=packet[at+5+2*j];
  cells[save]=packet[at+4+2*j];cells[save+1]=packet[at+5+2*j];
 }
 if(changed){dirty_cell(cells,dirty,state,cfg,id,0);cell_work(counters,cp,CM_RECEIPT_ACCEPTED);cell_work(counters,cp,CM_RECEIPT_WITNESS_ROWS,count);}
 else cell_work(counters,cp,CM_RECEIPT_REDUNDANT);
}

bool marker_anchor(device const uint* code,device uint* rows,device uint* rowCells,device uint* instanceMeta,device const uint* packet,constant uint* cfg,uint lane){
 uint at=cfg[29]+lane*(4+2*P),cp=packet[at],r=packet[at+1];
 return cp<NC && r<cfg[0] && R(r,1) && R(r,2)==packet[at+2] && packet[at+3]>0 && packet[at+3]<=code[CELL_META+cp*6+2]
     && cell_lookup(rowCells,instanceMeta,r,cp)==NO && anchor_matches(code,rows,cp,r);
}
kernel void family_marker_count(CTX,uint lane [[thread_position_in_grid]]){
 if(lane<cfg[30] && marker_anchor(code,rows,rowCells,instanceMeta,packet,cfg,lane))atomic_fetch_add_explicit(registry+2,1,memory_order_relaxed);
}
kernel void family_marker_allocate(CTX,uint lane [[thread_position_in_grid]]){
 if(lane>=cfg[30])return;uint at=cfg[29]+lane*(4+2*P),row=packet[at+1];
 // Receipts are sorted by anchor then plan; one writer owns each row list.
 if(lane>0 && packet[at-(4+2*P)+1]==row)return;
 uint oldSize=atomic_load_explicit(registry,memory_order_relaxed),freeCount=atomic_load_explicit(registry+1,memory_order_relaxed);
 for(uint i=lane;i<cfg[30] && packet[cfg[29]+i*(4+2*P)+1]==row;i++){
  if(!marker_anchor(code,rows,rowCells,instanceMeta,packet,cfg,i))continue;
  uint cp=packet[cfg[29]+i*(4+2*P)],ticket=atomic_fetch_add_explicit(registry+3,1,memory_order_relaxed);
  uint id=ticket<freeCount?freeCells[freeCount-1-ticket]:oldSize+ticket-freeCount;
  if(id>=cfg[22]){atomic_fetch_or_explicit(counters+1,32,memory_order_relaxed);return;}
  uint base=id*CS,revision=cells[base+2],portRevision=cells[base+6],queued=cells[base+3];
  if(revision==NO || portRevision==NO){atomic_fetch_or_explicit(counters+1,4,memory_order_relaxed);return;}
  for(uint j=0;j<CS;j++)cells[base+j]=0;
  cells[base+2]=revision+1;cells[base+6]=portRevision+1;cells[base+3]=queued;
  instanceMeta[id*3]=row;instanceMeta[id*3+1]=cp;instanceMeta[id*3+2]=rowCells[row];rowCells[row]=id;
 }
}
