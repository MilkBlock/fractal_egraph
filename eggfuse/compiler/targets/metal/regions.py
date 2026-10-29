"""Lower analyzer-owned Spine programs; no rule-name recognition."""
HEADER = 16


def lower(plan, tables, string_ids, index_columns, cols):
    ids={t['name']:i for i,t in enumerate(tables)};nodes=plan['nodes']
    def literal(x):
        n=string_ids[x['literal']] if x['sort']['kind']=='String' else int(x['literal'])
        return '(-9223372036854775807L-1L)' if n==-(1<<63) else str(n)+'L'
    def value(x):return f'v[{x["slot"]}]' if 'slot' in x else literal(x)
    source=[f'#define RC_NODES {len(nodes)}\n#define RC_DATA {HEADER+4*len(nodes)}\n']
    source.append('uint rc_version(uint n,device long*r){switch(n){')
    for n in nodes:
        version='0u'
        for t in n['reads']:version=f'max({version},uint(r[RC_VERSION+{ids[t]}]))'
        source.append(f'case {n["id"]}:return {version};')
    source.append('default:return 0;}}')
    source.append('''
void rc_compact(device long*c,device long*r){
 bool dirty=c[12]!=0;c[12]=0;
 for(uint n=0;n<RC_NODES;n++){uint h=16+4*n;if(c[h]==1&&uint(c[h+1])!=rc_version(n,r)){c[h]=2;c[7]++;dirty=true;}}
 if(!dirty)return;
 uint end=uint(c[0]),write=RC_DATA;
 for(uint at=RC_DATA;at<end;){uint n=uint(c[at]),span=uint(c[at+1]),h=16+4*n;
  if(c[h]&&uint(c[h+2])==at+2){for(uint i=0;i<span;i++)c[write+i]=c[at+i];c[h+2]=write+2;write+=span;}at+=span;
 }
 c[0]=write;c[8]++;
}
''')
    for n in nodes:
        ni=n['id'];parent=nodes[n['parent']] if n['parent'] is not None else None
        slots=len(n['sorts']);depth=n['tables'];width=n['width'];h=HEADER+4*ni
        atom=n['atom'];tid=ids[atom['table']];args=atom['args'];known=set(range(len(parent['sorts']))) if parent else set()
        out=[f'__attribute__((noinline)) bool rc_build_{ni}(device long*r,device long*old,device Meta&m,device uint*qheads,device uint*qlinks,device long*c){{',
             f'uint version=rc_version({ni},r);if(c[{h}]==1&&uint(c[{h+1}])==version){{c[2]++;return true;}}']
        if parent:out.append(f'if(!rc_build_{parent["id"]}(r,old,m,qheads,qlinks,c))return false;')
        out.append(f'uint oldStart=uint(c[{h+2}]),oldCount=c[{h}]?uint(c[{h+3}]):0,oldCursor=0;uint begin=uint(c[0]),count=0; if(begin+2>uint(c[1])){{m.error=31;return false;}}c[0]=begin+2;c[3]++;')
        if parent:
            ph=HEADER+4*parent['id'];pw=parent['width'];ps=len(parent['sorts'])
            out.append(f'for(uint item=0;item<uint(c[{ph+3}]);item++){{uint pa=uint(c[{ph+2}])+item*{pw};long v[{max(1,slots)}]={{0}};')
            out.extend(f'v[{j}]=c[pa+{j}];' for j in range(ps))
            # Anchor fact IDs are monotonically allocated; scalar query-index
            # enumeration and keyed extension preserve their descending order.
            # Merge with the previous tuple stream, then validate witnesses via
            # the shared fact-ID -> current-row map (no table-key lookup).
            out.append(f'uint anchor=uint(c[pa+{ps+1}]);while(oldCursor<oldCount&&uint(c[oldStart+oldCursor*{width}+{slots+1}])>anchor)oldCursor++;')
            out.append(f'if(oldCursor<oldCount&&uint(c[oldStart+oldCursor*{width}+{slots+1}])==anchor){{uint prev=oldStart+oldCursor*{width};bool valid=true;')
            for j in range(depth):
                out.append(f'uint fid{j}=uint(c[prev+{slots+2*j+1}]),row{j}=uint(r[RC_FACT_ROW+fid{j}]);valid&=row{j}>0; if(row{j}>0){{row{j}--;valid&=old[row{j}*STRIDE+3]==fid{j}&&old[row{j}*STRIDE+1]==0&&old[row{j}*STRIDE+2]==c[prev+{slots+2*j}];}}')
            out.append(f'if(valid){{uint at=uint(c[0]);if(at+{width}>uint(c[1])){{c[0]=begin;m.error=31;return false;}}for(uint j=0;j<{width};j++)c[at+j]=c[prev+j];c[0]=at+{width};count++;c[11]++;continue;}}}}')
        else:out.append(f'do{{long v[{max(1,slots)}]={{0}};')
        bound=[i for i,a in enumerate(args) if 'literal' in a or a['slot'] in known]
        if all(i in bound for i in range(len(args)-1)):
            vals=','.join(value(a) for a in args[:-1]) or '0'
            out.append('long key[COLS]={'+vals+'};c[6]++;'+f'long row=query_lookup(r,{tid},key);if(row<0)continue;do{{uint ri=uint(row);')
        else:
            col=bound[-1] if bound else cols;index_columns[tid].add(col)
            head=f'qheads[query_hash({tid},{col},{value(args[col])})]' if bound else f'qheads[HASH_CAP+{tid}]'
            out.append(f'for(uint qe={head};qe!=0xffffffff;qe=qlinks[qe]){{if(qe%QSTRIDE!={col})continue;uint ri=qe/QSTRIDE;c[6]++;')
        out.append(f'if(old[ri*STRIDE]!={tid}||old[ri*STRIDE+1]!=0)continue;')
        for col,a in enumerate(args):
            cell=f'old[ri*STRIDE+4+{col}]'
            if 'literal' in a or a['slot'] in known:out.append(f'if({cell}!={value(a)})continue;')
            else:out.append(f'{value(a)}={cell};');known.add(a['slot'])
        out.append(f'uint at=uint(c[0]);if(at+{width}>uint(c[1])){{c[0]=begin;m.error=31;return false;}}')
        out.extend(f'c[at+{j}]=v[{j}];' for j in range(slots))
        if parent:
            for j in range(depth-1):out.append(f'c[at+{slots+2*j}]=c[pa+{ps+2*j}];c[at+{slots+2*j+1}]=c[pa+{ps+2*j+1}];')
        out.append(f'c[at+{slots+2*(depth-1)}]=old[ri*STRIDE+2];c[at+{slots+2*(depth-1)+1}]=old[ri*STRIDE+3];c[0]=at+{width};count++;')
        out.append('}while(false);' if all(i in bound for i in range(len(args)-1)) else '}')
        out.append('}' if parent else '}while(false);')
        out.append(f'c[begin]={ni};c[begin+1]=uint(c[0])-begin;c[{h}]=1;c[{h+1}]=version;c[{h+2}]=begin+2;c[{h+3}]=count;c[4]+=count;c[12]=1;return true;}}')
        source.extend(out)
    return '\n'.join(source)


def restore(variant, nodes, slots, rid, pivot):
    node=nodes[variant['node']];h=HEADER+4*node['id'];ns=len(node['sorts'])
    lines=[f'if(!rc_build_{node["id"]}(r,old,m,qheads,qlinks,contexts))return;',
           f'for(uint rc_i=0;rc_i<uint(contexts[{h+3}]);rc_i++){{uint rc_at=uint(contexts[{h+2}])+rc_i*{node["width"]};contexts[5]++;']
    for name,slot in variant['slots'].items():lines.append(f'v[{slots[name]}]=contexts[rc_at+{slot}];')
    stamps=[]
    for j,atom in enumerate(variant['table_atoms']):
        stamp=f'uint(contexts[rc_at+{ns+2*j}])';stamps.append(stamp)
        if pivot is not None and atom<=pivot:
            lines.append(f'if({stamp}{"<=" if atom==pivot else ">"}watermarks[{rid}])continue;')
    return lines,stamps,f'uint(contexts[rc_at+{ns+1}])'
