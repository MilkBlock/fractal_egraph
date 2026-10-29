// Snapshot-only chains: entries identify both row and column, so hash collisions
// across tables/columns never splice incompatible next pointers.
uint query_hash(uint table,uint column,long value){
 ulong x=ulong(value) ^ (ulong(table+1)*0x9e3779b97f4a7c15UL) ^ (ulong(column+1)*0xbf58476d1ce4e5b9UL);
 x^=x>>30;x*=0xbf58476d1ce4e5b9UL;x^=x>>27;x*=0x94d049bb133111ebUL;x^=x>>31;
 return uint(x)&(HASH_CAP-1);
}
// Resident matching uses generation-tagged heads. Rebuild may use the head
// storage as scratch; advancing the generation invalidates every old bucket
// without a serial scan over the maximum-capacity allocation.
uint query_head(device uint*heads,uint h){return RC_ENABLED&&!PARALLEL_PREPARE&&heads[Q_TOUCH+2+h]!=heads[Q_TOUCH]?0xffffffff:heads[h];}
// Reverse links are needed only for output/timestamp entries. Input columns
// never move without rebuild, so do not allocate a reverse link per column.
uint query_reverse_slot(device long*rows,uint e){
 uint i=e/QSTRIDE,c=e%QSTRIDE;
 if(c==TIME_COLUMN)return i*2+1;
 if(c==WIDTH[uint(rows[i*STRIDE])]-1)return i*2;
 return 0xffffffff;
}
void set_query_previous(device long*rows,device uint*previous,uint e,uint before){
 if(e==0xffffffff)return;uint slot=query_reverse_slot(rows,e);
 if(slot!=0xffffffff)previous[slot]=before;
}
void append_query_link(device long*rows,device uint*heads,device uint*links,device uint*previous,uint h,uint e){
 if(RC_ENABLED&&!PARALLEL_PREPARE&&heads[Q_TOUCH+2+h]!=heads[Q_TOUCH]){heads[h]=0xffffffff;heads[QBUCKETS+h]=0xffffffff;heads[Q_TOUCH+2+h]=heads[Q_TOUCH];}
 links[e]=0xffffffff;
 set_query_previous(rows,previous,e,heads[QBUCKETS+h]);
 if(heads[h]==0xffffffff){heads[h]=e;}else links[heads[QBUCKETS+h]]=e;
 heads[QBUCKETS+h]=e;
}
__attribute__((noinline)) void build_query_index(device long*old,uint start,uint count,bool reset,device uint*heads,device uint*links,device uint*previous){
 if(reset){
  if(RC_ENABLED&&!PARALLEL_PREPARE){if(++heads[Q_TOUCH]==0){for(uint h=0;h<QBUCKETS;h++)heads[Q_TOUCH+2+h]=0;heads[Q_TOUCH]=1;}}
  else for(uint j=0;j<2*QBUCKETS;j++)heads[j]=0xffffffff;
  for(uint t=0;t<TABLE_COUNT;t++)heads[2*QBUCKETS+1+t]=0;
 }
 // Tail insertion preserves physical-row order across append-only updates.
 for(uint i=start;i<count;i++){if(old[i*STRIDE+1]!=0)continue;uint t=uint(old[i*STRIDE]);
  heads[2*QBUCKETS+1+t]=max(heads[2*QBUCKETS+1+t],uint(old[i*STRIDE+2]));
  if(INDEX_COLUMN[t*QSTRIDE+COLS])append_query_link(old,heads,links,previous,HASH_CAP+t,i*QSTRIDE+COLS);
  if(INDEX_COLUMN[t*QSTRIDE+TIME_COLUMN])append_query_link(old,heads,links,previous,query_hash(t,TIME_COLUMN,old[i*STRIDE+2]),i*QSTRIDE+TIME_COLUMN);
  for(uint c=0;c<WIDTH[t];c++)if(INDEX_COLUMN[t*QSTRIDE+c])append_query_link(old,heads,links,previous,query_hash(t,c,old[i*STRIDE+4+c]),i*QSTRIDE+c);
 }
 heads[2*QBUCKETS]=count;
}

void move_query_link(device long*rows,device uint*heads,device uint*links,device uint*previous,uint e,uint from,uint to){
 if(from==to)return;
 if(RC_ENABLED&&!PARALLEL_PREPARE&&heads[Q_TOUCH+2+to]!=heads[Q_TOUCH]){
  heads[to]=0xffffffff;heads[QBUCKETS+to]=0xffffffff;heads[Q_TOUCH+2+to]=heads[Q_TOUCH];
 }
 uint before=previous[query_reverse_slot(rows,e)],after=links[e];
 if(before==0xffffffff)heads[from]=after;else links[before]=after;
 if(after==0xffffffff)heads[QBUCKETS+from]=before;else set_query_previous(rows,previous,after,before);
 // Matching carries explicit loop-row coordinates in the new resident ABI.
 // A patch can therefore insert at the head in O(1); the resulting bindings
 // are sorted by those coordinates, not by their discovery/arrival order.
 if(!QUERY_PATCH_SORTED){
  after=heads[to];links[e]=after;set_query_previous(rows,previous,e,0xffffffff);
  if(after==0xffffffff)heads[QBUCKETS+to]=e;else set_query_previous(rows,previous,after,e);
  heads[to]=e;return;
 }
 // Retain physical-row traversal order, including partial-key hash collisions.
 // Appending or prepending is constant time; middle insertion scans that bucket.
 uint tail=heads[QBUCKETS+to];
 if(tail==0xffffffff||tail<e){append_query_link(rows,heads,links,previous,to,e);return;}
 before=0xffffffff;after=heads[to];
 while(after!=0xffffffff&&after<e){before=after;after=links[after];}
 if(before==0xffffffff)heads[to]=e;else links[before]=e;
 links[e]=after;set_query_previous(rows,previous,e,before);
 if(after==0xffffffff)heads[QBUCKETS+to]=e;else set_query_previous(rows,previous,after,e);
}
__attribute__((noinline)) void patch_query_index(device long*rows,device uint*heads,device uint*links,device uint*previous){
 for(uint n=0;n<uint(rows[QUERY_LOG]);n++){
  uint i=uint(rows[QUERY_CHANGED+n]),t=uint(rows[i*STRIDE]),out=WIDTH[t]-1;
  if(INDEX_COLUMN[t*QSTRIDE+out])move_query_link(rows,heads,links,previous,i*QSTRIDE+out,
   query_hash(t,out,rows[QUERY_OLD_VALUE+i]),query_hash(t,out,rows[i*STRIDE+4+out]));
  if(INDEX_COLUMN[t*QSTRIDE+TIME_COLUMN])move_query_link(rows,heads,links,previous,i*QSTRIDE+TIME_COLUMN,
   query_hash(t,TIME_COLUMN,rows[QUERY_OLD_TIME+i]),query_hash(t,TIME_COLUMN,rows[i*STRIDE+2]));
  heads[2*QBUCKETS+1+t]=max(heads[2*QBUCKETS+1+t],uint(rows[i*STRIDE+2]));
 }
}

// Enumerate only timestamp buckets newer than this rule's watermark.
// Timestamp buckets share the hash arena, so verify both entry kind and key.
uint delta_first(uint t,uint limit,thread uint&stamp,device long*old,device uint*heads,device uint*links){
 while(stamp<limit){
  uint e=query_head(heads,query_hash(t,TIME_COLUMN,long(stamp)));
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
