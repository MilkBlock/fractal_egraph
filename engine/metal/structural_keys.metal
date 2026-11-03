// Ordered key evaluator. Each resident instance owns its arena and comparison
// stack. These calls are excluded from parallel query/no-op/RHS execution.
uchar sk_byte(device long*k,long id,uint i){return uchar(ulong(k[id+5+i/8])>>(8*(i%8)));}
long sk_child(device long*k,long id,uint i){return k[id+5+(k[id+3]+7)/8+i];}
int sk_cmp(device long*r,device Meta&m,long a,long b){
 device long*k=r+SK_BASE;device long*stack=k+SK_WORDS;uint depth=0;stack[0]=a;stack[1]=b;stack[2]=-1;
 while(true){
  uint at=3*depth;a=stack[at];b=stack[at+1];long next=stack[at+2];
  if(a<8||b<8||a>=k[0]||b>=k[0]){m.error=13;return 0;}
  if(a==b){if(!depth)return 0;depth--;continue;}
  if(next<0){
   if(k[a+1]!=k[b+1])return k[a+1]>k[b+1]?-1:1;
   if(k[a+2]!=k[b+2])return k[a+2]<k[b+2]?-1:1;
   for(uint i=0;i<uint(min(k[a+3],k[b+3]));i++){uchar x=sk_byte(k,a,i),y=sk_byte(k,b,i);if(x!=y)return x<y?-1:1;}
   if(k[a+3]!=k[b+3])return k[a+3]<k[b+3]?-1:1;
   next=0;
  }
  if(next<min(k[a+4],k[b+4])){
   stack[at+2]=next+1;if(++depth>=SK_WORDS/5){m.error=13;return 0;}
   stack[3*depth]=sk_child(k,a,uint(next));stack[3*depth+1]=sk_child(k,b,uint(next));stack[3*depth+2]=-1;
  }else{if(k[a+4]!=k[b+4])return k[a+4]<k[b+4]?-1:1;if(!depth)return 0;depth--;}
 }
}
long sk_make(device long*r,device Meta&m,bool anchor,thread uchar*label,uint len,thread long*children,uint arity){
 device long*k=r+SK_BASE;
 for(long at=8;at<k[0];at+=k[at]){
  if(k[at+1]!=long(anchor)||k[at+3]!=len||k[at+4]!=arity)continue;bool equal=true;
  for(uint i=0;i<len&&equal;i++)equal=sk_byte(k,at,i)==label[i];
  for(uint i=0;i<arity&&equal;i++)equal=sk_child(k,at,i)==children[i];if(equal)return at;
 }
 uint packed=(len+7)/8,words=5+packed+arity;long at=k[0];if(at+words>SK_WORDS){m.error=14;return 0;}
 long height=0;for(uint i=0;i<arity;i++){if(children[i]<8||children[i]>=at){m.error=13;return 0;}height=max(height,k[children[i]+2]+1);}
 k[at]=words;k[at+1]=anchor;k[at+2]=anchor?0:height;k[at+3]=len;k[at+4]=arity;
 for(uint i=0;i<packed;i++){ulong word=0;for(uint j=0;j<8&&i*8+j<len;j++)word|=ulong(label[i*8+j])<<(8*j);k[at+5+i]=long(word);}
 for(uint i=0;i<arity;i++)k[at+5+packed+i]=children[i];k[0]=at+words;return at;
}
long sk_op(device long*r,device Meta&m,uint op,uint param,thread long*args,uint arity){
 device long*k=r+SK_BASE;k[1]++;
 if(op==11){uint id=uint(args[0]),count=SK_DECODE_COUNT[id],pc=SK_DECODE_START[id];if(!count){m.error=5;return 0;}
  if(count>=SK_WORDS/5){m.error=14;return 0;}device long*values=k+SK_WORDS;
  for(uint i=0;i<count;i++){bool anchored=bool(SK_DECODE_CODE[pc++]);uint labelid=SK_DECODE_CODE[pc++],n=SK_DECODE_CODE[pc++];uchar bytes[SK_MAX_LABEL];uint len=0;long children[SK_DECODE_ARITY];
   for(uint j=SK_LABEL_START[labelid];j<SK_LABEL_START[labelid+1];j++)bytes[len++]=SK_LABELS[j];
   for(uint j=0;j<n;j++)children[j]=values[SK_DECODE_CODE[pc++]];values[i]=sk_make(r,m,anchored,bytes,len,children,n);if(m.error)return 0;
  }return values[SK_DECODE_ROOT[id]];
 }
 if(op==0)return sk_cmp(r,m,args[0],args[1])<=0?args[0]:args[1];
 if(op==2)return sk_cmp(r,m,args[0],args[1]);
 if(op==3)return args[0]>args[1]||(args[0]==args[1]&&sk_cmp(r,m,args[2],args[3])<=0);
 if(op==4)return args[2]?args[0]:args[1];
 uchar label[SK_MAX_LABEL];uint len=0,which=param,children=0;bool anchor=false;
 if(op==1){if(k[args[0]+1])return args[0];which=SK_NODE_COUNT;children=1;anchor=true;}
 else if(op==5)which=SK_NODE_COUNT+1;
 else if(op==6)which=SK_NODE_COUNT+(args[0]?3:2);
 else if(op==7)which=SK_NODE_COUNT+4;
 else if(op==8)which=SK_NODE_COUNT+6+uint(args[0]);
 else if(op==9)which=SK_NODE_COUNT+5;
 else if(op==10)children=arity;
 else {m.error=15;return 0;}
 for(uint i=SK_LABEL_START[which];i<SK_LABEL_START[which+1];i++)label[len++]=SK_LABELS[i];
 if(op==5){ulong mag=args[0]<0?0UL-ulong(args[0]):ulong(args[0]);if(args[0]<0)label[len++]='-';uchar ds[20];uint n=0;do{ds[n++]=uchar('0'+mag%10);mag/=10;}while(mag);while(n)label[len++]=ds[--n];}
 if(op==9){ulong bits=ulong(args[0]);if((bits&0x7fffffffffffffffUL)==0)bits=0;else if((bits&0x7ff0000000000000UL)==0x7ff0000000000000UL&&(bits&0xfffffffffffffUL))bits=0x7ff8000000000000UL;
  for(int i=15;i>=0;i--){uint d=uint((bits>>(4*i))&15);label[len++]=uchar(d<10?'0'+d:'a'+d-10);}}
 return sk_make(r,m,anchor,label,len,args,children);
}
