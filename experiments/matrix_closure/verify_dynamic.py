"""Native differential tests for growth, expiration, aliases and slot reuse."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import numpy as np
from dynamic import DynamicContext
from dynamic_client import Resident
from dynamic_events import SnapshotEvents
from run import native_adapter

HERE=Path(__file__).resolve().parent

class Oracle:
    def __init__(self,frontend,source,out):
        self.log=(out/'native.log').open('w')
        self.p=subprocess.Popen([str(frontend.resolve()),str(source.resolve())],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=self.log,text=True)
        line=self.p.stdout.readline()
        if not line:raise RuntimeError('native compile failed')
        self.core=json.loads(line)
    def ask(self,data):
        self.p.stdin.write(json.dumps(data)+'\n');self.p.stdin.flush();line=self.p.stdout.readline()
        if not line:raise RuntimeError('native oracle ended; see native.log')
        return json.loads(line)
    def close(self):
        if self.p.poll() is None:self.p.stdin.write('{"op":"quit"}\n');self.p.stdin.flush();self.p.wait(timeout=30)
        self.p.stdin.close();self.p.stdout.close();self.log.close()

def full_check(compiler,core,snapshot,arrays):
    state,hidden,parents,_=arrays
    encode,compare=native_adapter()
    carrier=dict(core,rulesets=[],schedule=[],initial=[r for r in snapshot['rows'] if r['table'] not in compiler.outputs])
    p=encode(carrier,capacity=len(carrier['initial'])+len(state)+1,match_capacity=1)
    stride=p['stride'];flat=p['initial_rows'];rows=[flat[i:i+stride] for i in range(0,len(flat),stride)]
    ids={tuple(a):i+1 for i,a in enumerate(p['anchors'])};table_ids={t['name']:i for i,t in enumerate(p['tables'])}
    # Pin observed input classes; predicate result identities remain opaque and
    # are reconstructed only now, never used as verification labels in inference.
    keys=[];seen=set()
    for i,c in enumerate(compiler.coordinates):
        root=i
        while parents[root]!=root:root=int(parents[root])
        if not state[root]:continue
        value=compiler.canonical(c['input']);key=(c['table'],value['sort'],value['eq'])
        if key in seen:continue
        seen.add(key);keys.append((key,bool(hidden[root])))
        pair=key[1:]
        if pair not in ids:ids[pair]=len(ids)+1;p['anchors'].append(pair)
    next_id=len(ids)+1
    for (table,sort,value),hide in sorted(keys):
        rows.append([table_ids[table],int(hide),1,len(rows)+1,ids[(sort,value)],next_id]+[0]*(stride-6));next_id+=1
    expected=dict(snapshot,anchors=[{'sort':s,'old':v,'now':v} for s,v in p['anchors']])
    return compare(p,rows,list(range(next_id)),expected,p['initial_sets'])

def fixture(frontend,worker,out):
    out.mkdir(parents=True,exist_ok=False);oracle=Oracle(frontend,HERE/'dynamic_fixture.egg',out)
    compiler=DynamicContext(oracle.core,'closure');events=SnapshotEvents(compiler.templates,oracle.core['initial'])
    gpu=Resident(worker,compiler.library(),out/'matrix',record_commands=True);records=[];patches=[]
    def check(label):
        info,arrays=gpu.snapshot();view=compiler.decode(*arrays[:3])
        expected=oracle.ask({'op':'unary_predicates','tables':compiler.outputs})
        if view!=expected:raise AssertionError((label,view,expected))
        records.append({'label':label,'view':view,'info':info,'state_sha256':hashlib.sha256(arrays.tobytes()).hexdigest()})
        return info,arrays,view
    def step(label):
        got=gpu.step();native=oracle.ask({'op':'run','set':'closure'})
        assert got['updated']==native['updated'],label
        return check(label)
    def edit(source,label,seeds=()):
        oracle.ask({'op':'edit_actions','source':source});snapshot=oracle.ask({'op':'snapshot'})
        patch=compiler.apply(**events.diff(snapshot),seeds=seeds)
        ack=gpu.apply(patch);patches.append({'label':label,'compiler':patch['stats'],'gpu':ack})
        oracle.ask({'op':'track_anchors'});check(label)
        return ack
    try:
        initial=compiler.bootstrap(oracle.core['initial']);gpu.apply(initial);oracle.ask({'op':'track_anchors'});check('bootstrap')
        edit('(subsume (Leaf 99)) (subsume (Leaf 1))','retire_before_first_run')
        assert patches[-1]['compiler']['contexts_retired']==1,'shared Base witness must survive Leaf retirement'
        old,empty=gpu.send([2,1,1],allow_error=True);assert not old['ok'] and not empty
        step('first_run');step('second_run')
        edit('(let new1 (Forward a)) (let new2 (Forward new1))','grow_two_dependencies')
        one=step('new_first_run')[2];two=step('new_second_run')[2]
        assert len(two['Ready'])==len(one['Ready'])+1,'new downstream fact must wait for its next Run'
        edit('(set (Selected key) (Blocked 21))','replace_function_anchor')
        step('replacement_run')
        edit('(union blocked (Blocked 3))','alias_unlocks_repeated_variable')
        step('new_alias_match')
        a_value=next(r['values'][-1] for r in oracle.ask({'op':'snapshot'})['rows'] if r['table']=='Leaf' and r['values'][0].get('value')==1)
        edit('(subsume (Ready a)) (let hidden-consumer (Pair a key))','hide_predicate',seeds=[('Ready',a_value,True)])
        step('hidden_input_cannot_trigger')
        edit('(union a key)','hidden_alias_wins')
        step('hidden_alias_and_new_structural_match')
        edit(' '.join(['(subsume (Leaf 0))','(subsume (Leaf 1))','(subsume (Leaf 99))','(subsume (Base 1))',
             '(subsume (Blocked 2))','(subsume (Blocked 3))','(subsume (Blocked 20))','(subsume (Blocked 21))',
             '(subsume (Forward a))','(subsume (Forward new1))','(subsume (Pair blocked (Blocked 3)))',
             '(subsume (Pair a key))','(delete (Selected key))']),'clear_all_contexts')
        info,arrays,_=step('quiet_without_contexts')
        assert info['live_contexts']==0 and info['context_reserved_bytes']==0
        old_known=int(arrays[0].sum());assert old_known>0,'retirement must not retract past facts'
        edit('(let fresh (Leaf 777)) (Forward fresh)','reuse_after_clear')
        step('reused_slot_first_run');info,arrays,_=step('reused_slot_second_run')
        assert info['live_contexts']==2
        # A current-epoch packet with an old slot generation is also rejected.
        count=len(compiler.coordinates);n=len(compiler.banks[0]);key=compiler.banks[0][0]
        bad=[1,compiler.epoch,compiler.epoch+1,count,0,0,0,1,0,n,n,1,0,1,key[1]]
        rejected,_=gpu.send(bad,allow_error=True);assert not rejected['ok'] and rejected['epoch']==compiler.epoch
        check('stale_slot_generation_rejected')
        current=oracle.ask({'op':'current_program'})
        final=full_check(compiler,current,oracle.ask({'op':'snapshot'}),arrays)
        commands=gpu.commands[:]
        result={'scope':'native dynamic action stream, resident GPU closure; verification snapshots are diagnostic',
            'records':records,'patches':patches,'full_graph_check':final,'commands':commands,
            'predicate_truth_used_as_feedback':False,'upload_bytes':gpu.up,'download_bytes_including_audit':gpu.down}
        (out/'report.json').write_text(json.dumps(result,indent=2)+'\n')
        return result
    except Exception as error:
        (out/'failure.json').write_text(json.dumps({'error':str(error),'records':records,'patches':patches},indent=2)+'\n')
        raise
    finally:gpu.close();oracle.close()

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--frontend',type=Path,required=True);p.add_argument('--worker',type=Path,required=True);p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();r=fixture(a.frontend,a.worker,a.output);print('passed',len(r['records']),'dynamic boundaries',r['full_graph_check'])
