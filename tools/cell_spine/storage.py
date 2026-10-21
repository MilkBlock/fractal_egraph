"""Current-input storage value function; execute only the compact runtime.

The existing exact input row-cover policy is recomputed for every priced ABI.
It is not a search over every possible initializer (or over unseen inputs).
Native hashcons, union and subsumption decide retained rows. No closure export,
trace replay, graph dump, fabricated RSS estimate, or candidate truncation.
"""
import json
import hashlib
from pathlib import Path
import subprocess
import time
from . import markers
from .terms import actions


class InstanceStorage:
    def __init__(self, data, candidates, environment, signatures, runner, *, measure_search=False):
        self.data=data;self.candidates=candidates;self.environment=environment
        self.signatures=signatures;self.runner=Path(runner)
        self.cache={};self.evaluations=0;self.seconds=0.
        self.measure_search=measure_search
        self.runner_sha256=hashlib.sha256(self.runner.read_bytes()).hexdigest() if measure_search else None

    def search_work(self,bundle,selected,folder):
        if not self.measure_search:raise ValueError('search counters not enabled')
        measured=self(bundle,selected,folder)
        counters=measured['native'].get('run_probe_counters')
        if counters is None or not {'probes','candidate_rows'}<=counters.keys():
            raise ValueError('native runner lacks search counters; rebuild it')
        return dict(value=counters['probes']+counters['candidate_rows'],counters=counters,runner_sha256=self.runner_sha256,
                    native=measured,scope='one current-input native saturation; handles plus row visits, including failed searches')

    def lower_bound(self,bundle):
        """Rows no certified initializer can cover must remain raw.

        Collapse all e-class arguments (and numeric primitives) when counting
        keys. This permits MORE union than native execution and hence only
        undercounts. Distinct String keys and distinct tables cannot coalesce.
        No projected initializer or candidate is removed by this bound.
        """
        from .input_spine import initializers
        if any(a['action']['kind']=='union' for a in self.data['actions']):return 0
        arbitrary={f:c['marker_layouts'][0] for f,c in bundle['chosen'].items()}
        dag,_,_,obligations=initializers(self.data,self.candidates,bundle['chosen'],arbitrary,bundle)
        covered={i for _,rows in obligations for i in rows}
        keys={}
        for i,(expr,children) in enumerate(dag.nodes):
            if expr['kind']!='call' or i in covered:continue
            signature=[]
            for child in children:
                e,_=dag.nodes[child]
                signature.append(('String',json.loads(e['text'])) if e['kind']=='literal' and e['sort']=='String' else ('any',dag.sort(child)))
            keys[(expr['head'],tuple(signature))]=len(children)+3 # output + timestamp + subsume
        return sum(keys.values())+2*len(dag.globals)

    def __call__(self, bundle, selected, folder):
        from .compiler import runtime_header
        from .input_spine import project
        from .pricing import rule_recipe
        folder.mkdir(parents=True,exist_ok=True)
        start=time.perf_counter()
        initial,plan=project(self.data,self.candidates,bundle['chosen'],selected,bundle,folder)
        inputs=initial if initial is not None else actions(
            [a['action'] for a in self.data['actions']],True,self.data['signatures'])
        environment=runtime_header(bundle['chosen'],selected,self.environment,self.signatures)
        lowered=[markers.lower_ast(r,selected,self.signatures) for r in bundle['consumers']]
        lowered=[r for r in lowered if not r.get('disabled')]
        # Preserve rule order, tables, guards and input exactly. Only names and
        # local variable spelling can differ; those do not alter stored rows.
        emitted=[markers.render(r) for r in lowered]
        # Search plans can depend on syntactic order and scheduling names.
        # Semantic/alpha equivalence alone is not a performance cache key.
        key=json.dumps((environment,inputs,emitted if self.measure_search else
                        [(r.get('ruleset'),rule_recipe(r)) for r in lowered]),sort_keys=True)
        environment += emitted
        request=dict(environment='\n'.join(environment),input='\n'.join(inputs),run=markers.RUNTIME_SCHEDULE,
                     measure_search=self.measure_search)
        if key not in self.cache:
            path=folder/'storage-input.json';path.write_text(json.dumps(request)+'\n')
            with (folder/'storage.stderr').open('w') as stderr:
                proc=subprocess.run([self.runner,'storage',path],text=True,stdout=subprocess.PIPE,stderr=stderr)
            if proc.returncode:
                raise RuntimeError(f'instance storage execution failed; see {folder/"storage.stderr"}')
            measured=json.loads(proc.stdout.splitlines()[-1])
            if measured.get('schema')!='cell-instance-storage/v1':raise ValueError('unsupported instance storage report')
            self.cache[key]=measured;self.evaluations+=1
        measured=self.cache[key]
        result=dict(value=measured['checkpoint_peak_value_slots'],native=measured,
            initial=initial,input_plan=plan,folder=str(folder),
            scope='current input under the exact initial-row-cover policy for this ABI; initializer ties and future inputs are not globally optimized')
        (folder/'storage.json').write_text(json.dumps(result,indent=2)+'\n')
        self.seconds+=time.perf_counter()-start
        return result
