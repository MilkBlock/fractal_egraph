// UPDATE_INSTRUCTIONS
// Existing-owner bags use the same immutable phase projection as fresh Cells.
// Extrema can reject a whole bag, but never replace its correlated witnesses.
kernel void up_groups(JP_ARGS,device const uint*lists[[buffer(5)]],device const uint*fmap[[buffer(6)]],device const uint*counts[[buffer(7)]],device const uint*offsets[[buffer(8)]],device const uint*stamps[[buffer(9)]],device const uint*ranges[[buffer(10)]],device uint*g[[buffer(11)]],constant uint2&cfg[[buffer(12)]],uint i[[thread_position_in_grid]]){
 if(i>=cfg.x||m.error)return;uint q=16*i;for(uint j=0;j<16;j++)g[q+j]=0;
 uint rank=i%3,row=lists[cfg.y+i/3];if(sv[row*STRIDE+1])return;uint owner=uint(sv[row*STRIDE+4]);
 if(CP_ENABLED&&cpInfo[4*cpOwnerMap[owner]+2]>0&&cpInfo[4*cpOwnerMap[owner]+2]<8){jp_add(r,126,1);return;}
 uint rule=JP_HELPER_RULES[rank],latest=uint(sv[row*STRIDE+2]),sizes[3]={1,1,1},starts[3]={0},fr=0,cand=uint(sv[row*STRIDE+5]),old=0;
 if(rank){sizes[0]=counts[cand];starts[0]=offsets[cand];latest=max(latest,stamps[cand]);}
 if(rank==1){fr=fmap[owner];if(!fr)return;old=uint(sv[(fr-1)*STRIDE+5]);sizes[1]=counts[old];starts[1]=offsets[old];latest=max(latest,max(uint(sv[(fr-1)*STRIDE+2]),stamps[old]));}
 if(jp_seminaive(rule)&&latest<=wm[rule])return;
 ulong n=ulong(sizes[0])*sizes[1];jp_add(r,25,1);
 if((rank==1&&ranges[2*cand]&&ranges[2*old+1]&&sv[(ranges[2*cand]-1)*STRIDE+5]>=sv[(ranges[2*old+1]-1)*STRIDE+5])||
    (rank==2&&ranges[2*cand]&&sv[(ranges[2*cand]-1)*STRIDE+5]>=0)){jp_add(r,26,uint(min(n,ulong(0xffffffff))));return;}
 if(n>MATCHCAP){jp_fail(m,2);return;}
 g[q]=uint(n);g[q+2]=row;g[q+3]=rule;g[q+4]=rank;g[q+5]=owner;g[q+6]=fr?fr-1:0xffffffff;
 for(uint j=0;j<3;j++){g[q+9+j]=sizes[j];g[q+12+j]=starts[j];}
}
kernel void up_actions(JP_ARGS,device const uint*g[[buffer(5)]],device const uint*items[[buffer(6)]],device long*matches[[buffer(7)]],device uint*witnesses[[buffer(8)]],device uint*valid[[buffer(9)]],device uint*info[[buffer(10)]],device long*payload[[buffer(11)]],device const uint*rootmap[[buffer(12)]],constant uint&ngroups[[buffer(13)]],uint job[[thread_position_in_grid]]){
 if(job>=jp_count(r,9)||m.error)return;valid[job]=0;uint lo=0,hi=ngroups;while(lo+1<hi){uint mid=(lo+hi)/2;if(g[mid*16+1]<=job)lo=mid;else hi=mid;}
 uint at=16*lo,rank=g[at+4],rule=g[at+3],offset=job-g[at+1],selected[4]={g[at+2],g[at+6],0xffffffff,0xffffffff};
 if(rank){selected[2]=items[g[at+12]+offset%g[at+9]];if(rank==1)selected[3]=items[g[at+13]+offset/g[at+9]];}
 jp_add(r,27,1);if((rank==1&&sv[selected[2]*STRIDE+5]>=sv[selected[3]*STRIDE+5])||(rank==2&&sv[selected[2]*STRIDE+5]>=0))return;
 uint atoms=jp_atom_count(rule),w[JP_ATOMS],pivot=0;bool fresh=!jp_seminaive(rule);
 for(uint j=0;j<atoms;j++){w[j]=selected[up_role(rank,j)];if(jp_seminaive(rule)&&!fresh&&uint(sv[w[j]*STRIDE+2])>wm[rule]){pivot=jp_delta(rule)?j:0;fresh=true;}}
 if(!fresh)return;
 long v[UP_WIDTH]={0};up_parameters(rank,selected,sv,v);for(uint j=0;j<UP_WIDTH;j++)payload[job*UP_WIDTH+j]=v[j];
 uint q=job*MSTRIDE;matches[q]=rule;uint seed=jp_seed_atom(rule,pivot);
 matches[q+1+VARS]=jp_rule_rank(rule);matches[q+2+VARS]=pivot;matches[q+3+VARS]=jp_delta(rule)&&seed!=0xffffffff&&wm[rule]&&seed==pivot?uint(sv[w[seed]*STRIDE+2]):0;matches[q+4+VARS]=seed==0xffffffff?0:w[seed];
 for(uint j=0;j<atoms;j++){witnesses[job*JP_ATOMS+j]=w[j];uint producer=uint(r[JP_ROW_TAG+w[j]])&3;if(producer<3&&producer!=1){jp_add(r,59,1);uint bit=1u<<9,old=atomic_fetch_or_explicit((device atomic_uint*)(r+JP_ROW_TAG+w[j]),bit,memory_order_relaxed);if(!(old&bit))jp_add(r,60,1);}}
 info[job*6]=rootmap[g[at+5]];info[job*6+1]=rank;info[job*6+2]=selected[1]!=0xffffffff;info[job*6+3]=g[at+9]>1||g[at+10]>1;valid[job]=1;
}
kernel void up_commit(JP_ARGS,device const uint*order[[buffer(5)]],device const uint*info[[buffer(6)]],device const long*payload[[buffer(7)]],device uint*rootstate[[buffer(8)]],uint lane[[thread_position_in_grid]]){
 if(lane||m.error)return;p[IDCAP]=1;p[IDCAP+1]=0;r[JP_STATE+109]=0;r[JP_STATE+119]=1;
 for(uint i=0;i<m.matched&&!m.error;i++){uint job=order[i],owner=info[job*6],rank=info[job*6+1],before=m.count;long v[UP_WIDTH];for(uint j=0;j<UP_WIDTH;j++)v[j]=payload[job*UP_WIDTH+j];
  r[JP_STATE+118]=owner;up_execute(rank,v,r,p,fp,m);uint old=rootstate[owner*8];rootstate[owner*8]=old|2|16;if(!(old&16))jp_add(r,31,1);
  rootstate[owner*8+2]+=m.count-before;jp_add(r,57,m.count-before);jp_add(r,28,1);if(info[job*6+2])jp_add(r,29,1);if(info[job*6+3])jp_add(r,30,1);
 }
 p[IDCAP]=0;if(p[IDCAP+1])m.error=24;r[JP_STATE+23]+=m.count-uint(r[JP_STATE+20]);
 uint set=JP_SETS[1];for(uint i=SET_START[set];i<SET_START[set+1];i++)wm[SET_RULES[i]]=m.epoch-1;
}
