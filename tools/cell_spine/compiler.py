"""Native Bake -> exact catalog -> Gurobi -> multi-node spine .egg.

Only positive constructor/equality certificates authorize template substitution.
Other programs are explicitly emitted as unchanged native fallbacks.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import subprocess
import sys

from .terms import actions, expressions, text, walk
from .planner import solve

ROOT = Path(__file__).resolve().parents[2]


def run(args, **kwargs):
    subprocess.run([str(a) for a in args], cwd=ROOT, check=True, **kwargs)


def ast(path, dest):
    run([ROOT / 'target/release/examples/lhs_ast', path, dest])
    return json.loads(dest.read_text())


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def commands(data, kinds):
    return [c['text'] for c in data['commands'] if c['kind'] in kinds]


def shape(acts, params):
    return sorted({text(e, params=params) for a in expressions(acts) for e in walk(a)
                   if e['kind'] == 'call' and text(e) not in params})


def load_candidates(analysis, work, source):
    report = json.loads((analysis / 'analysis.json').read_text())['saturated_rule_composition']
    catalog = (report.get('catalog') or {}).get('catalog') or {}
    families = {t['binding_origin']['candidate_id']: t['saturated_rule_composition']
                for t in catalog.get('triggers', [])}
    mappings = {t['binding_origin']['candidate_id']: t['value_map'] for t in catalog.get('triggers', [])}
    candidates, rejected = [], []
    for job in report.get('jobs', []):
        cid = job['candidate_id']
        folder = Path(job.get('folder', ''))
        if not folder.is_absolute():
            folder = ROOT / folder
        try:
            if cid not in families:
                raise ValueError(f"no completed exact certificate ({job.get('state')})")
            r = json.loads((folder / 'ripen.json').read_text())
            snapshot = json.loads((folder / 'saturated-rule-composition.json').read_text())
            if (r['ripen']['state'] != 'Saturated' or r['ripen'].get('updated') is not False
                or r.get('native_stop_reason') is not None
                or r.get('saturated_rule_composition',{}).get('status') != 'exported'
                or r['checks'] != 'passed'
                or snapshot['scope'].get('semantics') != 'positive-constructor-equality/v1'
                or snapshot['scope'].get('observation_only') or snapshot.get('subsumed_rows')):
                raise ValueError('requires complete positive constructor/equality certificate')
            o = r['origin']
            marker = o['parameter_marker']
            markers = set(o.get('parameter_markers', {}).values()) or {marker}
            if o.get('excluded_seed_rules') or snapshot['scope'].get('symbolic_size_conditions'):
                raise ValueError('excluded seed rules or erased scalar conditions are not spine certificates')
            probe = work / f'path-{cid}.egg'
            seed = '\n'.join(f for s in o['staged_injections'] for f in s['facts'])
            probe.write_text(o['initial_source'] + '\n' + seed + '\n')
            data = ast(probe, work / f'path-{cid}.json')
            if any(s['kind'] != 'constructor' for s in data['signatures'].values()):
                raise ValueError('relation/function effects are not constructor certificates')
            acts = [a['action'] for a in data['actions']]
            params = {}
            param_sorts = {}
            for e in expressions(acts):
                for sub in walk(e):
                    if sub['kind'] == 'call' and sub['head'] in markers:
                        raw = text(sub)
                        params[raw] = 'p' + str(len(params)) if raw not in params else params[raw]
                        param_sorts[raw] = data['signatures'][sub['head']]['output']
            # Constant RipenInput identity belongs to the interface, not its shape.
            acts = [a for a in acts if not (a['kind'] == 'expr' and text(a['expr']) in params)]
            if not acts or any(a['kind'] not in ('expr', 'union') for a in acts):
                raise ValueError('no declarative positive seed forest')
            bound = {text(e) for a in expressions(acts) for e in walk(a)}
            if not set(params) <= bound:
                raise ValueError('unbound isolated input requires an explicit interface')
            # Forces every expression through the typed constructor whitelist.
            actions(acts, True, data['signatures'], params)
            roots = {text(e): e for e in expressions(acts)}
            nested = {text(e) for root in roots.values() for child in root.get('args', []) for e in walk(child)}
            roots = [e for raw, e in roots.items() if raw not in nested]
            candidates.append(dict(id=cid, family=families[cid], acts=acts, roots=roots,
                params=params, param_sorts=param_sorts, nodes=shape(acts, params),
                lhs_atoms=len(shape(acts, params)) + sum(a['kind']=='union' for a in acts),
                signatures=data['signatures'], origin=o, snapshot=snapshot,
                value_map=mappings[cid], source_folder=str(folder),
                rulesets=r['ripen']['rulesets'], marker=marker, markers=markers))
        except (ValueError, KeyError, FileNotFoundError) as e:
            rejected.append(dict(unit=cid, reason=str(e)))
    edges = [(families[p], c['family']) for c in candidates
             for p in (c['origin'].get('composition') or {}).get('parts', []) if p in families]
    return candidates, edges, rejected


def fork_definitions(signatures):
    sorts = sorted({s['output'] for s in signatures.values()})
    out = ['(ruleset Spine-Run)', '(ruleset Spine-Export)']
    out += [f'(sort Spine-{s})' for s in sorts]
    for op, sig in signatures.items():
        inputs = ' '.join('Spine-' + t if t in sorts else t for t in sig['input'])
        out.append(f'(constructor Spine-{op} ({inputs}) Spine-{sig["output"]})')
    for s in sorts:
        out.append(f'(function Spine-Reified-{s} (Spine-{s}) {s} :merge old)')
    for op, sig in signatures.items():
        xs = [f'x{i}' for i in range(len(sig['input']))]
        ys = [f'y{i}' if t in sorts else x for i,(t,x) in enumerate(zip(sig['input'],xs))]
        body = [f'(= s (Spine-{op} {" ".join(xs)}))']
        body += [f'(= {y} (Spine-Reified-{t} {x}))' for t,x,y in zip(sig['input'],xs,ys) if t in sorts]
        original = f'({op} {" ".join(ys)})'
        reified = f'(Spine-Reified-{sig["output"]} s)'
        out.append(f'(rule ({" ".join(body)}) ((set {reified} {original})) :ruleset Spine-Export)')
        # Retaining old in the function is only a representative choice. Union
        # every alternate constructor denotation explicitly; none may be lost.
        out.append(f'(rule ({" ".join(body)} (= previous {reified})) ((union previous {original})) :ruleset Spine-Export)')
    return out


def resolve(snapshot, expr):
    if expr['kind'] == 'literal':
        return next(i for i,v in enumerate(snapshot['values']) if v['sort']==expr['sort'] and v['literal']==expr['text'])
    args = [resolve(snapshot, a) for a in expr['args']]
    return next(row['result'] for row in snapshot['rows'] if row['op']==expr['head'] and row['args']==args)


def mapping_to(source, target):
    """Map opaque inputs through the certified full-state bijection."""
    def port_key(c, raw):
        e = parse_param(c,raw)
        # Two distinct input ports may already be unioned in the closure.
        # Preserve their marker rows/arguments, not just their merged result.
        return (e['head'], tuple(c['value_map'][resolve(c['snapshot'],a)] for a in e['args']))
    target_values = {port_key(target,raw): name
                     for raw,name in target['params'].items()}
    return {raw: target_values[port_key(source,raw)]
            for raw in source['params']}


def parse_param(c, raw):
    return next(e for root in expressions(c['acts']) for e in walk(root) if text(e)==raw)


def pattern(c, params):
    sig = c['signatures']
    body = []
    for i,a in enumerate(c['acts']):
        if a['kind']=='expr':
            body.append(f'(= match_{i} {text(a["expr"], True, sig, params)})')
        else:
            body.append('(= ' + ' '.join(text(e, True, sig, params) for e in a['args']) + ')')
    return list(dict.fromkeys(body))


def summary(c, params, roots=None):
    args = roots if roots is not None else [text(e, True, c['signatures'], params) for e in c['roots']]
    args += [params[raw] for raw in c['params']]
    return f'(Spine-Summary-{c["family"]} {" ".join(args)})'


def incremental_body(c, params, by_id, chosen):
    """Factor an observed parent only when its seed is syntactically contained.

    Token IDs come from one native history, not e-class ID guesses. The
    containment check proves the ingress path can create the parent Summary.
    No search over arbitrary Cell pairs is performed.
    """
    child_exprs = {text(e): e for a in expressions(c['acts']) for e in walk(a)}
    token_terms = {v['token']: v['term'] for v in c['origin']['symbolic_values']
                   if v['term'] in child_exprs}
    child_nodes = {text(e, True, c['signatures'], params) for e in child_exprs.values()}
    for pid in (c['origin'].get('composition') or {}).get('parts', []):
        if pid not in by_id:
            continue
        parent = by_id[pid]
        if parent['family'] == c['family']:
            continue
        native_tokens = {v['term']: v['token'] for v in parent['origin']['symbolic_values']}
        try:
            subst = {raw: text(child_exprs[token_terms[native_tokens[raw]]], True, c['signatures'], params)
                     for raw in parent['params']}
        except KeyError:
            continue
        if any(a['kind'] != 'expr' for a in parent['acts']):
            continue  # Equality obligations cannot be erased by node containment.
        provided = {text(e, True, parent['signatures'], subst)
                    for a in expressions(parent['acts']) for e in walk(a)}
        if not provided <= child_nodes:
            continue
        target = chosen[parent['family']]
        normalized = mapping_to(parent, target)
        slots = {normalized[raw]: value for raw,value in subst.items()}
        target_subst = {raw: slots[name] for raw,name in target['params'].items()}
        root_names = [f'parent_root_{i}' for i in range(len(target['roots']))]
        root_aliases = {text(e,True,target['signatures'],target_subst): n for e,n in zip(target['roots'],root_names)}
        residual_params = dict(params)
        for raw,e in child_exprs.items():
            if text(e,True,c['signatures'],params) in root_aliases:
                residual_params[raw] = root_aliases[text(e,True,c['signatures'],params)]
        residual = []
        removed = 0
        for i,a in enumerate(c['acts']):
            if a['kind']=='expr' and text(a['expr'],True,c['signatures'],params) in provided:
                removed += 1
                continue
            if a['kind']=='expr':
                residual.append(f'(= match_{i} {text(a["expr"],True,c["signatures"],residual_params)})')
            else:
                residual.append('(= '+' '.join(text(e,True,c['signatures'],residual_params) for e in a['args'])+')')
        if removed:
            return [summary(target,target_subst,root_names)] + residual, dict(parent_unit=pid,parent_family=parent['family'],removed_seed_atoms=removed)
    return pattern(c,params), None


def closure_export(c):
    """Emit the cached finite graph as one summary-driven action batch.

    A spanning constructor forest creates e-class representatives. Remaining
    rows union into those representatives, preserving cycles and sharing.
    Opaque RipenInput rows become actual decoded bindings, never real holes.
    """
    state = c['snapshot']
    values = {i:v['literal'] for i,v in enumerate(state['values']) if v['literal'] is not None}
    roots=[f'export_root_{i}' for i in range(len(c['roots']))]
    body=[summary(c,c['params'],roots)]
    head=[]
    for i,(raw,p) in enumerate(c['params'].items()):
        value=resolve(state,parse_param(c,raw))
        actual=f'export_input_{i}'
        body.append(f'(= {actual} (Spine-Reified-{c["param_sorts"][raw]} {p}))')
        if value in values:
            head.append(f'(union {values[value]} {actual})')
        else:
            values[value]=actual
    rows=[r for r in state['rows'] if r['op'] not in c['markers']]
    pending=list(rows)
    while pending:
        deferred=[]
        changed=False
        for row in pending:
            if not all(i in values for i in row['args']):
                deferred.append(row);continue
            expr='('+' '.join([row['op']]+[values[i] for i in row['args']])+')'
            result=row['result']
            if result not in values:
                name=f'export_class_{result}'
                head.append(f'(let {name} {expr})');values[result]=name
            else:
                head.append(f'(union {values[result]} {expr})')
            changed=True
        if not changed:
            raise ValueError(f'Cell {c["id"]} cannot reconstruct an anchored constructor forest')
        pending=deferred
    # Connect the selected forked roots to the correct native e-classes; this
    # also lets a later enclosing summary consume the cached output directly.
    for root,expr in zip(roots,c['roots']):
        v=values[resolve(state,expr)]
        sort=c['signatures'][expr['head']]['output']
        head.append(f'(set (Spine-Reified-{sort} {root}) {v})')
    return f'(rule ({" ".join(body)}) ({" ".join(head)}) :ruleset Spine-Export)'


def make_library(candidates, selected, environment, signatures):
    chosen = {c['family']:c for c in candidates if c['id'] in selected}
    out = environment + fork_definitions(signatures)
    for c in chosen.values():
        types = [signatures[e['head']]['output'] for e in c['roots']]
        types += [c['param_sorts'][raw] for raw in c['params']]
        out.append(f'(relation Spine-Summary-{c["family"]} ({" ".join("Spine-"+t for t in types)}))')
    emitted = {}
    by_id = {c['id']:c for c in candidates}
    transitions = []
    for c in candidates:
        target = chosen[c['family']]
        params = mapping_to(c, target)
        head = actions(target['acts'], True, signatures, target['params'])
        # Carry every proven equality between visible spine nodes and ports.
        # Hidden smooth constructor rows remain in the cached export only.
        visible = {}
        for path,binding in [(c,params),(target,target['params'])]:
            for root in expressions(path['acts']):
                for e in walk(root):
                    if e['kind'] != 'call':
                        continue
                    cls = path['value_map'][resolve(path['snapshot'],e)]
                    term = text(e,True,signatures,binding)
                    if cls in visible and visible[cls] != term:
                        head.append(f'(union {visible[cls]} {term})')
                    else:
                        visible[cls] = term
        head = list(dict.fromkeys(head))
        head.append(summary(target, target['params']))
        body, transition = incremental_body(c,params,by_id,chosen)
        rule = f'(rule ({" ".join(body)}) ({" ".join(head)}) :ruleset Spine-Run)'
        key = (tuple(pattern(c,params)),tuple(head))
        if key not in emitted or (transition and not emitted[key][2]):
            emitted[key] = (c,rule,transition)
    for c,rule,transition in emitted.values():
        if transition:
            transitions.append(dict(target_unit=c['id'],target_family=c['family'],**transition))
        out += [f'; exact Cell {c["family"]}: observed path {c["id"]} -> selected path {chosen[c["family"]]["id"]}',rule]
    out += [closure_export(c) for c in chosen.values()]
    return '\n'.join(out)+'\n', chosen, transitions


def compatible_program(data):
    if len(data['rule_rulesets']) != 1:
        return 'requires exactly one rule-bearing ruleset'
    if data['duplicate_signatures']:
        return 'duplicate signatures'
    if any(s['kind']!='constructor' for s in data['signatures'].values()):
        return 'relation/function semantics need an explicit spine effect contract'
    if any(d.get('presort') for d in data['declarations']):
        return 'container sorts need an explicit spine interface contract'
    if any(c['kind']=='unsupported' for c in data['commands']):
        return 'unsupported top-level command (include, extraction, stack or options)'
    schedules = [c for c in data['commands'] if c['kind']=='schedule']
    if (len(schedules)!=1 or data['schedules'][0].get('kind')!='saturate_single'
        or data['schedules'][0].get('has_until')
        or data['schedules'][0].get('ruleset') != data['rule_rulesets'][0]):
        return 'exact export currently requires one saturation boundary, not a finite-step schedule'
    cut = schedules[0]['ordinal']
    if any(c['ordinal']<cut for c in data['checks']):
        return 'early checks must retain their original stage'
    if any(a['action']['kind'] not in ('expr','let','union') for a in data['actions']):
        return 'non-positive initial action'
    if any(e['kind']=='call' and e['head'] not in data['signatures']
           for root in expressions([a['action'] for a in data['actions']]) for e in walk(root)):
        return 'primitive computation in initial actions must retain native evaluation'
    if any(name.startswith('Spine-') for name in data['signatures']):
        return 'Spine- namespace already used by source'
    first = min((a['command_ordinal'] for a in data['actions']), default=cut)
    if any(c['kind'] in ('declaration','rule') and c['ordinal']>first for c in data['commands']):
        return 'interleaved declarations/rules may capture earlier global bindings'
    if any(a['command_ordinal']>cut for a in data['actions']):
        return 'late external injection must retain its original stage'
    return None


def cellify(data, library):
    sig = data['signatures']
    acts = [a['action'] for a in data['actions']]
    lines = [library, '; Automatic constructor-level ingress; no scan of a materialized original graph.']
    lines += actions(acts, True, sig)
    lines += ['(run-schedule (saturate Spine-Run))']
    runtime = '\n'.join(lines)+'\n'
    types = {}
    def sort(e):
        if e['kind']=='call': return sig[e['head']]['output']
        if e['kind']=='literal': return e['sort']
        return types[e['name']]
    for a in acts:
        if a['kind']=='let': types[a['name']] = sort(a['expr'])
    eqsorts = {s['output'] for s in sig.values()}
    def reify(e):
        if e['kind']=='var' and types.get(e['name']) in eqsorts:
            return f'(Spine-Reified-{types[e["name"]]} {e["name"]})'
        if e['kind']=='call': return '('+' '.join([e['head']]+[reify(a) for a in e['args']])+')'
        return text(e)
    restore = ['(run-schedule (saturate Spine-Export))']
    for check in data['checks']:
        facts=[]
        for f in check['facts']:
            facts.append('(= '+' '.join(reify(e) for e in f['args'])+')' if f['kind']=='eq' else reify(f['expr']))
        restore.append('(check '+' '.join(facts)+')')
    return runtime, '\n'.join(restore)+'\n'


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--source',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    p.add_argument('--analysis-dir',type=Path)
    p.add_argument('--storage-weight',type=float,default=1.)
    p.add_argument('--search-weight',type=float,default=1.)
    p.add_argument('--update-weight',type=float,default=1.)
    p.add_argument('--skip-build',action='store_true')
    a=p.parse_args()
    weights=(a.storage_weight,a.search_weight,a.update_weight)
    if not all(math.isfinite(w) and w>=0 for w in weights) or not any(weights):
        raise ValueError('weights must be finite, nonnegative and not all zero')
    a.source=a.source.resolve(); a.output=a.output.resolve()
    if a.output.exists() and any(a.output.iterdir()):
        raise ValueError('output must be empty; use a fresh output directory')
    a.output.mkdir(parents=True,exist_ok=True)
    work=a.output/'work';work.mkdir()
    if not a.skip_build:
        run(['cargo','build','--offline','--release','--bin','egg_layout','--example','lhs_ast','--example','spine_validate'])
    data=ast(a.source,work/'input.json')
    report=dict(schema='cell-spine/v1',source=str(a.source),source_sha256=digest(a.source))
    problem=compatible_program(data)
    if problem:
        (a.output/'runtime.egg').write_bytes(a.source.read_bytes())
        report.update(status='native_fallback',reason=problem,compressed=False)
    else:
        try:
            import gurobipy  # Fail before native analysis if the requested solver is absent.
        except ImportError as e:
            raise RuntimeError('install tools/cell_spine/requirements.txt in the Python environment running this command') from e
        analysis=a.analysis_dir.resolve() if a.analysis_dir else a.output/'analysis'
        if not a.analysis_dir:
            env=dict(os.environ,EGG_LAYOUT_GRAPHS='0')
            run([ROOT/'target/release/egg_layout','analyze','--source',a.source,'--output',analysis],env=env)
        else:
            captured=analysis/'source.egg'
            if not captured.exists(): captured=analysis/'input.egg'
            if not captured.exists() or captured.read_text().strip()!=a.source.read_text().strip():
                raise ValueError('analysis source does not match input')
        cs,edges,rejected=load_candidates(analysis,work,data)
        if not cs:
            (a.output/'runtime.egg').write_bytes(a.source.read_bytes())
            report.update(status='native_fallback',reason='no supported completed Cell paths',rejected=rejected,compressed=False)
        else:
            plan=solve(cs,edges,a.output,weights)
            # Definitions preserve the source rules. Only opaque parameter constructors are added.
            sig=dict(data['signatures']);environment=commands(data,{'declaration','rule'})
            for c in cs:
                for name,s in c['signatures'].items():
                    if name not in sig:
                        environment.append(f'(constructor {name} ({" ".join(s["input"])}) {s["output"]})')
                        sig[name]=s
            lib,chosen,transitions=make_library(cs,plan['selected'],environment,sig)
            runtime,restore=cellify(data,lib)
            cases=[]
            for c in cs:
                # Check the actual staged native entry against flattened spine export.
                src=Path(c['source_folder'])/'entry.egg'
                seed=(work/f'path-{c["id"]}.json')
                ds=json.loads(seed.read_text())
                compact='\n'.join(actions([x['action'] for x in ds['actions']],True,sig))+'\n(run-schedule (saturate Spine-Run))\n'
                ruleset=c['rulesets'][0] if c['rulesets'] else ''
                finish='(run-schedule (saturate Spine-Export))\n'
                cases.append(dict(name=f'path-{c["id"]}',reference_path=str(src),reference=f'\n(run-schedule (saturate (run {ruleset})))',compressed=compact,restore=finish,
                                  expected_summary=f'Spine-Summary-{c["family"]}'))
            cases.append(dict(name='source',reference_path=str(a.source),reference='',compressed=runtime.removeprefix(lib),restore=restore))
            manifest=dict(constructors=sorted(sig),cases=cases,library=lib)
            (work/'validation.json').write_text(json.dumps(manifest))
            run([ROOT/'target/release/examples/spine_validate',work/'validation.json',a.output/'validation.json'])
            # Publish executable output only after every native differential check.
            (a.output/'library.egg').write_text(lib)
            (a.output/'runtime.egg').write_text(runtime)
            (a.output/'restore.egg').write_text(runtime+restore)
            report.update(status='compiled',compressed=True,analysis=str(analysis),plan=plan,rejected=rejected,
                          library_bytes=len(lib.encode()), runtime_bytes=len(runtime.encode()),
                          transitions=transitions,
                          paths=[dict(id=c['id'],family=c['family'],nodes=c['nodes'],lhs_atoms=c['lhs_atoms'],source_folder=c['source_folder']) for c in cs],
                          guarantees='exact finite constructor closure at explicit export from cached snapshots; original rules do not run on compressed path',
                          boundary='baked path whitelist only; no general hidden-fact matcher or automatic dynamic rebake')
    (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ('status','compressed','reason') if k in report}))


if __name__=='__main__':
    main()
