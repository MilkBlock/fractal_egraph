"""Frozen Pass-3 projected-query oracle and shared response experiment."""
import argparse
from collections import defaultdict
import gzip
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import time
import numpy as np
from model import ResponseBank,reference


def load(snapshot,profile):
    with gzip.open(profile,'rt') as f:header=json.loads(next(f))['header']
    tables=defaultdict(list)
    for name,args,result,hidden in json.loads(snapshot.read_text())['rows']:
        if hidden:continue
        row=[]
        for v in [*args,result]:
            kind=next(iter(v));sort,x=v[kind];row.append((kind,sort,str(x)))
        tables[name].append(tuple(row))
    return tables,header


def native_check(binary,bank,output):
    """Native joins on the EXACT frozen typed relations, not original RHS replay.

    Opaque bijective i64 IDs preserve all typed equalities and literal tests.
    No primitive guards are eligible. Function outputs are frozen relation
    columns: this does NOT verify merge, Run chronology or actual mutations.
    """
    names=sorted({atom['table'] for qi in bank.eligible for atom in bank.queries[qi]['shape']['atoms']})
    labels={name:'F'+str(i) for i,name in enumerate(names)}
    arities={atom['table']:len(atom['columns']) for qi in bank.eligible for atom in bank.queries[qi]['shape']['atoms']}
    values={}
    def code(v):
        if v not in values:values[v]=len(values)
        return str(values[v])
    source=['(relation Found (i64 i64))','(ruleset queries)']
    for name in names:
        source.append(f'(relation {labels[name]} ({" ".join(["i64"]*(arities[name]+1))}))')
        for rid,row in enumerate(bank.tables.get(name,[])):
            source.append(f'({labels[name]} {rid} {" ".join(code(v) for v in row)})')
    for qi,ai in bank.eligible.items():
        atoms=bank.queries[qi]['shape']['atoms'];lhs=[]
        for i,a in enumerate(atoms):
            fields=[code(('Base',sort,c['Literal'])) if 'Literal' in c else 's'+str(c['Slot'])
                    for c,sort in zip(a['columns'],a['sorts'],strict=True)]
            lhs.append(f'({labels[a["table"]]} row{i} {" ".join(fields)})')
        source.append(f'(rule ({" ".join(lhs)}) ((Found {qi} row{ai})) :ruleset queries)')
    source+=['(run queries 1)','(print-function Found 10000000)']
    path=output/'oracle.egg';path.write_text('\n'.join(source)+'\n')
    start=time.perf_counter()
    run=subprocess.run([str(binary),str(path)],text=True,capture_output=True)
    (output/'oracle.stderr').write_text(run.stderr);run.check_returncode()
    actual={tuple(map(int,s.split())) for s in re.findall(r'^\s*\(Found ([^)\n]+)\)\s*->',run.stdout,re.M)}
    expected=bank.answers()
    if actual!=expected:raise AssertionError(f'native mismatch missing={len(expected-actual)} extra={len(actual-expected)}')
    return {'scope':'native frozen relational LHS replay of all eligible queries; anchor-row projection only',
            'queries':len(bank.eligible),'root_rule_pairs':len(actual),'exact_root_projection':True,
            'binary_sha256':hashlib.sha256(binary.read_bytes()).hexdigest(),
            'program_sha256':hashlib.sha256(path.read_bytes()).hexdigest(),
            'wall_seconds_including_parse_compile_print':time.perf_counter()-start}


def packet(bank,path):
    # Exclude empty root tables from device jobs, but do not drop rules of a
    # nonempty table. Profiles and full rows use the same exact weight matrix.
    groups=[(name,g) for name,g in bank.groups.items() if len(g['x'])]
    with path.open('wb') as f:
        f.write(b'RESPBNK2');f.write(struct.pack('<I',len(groups)))
        for name,g in groups:
            x,w,z=g['x'],g['w'],g['profiles'];n,k=x.shape;q=w.shape[1];p=len(z)
            folded=g['fold'];fk=folded['z'].shape[1];gates=len(folded['threshold'])
            f.write(struct.pack('<6I',n,k,q,p,fk,gates))
            for a in (x,w,folded['z'],folded['w']):f.write(a.astype('<f2').tobytes())
            f.write(g['route'].astype('<u4').tobytes())
            f.write(folded['gate_route'].astype('<u4').tobytes())
            f.write(g['threshold'].astype('<u4').tobytes())
            f.write(folded['threshold'].astype('<u4').tobytes())
            f.write(g['answer'].astype('u1').tobytes())
    return [name for name,g in groups]


def previous_projection_digest(report_path,report):
    """Read previous full oracle target, not just its cardinality."""
    data=(report_path.parent/'input.bin').read_bytes();version=data[:8]
    if version not in (b'RESPBANK',b'RESPBNK2'):raise ValueError('unknown oracle packet')
    cursor=8
    def words(n):
        nonlocal cursor
        values=struct.unpack_from('<'+'I'*n,data,cursor);cursor+=4*n;return values
    groups=words(1)[0];byname={g['table']:g for g in report['summary']['by_group']};found=[]
    if groups!=len(report['gpu_job_tables']):raise ValueError('old packet group mismatch')
    for name in report['gpu_job_tables']:
        if version==b'RESPBANK':
            n,k,q,p=words(4);cursor+=2*(n*k+k*q+p*k)+4*(n+q)
        else:
            n,k,q,p,fk,gates=words(6);cursor+=2*(n*k+k*q+p*fk+fk*gates)+4*(n+q+q+gates)
        answer=np.frombuffer(data,dtype='u1',count=n*q,offset=cursor).reshape(n,q);cursor+=n*q
        ids=byname[name]['rule_ids']
        found.extend((ids[int(j)],int(i)) for i,j in np.argwhere(answer))
    if cursor!=len(data):raise ValueError('old packet trailing bytes')
    return hashlib.sha256(str(sorted(found)).encode()).hexdigest()


def main():
    p=argparse.ArgumentParser();p.add_argument('--snapshot',type=Path,required=True)
    p.add_argument('--profile',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    p.add_argument('--native',type=Path);p.add_argument('--reuse-native-report',type=Path)
    a=p.parse_args();a.output.mkdir(parents=True,exist_ok=False)
    if a.native and a.reuse_native_report:p.error('choose native execution OR an unchanged native oracle report')
    start=time.perf_counter();tables,header=load(a.snapshot,a.profile);load_time=time.perf_counter()-start
    start=time.perf_counter();bank=ResponseBank(header['queries'],tables);compile_time=time.perf_counter()-start
    start=time.perf_counter();bank.encode();encode_time=time.perf_counter()-start
    start=time.perf_counter();wanted=reference(header['queries'],tables,bank.eligible)
    assert wanted==bank.answers();reference_time=time.perf_counter()-start
    jobs=packet(bank,a.output/'input.bin')
    report={'scope':'standalone exact shared predicate response on frozen queries; not an egglog execution backend',
        'snapshot':str(a.snapshot),'snapshot_sha256':hashlib.sha256(a.snapshot.read_bytes()).hexdigest(),
        'profile':str(a.profile),'profile_sha256':hashlib.sha256(a.profile.read_bytes()).hexdigest(),
        'queries_in_header':len(header['queries']),'rejected_by_header':len(header['rejected']),
        'summary':bank.summary(),'independent_full_join_root_projection_equal':True,'gpu_job_tables':jobs,
        'python_seconds':{'load':load_time,'projection_indices_and_compile':compile_time,
            'feature_materialization_and_profile_quotient_and_score':encode_time,'independent_reference':reference_time},
        'native':None,
        'limits':['Only guard-free anchor-complete queries are eligible.',
            'Features require real projected-key reads; this prototype does not avoid or incrementally maintain them.',
            'Sharing a Boolean response does not share term identities or authorize dropping effect bindings.',
            'All original atoms remain available as correlated whole-row payloads.',
            'Rulesets retain separate labels; cross-ruleset evaluation is a read-only census, not early execution.',
            'Guards, fresh effects, union/rebuild, delta watermarks, and RHS submission are not a runtime loop here.']}
    if a.native:report['native']=native_check(a.native,bank,a.output)
    if a.reuse_native_report:
        previous=json.loads(a.reuse_native_report.read_text())
        for key in ('snapshot_sha256','profile_sha256'):
            if previous[key]!=report[key]:raise ValueError('native report source mismatch')
        old=previous['summary'];new=report['summary']
        if [(g['table'],g['rule_ids']) for g in old['by_group']]!=[(g['table'],g['rule_ids']) for g in new['by_group']]:
            raise ValueError('native query set changed')
        if previous['native']['root_rule_pairs']!=len(wanted):raise ValueError('native count mismatch')
        # Compare complete answer identities as well as source/query identity.
        digest=hashlib.sha256(str(sorted(wanted)).encode()).hexdigest()
        old_digest=previous.get('root_projection_sha256') or previous_projection_digest(a.reuse_native_report,previous)
        if old_digest!=digest:raise ValueError('native projected answers changed')
        report['native']={**previous['native'],'reused_report':str(a.reuse_native_report)}
    report['root_projection_sha256']=hashlib.sha256(str(sorted(wanted)).encode()).hexdigest()
    (a.output/'results.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('summary','limits','gpu_job_tables')},indent=2))
    print(json.dumps({k:v for k,v in report['summary'].items() if k!='by_group'},indent=2))


if __name__=='__main__':main()
