uint row_hash(uint t,thread long*args,device long*r,device uint*p){
 ulong h=ulong(t)+0x9e3779b97f4a7c15UL;
 for(uint c=0;c<WIDTH[t]-1;c++){ulong v=ulong(canon(r,p,t,c,args[c]));v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);}
 return uint(h^(h>>32))&(HASH_CAP-1);
}
void row_index_insert(device long*r,device uint*p,uint i){
 uint t=uint(r[i*STRIDE]);device long*index=r+(jp_active(r)&&jp_key_table(t)?JP_HASH:INDEX_OFFSET+1);long args[COLS]={0};
 for(uint c=0;c<WIDTH[t]-1;c++)args[c]=r[i*STRIDE+4+c];
 uint slot=row_hash(t,args,r,p);
 while(index[slot]){if(keyeq(r,uint(index[slot]-1),t,args,p))return;slot=(slot+1)&(HASH_CAP-1);}
 if(RC_ENABLED&&!jp_active(r)){uint n=uint(r[RC_INDEX_TOUCH]);r[RC_INDEX_TOUCH+1+n]=slot;r[RC_INDEX_TOUCH]=n+1;}
 index[slot]=long(i)+1;
}
__attribute__((noinline)) void ensure_row_index(device long*r,device uint*p,device Meta&m){
 if(jp_active(r)||r[INDEX_OFFSET]==long(m.unions)+1)return;
 r[INDEX_OFFSET+HASH_CAP+1]++;
 if(RC_ENABLED){for(uint i=0;i<uint(r[RC_INDEX_TOUCH]);i++)r[INDEX_OFFSET+1+uint(r[RC_INDEX_TOUCH+1+i])]=0;r[RC_INDEX_TOUCH]=0;}
 else for(uint i=0;i<HASH_CAP;i++)r[INDEX_OFFSET+1+i]=0;
 for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2)row_index_insert(r,p,i);
 r[INDEX_OFFSET]=long(m.unions)+1;
 r[INDEX_MERGE_VERSION]=p[MERGE_GENERATION];
}
long row_lookup(device long*r,device uint*p,device Meta&m,uint t,thread long*args){
 ensure_row_index(r,p,m);device long*index=r+(jp_active(r)&&jp_key_table(t)?JP_HASH:INDEX_OFFSET+1);uint slot=row_hash(t,args,r,p);
 while(index[slot]){uint i=uint(index[slot]-1);if(keyeq(r,i,t,args,p))return i;slot=(slot+1)&(HASH_CAP-1);}
 return -1;
}
// Match-phase keys come from canonical snapshot rows or pure primitives. This
// lookup never compresses UF paths or mutates container/index state.
long query_lookup(device long*r,uint t,thread long*args){
 ulong hash=ulong(t)+0x9e3779b97f4a7c15UL;
 for(uint c=0;c<WIDTH[t]-1;c++){ulong v=ulong(args[c]);v^=v>>30;v*=0xbf58476d1ce4e5b9UL;hash^=v+(hash<<6)+(hash>>2);}
 uint h=uint(hash^(hash>>32))&(HASH_CAP-1);device long*index=r+(jp_active(r)&&jp_key_table(t)?JP_HASH:INDEX_OFFSET+1);
 while(index[h]){uint i=uint(index[h]-1);bool same=r[i*STRIDE]==t&&r[i*STRIDE+1]!=2;
  for(uint c=0;c<WIDTH[t]-1;c++)same&=r[i*STRIDE+4+c]==args[c];
  if(same)return i;h=(h+1)&(HASH_CAP-1);
 }
 return -1;
}
