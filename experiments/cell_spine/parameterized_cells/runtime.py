"""Batched parameter updates; CPU routes/deduplicates child-family instances.

An instance is (family, numeric parameters), not a claim that different numeric
closures are equivalent. Every accepted RHS and union remains in the effect log.
"""
import json
import subprocess

from .compiler import MIN, MAX, OPS


def evaluate(t, values):
    tape = []
    for i,(op,a,b) in enumerate(t['tape']):
        if i == t['guard_end'] and not all(tape[g] for g in t['guard_ids']): return [0, []]
        name = OPS[op]
        if name == 'slot': v = values[a]
        elif name == 'lit': v = a
        else:
            x,y = tape[a],tape[b]
            if name == '+': v = x+y
            elif name == '-': v = x-y
            elif name == '*': v = x*y
            elif name == '>': v = int(x>y)
            elif name == '>=': v = int(x>=y)
            elif name == '<': v = int(x<y)
            elif name == '<=': v = int(x<=y)
            elif name == '==': v = int(x==y)
            else: v = int(x!=y)
        if not MIN <= v <= MAX: return [2, []]
        tape.append(v)
    if not all(tape[g] for g in t['guard_ids']): return [0, []]
    return [1, [tape[i] for i in t['output_ids']]]


def validate_tasks(program, tasks):
    for tid,values in tasks:
        if not 0 <= tid < len(program.templates): raise ValueError('unknown template')
        if len(values) != program.templates[tid]['arity']: raise ValueError('wrong parameter arity')
        if any(type(v) is not int or not MIN <= v <= MAX for v in values):
            raise ValueError('parameter outside exact int32 device domain; native fallback required')


class CPU:
    def __init__(self, program): self.program = program
    def batch(self, tasks):
        validate_tasks(self.program,tasks)
        return [evaluate(self.program.templates[t],v) for t,v in tasks]


class GPU:
    """Persistent Metal process: no process/device creation per frontier."""
    def __init__(self, program, executable):
        self.program = program; self.device = None
        self.proc = subprocess.Popen([str(executable)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                     text=True, bufsize=1)
        self.dispatches = 0

    def batch(self, tasks):
        validate_tasks(self.program,tasks)
        if not tasks: return []
        payload = dict(templates=[dict(ops=t['tape'],guard_end=t['guard_end'],guards=t['guard_ids'],
                                      outputs=t['output_ids']) for t in self.program.templates], tasks=tasks)
        self.proc.stdin.write(json.dumps(payload)+'\n'); self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line: raise RuntimeError('Metal worker exited before producing a result')
        raw = json.loads(line)
        if 'error' in raw: raise RuntimeError(raw['error'])
        self.device = raw['device']; self.dispatches += 1
        return [[r['status'],r['values']] for r in raw['results']]

    def close(self):
        self.proc.stdin.close()
        self.proc.wait(timeout=30)
        self.proc.stdout.close()
        if self.proc.returncode: raise RuntimeError('Metal worker failed')


def explore(program, seeds, backend, allowed=None):
    """No speculative neighbors: next work comes from emitted child connections.

    `allowed` is the set of native-observed rule templates admitted by Bake.
    Unknown matched templates are pending, never silently treated as no-match.
    There is no hidden depth cutoff; callers must choose terminating fixtures.
    """
    seen = set(); frontier = []
    def offer(family, params):
        key = (family,tuple(params))
        if family not in program.families: raise ValueError('unknown family')
        if key not in seen: seen.add(key); frontier.append(key)
    for f,p in seeds: offer(f,p)
    effects = []; waves = []; pending = []
    while frontier:
        current,frontier = frontier,[]
        tasks = [(i,list(p)) for f,p in current for i in program.by_family[f]]
        answers = backend.batch(tasks)
        if len(answers) != len(tasks): raise ValueError('wrong batch output size')
        accepted = 0
        for (tid,values),(status,output) in zip(tasks,answers):
            t = program.templates[tid]
            if status == 0: continue
            if status == 2 or (allowed is not None and tid not in allowed):
                pending.append(dict(template=tid,parameters=values,reason='overflow' if status==2 else 'unbaked-template'))
                continue
            if status != 1 or len(output) != len(t['outputs']): raise ValueError('invalid transition result')
            children = [dict(family=c['family'],path=c['path'],parameters=[output[i] for i in c['outputs']])
                        for c in t['children']]
            effects.append(dict(template=tid,source=t['family'],parameters=values,output=output,children=children))
            accepted += 1
            for c in children: offer(c['family'],c['parameters'])
        waves.append(dict(instances=len(current),checks=len(tasks),accepted=accepted))
    return dict(instances=sorted(seen),effects=effects,waves=waves,pending=pending,
                complete=not pending,child_occurrences=sum(len(e['children']) for e in effects))


def term(t, output, offset):
    if t[0] == 'value': return str(output[offset+t[1]])
    return '(' + t[1] + (' ' if len(t)>2 else '') + ' '.join(term(c,output,offset) for c in t[2:]) + ')'


def materialize(program, result, seeds):
    if not result['complete']: raise ValueError('cannot publish unfinished numeric closure')
    lines = ['; All source facts and intermediate RHS effects are retained.']
    for f,p in seeds: lines.append('('+f+' '+' '.join(map(str,p))+')')
    for e in result['effects']:
        t = program.templates[e['template']]
        lhs = '('+t['family']+' '+' '.join(map(str,e['parameters']))+')'
        lines.append('(union '+lhs+' '+term(t['rhs'],e['output'],t['leaf_offset'])+')')
    return '\n'.join(lines)+'\n'
