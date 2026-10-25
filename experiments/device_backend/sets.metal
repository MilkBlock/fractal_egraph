// Variable-sized immutable set records in a device arena following the row pool.
// Header: allocated length, live length, sort ID, last normalization epoch.
// Rebuild may canonicalize/deduplicate elements in place; handles remain valid.
long set_canon(device long*r,device uint*p,long h){
 device long*s=r+ROW_WORDS;uint n=uint(s[h+1]);bool changed=false;
 for(uint i=0;i<n;i++){long v=root(p,uint(s[h+4+i]));changed|=v!=s[h+4+i];s[h+4+i]=v;}
 for(uint i=1;i<n;i++){long v=s[h+4+i];uint j=i;while(j&&s[h+4+j-1]>v){s[h+4+j]=s[h+4+j-1];j--;}s[h+4+j]=v;}
 uint count=0;for(uint i=0;i<n;i++)if(!count||s[h+4+i]!=s[h+4+count-1])s[h+4+count++]=s[h+4+i];
 changed|=count!=n;s[h+1]=count;if(changed)s[h+3]=s[1];
 for(long q=2;q<h;q+=s[q]+4){
  // Earlier records are normalized by the rebuild arena pass before row repair.
  if(s[q+2]!=s[h+2]||s[q+1]!=count)continue;
  bool same=true;for(uint i=0;i<count;i++)same&=s[q+4+i]==s[h+4+i];if(same)return q;
 }
 return h;
}
long set_apply(device long*r,device uint*p,device Meta&m,uint op,uint sortid,long a,long b){
 device long*s=r+ROW_WORDS;
 if(op!=0)a=set_canon(r,p,a);
 if(op==2||op==3)b=set_canon(r,p,b);
 if(op==4)return s[a+1];
 uint na=op==0?0:uint(s[a+1]),nb=(op==2||op==3)?uint(s[b+1]):0;
 uint reserve=na+nb+(op==1?1:0);long h=s[0];
 if(h+4+reserve>SET_WORDS){m.error=9;return 0;}
 s[h]=reserve;s[h+1]=0;s[h+2]=sortid;s[h+3]=m.epoch;
 uint n=0;
 for(uint i=0;i<na;i++){
  long v=s[a+4+i];bool keep=true;
  if(op==3){keep=false;for(uint j=0;j<nb;j++)keep|=v==s[b+4+j];}
  if(keep)s[h+4+n++]=v;
 }
 if(op==1)s[h+4+n++]=root(p,uint(b));
 if(op==2)for(uint j=0;j<nb;j++)s[h+4+n++]=s[b+4+j];
 s[h+1]=n;long result=set_canon(r,p,h);
 if(result==h)s[0]=h+4+reserve;
 return result;
}
