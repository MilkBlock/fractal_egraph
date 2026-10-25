"""Export the existing speculative spine model for a live native query hook.

Selection is structural: ordinary constructor anchor, >=2 unary analysis
conditions, no guard/residual. No predicate or rule name selects a plan.
"""
import argparse, hashlib, json, sys
from pathlib import Path
import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent/'structural_key_memory'))
import train
import summary_features
from features import constructors, analyze_vocabulary, native_graph
from model import choose_anchor, local_key
from memory import fingerprint

def main():
    p=argparse.ArgumentParser()
    for n in ('snapshot','profile','source','analyze-queue','output'):
        p.add_argument('--'+n, type=Path, required=True)
    p.add_argument('--residuals',choices=['relations','all'],default='relations',help='all also admits constructor/function residuals under the same checked anchor contract')
    p.add_argument('--summary-markers',type=Path,help='existing solver-generated shared-markers.egg')
    p.add_argument('--base-model',type=Path,help='unchanged model for paired live audit')
    a=p.parse_args()
    tables, header=train.memory_run.response_run.load(a.snapshot,a.profile)
    bank=train.memory_run.ResponseBank(header['queries'],tables)
    groups,_,_=train.memory_run.prepare(bank)
    allowed=constructors(a.source)
    vocab,evidence=analyze_vocabulary(a.analyze_queue,allowed)
    tokens=native_graph(tables,allowed).tokens(2,vocab)
    coords=train.coordinates(groups,tokens)
    models={}
    for t,g in groups.items():
        held,_=train.heldout_split(g,coords[t],'new_binding')
        _,_,models[t]=train.fit(g,coords[t],held)
    plans=[]; channels={}
    for q in header['queries']:
        shape=q['shape']; ai=choose_anchor(shape)
        if q['ruleset']!='always-run' or ai is None or len(shape['atoms'])<3: continue
        anchor=shape['atoms'][ai]
        if a.residuals=='relations' and anchor['table'] not in allowed: continue
        eq_sorts={out for _,out in allowed.values()}
        if any(sort not in eq_sorts|{'i64','f64','String','bool','Unit'} and not sort.startswith('___') for atom in shape['atoms'] for sort in atom['sorts']):continue
        av={c['Slot'] for c in anchor['columns'] if 'Slot' in c}
        steps=[]
        for i,atom in enumerate(shape['atoms']):
            if i==ai:continue
            # Native relations are lowered to constructors of a generated sort.
            if a.residuals=='relations' and not atom['sorts'][-1].startswith('___'):break
            (table,sorts,cols),ids=local_key(atom)
            bound=[s for s in ids if s in av]
            if len(bound)!=1:break
            key=(table,sorts,cols,tuple(ids[s] for s in bound))
            t=train.memory_run.schema(key)
            if t not in groups or len(t)!=1 or t[0] not in eq_sorts:break
            ch=groups[t]['indices'].index(key)
            channels.setdefault((t,ch),len(channels))
            steps.append({'atom':i,'slot':bound[0],'model':channels[t,ch]})
        else:
            plans.append({'name':q['rule'],'ruleset':q['ruleset'],
                          'shape':shape,'anchor':ai,'steps':steps})
    vocabulary=sorted(vocab,key=repr)
    definitions=[];summary_evidence=[];summary_tokens={}
    if a.summary_markers:
        schemas={atom['table']:atom['sorts'] for q in header['queries'] for atom in q['shape']['atoms']}
        definitions=summary_features.load(a.summary_markers,schemas)
        summary_tokens,extra,summary_evidence=summary_features.tokens(definitions,tables,{t[0] for t,ch in channels})
        vocabulary+=extra
    exported=[];coordinate_cache={}
    for (t,ch),mid in channels.items():
        channel=groups[t]['indices'][ch][0]
        masked=summary_features.excluded(definitions,vocabulary,channel)
        m=models[t];column=ch
        useful=any(d['sorts'][s]==t[0] and ('summary',d['identity'],s) in vocabulary and vocabulary.index(('summary',d['identity'],s)) not in masked for d in definitions for s in d['carried'])
        if useful:
            mask=tuple(masked)
            if mask not in coordinate_cache:
                blocked={vocabulary[i] for i in masked}
                enriched={v:set(ts) | (summary_tokens.get(v,set())-blocked) for v,ts in tokens.items()}
                for v,ts in summary_tokens.items():enriched.setdefault(v,set()).update(ts-blocked)
                coordinate_cache[mask]=train.coordinates(groups,enriched)
            c=coordinate_cache[mask][t];held,_=train.heldout_split(groups[t],c,'new_binding')
            g=dict(groups[t],payload=groups[t]['payload'][:,ch:ch+1])
            _,_,m=train.fit(g,c,held);column=0
        exported.append({'id':mid,'sort':t[0],'channel':groups[t]['indices'][ch][0],
                         'excluded_tokens':masked,'weights':m['w'][:,column].view(np.uint16).tolist(),
                         'bias':float(m['bias'][column]),
                         'sort_hash':fingerprint((0,'sort',t[0]))})
    if definitions:
        if not a.base_model:raise ValueError('summary mode needs the unchanged base model for paired audit')
        base=json.loads(a.base_model.read_text())
        if base['source_sha256']!=hashlib.sha256(a.source.read_bytes()).hexdigest() or base['training_snapshot_sha256']!=hashlib.sha256(a.snapshot.read_bytes()).hexdigest() or base['plans']!=plans:
            raise ValueError('base model source/snapshot/query plans differ')
        if [v['token'] for v in base['vocabulary']]!=[list(t) for t in vocabulary[:len(vocab)]]:
            raise ValueError('base vocabulary differs')
        primary=len(exported)
        for i,m in enumerate(base['models']):
            if m['channel']!=exported[i]['channel'] or m['sort']!=exported[i]['sort']:raise ValueError('base model channel mismatch')
            exported[i]['reference_model']=i+primary
            exported.append(dict(m,id=i+primary,excluded_tokens=list(range(len(vocab),len(vocabulary)))))
    result={'schema':'live-spine-proposals/v1','width':256,'plans':plans,'models':exported,
            'vocabulary':[{'token':t,'hash':fingerprint((0,*t))} for t in vocabulary],
            'summary_markers':definitions,'summary_evidence':summary_evidence,
            'summary_source_sha256':hashlib.sha256(a.summary_markers.read_bytes()).hexdigest() if a.summary_markers else None,
            'constructors':allowed,'selection':a.residuals,'source_sha256':hashlib.sha256(a.source.read_bytes()).hexdigest(),
            'training_snapshot_sha256':hashlib.sha256(a.snapshot.read_bytes()).hexdigest(),
            'analyze_queue_sha256':evidence['queue_sha256'],
            'scope':'Speculative fixed model, not closure proof. Same-program final-snapshot training; current Run answers are never input features.'}
    if definitions:result['scope']+=' Solver-selected typed joint spine features are live inputs. Target-dependent markers masked per channel; no new closure proof.'
    a.output.parent.mkdir(parents=True,exist_ok=True)
    a.output.write_text(json.dumps(result,separators=(',',':'))+'\n')
    print(json.dumps({'plans':len(plans),'models':len(exported),'vocabulary':len(vocabulary),'bytes':a.output.stat().st_size}))

if __name__=='__main__':main()
