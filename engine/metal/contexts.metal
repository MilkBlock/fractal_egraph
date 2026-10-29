kernel void context_reset(KERNEL_ARGS){if(lane)return;for(uint i=0;i<16;i++)context_info[i]=0;context_info[5]=m.epoch;}
kernel void context_dispatch(KERNEL_ARGS){
 if(lane)return;
 context_info[2]=(!m.error&&!context_info[1])?(context_info[0]+255)/256:0;
 context_info[3]=1;context_info[4]=1;
}
kernel void context_restart(KERNEL_ARGS){
 // Prefix enumeration has not committed any effects. Queue overflow therefore
 // permits a complete original-query restart, including unsplit rules.
 if(lane||m.error||!context_info[1])return;m.matched=0;
}
kernel void context_predict(KERNEL_ARGS){
 if(m.error||context_info[1]||lane>=context_info[0])return;
 uint ca=lane*CSTRIDE,plan=uint(contexts[ca]);
 context_results[lane*2]=CONTEXT_RIDGE?context_score(plan,ca,contexts):0.0f;
 context_results[lane*2+1]=0.0f;
}
kernel void context_training_export(KERNEL_ARGS,device float*data[[buffer(26)]]){
 if(context_info[1]||lane>=context_info[0])return;
 uint ca=lane*CSTRIDE,plan=uint(contexts[ca]),out=lane*(CONTEXT_FEATURES+2);
 data[out]=float(plan);
 for(uint f=0;f<CONTEXT_FEATURES;f++)data[out+1+f]=context_feature(plan,f,ca,contexts);
 data[out+1+CONTEXT_FEATURES]=context_results[lane*2+1];
}
