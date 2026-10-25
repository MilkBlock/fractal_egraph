// Snapshot-only chains: entries identify both row and column, so hash collisions
// across tables/columns never splice incompatible next pointers.
uint query_hash(uint table,uint column,long value){
 ulong x=ulong(value) ^ (ulong(table+1)*0x9e3779b97f4a7c15UL) ^ (ulong(column+1)*0xbf58476d1ce4e5b9UL);
 x^=x>>30;x*=0xbf58476d1ce4e5b9UL;x^=x>>27;x*=0x94d049bb133111ebUL;x^=x>>31;
 return uint(x)&(HASH_CAP-1);
}
__attribute__((noinline)) void build_query_index(device long*old,uint count,device uint*heads,device uint*links){
 for(uint j=0;j<HASH_CAP+TABLE_COUNT;j++)heads[j]=0xffffffff;
 // Descending insertion preserves the original ascending physical-row order.
 for(uint ii=count;ii>0;ii--){uint i=ii-1;if(old[i*STRIDE+1]!=0)continue;uint t=uint(old[i*STRIDE]);
  uint e=i*QSTRIDE+COLS;links[e]=heads[HASH_CAP+t];heads[HASH_CAP+t]=e;
  for(uint c=0;c<WIDTH[t];c++){uint h=query_hash(t,c,old[i*STRIDE+4+c]);e=i*QSTRIDE+c;links[e]=heads[h];heads[h]=e;}
 }
}
