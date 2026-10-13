"""Project-local, rule-scoped knowledge from successful native Cell Bakes.

Concrete contracts are reusable only under the same conservative environment
key and are rechecked by the compiler's native differential validator. Learned
merge descriptions are evidence compression, never unconditional rewrite rules.
"""
import argparse
import copy
import fcntl
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile
from contextlib import contextmanager

from . import merge_learning as ml
from .terms import actions, expressions, text

ROOT=Path(__file__).resolve().parents[2]
SCHEMA='cell-knowledge/v1'


def environment(data):
    # Native parser/printer removes comments/formatting. Preserve declaration
    # and rule order, names, schedules, types, merge and primitive semantics.
    # Global-capturing/interleaved rules are rejected by the caller beforehand.
    return dict(contract='positive-constructor-equality/v1',
        program=[c['text'] for c in data['commands'] if c['kind'] in ('declaration','rule','schedule')])


def sample_id(data):return ml.digest(actions([a['action'] for a in data['actions']]))


def read(path):
    if not path.exists():return dict(schema=SCHEMA,environments={})
    value=json.loads(path.read_text())
    if value.get('schema')!=SCHEMA:raise ValueError('unsupported Cell knowledge schema')
    for env in value['environments'].values():
        families={f['id']:f for f in env.get('families',[])}
        for tid,r in env.get('transitions',{}).items():
            if 'encoding' in r:
                e=r.pop('encoding');r['template']=ml.instantiate(families[e['family']]['template'],e['bindings'])
            if ml.digest(r['template'])!=tid:raise ValueError('merge evidence hash mismatch')
    return value


@contextmanager
def locked(path):
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.with_suffix(path.suffix+'.lock').open('a') as lock:
        fcntl.flock(lock,fcntl.LOCK_EX)
        try:yield
        finally:fcntl.flock(lock,fcntl.LOCK_UN)


def save(path,value):
    stored=copy.deepcopy(value)
    for env in stored['environments'].values():
        for family in env.get('families',[]):
            for tid in family['transitions']:
                record=env['transitions'][tid];template=record.pop('template')
                values=ml.bindings(family['template'],template)
                if values is None or ml.instantiate(family['template'],values)!=template:
                    raise ValueError('learned dictionary does not reconstruct its evidence')
                record['encoding']=dict(family=family['id'],bindings=values)
    fd,tmp=tempfile.mkstemp(prefix=path.name+'.',suffix='.tmp',dir=path.parent)
    try:
        with os.fdopen(fd,'w') as f:
            json.dump(stored,f,sort_keys=True,separators=(',',':'));f.write('\n');f.flush();os.fsync(f.fileno())
        os.replace(tmp,path)
    finally:
        if os.path.exists(tmp):os.unlink(tmp)


def state_key(s):
    return ml.key([s['scope'],sorted((v['sort'],v.get('literal') or '') for v in s['values']),
                   sorted((r['op'],len(r['args'])) for r in s['rows']),sorted(s.get('ports',{}))])


class States:
    def __init__(self,cells,folder):
        self.cells=copy.deepcopy(cells);self.folder=folder;folder.mkdir(parents=True,exist_ok=True)
        self.buckets={};self.paths={};self.comparisons=0
        for k,s in self.cells.items():self.buckets.setdefault(state_key(s),[]).append(k)
    def path(self,k):
        if k not in self.paths:
            p=self.folder/(k+'.json');p.write_text(json.dumps(self.cells[k]));self.paths[k]=p
        return self.paths[k]
    def intern(self,state):
        clean={k:v for k,v in state.items() if k!='local_ids'};sid=ml.digest(clean)
        if sid in self.cells:return sid,list(range(len(state['values'])))
        left=self.folder/'incoming.json';left.write_text(json.dumps(clean))
        for old in self.buckets.get(state_key(clean),[]):
            r=subprocess.run([ROOT/'target/release/egg_layout','saturated-rule-composition-compare',left,self.path(old)],capture_output=True,text=True)
            if r.returncode:raise RuntimeError('native Cell comparison failed: '+r.stderr)
            self.comparisons+=1;result=json.loads(r.stdout)
            if result['status']=='equivalent':return old,result['value_map']
        self.cells[sid]=clean;self.buckets.setdefault(state_key(clean),[]).append(sid)
        return sid,list(range(len(state['values'])))


def recipe_key(c,cell):
    return ml.digest([cell,actions(c['acts'],params=c['params']),[text(e,params=c['params']) for e in c['roots']],
                      list(c['param_sorts'].values())])


class Session:
    def __init__(self,path,data,source,work,source_sha256=None):
        self.path=Path(path).resolve();self.data=data;self.source=Path(source).resolve();self.work=work
        self.contract=environment(data);self.env_id=ml.digest(self.contract);self.sample=sample_id(data)
        self.source_sha256=source_sha256 or hashlib.sha256(self.source.read_bytes()).hexdigest()
        self.previous=read(self.path)['environments'].get(self.env_id,{})
        self.states=States(self.previous.get('cells',{}),work/'knowledge/states')
        self.current_ids=set();self.imported=[]
    def augment(self,candidates):
        ids={};families={};current_keys={}
        for c in candidates:
            self.current_ids.add(c['id'])
            sid,mapping=self.states.intern(c['snapshot']);families.setdefault(sid,len(families))
            c['family']=families[sid];c['value_map']=mapping;c['knowledge_cell']=sid
            rid=recipe_key(c,sid);c['knowledge_recipe']=rid;current_keys.setdefault(rid,c['id'])
        next_id=max(self.current_ids,default=-1)+1
        # Existing Cell contracts may become source/target layouts on this run,
        # including a known target not discovered in the current history.
        for rid,record in self.previous.get('recipes',{}).items():
            if rid in current_keys:ids[rid]=current_keys[rid];continue
            c=copy.deepcopy(record['candidate']);sid=record['cell'];families.setdefault(sid,len(families))
            c.update(id=next_id,family=families[sid],knowledge_cell=sid,knowledge_recipe=rid,
                     snapshot=copy.deepcopy(self.states.cells[sid]),value_map=list(range(len(self.states.cells[sid]['values']))))
            # Expressions resolve by constructor/literal identity in the shared
            # graph. Store the closure once, not one snapshot per seed recipe.
            folder=self.work/'knowledge/imports'/rid;folder.mkdir(parents=True,exist_ok=True)
            (folder/'entry.egg').write_text(record['entry_source'])
            (self.work/f'path-{next_id}.json').write_text(json.dumps({'actions':record['seed_actions']}))
            (self.work/f'path-{next_id}.egg').write_text(record['seed_source'])
            c['source_folder']=str(folder);self.imported.append(c);ids[rid]=next_id;next_id+=1
        ids.update(current_keys)
        by_id={c['id']:c for c in candidates+self.imported}
        for c in self.imported:
            record=self.previous['recipes'][c['knowledge_recipe']]
            c['origin']['composition']['parts']=[ids[p['recipe']] for p in record['parents'] if p['recipe'] in ids]
            c['origin']['bound_parent_substitutions']={}
            for p in record['parents']:
                if p['recipe'] not in ids:continue
                pid=ids[p['recipe']];parent=by_id[pid]
                c['origin']['bound_parent_substitutions'][str(pid)]={raw:p['bindings'][name] for raw,name in parent['params'].items()}
        return candidates+self.imported

    def record(self,c,by_id):
        from .compiler import parent_wirings
        cid=c['id'];fields=('acts','roots','params','param_sorts','nodes','lhs_atoms','signatures','rulesets','marker','markers')
        value={k:copy.deepcopy(c[k]) for k in fields};value['markers']=sorted(value['markers'])
        value['origin']={k:copy.deepcopy(c['origin'].get(k) or ({} if k=='composition' else [])) for k in ('symbolic_values','composition')}
        value['origin']['composition']['parts']=[]
        return dict(cell=c['knowledge_cell'],candidate=value,
            entry_source=(Path(c['source_folder'])/'entry.egg').read_text(),
            seed_source=(self.work/f'path-{cid}.egg').read_text(),
            seed_actions=json.loads((self.work/f'path-{cid}.json').read_text())['actions'],
            parents=[dict(recipe=w['parent']['knowledge_recipe'],bindings={w['parent']['params'][raw]:e for raw,e in w['native_subst'].items()})
                     for w in parent_wirings(c,c['params'],by_id)])

    def commit(self,candidates,edges,output):
        """Call only AFTER all native seed/source checks have passed."""
        by_id={c['id']:c for c in candidates};records={c['knowledge_recipe']:self.record(c,by_id) for c in candidates}
        incoming=observations(candidates,edges,self.current_ids)
        sha=hashlib.sha256(self.source.read_bytes()).hexdigest()
        if sha!=self.source_sha256:raise ValueError('source changed during Bake; knowledge was not published')
        with locked(self.path):
            library=read(self.path)
            env=library['environments'].setdefault(self.env_id,dict(contract=self.contract,cells={},recipes={},samples={},sources={},transitions={},families=[]))
            known_before=set(env['cells'])
            # Re-intern against the latest locked index: two concurrent Bakes
            # must neither overwrite each other nor invent conflicting IDs.
            states=States(env['cells'],self.work/'knowledge/commit-states');remap={}
            for sid,s in self.states.cells.items():remap[sid]=states.intern(s)[0]
            env['cells']=states.cells
            # Recipe IDs use the session's stable semantic cell identity. They
            # remain opaque keys if a concurrent equivalent state won the race.
            for rid,r in records.items():
                r['cell']=remap[r['cell']];env['recipes'].setdefault(rid,r)
            source=dict(source=str(self.source),source_sha256=sha,sample=self.sample)
            env['sources'][sha]=source
            env['samples'].setdefault(self.sample,dict(sources=[],cells=[],recipes=[]))
            sample=env['samples'][self.sample]
            if sha not in sample['sources']:sample['sources'].append(sha)
            sample['cells']=sorted({remap[c['knowledge_cell']] for c in candidates if c['id'] in self.current_ids})
            sample['recipes']=sorted({c['knowledge_recipe'] for c in candidates if c['id'] in self.current_ids})
            for r in incoming:
                state=r['template']['states']
                for node in state['sources']+[state['target']]:node['identity']=remap[node['identity']]
                tid=ml.digest(r['template']);old=env['transitions'].setdefault(tid,dict(r,samples={},examples=[]))
                old['samples'].setdefault(self.sample,[])
                if r['event'] not in old['samples'][self.sample]:old['samples'][self.sample].append(r['event'])
                example=dict(source=str(self.source),source_sha256=sha,sample=self.sample,transition=r['event'])
                if example not in old['examples']:old['examples'].append(example)
            learned=ml.learn(env['transitions'],env['families']);env['families']=learned['families']
            env['learning']={k:v for k,v in learned.items() if k!='families'}
            if hashlib.sha256(self.source.read_bytes()).hexdigest()!=sha:
                raise ValueError('source changed during knowledge learning; knowledge was not published')
            save(self.path,library)
            report=listing(library,self.path,self.data,self.source)
        dest=output/'merge-families.json';dest.write_text(json.dumps(report,indent=2)+'\n')
        md=output/'merge-families.md';md.write_text(markdown(report))
        return dict(library_path=str(self.path),environment_id=self.env_id,sample_id=self.sample,
                    imported_cells=len({c['knowledge_cell'] for c in self.imported}),imported_recipes=len(self.imported),
                    new_cells=len(set(sample['cells'])-known_before),reused_cells=len(set(sample['cells'])&known_before),family_count=len(report['families']),
                    comparisons=self.states.comparisons,report_json=str(dest),report_markdown=str(md),
                    scope='native-revalidated historical Cell contracts participate in compilation; generalized families are descriptive hypotheses only')


def observations(candidates,edges,current_ids):
    from .compiler import parent_wirings
    from .composition import substitute
    by_id={c['id']:c for c in candidates};out=[]
    for edge in edges:
        cid=edge.get('target_unit',edge.get('id'))
        if cid not in current_ids:continue
        c=by_id[cid]
        if edge.get('kind')=='certified_seed_cut':
            pairs=[(by_id[p['parent']],p['bindings']) for p in edge['seed_cut']['parents']]
            kind='certified-seed-cut'
        else:
            pairs=[(w['parent'],w['native_subst']) for w in parent_wirings(c,c['params'],by_id)]
            kind='observed-history'
        if not pairs:continue
        def term(e):
            raw=text(e)
            if raw in c['params']:return dict(sort=c['param_sorts'][raw],port=c['params'][raw])
            if e['kind']=='literal':return dict(sort=e['sort'],literal=e['text'])
            return dict(sort=c['signatures'][e['head']]['output'],op=e['head'],args=[term(x) for x in e['args']])
        def fact(a,subst={}):
            return dict(kind=a['kind'],terms=[term(substitute(e,subst)) for e in expressions([a])])
        parents=[dict(roots=[term(substitute(r,b)) for r in p['roots']],facts=[fact(a,b) for a in p['acts']]) for p,b in pairs]
        supplied={ml.key(f) for p in parents for f in p['facts']}
        facts=[fact(a) for a in c['acts']]
        # Guards/unions are explicit and retained even if also supplied by a
        # parent. A learned hole is not permission to erase a precondition.
        template=dict(states=dict(sources=[dict(sort='CellState',identity=p['knowledge_cell']) for p,_ in pairs],
                                 target=dict(sort='CellState',identity=c['knowledge_cell'])),
                      parents=parents,target=dict(roots=[term(r) for r in c['roots']],facts=facts),
                      external=[f for f in facts if f['kind']=='union' or ml.key(f) not in supplied])
        bucket=[len(parents),[r['sort'] for r in template['target']['roots']],
                sorted({r['op'] for r in template['target']['roots'] if 'op' in r})]
        out.append(dict(template=template,bucket=bucket,event=str(edge['id']),evidence_kind=kind))
    return out


def listing(library,path,data,source):
    sha=hashlib.sha256(Path(source).read_bytes()).hexdigest();eid=ml.digest(environment(data))
    env=library['environments'].get(eid,{});record=env.get('sources',{}).get(sha)
    families=[]
    if record:
        sid=record['sample']
        for family in env.get('families',[]):
            ids=[i for i in family['transitions'] if sid in env['transitions'][i]['samples']]
            if not ids:continue
            examples=[dict(x,bindings=ml.bindings(family['template'],env['transitions'][i]['template']))
                      for i in family['transitions'] for x in env['transitions'][i]['examples']]
            families.append(dict(family,current_instances=sum(len(env['transitions'][i]['samples'][sid]) for i in ids),examples=examples,
                count_scope='admitted transition descriptions, not runtime matches or committed mutations'))
    return dict(schema='cell-merge-families/v1',source=str(Path(source).resolve()),source_sha256=sha,
                library_path=str(Path(path).resolve()),environment_id=eid,registered=bool(record),families=families,
                library_samples=len(env.get('samples',{})),library_cells=len(env.get('cells',{})),
                scope='current source evidence in a cross-fact library; hypotheses are not universal rewrite guarantees')


def markdown(report):
    lines=['# Merge families', '', f'Source: `{report["source"]}`', '',
           'Generalized hypotheses summarize verified instances; they do not authorize arbitrary recursion.', '',
           '| Family | Status | Current instances | Input programs | Parameters |', '|---|---|---:|---:|---:|']
    for f in report['families']:lines.append(f'| {f["id"][:12]} | {f["status"]} | {f["current_instances"]} | {f["support_samples"]} | {len(f["parameters"])} |')
    for f in report['families']:
        lines+=['',f'## {f["id"][:12]}', '',f['summary'],'','```text',f.get('pattern',ml.describe(f['template'])),'```']
    return '\n'.join(lines)+'\n'


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('command',choices=['list'])
    p.add_argument('--source',type=Path,required=True);p.add_argument('--library',type=Path,required=True)
    a=p.parse_args()
    from .compiler import ast
    with tempfile.TemporaryDirectory() as d:data=ast(a.source,Path(d)/'source.json')
    print(json.dumps(listing(read(a.library),a.library,data,a.source)))


if __name__=='__main__':main()
