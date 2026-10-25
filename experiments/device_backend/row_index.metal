uint row_hash(uint t,thread long*args,device long*r,device uint*p){
 ulong h=ulong(t)+0x9e3779b97f4a7c15UL;
 for(uint c=0;c<WIDTH[t]-1;c++){ulong v=ulong(canon(r,p,t,c,args[c]));v^=v>>30;v*=0xbf58476d1ce4e5b9UL;h^=v+(h<<6)+(h>>2);}
 return uint(h^(h>>32))&(HASH_CAP-1);
}
void row_index_insert(device long*r,device uint*p,uint i){
 device long*index=r+INDEX_OFFSET+1;uint t=uint(r[i*STRIDE]);long args[COLS]={0};
 for(uint c=0;c<WIDTH[t]-1;c++)args[c]=r[i*STRIDE+4+c];
 uint slot=row_hash(t,args,r,p);
 while(index[slot]){if(keyeq(r,uint(index[slot]-1),t,args,p))return;slot=(slot+1)&(HASH_CAP-1);}
 index[slot]=long(i)+1;
}
__attribute__((noinline)) void ensure_row_index(device long*r,device uint*p,device Meta&m){
 if(r[INDEX_OFFSET]==long(m.unions)+1)return;
 for(uint i=0;i<HASH_CAP;i++)r[INDEX_OFFSET+1+i]=0;
 for(uint i=0;i<m.count;i++)if(r[i*STRIDE+1]!=2)row_index_insert(r,p,i);
 r[INDEX_OFFSET]=long(m.unions)+1;
}
long row_lookup(device long*r,device uint*p,device Meta&m,uint t,thread long*args){
 ensure_row_index(r,p,m);device long*index=r+INDEX_OFFSET+1;uint slot=row_hash(t,args,r,p);
 while(index[slot]){uint i=uint(index[slot]-1);if(keyeq(r,i,t,args,p))return i;slot=(slot+1)&(HASH_CAP-1);}
 return -1;
}
