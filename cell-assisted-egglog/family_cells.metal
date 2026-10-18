// A certified prefix has ONE current binding for its anchored reads. Bake
// supplies admissible alias/literal shapes; the compiler requires a complete native function key for
// every subsequent read. Those keys determine at most one current row.
// No CPU instance registry, history, or per-match admission is involved.
kernel void family_cells(CTX,uint lane [[thread_position_in_grid]]){
 uint r=lane/NC,cp=lane%NC;if(r>=cfg[0])return;
 uint base=(r*NC+cp)*CS,m=CELL_META+cp*6,q=code[m],anchor=code[m+1],k=code[m+2];
 if(R(r,1)==0 || R(r,0)!=code[ATOM+(q*P+anchor)*(2+2*C)]){cells[base]=0;return;}
 bool fresh=cells[base]==2 && cells[base+1]==R(r,2);
 if(fresh)for(uint j=0;j<k;j++){
  uint at=base+CH+j*2,row=cells[at];
  if(R(row,1)==0 || R(row,2)!=cells[at+1]){fresh=false;break;}
 }
 if(fresh){if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+6,1,memory_order_relaxed);return;}
 if(cells[base] && PROFILE_JOIN)atomic_fetch_add_explicit(counters+8,1,memory_order_relaxed);
 cells[base]=0;
 uint v[S],lo[S],hi[S],b[S];for(uint i=0;i<S;i++){v[i]=lo[i]=hi[i]=b[i]=0;}
 if(!bind_atom(code,rows,q,anchor,r,v,lo,hi,b))return;
 if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+5,1,memory_order_relaxed);
 for(uint j=0;j<k;j++){
  uint step=CELL_STEP+(cp*(P-1)+j)*2,t=code[step],sp=code[step+1],atom=ATOM+(q*P+t)*(2+2*C),at=SPEC+sp*(2+C),h=0x31415926u;
  for(uint c=0;c<code[at+1];c++){uint col=code[at+2+c],kind=code[atom+2+2*col],slot=code[atom+3+2*col];h=mixkey(h^(kind?slot:v[slot]));}
  uint bucket=sp*cfg[4]+(h&(cfg[4]-1)),head=atomic_load_explicit(heads+bucket,memory_order_relaxed),chosen=NO;
  if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+9,1,memory_order_relaxed);
  for(uint e=head;e!=NO;e=links[e*NI+sp]){
   if(!live_event(rows,events,e))continue;
   uint vv[S],ll[S],hh[S],bb[S];for(uint z=0;z<S;z++){vv[z]=v[z];ll[z]=lo[z];hh[z]=hi[z];bb[z]=b[z];}
   uint row=E(e,0);if(!bind_atom(code,rows,q,t,row,vv,ll,hh,bb))continue;
   chosen=row;break; // Complete key: no second canonical row can exist.
  }
  if(chosen==NO)return;
  bind_atom(code,rows,q,t,chosen,v,lo,hi,b);
  uint save=base+CH+j*2;cells[save]=chosen;cells[save+1]=R(chosen,2);
 }
 bool shape=false;
 for(uint a=0;a<code[m+4];a++){
  bool good=true;uint at=CELL_VARIANT+code[m+3]+a*2*S;
  for(uint i=0;i<S;i++){uint kind=code[at+2*i],x=code[at+2*i+1];if((kind==1 && v[i]!=v[x]) || (kind==2 && v[i]!=x)){good=false;break;}}
  if(good){shape=true;break;}
 }
 if(!shape){if(PROFILE_JOIN)atomic_fetch_add_explicit(counters+7,1,memory_order_relaxed);return;}
 for(uint i=0;i<code[m+5];i++){uint f=code[CELL_FIELDS+cp*S+i];cells[base+CV+i*3]=v[f];cells[base+CV+i*3+1]=lo[f];cells[base+CV+i*3+2]=hi[f];}
 cells[base+1]=R(r,2);cells[base]=2;
}
