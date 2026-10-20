// Pure LHS constraints. bound=2 means an exact integer is known but its native
// intern ID will be supplied by the matching table row. No ID is invented.
bool pu_lt(uint2 a,uint2 b){return a.y<b.y||(a.y==b.y&&a.x<b.x);}
uint2 pu_sub(uint2 a,uint2 b){return uint2(a.x-b.x,a.y-b.y-uint(a.x<b.x));}
uint3 nrem(uint2 a,uint2 b){
 if((b.x|b.y)==0 || (a.x==0 && a.y==0x80000000u && b.x==0xffffffffu && b.y==0xffffffffu))return uint3(0,0,0);
 bool neg=(a.y&0x80000000u)!=0;if(neg)a=nneg(a);if(b.y&0x80000000u)b=nneg(b);
 uint2 r=uint2(0);
 for(int i=63;i>=0;i--){uint bit=i>=32?((a.y>>(i-32))&1u):((a.x>>i)&1u);r=uint2((r.x<<1)|bit,(r.y<<1)|(r.x>>31));if(!pu_lt(r,b))r=pu_sub(r,b);}
 return uint3(neg?nneg(r):r,1);
}
bool primitive_apply(device const uint* code,uint at,thread uint* v,thread uint* lo,thread uint* hi,thread uint* bound){
   uint op=code[at];
   uint ak=code[at+1],ai=code[at+2],bk=code[at+4],bi=code[at+5];
   if(op==4){if((ak?ai:v[ai])==(bk?bi:v[bi]))return false;}
   else{
    uint2 a=ak?uint2(ai,code[at+3]):uint2(lo[ai],hi[ai]);
    uint2 b=bk?uint2(bi,code[at+6]):uint2(lo[bi],hi[bi]);
    if(op<6){bool lt=nless(a,b),gt=nless(b,a);if(!(op==0?lt:op==1?gt:op==2?!gt:op==3?!lt:!all(a==b)))return false;}
    else{
     uint3 z=op==6?nadd(a,b):nrem(a,b);if(z.z==0)return false;
     uint k=code[at+7],j=code[at+8];
     if(k==1){if(z.x!=j||z.y!=code[at+9])return false;}
     else if(bound[j]){if(z.x!=lo[j]||z.y!=hi[j])return false;}
     else{v[j]=0;lo[j]=z.x;hi[j]=z.y;bound[j]=2;}
    }
   }
   return true;
}
bool primitive_constraints(device const uint* code,uint q,thread uint* v,thread uint* lo,thread uint* hi,thread uint* bound,bool complete){
#if HAS_PRIMITIVES
 uint n=code[PMETA+q];bool done[NP];for(uint i=0;i<n;i++)done[i]=false;
 for(uint pass=0;pass<n;pass++){
  bool progress=false;
  for(uint i=0;i<n;i++){
   if(done[i])continue;uint at=PRIMITIVE+(q*NP+i)*10;
   if((code[at+1]==0&&bound[code[at+2]]==0)||(code[at+4]==0&&bound[code[at+5]]==0))continue;
   if(!primitive_apply(code,at,v,lo,hi,bound))return false;
   done[i]=true;progress=true;
  }
  if(!progress)break;
 }
 if(complete)for(uint i=0;i<n;i++)if(!done[i])return false;
#endif
 return true;
}
bool stage_constraints(device const uint* code,uint q,uint anchor,uint depth,thread uint* v,thread uint* lo,thread uint* hi,thread uint* bound){
 uint at=PSTAGE+((q*P+anchor)*P+depth)*2;
 for(uint i=0;i<code[at+1];i++){
  uint op=code[POPS+code[at]+i];
  if(!primitive_apply(code,PRIMITIVE+(q*NP+op)*10,v,lo,hi,bound))return false;
 }
 return true;
}
