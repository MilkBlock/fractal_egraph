"""Diagnose residual errors without using held-out labels to choose features."""
import argparse
from collections import Counter
import json
from pathlib import Path
import numpy as np
import train
from features import constructors,analyze_vocabulary,native_graph


def setup(snapshot,profile,source,queue):
    tables,header=train.memory_run.response_run.load(snapshot,profile)
    bank=train.memory_run.ResponseBank(header['queries'],tables).encode()
    groups,routes,_=train.memory_run.prepare(bank)
    allowed=constructors(source);vocab,evidence=analyze_vocabulary(queue,allowed)
    graph=native_graph(tables,allowed)
    return bank,groups,routes,allowed,vocab,graph


def diagnose(bank,groups,routes,coords,predictions,scope):
    errors=Counter();missed_by_rule=Counter();examples={};total_missed=0
    false_errors=Counter();false_examples={}
    for name,g in bank.groups.items():
        exact=g['x'].astype(bool);guess=exact.copy();affected=np.zeros_like(exact)
        for col in range(exact.shape[1]):
            if (name,col) not in routes:continue
            t,c,ids=routes[name,col];guess[:,col]=predictions[t][0][ids,c]>=.5
            affected[:,col]=scope[t][ids]
        for q,need in enumerate(g['requirements']):
            need=sorted(need);selected=affected[:,need].any(1)
            failed=np.flatnonzero(selected&g['answer'][:,q]&~guess[:,need].all(1))
            total_missed+=len(failed);qi=g['rules'][q];missed_by_rule[qi]+=len(failed)
            for row in failed:
                for col in need:
                    if guess[row,col] or (name,col) not in routes:continue
                    t,c,ids=routes[name,col];index=groups[t]['indices'][c]
                    key=(t,index[0],index[3]);errors[key]+=1
                    if key not in examples:
                        value=groups[t]['query_values'][ids[row]]
                        examples[key]={'key':value,'query':qi,'rule':bank.queries[qi]['rule'],
                            'score':float(predictions[t][0][ids[row],c]),'heldout_key':bool(scope[t][ids[row]])}
            false=np.flatnonzero(selected&~g['answer'][:,q]&guess[:,need].all(1))
            for row in false:
                for col in need:
                    if exact[row,col] or (name,col) not in routes:continue
                    t,c,ids=routes[name,col];index=groups[t]['indices'][c];key=(t,index[0],index[3])
                    false_errors[key]+=1
                    false_examples.setdefault(key,{'key':groups[t]['query_values'][ids[row]],'query':qi,
                        'rule':bank.queries[qi]['rule'],'score':float(predictions[t][0][ids[row],c]),
                        'heldout_key':bool(scope[t][ids[row]])})
    contradictions=[]
    for t,g in groups.items():
        c=coords[t];pop=np.bincount(c['domain'],minlength=len(c['x']))
        count=np.zeros((len(c['x']),g['payload'].shape[1]),dtype=np.int64);np.add.at(count,c['domain'],g['payload'])
        unavoidable=np.minimum(count,pop[:,None]-count)
        for ch,index in enumerate(g['indices']):
            n=int(unavoidable[:,ch].sum())
            if n:contradictions.append({'schema':t,'table':index[0],'kept_slots':index[3],
                'oracle_minimum_errors_for_any_signature_only_classifier':n})
    return {'missed_rule_anchor_pairs':total_missed,
        'failed_conditions':[{'schema':k[0],'table':k[1],'kept_slots':k[2],
            'responsible_occurrences':v,'example':examples[k]} for k,v in errors.most_common()],
        'rules':[{'id':q,'missed':n,'source':bank.queries[q]['rule']} for q,n in missed_by_rule.most_common() if n],
        'false_candidate_conditions':[{'schema':k[0],'table':k[1],'kept_slots':k[2],
            'responsible_occurrences':v,'example':false_examples[k]} for k,v in false_errors.most_common()],
        'ambiguous_conditions':sorted(contradictions,key=lambda x:x['oracle_minimum_errors_for_any_signature_only_classifier'],reverse=True),
        'limits':['Several failed conditions can explain one missed rule/anchor pair; counts are not additive.',
            'Ambiguity lower bounds use all labeled keys for diagnosis only and are not supplied to training.',
            'This is the same frozen graph, not a new workload or a runtime profiler.']}


def main():
    p=argparse.ArgumentParser()
    for n in ('snapshot','profile','source','analyze-queue','output'):p.add_argument('--'+n,type=Path,required=True)
    a=p.parse_args();bank,groups,routes,allowed,vocab,graph=setup(a.snapshot,a.profile,a.source,a.analyze_queue)
    coords=train.coordinates(groups,graph.tokens(2,vocab));predictions={};scope={}
    for t,g in groups.items():
        held,scope[t]=train.heldout_split(g,coords[t],'new_binding')
        scores,prior,_=train.fit(g,coords[t],held);predictions[t]=(scores,prior)
    report=diagnose(bank,groups,routes,coords,predictions,scope)
    a.output.parent.mkdir(parents=True,exist_ok=True);a.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'missed':report['missed_rule_anchor_pairs'],
        'conditions':[{k:v for k,v in x.items() if k!='example'} for x in report['failed_conditions'][:12]],
        'ambiguous':report['ambiguous_conditions'][:8]},indent=2))


if __name__=='__main__':main()
