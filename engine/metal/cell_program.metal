// CELL_INSTRUCTIONS
kernel void cp_index(JP_ARGS,device const uint*markers[[buffer(5)]],device const uint*lists[[buffer(6)]],constant uint&lc[[buffer(7)]],uint i[[thread_position_in_grid]]){
 if(!CP_ENABLED)return;
 if(i<jp_count(r,11)){uint owner=cpOwnerMap[markers[8*i+2]];atomic_fetch_add_explicit((device atomic_uint*)(cpInfo+4*owner),1u,memory_order_relaxed);}
 if(i<jp_count(r,14)){uint row=lists[lc+i];if(!r[row*STRIDE+1]){uint owner=cpOwnerMap[uint(r[row*STRIDE+4])];atomic_fetch_add_explicit((device atomic_uint*)(cpInfo+4*owner+1),1u,memory_order_relaxed);}}
}
// Recognition only: no effects are exposed before the original stage commit.
kernel void cp_recognize(JP_ARGS,device const uint*markers[[buffer(5)]],device const uint*fmap[[buffer(6)]],device const uint*counts[[buffer(7)]],device const uint*offsets[[buffer(8)]],device const uint*items[[buffer(9)]],device const uint*knownCosts[[buffer(10)]],uint j[[thread_position_in_grid]]){
 if(!CP_ENABLED||m.error||j||jp_count(r,1)!=0)return;
 for(uint j=0;j<jp_count(r,11)&&!m.error;j++){
 uint row=markers[8*j],rootid=markers[8*j+2],owner=cpOwnerMap[rootid];
 if(cpInfo[4*owner+2]||cpInfo[4*owner]!=1||cpInfo[4*owner+1]||fmap[rootid])continue;
 int tpl=cp_template(uint(r[row*STRIDE]));if(tpl<0)continue;
 long v[CP_WIDTH]={0};bool known[CP_WIDTH]={false};
 if(!cp_bind_parameters(uint(tpl),row,r,p,fmap,counts,offsets,items,cpOwnerMap,cpInfo,cpData,v,known))continue;
 // Lookups may compress identity paths; serialize this admission pass. No
 // constructor/function/union effect is published by the recipe probe.
 if(!cp_evaluate(uint(tpl),v,known,true,0,r,p,fp,m)||!cp_guard(uint(tpl),v,known,p,counts,knownCosts))continue;
 uint at=owner*(CP_WIDTH+1);cpData[at]=tpl+1;for(uint k=0;k<CP_WIDTH;k++)cpData[at+1+k]=v[k];
 cpInfo[4*owner+2]=1;jp_add(r,124,1);
 }
}
// Fixed stage continuations execute artifact operations directly. There is no
// jp_bind, rule LHS query, or original RHS bytecode interpreter on this path.
kernel void cp_emit(JP_ARGS,device uint*rootstate[[buffer(5)]],device uint*choiceCertificates[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(i||!CP_ENABLED||m.error)return;uint phase=jp_count(r,1),beforeAll=m.count;
 for(uint owner=0;owner<jp_count(r,12)&&!m.error;owner++){
  uint state=cpInfo[4*owner+2],stage;if(phase==0&&state==1)stage=0;else if(phase==1&&state==2)stage=1;else if(phase==1&&state==3)stage=2;else continue;
  uint at=owner*(CP_WIDTH+1),tpl=uint(cpData[at]-1),before=m.count;long v[CP_WIDTH];bool known[CP_WIDTH];for(uint k=0;k<CP_WIDTH;k++){v[k]=cpData[at+1+k];known[k]=true;}
  if(!cp_evaluate(tpl,v,known,false,stage,r,p,fp,m))return;
  if(stage>0){long key[COLS]={cp_value(tpl,5,v)},value=cp_value(tpl,stage==1?3:0,v);long oldrow=JP_BLOCK_AUDIT?query_lookup(r,JP_IDS[2],key):-1,oldvalue=oldrow>=0?r[oldrow*STRIDE+5]:-1;
   r[JP_STATE+118]=owner;write_row(r,p,fp,m,JP_IDS[2],key,value,false);
   if(JP_BLOCK_AUDIT){long check=query_lookup(r,JP_IDS[2],key);if(check!=oldrow||(check>=0&&r[check*STRIDE+5]!=oldvalue)){m.error=28;return;}jp_add(r,117,1);}
  }
  if(stage==2){long key[COLS]={cp_value(tpl,3,v),cp_value(tpl,0,v)};long row=row_lookup(r,p,m,JP_IDS[3],key);if(row<0){m.error=28;return;}choiceCertificates[row]=1;}
  for(uint k=0;k<CP_WIDTH;k++)cpData[at+1+k]=v[k];cpInfo[4*owner+2]=stage+2;
  rootstate[owner*8]|=1u<<phase;rootstate[owner*8+1+phase]+=m.count-before;jp_add(r,56+phase,m.count-before);jp_add(r,125,cp_operation_count(tpl,stage));
 }
 r[JP_STATE+23]+=m.count-beforeAll;
}
kernel void cp_validate(JP_ARGS,device const uint*fmap[[buffer(5)]],device const uint*counts[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(!CP_ENABLED||i>=jp_count(r,12))return;uint state=cpInfo[4*i+2];if(state<2||state>=8)return;
 device const long*data=cpData+i*(CP_WIDTH+1);if(!cp_still_valid(uint(data[0]-1),data+1,r,p,fmap,counts,state,jp_count(r,1))){cpInfo[4*i+2]=8;jp_add(r,127,1);}
}
kernel void cp_seed(JP_ARGS,device uint*local[[buffer(5)]],device uint*control[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(!CP_ENABLED||i>=jp_count(r,12)||cpInfo[4*i+2]!=4)return;device const long*data=cpData+i*(CP_WIDTH+1);uint t=uint(data[0]-1);cu_union(local,uint(cp_value(t,4,data+1)),uint(cp_value(t,0,data+1)),control,r);
}
kernel void cp_seed_finish(JP_ARGS,device uint*control[[buffer(5)]],uint i[[thread_position_in_grid]]){if(!i){control[4]+=control[5];control[5]=0;}}
kernel void cp_finish(JP_ARGS,device const uint*fmap[[buffer(5)]],device uint*rootstate[[buffer(6)]],uint i[[thread_position_in_grid]]){
 if(!CP_ENABLED||i>=jp_count(r,12)||cpInfo[4*i+2]!=4)return;device const long*data=cpData+i*(CP_WIDTH+1);uint t=uint(data[0]-1),rootid=uint(cp_value(t,5,data+1)),f=fmap[rootid];
 if(!f||cu_root(p,uint(r[(f-1)*STRIDE+5]))!=cu_root(p,uint(cp_value(t,0,data+1)))){cpInfo[4*i+2]=8;jp_add(r,127,1);return;}
 cpInfo[4*i+2]=5;rootstate[i*8]|=4;jp_add(r,125,cp_operation_count(t,3));
}
