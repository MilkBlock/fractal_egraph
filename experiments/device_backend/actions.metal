// Fixed-width instructions [opcode,destination,operation,operand-kind,value].
// Only the currently executing binding uses this device register file.
// A snapshot no-op is safe to omit only if it remains a no-op throughout this
// deferred-union RHS phase. No mutable function reads, custom/new/old merge,
// allocating set operations or fallible actions are certified here.
// EMPTY_EFFECT_PROOFS
__attribute__((noinline)) void execute_bytecode(uint rule,uint mi,device long*code,device long*v,device long*matches,
 device long*r,device uint*p,device uint*fp,device Meta&m){
 for(uint i=0;i<QUERY_WIDTH[rule];i++)v[i]=matches[mi*MSTRIDE+1+i];
 long args[COLS]={0};
 for(uint pc=ACTION_START[rule];pc<ACTION_START[rule+1]&&!m.error;pc+=5){
  uint op=uint(code[pc]),dst=uint(code[pc+1]),sub=uint(code[pc+2]);
  long value=code[pc+3]==2?matches[mi*MSTRIDE+1+code[pc+4]]:(code[pc+3]?v[code[pc+4]]:code[pc+4]);
  switch(op){
   case 0:args[dst]=value;break;
   case 1:v[dst]=value;break;
   case 2:unite(p,args[0],args[1],m);break;
   case 3:v[dst]=write_row(r,p,fp,m,sub,args,0,true);break;
   case 4:write_row(r,p,fp,m,sub,args,args[WIDTH[sub]-1],false);break;
   case 5:if(!hide_row(r,p,fp,m,sub,args))return;break;
   case 6:if(!integer_valid(sub,args[0],args[1])){m.error=5;return;}v[dst]=integer_eval(sub,args[0],args[1]);break;
   case 7:{
    long x=0;
    if(sub==0)x=!args[0];
    if(sub==1)x=args[0]&&args[1];
    if(sub==2)x=args[0]!=args[1];
    if(sub==3)x=!args[0]||args[1];
    if(sub==4)for(uint i=0;i<uint(value);i++)x|=args[i];
    v[dst]=x;break;
   }
   case 8:v[dst]=set_apply(r,p,m,sub,uint(value),args[0],args[1]);break;
   case 9:m.error=8;return;
   default:m.error=10;return;
  }
 }
}

// DIRECT_ACTIONS
