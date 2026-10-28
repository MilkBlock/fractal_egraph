// SPINE_INSTRUCTIONS
// The marker and exact child-port projections already establish the joins.
// Keep the full witness tuple to enforce frontier/order; do not rebind all vars.
kernel void sd_actions(JP_ARGS,device const uint*g[[buffer(5)]],device const uint*items[[buffer(6)]],device long*matches[[buffer(7)]],device uint*witnesses[[buffer(8)]],device uint*valid[[buffer(9)]],device uint*info[[buffer(10)]],device long*payload[[buffer(11)]],device half*x[[buffer(12)]],device const uint*rootmap[[buffer(13)]],constant uint2&cfg[[buffer(14)]],uint job[[thread_position_in_grid]]){
 if(job>=jp_count(r,9)||m.error)return;uint lo=0,hi=cfg.x;while(lo+1<hi){uint mid=(lo+hi)/2;if(g[mid*16+1]<=job)lo=mid;else hi=mid;}
 uint at=16*lo,recipe=g[at+4];int index=sd_index(recipe);if(index<0)return;valid[job]=0;jp_add(r,38,1);
 for(uint j=0;j<cfg.y;j++)x[job*cfg.y+j]=0;
 uint selected[7],costs[3]={0xffffffff,0xffffffff,0xffffffff};for(uint j=0;j<7;j++)selected[j]=0xffffffff;selected[0]=g[at+2];uint offset=job-g[at+1];
 for(uint j=0;j<JP_ARITY[recipe];j++){costs[j]=items[g[at+12+j]+offset%g[at+9+j]];offset/=g[at+9+j];selected[1+j]=g[at+6+j];selected[4+j]=costs[j];}
 uint rule=g[at+3],n=jp_atom_count(rule),w[JP_ATOMS],pivot=0;bool fresh=!jp_seminaive(rule),safe=true;
 for(uint j=0;j<n;j++){w[j]=selected[sd_role(uint(index),j)];if(jp_seminaive(rule)&&!fresh&&uint(sv[w[j]*STRIDE+2])>wm[rule]){pivot=jp_delta(rule)?j:0;fresh=true;}}
 if(!fresh)return;
 long v[SD_WIDTH];sd_parameters(uint(index),selected,sv,v);for(uint j=0;j<sd_nparams(uint(index));j++)payload[job*SD_WIDTH+j]=v[j];
 uint q=job*MSTRIDE;matches[q]=rule;uint seed=jp_seed_atom(rule,pivot);
 matches[q+1+VARS]=jp_rule_rank(rule);matches[q+2+VARS]=pivot;matches[q+3+VARS]=jp_delta(rule)&&seed!=0xffffffff&&wm[rule]&&seed==pivot?uint(sv[w[seed]*STRIDE+2]):0;matches[q+4+VARS]=seed==0xffffffff?0:w[seed];
 for(uint j=0;j<n;j++){witnesses[job*JP_ATOMS+j]=w[j];uint producer=uint(r[JP_ROW_TAG+w[j]])&3;if(producer>0&&producer<3){jp_add(r,59,1);uint bit=1u<<8,old=atomic_fetch_or_explicit((device atomic_uint*)(r+JP_ROW_TAG+w[j]),bit,memory_order_relaxed);if(!(old&bit))jp_add(r,60,1);}}
 info[6*job]=rootmap[g[at+5]];info[6*job+1]=recipe;
 for(uint j=0;j<3;j++){info[6*job+2+j]=costs[j];if(costs[j]!=0xffffffff){long c=sv[costs[j]*STRIDE+5];safe=safe&&c>=-2048&&c<=2048;}}
 info[6*job+5]=safe;if(safe){for(uint j=0;j<3;j++)if(costs[j]!=0xffffffff)x[job*cfg.y+4*recipe+j]=half(sv[costs[j]*STRIDE+5]);x[job*cfg.y+4*recipe+3]=1;}
 if(g[at+9]>1||g[at+10]>1||g[at+11]>1)jp_add(r,41,1);valid[job]=1;
}
kernel void sd_commit(JP_ARGS,device long*matches[[buffer(5)]],device const uint*order[[buffer(6)]],device const long*numeric[[buffer(7)]],device const uint*info[[buffer(8)]],device long*code[[buffer(9)]],device long*registers[[buffer(10)]],device uint*rootstate[[buffer(11)]],device const long*payload[[buffer(12)]],uint lane[[thread_position_in_grid]]){
 if(lane||m.error)return;p[IDCAP]=1;p[IDCAP+1]=0;
 for(uint i=0;i<m.matched&&!m.error;i++){uint job=order[i],owner=info[job*6],before=m.count;int index=sd_index(info[job*6+1]);
  if(index>=0){long v[SD_WIDTH]={0};for(uint j=0;j<sd_nparams(uint(index));j++)v[j]=payload[job*SD_WIDTH+j];sd_execute(uint(index),v,numeric+job*JP_OPS,r,p,fp,m);jp_add(r,39,1);}
  else if(!SD_ALL){for(uint j=0;j<JP_OPS;j++)r[JP_NUM+j]=numeric[job*JP_OPS+j];execute_bytecode(uint(matches[job*MSTRIDE]),job,code,registers,matches,r,p,fp,m);jp_add(r,40,1);}
  else{m.error=30;return;}
  rootstate[owner*8]|=1;rootstate[owner*8+1]+=m.count-before;jp_add(r,56,m.count-before);
 }
 p[IDCAP]=0;if(p[IDCAP+1])m.error=24;r[JP_STATE+23]+=m.count-uint(r[JP_STATE+20]);
 uint set=JP_SETS[0];for(uint i=SET_START[set];i<SET_START[set+1];i++)wm[SET_RULES[i]]=m.epoch-1;
}
