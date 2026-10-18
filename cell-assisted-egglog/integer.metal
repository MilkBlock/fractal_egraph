// Exact i64 operations shared by the matrix and resident-table executors.
bool nless(uint2 a,uint2 b) {int x=as_type<int>(a.y),y=as_type<int>(b.y);return x<y || (x==y && a.x<b.x);}
uint2 nneg(uint2 a) {uint lo=0u-a.x;return uint2(lo,~a.y+uint(a.x==0));}
uint3 nadd(uint2 a,uint2 b) {
  uint lo=a.x+b.x;uint hi=a.y+b.y+uint(lo<a.x);
  bool ok=((a.y^hi)&(b.y^hi)&0x80000000u)==0;return uint3(lo,hi,uint(ok));
}
uint3 nsub(uint2 a,uint2 b) {
  uint lo=a.x-b.x,hi=a.y-b.y-uint(a.x<b.x);
  bool ok=((a.y^b.y)&(a.y^hi)&0x80000000u)==0;return uint3(lo,hi,uint(ok));
}
// Explicit 16-bit partial products avoid an AGX compiler failure on mulhi in
// this dynamic operator graph. All intermediates here fit unsigned 32 bits.
uint nhi(uint a,uint b) {
  uint a0=a&65535u,a1=a>>16,b0=b&65535u,b1=b>>16;
  uint low=a0*b0,t=a1*b0+(low>>16),middle=(t&65535u)+a0*b1;
  return a1*b1+(t>>16)+(middle>>16);
}
uint3 nmul(uint2 a,uint2 b) {
  bool neg=((a.y^b.y)&0x80000000u)!=0;
  if(a.y&0x80000000u)a=nneg(a);if(b.y&0x80000000u)b=nneg(b);
  uint lo=a.x*b.x,hi=nhi(a.x,b.x),p=a.x*b.y,q=a.y*b.x;
  bool ok=!(a.y && b.y) && nhi(a.x,b.y)==0 && nhi(a.y,b.x)==0;
  uint next=hi+p;ok&=next>=hi;hi=next;next=hi+q;ok&=next>=hi;hi=next;
  ok&=hi<0x80000000u || (neg && hi==0x80000000u && lo==0);
  uint2 value=uint2(lo,hi);return uint3(neg?nneg(value):value,uint(ok));
}
