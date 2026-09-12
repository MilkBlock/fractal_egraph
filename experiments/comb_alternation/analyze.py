"""Equal-length, matched-rule-multiset comparison of witnessed dependency paths."""
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path


def stable(x):
    return json.dumps(x, sort_keys=True, separators=(',', ':'))


def category(seq):
    counts = Counter(seq)
    if len(counts) == 1:
        return 'repeat_only'
    if len(counts) != 2 or len(set(counts.values())) != 1:
        return 'other'
    runs = 1 + sum(a != b for a, b in zip(seq, seq[1:]))
    if runs == 2:
        return 'blocked'
    if runs == len(seq):
        return 'alternating'
    return 'other'


def extract(profile, length, cap=1000000):
    events = {e['id']: e for e in profile['events'] if e['round'] is not None}
    incoming = defaultdict(dict)
    for w in profile['witnesses']:
        c = w['consumer']
        if c not in events:
            continue
        for edge in w['support']:
            p = edge['producer']
            if p in events and events[p]['scope'] == events[c]['scope']:
                incoming[c].setdefault(p, []).append(edge)
    paths = []
    def walk(back):
        if len(paths) >= cap:
            raise RuntimeError('path budget exceeded; no complete estimate')
        if len(back) == length:
            paths.append(tuple(reversed(back)))
            return
        for p in sorted(incoming[back[-1]]):
            if p not in back:
                walk(back + [p])
    for root in sorted(events):
        walk([root])
    output = []
    for path in paths:
        es = [events[i] for i in path]
        rounds = [e['round'] for e in es]
        # Hold out new root events; shared earlier ancestors are reported explicitly.
        assert rounds == sorted(rounds), 'dependency crosses rounds backwards'
        split = 'train' if rounds[-1] <= 4 else 'test'
        seq = [e['rule'] for e in es]
        aliases = {}
        bindings = []
        concrete = []
        for e in es:
            b = {k:v for k,v in e['bindings'].items() if not k.startswith('@')}
            concrete.append(b)
            bindings.append({k:aliases.setdefault(v,len(aliases)) for k,v in sorted(b.items())})
        wiring = []
        for p,c in zip(path,path[1:]):
            wiring.append(sorted([{'table':a['table'],'producer_sites':a['producer_sites'],
                                   'consumer_sites':a['consumer_sites'],'kind':a['kind']}
                                  for a in incoming[c][p]], key=stable))
        shape = stable({'rules':seq,'bindings':bindings,'connections':wiring})
        external = sum(len(set(incoming[c]) - set(path[:i])) for i,c in enumerate(path[1:],1))
        output.append({'path':path,'rules':seq,'category':category(seq), 'split':split,
                       'pair':','.join(sorted(set(seq))), 'shape':shape,
                       'binding':stable(concrete), 'root':path[-1], 'entry':path[0],
                       'writes':sorted({a['write'] for p,c in zip(path,path[1:]) for a in incoming[c][p]}),
                       'productive':es[-1]['productive'], 'external_producers':external, 'cross_boundary':min(rounds)<=4<max(rounds)})
    return output


def summarize(rows, k=8):
    train = [r for r in rows if r['split']=='train']
    test = [r for r in rows if r['split']=='test']
    counts = Counter(r['shape'] for r in train)
    chosen = set(sorted(counts, key=lambda s:(-counts[s],s))[:k])
    hits = [r for r in test if r['shape'] in chosen]
    def stats(xs):
        return {'paths':len(xs),'distinct_roots':len({r['root'] for r in xs}),
                'productive_roots':len({r['root'] for r in xs if r['productive']}),
                'distinct_entries':len({r['entry'] for r in xs}),
                'distinct_concrete_bindings':len({r['binding'] for r in xs}),
                'distinct_templates':len({r['shape'] for r in xs}),
                'covered_events':len({i for r in xs for i in r['path']}),
                'covered_producer_write_ids':len({i for r in xs for i in r['writes']})}
    sequence_counts=Counter(stable(r['rules']) for r in train)
    selected_sequences=set(sorted(sequence_counts,key=lambda s:(-sequence_counts[s],s))[:k])
    sequence_hits=[r for r in test if stable(r['rules']) in selected_sequences]
    return {'rule_names_only_ablation':{'selected_templates':len(selected_sequences),'test_hits':stats(sequence_hits),'scope':'ignores binding wiring; optimistic control only'},'train':stats(train),'test':stats(test),'cross_boundary_paths':sum(r['cross_boundary'] for r in rows),
            'library_budget':k,'selected_templates':len(chosen),'test_hits':stats(hits),
            'test_path_hit_rate':len(hits)/len(test) if test else None,
            'top_train':[{'support':counts[s],'template':json.loads(s)} for s in sorted(chosen,key=lambda s:(-counts[s],s))],
            'example_paths':[{'events':r['path'],'rules':r['rules'],'external_producers':r['external_producers']} for r in rows[:3]]}


def analyze(profile):
    out = {'scope':'observational native row-provenance paths; basic rules are one-step family proxies, not certified fractal macros',
           'split':'root events in rounds 1-4 train, rounds 5-6 heldout; earlier ancestors allowed and overlap reported. Strict all-heldout-event path counts are reported separately.',
           'normalization':'rule identity + endpoint AST sites + cross-stage binding value aliasing; external branches remain unmodeled, not closed shortcut rules',
           'controls':'same path length and same two-rule multiset; top-8 templates per arm learned on training only',
           'lengths':{}}
    edges={(a['producer'],w['consumer']) for w in profile['witnesses'] for a in w['support']}
    events=sorted([e for e in profile['events'] if e['round'] is not None],key=lambda e:e['id'])
    negative=Counter()
    for i in range(len(events)-3):
        window=events[i:i+4]
        if len({e['scope'] for e in window})!=1: continue
        c=category([e['rule'] for e in window]);negative[c]+=1
        if all((a['id'],b['id']) in edges for a,b in zip(window,window[1:])): negative[c+'_dependency_confirmed']+=1
    out['event_id_adjacency_negative_control']=dict(negative)
    for length in [4,6]:
        rows = extract(profile,length)
        groups = {c:summarize([r for r in rows if r['category']==c]) for c in ['blocked','alternating','repeat_only','other']}
        pairs = {}
        for pair in sorted({r['pair'] for r in rows if r['category'] in ['blocked','alternating']}):
            subsets = {c:[r for r in rows if r['category']==c and r['pair']==pair] for c in ['blocked','alternating']}
            k = min(8, *(len({r['shape'] for r in xs if r['split']=='train'}) for xs in subsets.values()))
            pairs[pair] = {'common_library_budget':k, **{c:summarize(xs,k) for c,xs in subsets.items()}}
        out['lengths'][str(length)] = {'total_paths':len(rows),'strict_all_events_heldout_paths':sum(r['split']=='test' and not r['cross_boundary'] for r in rows),'groups':groups,'matched_pairs':pairs}
    return out


if __name__ == '__main__':
    p=argparse.ArgumentParser();p.add_argument('profile');p.add_argument('output');a=p.parse_args()
    result=analyze(json.loads(Path(a.profile).read_text()))
    Path(a.output).write_text(json.dumps(result,ensure_ascii=False,indent=2)+'\n')
