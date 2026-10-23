"""Generate readable Python from the frozen selected IR and replay native evidence.

This small exporter deliberately accepts only the operators in vec_add, and fails
on other computation. It is not an egglog parser or a replacement for Tiger.
"""
import hashlib
import json
from pathlib import Path
import random
import re
import struct


class Machine:
    """Teaching memory model: a token makes the selected effect order explicit."""
    def __init__(self):
        self.clock = 0
        self.events = []

    def tick(self, state, operation, **detail):
        if state != self.clock:
            raise ValueError('An old state token was reused')
        self.clock += 1
        self.events.append(dict(operation=operation, state_in=state,
                                state_out=self.clock, **detail))
        return self.clock

    def load(self, array, index, state):
        value = array[index]
        return value, self.tick(state, 'load', index=index, value=value)

    def alloc(self, length, state):
        return [None]*length, self.tick(state, 'alloc', length=length)

    def write(self, array, index, value, state):
        next_state = self.tick(state, 'write', index=index, value=value)
        array[index] = value
        return next_state


def definitions(path):
    result = dict(re.findall(r'\(let (__tmp\d+) (.*)\)', path.read_text()))
    assert result['__tmp7124'] == '(Function "vec_add" __tmp7123 __tmp7121 __tmp7116)'
    return result


def generate(bindings):
    values = {}
    lines = ['def vec_add_ir(a, b, m):', '    s0 = 0']
    rows = []
    clock = 0
    sums = 0

    def emit(node, code, kind, **extra):
        lines.append('    '+code+'  # '+node)
        rows.append(dict(node=node, python_line=len(lines), code=code, kind=kind, **extra))

    for node, expr in bindings.items():
        op, *args = expr[1:-1].split()
        if op == 'Arg': values[node] = ('a','b','s0')
        elif op in ('Int','Float'): values[node] = args[0]
        elif op == 'Const': values[node] = values[args[0]]
        elif op == 'Get': values[node] = values[args[0]][int(args[1])]
        elif op == 'Bop':
            operator = bindings[args[0]][1:-1]
            left, right = (values[x] for x in args[1:])
            if operator == 'PtrAdd': values[node] = (left, right)
            elif operator == 'Load':
                array, index = left
                name = array+index
                clock += 1
                emit(node, f'{name}, s{clock} = m.load({array}, {index}, {right})', 'load',
                     value_name=name, input_name=array, index=int(index), state_in=right, state_out=f's{clock}')
                values[node] = (name, f's{clock}')
            elif operator == 'FAdd':
                name = f'r{sums}'; sums += 1
                emit(node, f'{name} = {left} + {right}', 'add', output=name, inputs=[left,right])
                values[node] = name
            else: raise ValueError(('Unsupported Bop', expr))
        elif op == 'Alloc':
            assert args[0] == '0'
            clock += 1
            emit(node, f'out, s{clock} = m.alloc({values[args[1]]}, {values[args[2]]})', 'alloc')
            values[node] = ('out',f's{clock}')
        elif op == 'Top':
            assert bindings[args[0]] == '(Write)'
            (array,index), value, state = (values[x] for x in args[1:])
            clock += 1
            emit(node, f's{clock} = m.write({array}, {index}, {value}, {state})', 'write',
                 array=array,index=int(index),value=value,state_in=state,state_out=f's{clock}')
            values[node] = f's{clock}'
        elif op == 'Single': values[node] = (values[args[0]],)
        elif op == 'Concat': values[node] = values[args[0]]+values[args[1]]
        elif op == 'Function':
            out, state = values[args[3]]
            emit(node, f'return {out}, {state}', 'return')
        elif op not in ('Load','PtrAdd','Write','FAdd','FloatT','PointerT','StateT','TNil','TCons','TupleT'):
            raise ValueError(('Unsupported selected node',expr))
    assert (clock,sums) == (10,3)
    return '\n'.join(lines)+'\n', rows


def evaluate_ir(bindings, a, b, m):
    """Independent direct interpreter for the selected IR, before renaming/inlining."""
    env = {}
    for node, expr in bindings.items():
        op, *xs = expr[1:-1].split()
        if op == 'Arg': env[node] = (a,b,0)
        elif op == 'Int': env[node] = int(xs[0])
        elif op == 'Float': env[node] = float(xs[0])
        elif op == 'Const': env[node] = env[xs[0]]
        elif op == 'Get': env[node] = env[xs[0]][int(xs[1])]
        elif op == 'Bop':
            operator, left, right = (env[x] for x in xs)
            if operator == 'PtrAdd': env[node] = (left,right)
            elif operator == 'Load': env[node] = m.load(*left,right)
            elif operator == 'FAdd': env[node] = left+right
            else: raise ValueError(operator)
        elif op == 'Top':
            operator, ptr, val, state = (env[x] for x in xs)
            assert operator == 'Write'
            env[node] = m.write(*ptr,val,state)
        elif op == 'Alloc': env[node] = m.alloc(env[xs[1]],env[xs[2]])
        elif op == 'Single': env[node] = (env[xs[0]],)
        elif op == 'Concat': env[node] = env[xs[0]]+env[xs[1]]
        elif op == 'Function': return env[xs[3]]
        else: env[node] = op
    raise AssertionError('No Function')


def pure_closure(nodes, available):
    """Existing graph only: each pure e-node is an AND; alternatives are OR.

    Copies and a scan keep this explanation short. Native Tiger uses reverse
    edges, persistent bits and dependency counters instead of repeated scans.
    """
    result = set(available)
    waves = []
    while True:
        ready = {n['class'] for n in nodes if not n['effectful']
                 and n['class'] not in result and set(n['children']) <= result}
        if not ready:
            return result, waves
        waves.append(sorted(ready))
        result.update(ready)


def replay(trace_path):
    events = [json.loads(line) for line in trace_path.read_text().splitlines()]
    nodes = [e for e in events if e['event']=='node']
    by_node = {(n['class'],n['node']):n for n in nodes}
    initial = next(e for e in events if e['event']=='init')
    initial_set, _ = pure_closure(nodes,{initial['position']})
    assert initial_set == set(initial['available'])
    states = {0:initial_set};costs = {0:initial['cost']};closures = {}; candidates = {}
    verified = dict(candidates=0,closures=0,publishes=0)
    detail = None
    for e in events:
        kind = e['event']
        if kind=='candidate':
            n = by_node[e['target'],e['node']]
            missing = [c for c in n['children'] if c not in states[e['from']]]
            assert (missing[0] if missing else -1) == e['first_missing']
            candidates[e['from'],e['target'],e['node']] = e
            verified['candidates'] += 1
        elif kind=='closure':
            before = states[e['from']]
            after,waves = pure_closure(nodes,before|{e['target']})
            assert after-before == set(e['added']), e
            closures[e['from'],e['target']] = after
            verified['closures'] += 1
            if (e['from'],e['target']) == (73,21):
                detail = dict(before=sorted(before),target=21,waves=waves,added=e['added'])
        elif kind=='publish':
            states[e['id']] = closures[e['prev'],e['target']]
            candidate = candidates[e['prev'],e['target'],e['node']]
            assert e['cost'] == costs[e['prev']]+candidate['incremental_estimate']
            costs[e['id']] = e['cost']
            verified['publishes'] += 1
    assert verified == dict(candidates=1864,closures=1465,publishes=807)
    assert 71 in detail['before'] and 72 not in detail['before'] and 37 not in detail['before']
    assert 72 in detail['waves'][0] and 37 in detail['waves'][1]
    return dict(verified=verified,closure_example=detail,
                selected_nodes={str(c):[n for n in nodes if n['class']==c] for c in [21,37,71,72]},
                scope='Python rechecks native decisions and pure closure deltas; it does not rerun native DP, liveness hashing or greedy choices.')


def export(out):
    bindings = definitions(out/'vec_add-extracted.egg')
    code,rows = generate(bindings)
    for row in rows:
        row['ir'] = bindings[row['node']]
    namespace = {};exec(compile(code,'<generated vec_add>','exec'),namespace)
    run = namespace['vec_add_ir']
    rng = random.Random(42)
    samples = [([2.,4.,6.],[10.,20.,30.]),([0.,-0.,1.],[-0.,0.,-1.]),
               ([1e100,1e-100,-9.],[1e99,-1e-100,10.])]
    samples += [([rng.uniform(-100,100) for _ in range(3)],
                 [rng.uniform(-100,100) for _ in range(3)]) for _ in range(128)]
    for a,b in samples:
        m1,m2 = Machine(),Machine()
        result1 = run(a,b,m1);result2 = evaluate_ir(bindings,a,b,m2)
        assert result1[1] == result2[1] == 10 and m1.events == m2.events
        assert [struct.pack('d',v) for v in result1[0]] == [struct.pack('d',v) for v in result2[0]]
        assert [struct.pack('d',v) for v in result1[0]] == [struct.pack('d',x+y) for x,y in zip(a,b)]
    machine = Machine();sample,_ = run(*samples[0],machine)
    # Record real Python locals after each generated statement for both media.
    steps = []
    by_line = {r['python_line']:r for r in rows}
    instrumented = []
    for i,line in enumerate(code.splitlines(),1):
        if i in by_line and by_line[i]['kind']=='return':
            instrumented.append(f'    record({i}, locals())')
        instrumented.append(line)
        if i in by_line and by_line[i]['kind']!='return':
            instrumented.append(f'    record({i}, locals())')
    def record(line, local):
        snapshot = {k:(list(v) if isinstance(v,list) else v) for k,v in local.items()
                    if k not in ('a','b','m')}
        steps.append(dict(**by_line[line],environment=snapshot))
    ns = {'record':record};exec('\n'.join(instrumented),ns)
    assert ns['vec_add_ir'](*samples[0],Machine())[0] == sample
    try: machine.load([1.],0,0)
    except ValueError: pass
    else: raise AssertionError('Stale token accepted')
    imports = 'from dataflow_runtime import Machine\n\n'
    trailer = '''\nif __name__ == '__main__':
    m = Machine()
    answer, state = vec_add_ir([2., 4., 6.], [10., 20., 30.], m)
    for event in m.events:
        print(event)
    print('result:', answer, 'final state:', state)
'''
    (out/'vec_add_python.py').write_text('# Generated from the selected eggcc IR; token model is explanatory.\n'+imports+code+trailer)
    import inspect
    (out/'dataflow_runtime.py').write_text(inspect.getsource(Machine))
    plain = ['def vec_add(a, b):']
    for row in rows:
        kind = row['kind']
        if kind=='load': line = f"{row['value_name']} = {row['input_name']}[{row['index']}]"
        elif kind=='alloc': line = 'out = [None] * 3'
        elif kind=='add': line = row['code']
        elif kind=='write': line = f"out[{row['index']}] = {row['value']}"
        elif kind=='return': line = 'return out'
        plain.append('    '+line+'  # '+row['node'])
    (out/'vec_add_plain.py').write_text('\n'.join(plain)+"\n\nif __name__ == '__main__':\n    print(vec_add([2.,4.,6.], [10.,20.,30.]))\n")
    namespace = {};exec('\n'.join(plain),namespace)
    for a,b in samples: assert namespace['vec_add'](a,b) == [x+y for x,y in zip(a,b)]
    evidence = replay(out/'statewalk-trace.jsonl')
    evidence.update(python=code,plain_python='\n'.join(plain),rows=rows,steps=steps,input_a=samples[0][0],input_b=samples[0][1],
                    execution=machine.events,output=sample,validation_cases=len(samples),
                    ir_sha256=hashlib.sha256((out/'vec_add-extracted.egg').read_bytes()).hexdigest(),
                    simplifications=['Type constructors omitted; PtrAdd and Get aliases inlined.',
                                     'Float addition and selected effect order preserved for this finite vec_add demo.',
                                     'Integer state token is a teaching device, not native eggcc state representation.',
                                     'No general pointer, allocation identity, aliasing or whole-program equivalence claim.'])
    (out/'dataflow.json').write_text(json.dumps(evidence,ensure_ascii=False,indent=2)+'\n')
    return evidence


if __name__ == '__main__':
    import argparse
    p=argparse.ArgumentParser();p.add_argument('--output',type=Path,required=True)
    result=export(p.parse_args().output)
    print(json.dumps(dict(verified=result['verified'],cases=result['validation_cases'],result=result['output'])))
