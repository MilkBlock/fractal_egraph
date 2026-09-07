#!/usr/bin/env python3
"""Generate versioned mutation streams and run isolated release-mode processes."""
import argparse,collections,hashlib,itertools,json,random,re,statistics,struct,subprocess,time
from pathlib import Path
ROOT=Path(__file__).resolve().parents[2]
OUT=ROOT/'results/zobrist_storage'
MODES=['raw','detect','shared_rehash','shared_zobrist']

def corpus():
    groups=collections.defaultdict(list)
    for line in (ROOT/'results/rule_history/data.jsonl').read_text().splitlines():
        x=json.loads(line); key=('arithmetic',x['replica'],x['shape'],x['schedule']);groups[key].append(x)
    for line in (OUT/'permutation_snapshots.jsonl').read_text().splitlines():
        x=json.loads(line); groups[(x['family'],x['instance'])].append(x)
    families=collections.defaultdict(list)
    for key,frames in groups.items():families[key[0]].append(sorted(frames,key=lambda x:x['round']))
    return families

def unique_frames(i):
    rng=random.Random(72891+i)
    nodes=set()
    while len(nodes)<9: nodes.add(tuple('p'+str(rng.randrange(6)) for _ in range(6)))
    nodes=sorted(nodes)
    return [dict(root='r',nodes=[dict(op='Sum',children=list(cs)) for cs in nodes[:8]]),dict(root='r',nodes=[dict(op='Sum',children=list(cs)) for cs in nodes[1:]])]

def normalized(frames,ops):
    values=sorted({c for x in frames for n in x['nodes'] for c in n['children']}|{x['root'] for x in frames})
    assert len(values)<512
    ids={v:i for i,v in enumerate(values)}
    width=1+max(len(n['children']) for x in frames for n in x['nodes'])
    return dict(width=width,frames=[(ids[x['root']],frozenset((ops[n['op']],tuple(ids[c] for c in n['children'])) for n in x['nodes'])) for x in frames])

def stream(path,family,count,sources,ops):
    if family=='unique':patterns=[normalized(unique_frames(i),ops) for i in range(count)]
    else:patterns=[normalized(x,ops) for x in sources[family]]
    pack=struct.pack
    updates=deletes=checks=0
    with path.open('wb',buffering=1024*1024) as f:
        f.write(b'ZST1')
        for i in range(count):
            p=patterns[i%len(patterns)];base=i*512+1
            f.write(b'C'+pack('<IBI',i,p['width'],base+p['frames'][0][0]))
        def node(n,base):
            op,cs=n
            return pack('<HB',op,len(cs))+pack('<'+'I'*len(cs),*(base+c for c in cs))
        for t in range(max(len(p['frames']) for p in patterns)):
            for i in range(count):
                p=patterns[i%len(patterns)]
                if t>=len(p['frames']):continue
                base=i*512+1;root,current=p['frames'][t]
                previous=p['frames'][t-1][1] if t else frozenset()
                f.write(b'R'+pack('<II',i,base+root))
                for insert,ns in [(False,previous-current),(True,current-previous)]:
                    for n in sorted(ns):
                        f.write(b'U'+pack('<IB',i,insert)+node(n,base));updates+=1;deletes+=not insert
                # An intentional redundant staged insertion: all modes must deduplicate.
                if current:
                    f.write(b'U'+pack('<IB',i,1)+node(min(current),base));updates+=1
                f.write(b'V'+pack('<III',i,base+root,len(current)))
                for n in sorted(current):f.write(node(n,base))
                checks+=1
        f.write(b'E'+pack('<I',count))
    final_nodes=sum(len(patterns[i%len(patterns)]['frames'][-1][1]) for i in range(count))
    return dict(family=family,classes=count,final_nodes=final_nodes,updates=updates,deletions=deletes,snapshots=checks,bytes=path.stat().st_size,sha256=hashlib.sha256(path.read_bytes()).hexdigest())

def run(mode,path,verify):
    cmd=['/usr/bin/time','-l',str(ROOT/'target/release/zobrist_storage'),mode,str(path)]
    if verify:cmd.append('verify')
    p=subprocess.run(cmd,cwd=ROOT,text=True,capture_output=True,check=True)
    result=json.loads(p.stdout)
    rss=re.search(r'(\d+)\s+maximum resident set size',p.stderr)
    result['peak_rss_bytes']=int(rss.group(1)) if rss else None
    return result

def main():
    ap=argparse.ArgumentParser();ap.add_argument('--repeats',type=int,default=3);args=ap.parse_args()
    OUT.mkdir(parents=True,exist_ok=True)
    sources=corpus()
    ops={op:i+1 for i,op in enumerate(sorted({n['op'] for xs in sources.values() for frames in xs for x in frames for n in x['nodes']}|{'Sum'}))}
    records=[];meta={};validation=[]
    sizes={'arithmetic':32768,'sum4':32768,'sum6':1024,'unique':16384}
    for family,count in sizes.items():
        small=OUT/f'{family}_validation.bin'
        sm=stream(small,family,128 if family=='unique' else len(sources[family]),sources,ops)
        for mode in MODES:
            r=run(mode,small,True);r['family']=family;validation.append(r)
            assert r['checked_snapshots']==sm['snapshots']
        assert len({r['checksum'] for r in validation if r['family']==family})==1
        print('validated',family,sm['snapshots'],'snapshots ×',len(MODES),flush=True)
        path=OUT/f'{family}.bin';meta[family]=stream(path,family,count,sources,ops)
        for repeat in range(args.repeats):
            order=MODES.copy();random.Random(441+repeat).shuffle(order)
            for mode in order:
                r=run(mode,path,False);r.update(family=family,repeat=repeat);records.append(r)
                print(f"{family} run={repeat} {mode}: retained={r['retained_bytes']/1048576:.3f} MiB update={r['update_ns']/1e9:.3f}s peakRSS={r['peak_rss_bytes']/1048576:.2f} MiB",flush=True)
        rows=[r for r in records if r['family']==family]
        assert len({r['checksum'] for r in rows})==1
        assert len({r['changed'] for r in rows})==1
        assert len({r['retained_bytes'] for r in rows if r['mode']=='shared_rehash'}|{r['retained_bytes'] for r in rows if r['mode']=='shared_zobrist'})==1
        (OUT/'runs.json').write_text(json.dumps(dict(metadata=meta,validation=validation,runs=records),indent=2)+'\n')
    report(meta,validation,records)

def report(meta,validation,records):
    lines=['# Zobrist online storage experiment','',
    'This is an executable local-eclass storage prototype driven by mutations derived from actual egglog snapshots. It is NOT an integrated replacement of egglog constructor tables, union-find or query indexes. No whole-engine memory reduction is claimed. Scaling repeats source histories with disjoint concrete child IDs; unique is a deliberately synthetic low-reuse negative control.','',
    'Raw is a packed sorted u32-row baseline (binary-search deduplication), with per-class fixed stride from the source schema. It retains no structural template or separate child bindings. Shared stores a reference-counted template of u64 tokens plus each instance\'s concrete u32 bindings. Ports are assigned online on first insertion; removed ports remain bound. There is no exhaustive canonicalization or prototype training.','',
    'All modes receive identical insertion, deletion, root-ID-update and redundant-insertion events. Removed/nonexistent and duplicate insertions preserve set semantics. Shared lookup uses (64-bit Zobrist, cardinality) followed by full token-set equality. Dead templates are reclaimed immediately. Shared-rehash recomputes the SAME Zobrist from the entire updated set; shared-zobrist XORs only the changed token. All other storage and exact checks are identical. Detect retains the raw data AND the prototype detector, measuring this concrete sidecar implementation\'s cost, not a lower bound on detection overhead.','',
    '## Measurements','',
    'Release build, separate process per run, randomized mode order, 3 repetitions by default. Retained/peak-requested memory counts actual Rust allocation request sizes, including bindings, dictionary, template buckets, vectors and root metadata; excludes common input-reader/process baseline and allocator metadata. Both layouts shrink unused vector capacity at the end. Peak RSS is whole-process /usr/bin/time -l on macOS, including file buffers and allocator effects. RSS numbers are benchmark processes, not egglog processes.','',
    'Update time sums timed store mutations; it includes online port assignment, deduplication, allocation, hashing, equality checks and reference reclamation, but excludes input reading, explicit root-ID assignment, validation and final compaction. Full scan directly reads the stored representation without materializing the entire graph. Counter instrumentation overhead is present in every mode. End-to-end stream time, compaction time, scan time and all individual results are in runs.json.','',
    '| Workload | classes | final nodes | mode | retained MiB | peak RSS MiB | updates s (median) | scan ms (median) |',
    '|---|---:|---:|---|---:|---:|---:|---:|']
    aggregate=[]
    for family,m in meta.items():
        for mode in MODES:
            rr=[r for r in records if r['family']==family and r['mode']==mode]
            q={k:statistics.median(r[k] for r in rr) for k in ['retained_bytes','peak_requested_bytes','peak_rss_bytes','update_ns','scan_ns','build_ns','compact_ns']}
            q.update(family=family,mode=mode,update_min_ns=min(r['update_ns'] for r in rr),update_max_ns=max(r['update_ns'] for r in rr));aggregate.append(q)
            lines.append(f"| {family} | {m['classes']} | {m['final_nodes']} | {mode} | {q['retained_bytes']/1048576:.3f} | {q['peak_rss_bytes']/1048576:.2f} | {q['update_ns']/1e9:.3f} | {q['scan_ns']/1e6:.3f} |")
    lines+=['','## Correctness and boundaries','',f"{sum(r['checked_snapshots'] for r in validation)} exact snapshot comparisons across all modes on unscaled validation streams, including deletions and root-ID changes. Larger runs have identical mutation counts and full-scan checksums across modes; exact collision behavior and copy-on-write are additionally covered by Rust differential tests, including a forced zero-bit hash.",'',
    'The Sum4/Sum6 rules rotate operands and swap adjacent operands of a commutative sum, and actually saturate in egglog to 24/720 permutations. They intentionally exhibit high repetition and establish a favorable regime, not general compiler-workload performance. Arithmetic uses the prior 16-shape/6-schedule corpus and is a small-eclass counterpoint. Unique has random six-argument patterns and two states per instance; it is not an egglog workload.','',
    'The prototype stores concrete child IDs and current root IDs, but external child-class contents, union-find, query indexes, timestamps, subsumption flags and mutation provenance are outside this local membership-store experiment. A fully integrated engine must account for all of them. Stable role bindings can miss isomorphic patterns with different discovery orders; hash equality is never accepted without structural equality.','',
    'New templates still require a sorted token vector; incremental hashing does not eliminate that work or equality checking. Any memory benefit comes from sharing templates; the rehash-vs-incremental contrast isolates the additional time benefit of Zobrist updates.','',
    '```sh','cargo run --bin zobrist_corpus -- results/zobrist_storage/permutation_snapshots.jsonl','cargo test --lib','cargo build --release --bin zobrist_storage','python3 experiments/zobrist_storage/run.py','```']
    (OUT/'report.md').write_text('\n'.join(lines)+'\n')
    (OUT/'summary.json').write_text(json.dumps(dict(metadata=meta,aggregate=aggregate,verified_snapshots=sum(r['checked_snapshots'] for r in validation)),indent=2)+'\n')

if __name__=='__main__':main()
