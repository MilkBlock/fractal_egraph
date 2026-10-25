// Snapshot-only chains: entries identify both row and column, so hash collisions
// across tables/columns never splice incompatible next pointers.
uint query_hash(uint table,uint column,long value){
 ulong x=ulong(value) ^ (ulong(table+1)*0x9e3779b97f4a7c15UL) ^ (ulong(column+1)*0xbf58476d1ce4e5b9UL);
 x^=x>>30;x*=0xbf58476d1ce4e5b9UL;x^=x>>27;x*=0x94d049bb133111ebUL;x^=x>>31;
 return uint(x)&(HASH_CAP-1);
}
void append_query_link(device uint*heads,device uint*links,uint h,uint e){
 links[e]=0xffffffff;
 if(heads[h]==0xffffffff)heads[h]=e;else links[heads[QBUCKETS+h]]=e;
 heads[QBUCKETS+h]=e;
}
__attribute__((noinline)) void build_query_index(device long*old,uint start,uint count,bool reset,device uint*heads,device uint*links){
 if(reset){for(uint j=0;j<2*QBUCKETS;j++)heads[j]=0xffffffff;for(uint t=0;t<TABLE_COUNT;t++)heads[2*QBUCKETS+1+t]=0;}
 // Tail insertion preserves physical-row order across append-only updates.
 for(uint i=start;i<count;i++){if(old[i*STRIDE+1]!=0)continue;uint t=uint(old[i*STRIDE]);
  heads[2*QBUCKETS+1+t]=max(heads[2*QBUCKETS+1+t],uint(old[i*STRIDE+2]));
  if(INDEX_COLUMN[t*QSTRIDE+COLS])append_query_link(heads,links,HASH_CAP+t,i*QSTRIDE+COLS);
  if(INDEX_COLUMN[t*QSTRIDE+TIME_COLUMN])append_query_link(heads,links,query_hash(t,TIME_COLUMN,old[i*STRIDE+2]),i*QSTRIDE+TIME_COLUMN);
  for(uint c=0;c<WIDTH[t];c++)if(INDEX_COLUMN[t*QSTRIDE+c])append_query_link(heads,links,query_hash(t,c,old[i*STRIDE+4+c]),i*QSTRIDE+c);
 }
 heads[2*QBUCKETS]=count;
}

// Enumerate only timestamp buckets newer than this rule's watermark.
// Timestamp buckets share the hash arena, so verify both entry kind and key.
uint delta_first(uint t,uint limit,thread uint&stamp,device long*old,device uint*heads,device uint*links){
 while(stamp<limit){
  uint e=heads[query_hash(t,TIME_COLUMN,long(stamp))];
  while(e!=0xffffffff){uint i=e/QSTRIDE;
   if(e%QSTRIDE==TIME_COLUMN&&old[i*STRIDE]==t&&uint(old[i*STRIDE+2])==stamp)return e;
   e=links[e];
  }
  stamp++;
 }
 return 0xffffffff;
}
uint delta_next(uint t,uint limit,uint current,thread uint&stamp,device long*old,device uint*heads,device uint*links){
 uint e=links[current];
 while(e!=0xffffffff){uint i=e/QSTRIDE;
  if(e%QSTRIDE==TIME_COLUMN&&old[i*STRIDE]==t&&uint(old[i*STRIDE+2])==stamp)return e;
  e=links[e];
 }
 stamp++;return delta_first(t,limit,stamp,old,heads,links);
}
