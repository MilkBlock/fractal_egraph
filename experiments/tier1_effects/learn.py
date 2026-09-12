"""Feed extracted tier-1 Comb DAGs to the existing babble adapter.
This is a wiring/round-trip test: train/test occurrences intentionally share templates.
"""
import json
from pathlib import Path
import subprocess
ROOT=Path(__file__).resolve().parents[2]
OUT=Path(__file__).resolve().parent

def term(sort,op,args=()):return {'sort':sort,'op':str(op),'args':list(args)}
def encode(roots,graph):
    nodes=graph['nodes'];classes={}
    for ident,n in nodes.items():classes.setdefault(n['eclass'],[]).append(ident)
    chosen={c:min(ids,key=lambda i:({'SmoothComb':0,'CoarseComb':1}.get(nodes[i]['op'],0),i)) for c,ids in classes.items()}
    indices={};definitions=[];active=set()
    def node(ident):
        n=nodes[ident]
        if n['eclass'].startswith('Comb-'):return comb(n['eclass'])
        return term(n['eclass'].split('-',1)[0],n['op'],[node(c) for c in n['children']])
    def comb(c):
        if c in active:raise ValueError('cyclic Comb cannot be encoded as a finite DAG')
        if c in indices:return term('CombRef',indices[c])
        index=len(indices);indices[c]=index;definitions.append(None);active.add(c)
        n=nodes[chosen[c]]
        body=term('Comb',n['op'],[node(child) for child in n['children']])
        active.remove(c);definitions[index]=term('Definition','def',[term('CombRef',index),body])
        return term('CombRef',index)
    rootrefs=term('CombRoots','roots',[comb(root) for root in roots])
    return term('CombDAG','root',[rootrefs,term('Definitions','defs',definitions)])

def main():
    r=json.loads((OUT/'results.json').read_text())
    entries=[]
    for split in ['train','test']:
        instances=[i for i in sorted(r['instances'],key=lambda i:i['event']) if ('train' if i['event']<20 else 'test')==split]
        entries.append({'class':split+'-shared-dag','split':split,'occurrences':len(instances),'program':encode([i['template'] for i in instances],r['native_egraph'])})
    corpus={'entries':entries,'scope':'same two hand-authored templates on two data instantiations; integration smoke test, not independent generalization or tier-0 compression'}
    (OUT/'babble_corpus.json').write_text(json.dumps(corpus,indent=2)+'\n')
    subprocess.run(['cargo','build','--release','--locked','--manifest-path','tools/babble_adapter/Cargo.toml'],cwd=ROOT,check=True,timeout=180)
    subprocess.run([str(ROOT/'tools/babble_adapter/target/release/combine-babble'),str(OUT/'babble_corpus.json'),str(OUT/'babble_results.json')],cwd=ROOT,check=True,timeout=180)
    result=json.loads((OUT/'babble_results.json').read_text())
    assert result['lossless_expansion']['train'] and result['lossless_expansion']['test']
    print(json.dumps({k:result[k] for k in ['train','test','selected_train_libraries','lossless_expansion']}))
if __name__=='__main__':main()
