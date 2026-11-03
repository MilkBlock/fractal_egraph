// Three phase-local caches. Signatures contain exact inputs, never hashes or
// predictions. A negative model score is not permission to leave a cache stale.
kernel void jp_cache_static(JP_ARGS,device const uint*props[[buffer(5)]],device const uint*markers[[buffer(6)]],device const uint*rootmap[[buffer(7)]],device const uint*offsets[[buffer(8)]],device const uint*items[[buffer(9)]],device const uint*roots[[buffer(10)]],device uint*stat[[buffer(11)]],device half*x[[buffer(12)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,12))return;float v[JP_FEATURES]={0};uint key=roots[i];v[0]=1;
 for(uint k=0;k<3;k++)v[16+k]=bool(props[key]&(1u<<k));
 v[10]=jp_log_count(stat[i*JP_FEATURES+10]);v[12]=jp_log_count(stat[i*JP_FEATURES+12]);v[15]=jp_log_count(stat[i*JP_FEATURES+15]);
 uint ports=0,producer=0,pr[3]={0,0,0};
 for(uint q=offsets[i];q<offsets[i+1];q++){uint b=8*items[q];for(uint j=0;j<markers[b+3];j++){
  uint child=markers[b+4+j];ports++;producer+=stat[rootmap[child]*JP_FEATURES+10];for(uint k=0;k<3;k++)pr[k]+=bool(props[child]&(1u<<k));}}
 stat[i*JP_FEATURES+14]=ports;v[11]=jp_log_count(producer);for(uint k=0;k<3;k++)v[19+k]=float(pr[k])/max(1u,ports);
 for(uint k=29;k<32;k++)v[k]=jp_log_count(stat[i*JP_FEATURES+k]);
 for(uint j=0;j<JP_RECIPES;j++)v[32+j]=float(stat[i*JP_FEATURES+32+j])/max(1u,stat[i*JP_FEATURES+10]);
 for(uint j=0;j<JP_FEATURES;j++)x[i*JP_FEATURES+j]=half(v[j]);
}

kernel void jp_cache_signature(JP_ARGS,device const uint*roots[[buffer(5)]],device const uint*fmap[[buffer(6)]],device const uint*costs[[buffer(7)]],device const uint*ranges[[buffer(8)]],device const uint*summary[[buffer(9)]],device const uint*choices[[buffer(10)]],device const uint*markers[[buffer(11)]],device const uint*offsets[[buffer(12)]],device const uint*items[[buffer(13)]],device const uint*rootstate[[buffer(14)]],device long*initial[[buffer(15)]],device long*signatures[[buffer(16)]],device uint*dirty[[buffer(17)]],constant uint2&cfg[[buffer(18)]],uint i[[thread_position_in_grid]]){
 uint n=jp_count(r,12);if(i>=n)return;uint fr=fmap[roots[i]],id=fr?uint(r[(fr-1)*STRIDE+5]):0;bool ready=fr&&costs[id]>0;
 long low=ready?r[(ranges[2*id]-1)*STRIDE+5]:0,high=ready?r[(ranges[2*id+1]-1)*STRIDE+5]:0;
 if(cfg.x){initial[i*4]=ready;initial[i*4+1]=low;initial[i*4+2]=id;initial[i*4+3]=fr!=0;}
 uint nready=0;for(uint q=offsets[i];q<offsets[i+1];q++){uint b=8*items[q];for(uint j=0;j<markers[b+3];j++){uint cf=fmap[markers[b+4+j]];nready+=cf&&costs[uint(r[(cf-1)*STRIDE+5])]>0;}}
 bool candidate=summary[i*4+2]!=0;uint previous=jp_count(r,1)==2?m.epoch:m.epoch-1;
 long s[16]={1,ready,low,high,ready?long(costs[id]):0,
  long(rootstate[i*8+1]+rootstate[i*8+2]+rootstate[i*8+3]+rootstate[i*8+4]),nready,
  summary[i*4],summary[i*4+1],fr?long(choices[id]):0,candidate,
  candidate?r[(summary[i*4+2]-1)*STRIDE+5]:0,candidate?r[(summary[i*4+3]-1)*STRIDE+5]:0,
  fr&&(uint(r[(fr-1)*STRIDE+2])==previous||uint(r[JP_VERSION+id])==previous),long(rootstate[i*8+1]+rootstate[i*8+2]),0};
 uint at=(jp_count(r,1)*n+i)*16;bool changed=cfg.y!=0;for(uint j=0;j<16;j++)changed|=signatures[at+j]!=s[j];
 dirty[i]=changed;if(changed)for(uint j=0;j<16;j++)signatures[at+j]=s[j];

}

kernel void jp_cache_dependents(JP_ARGS,device const uint*markers[[buffer(5)]],device const uint*rootmap[[buffer(6)]],device const uint*offsets[[buffer(7)]],device const uint*items[[buffer(8)]],device const uint*baseDirty[[buffer(9)]],device uint*dirty[[buffer(10)]],uint i[[thread_position_in_grid]]){
 if(i>=jp_count(r,12))return;bool changed=baseDirty[i]!=0;
 for(uint q=offsets[i];q<offsets[i+1]&&!changed;q++){uint b=8*items[q];for(uint j=0;j<markers[b+3];j++)changed|=baseDirty[rootmap[markers[b+4+j]]]!=0;}
 dirty[i]=changed;
}

kernel void jp_cached_features(JP_ARGS,device const half*immutable[[buffer(5)]],device const long*signatures[[buffer(6)]],device const long*initial[[buffer(7)]],device const uint*stat[[buffer(8)]],device const uint*dirty[[buffer(9)]],device half*x[[buffer(10)]],uint i[[thread_position_in_grid]]){
 uint n=jp_count(r,12);if(i>=n||!dirty[i])return;jp_add(r,97,1);uint row=jp_count(r,1)*n+i,at=row*JP_FEATURES;device const long*s=signatures+row*16;
 for(uint j=0;j<JP_FEATURES;j++)x[at+j]=immutable[i*JP_FEATURES+j];
 x[at+1+jp_count(r,1)]=1;x[at+4]=half(s[1]);x[at+5]=half(s[1]?jp_magnitude(s[2]):0);x[at+6]=half(s[1]?jp_magnitude(s[3]):0);x[at+7]=half(jp_log_count(uint(s[4])));x[at+8]=half(s[1]&&!initial[i*4]);
 x[at+9]=half(jp_log_count(uint(s[5])));uint ports=stat[i*JP_FEATURES+14];x[at+13]=half(float(s[6])/max(1u,ports));x[at+14]=half(float(ports-uint(s[6]))/max(1u,ports));
 x[at+22]=half(jp_log_count(uint(s[7])));x[at+23]=half(jp_log_count(uint(s[8])));x[at+24]=half(jp_log_count(uint(s[9])));
 x[at+25]=half(s[1]&&s[10]&&s[11]<s[3]);x[at+26]=half(s[1]&&s[10]&&s[12]>=s[2]);x[at+27]=half(s[13]);x[at+28]=half(jp_log_count(uint(s[14])));
 long numbers[4]={s[2],s[3],s[11],s[12]};for(uint a=0;a<4;a++){ulong bits=as_type<ulong>(numbers[a])^0x8000000000000000UL;for(uint b=0;b<8;b++)x[at+32+JP_RECIPES+8*a+b]=half((a<2?s[1]:s[10])?float((bits>>(56-8*b))&255)/255.0f:0);}
}

kernel void jp_cached_prediction(JP_ARGS,device const float*raw[[buffer(5)]],device float*scores[[buffer(6)]],device const float*scales[[buffer(7)]],device const uint*roots[[buffer(8)]],device const uint*fmap[[buffer(9)]],device uint*priority[[buffer(10)]],constant uint&enabled[[buffer(11)]],uint i[[thread_position_in_grid]]){
 uint n=jp_count(r,12);if(i>=n)return;uint at=(jp_count(r,1)*n+i)*8;
 for(uint j=0;j<3;j++){scores[i*8+j]=enabled?raw[at+j]*scales[j]+scales[3+j]:1;if(enabled&&scores[i*8+j]>=.5f)jp_add(r,92+j,1);}
 uint f=fmap[roots[i]];if(f&&scores[i*8+2]>=.5f)atomic_store_explicit((device atomic_uint*)(priority+uint(r[(f-1)*STRIDE+5])),1u,memory_order_relaxed);
}

kernel void jp_cache_audit(JP_ARGS,device const half*x[[buffer(5)]],device const half*reference[[buffer(6)]],device const float*encoded[[buffer(7)]],device const float*encodedRef[[buffer(8)]],device const half*input[[buffer(9)]],device const half*inputRef[[buffer(10)]],device const float*raw[[buffer(11)]],device const float*rawRef[[buffer(12)]],constant uint&model[[buffer(13)]],uint i[[thread_position_in_grid]]){
 uint n=jp_count(r,12);if(i>=n)return;uint at=jp_count(r,1)*n+i;bool equal=true;
 for(uint j=0;j<JP_FEATURES;j++)equal&=x[at*JP_FEATURES+j]==reference[i*JP_FEATURES+j];
 for(uint j=0;j<64;j++)equal&=encoded[at*64+j]==encodedRef[i*64+j];
 for(uint j=0;j<136;j++)equal&=input[at*136+j]==inputRef[i*136+j];
 if(model)for(uint j=0;j<3;j++)equal&=raw[at*8+j]==rawRef[i*8+j];
 if(!equal)jp_fail(m,25);if(i==0)r[JP_STATE+103]+=n;
}
