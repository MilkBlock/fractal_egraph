"""Native Bake -> exact catalog -> Gurobi -> multi-node spine .egg.

Only positive constructor/equality certificates authorize template substitution.
Snapshot archives are available only through explicit --mode stages; they are
not a fallback or a successful Cell compilation.
"""
import argparse
import hashlib
import itertools
from collections import defaultdict
import json
import math
import os
from pathlib import Path
import subprocess
import sys

from .terms import actions, expressions, text, walk
from .planner import solve
from . import takeover, markers

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


def load_candidates(analysis, work):
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
    return candidates, rejected


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


def summary(c, params, roots=None):
    args = list(roots) if roots is not None else [text(e, True, c['signatures'], params) for e in c['roots']]
    args += [params[raw] for raw in c['params']]
    return f'(Spine-Summary-{c["family"]} {" ".join(args)})'


def parent_wirings(c, params, by_id):
    """Only observed parents whose seeds/guards occur in this entry.

    History tokens align typed ports. Final closure equality is not sufficient
    evidence for dropping an entry precondition. Unsupported parents stay as
    explicit residual facts; never guess a join from family IDs alone.
    """
    child_exprs = {text(e): e for a in expressions(c['acts']) for e in walk(a)}
    token_terms = {v['token']: v['term'] for v in c['origin']['symbolic_values']
                   if v['term'] in child_exprs}
    child_nodes = {text(e, True, c['signatures'], params) for e in child_exprs.values()}
    def guards(path, binding):
        return {tuple(sorted(text(e, True, path['signatures'], binding) for e in a['args']))
                for a in path['acts'] if a['kind'] == 'union'}
    child_guards = guards(c, params)
    result = []
    for pid in dict.fromkeys((c['origin'].get('composition') or {}).get('parts', [])):
        parent = by_id.get(pid)
        if parent is None or parent['family'] == c['family']:
            continue  # A self Summary cannot bootstrap its own ingress.
        native_tokens = {v['term']: v['token'] for v in parent['origin']['symbolic_values']}
        try:
            subst = {raw: text(child_exprs[token_terms[native_tokens[raw]]], True, c['signatures'], params)
                     for raw in parent['params']}
            if any(parent['param_sorts'][raw] != c['signatures'][child_exprs[token_terms[native_tokens[raw]]]['head']]['output']
                   for raw in parent['params']):
                continue
        except KeyError:
            continue
        provided = {text(e, True, parent['signatures'], subst)
                    for a in expressions(parent['acts']) for e in walk(a)}
        if not provided <= child_nodes or not guards(parent, subst) <= child_guards:
            continue
        if not any(a['kind'] == 'expr' and text(a['expr'], True, c['signatures'], params) in provided
                   for a in c['acts']):
            continue
        result.append(dict(parent=parent, subst=subst, provided=provided,
                           native_subst={raw:child_exprs[token_terms[native_tokens[raw]]] for raw in parent['params']}))
    return result


def alias_binding(path, binding, aliases):
    result = dict(binding)
    for root in expressions(path['acts']):
        # Project children first, so Get(Single(x),0) can become x through
        # the parent's root field without querying the hidden Get row.
        for e in reversed(list(walk(root))):
            direct = text(e, True, path['signatures'], binding)
            projected = text(e, True, path['signatures'], result)
            if direct in aliases or projected in aliases:
                result[text(e)] = aliases[direct] if direct in aliases else aliases[projected]
    return result


def visible_projections(c, binding, roots):
    """Certified closed rows whose result can be read from a Summary field."""
    state = c['snapshot']
    known = {i:v['literal'] for i,v in enumerate(state['values']) if v['literal'] is not None}
    for raw, value in binding.items():
        known[resolve(state,parse_param(c,raw))] = value
    for expr, name in zip(c['roots'],roots):
        known.setdefault(resolve(state,expr),name)
    aliases = {}
    for row in state['rows']:
        if row['op'] in c['markers'] or row['result'] not in known:
            continue
        if all(arg in known for arg in row['args']):
            term = '('+' '.join(['Spine-'+row['op']]+[known[arg] for arg in row['args']])+')'
            aliases[term] = known[row['result']]
    return aliases


def lower_option(c, target, wirings, chosen):
    """Compile the exact hyperedge implementation that the ILP will price."""
    params = mapping_to(c, target)
    aliases, provided, body, parents = {}, set(), [], []
    projections = {}
    for i, w in enumerate(wirings):
        parent, subst = w['parent'], w['subst']
        representation = chosen[parent['family']]
        normalized = mapping_to(parent, representation)
        slots = {normalized[raw]: value for raw, value in subst.items()}
        binding = {raw: slots[name] for raw, name in representation['params'].items()}
        roots = []
        for j, e in enumerate(representation['roots']):
            term = text(e, True, representation['signatures'], binding)
            roots.append(aliases.setdefault(term, f'parent_{i}_root_{j}'))
        body.append(summary(representation, binding, roots))
        projections.update(visible_projections(representation,binding,roots))
        provided.update(w['provided'])
        parents.append(dict(unit=parent['id'], family=parent['family'], path=representation['id'],
                            slots=binding, roots=roots))
    residual_params = alias_binding(c, params, {**aliases, **projections})
    # Head materialization remains the selected spine, not its logical views.
    source_params = alias_binding(c, params, aliases)
    removed = 0
    for i, a in enumerate(c['acts']):
        if a['kind'] == 'expr':
            if text(a['expr'], True, c['signatures'], params) in provided:
                removed += 1
            else:
                body.append(f'(= match_{i} {text(a["expr"], True, c["signatures"], residual_params)})')
        else:
            # Includes cross-parent equalities: two Summaries alone are not a
            # certificate that independently bound ports have been unioned.
            body.append('(= ' + ' '.join(text(e, True, c['signatures'], residual_params) for e in a['args']) + ')')
    target_params = alias_binding(target, target['params'], aliases)
    head = []
    for a in target['acts']:
        if a['kind'] == 'expr' and text(a['expr'], True, target['signatures'], target['params']) in aliases:
            continue  # Reuse the Summary root rather than reconstruct its tree.
        head.extend(actions([a], True, target['signatures'], target_params))
    visible = {}
    for path, binding in [(c, source_params), (target, target_params)]:
        for root in expressions(path['acts']):
            for e in walk(root):
                if e['kind'] != 'call':
                    continue
                cls = path['value_map'][resolve(path['snapshot'], e)]
                term = text(e, True, path['signatures'], binding)
                if cls in visible and visible[cls] != term:
                    head.append(f'(union {visible[cls]} {term})')
                else:
                    visible[cls] = term
    head = list(dict.fromkeys(head))
    head.append(summary(target, target_params))
    body = list(dict.fromkeys(body))
    return dict(target_unit=c['id'], target_family=c['family'], target_path=target['id'],
                parents=parents, removed_seed_atoms=removed, body=body, head=head,
                requires=sorted({target['id'], *(p['path'] for p in parents)}),
                equality_guards=sum(a['kind'] == 'union' for a in c['acts']))


def transition_options(candidates):
    """All representation tuples on observed hyperedges; no arbitrary pairs.

    Parent instances are never deduplicated by family. Two occurrences of one
    family share its representation choice, but retain separate port mappings.
    No powerset search over parent subsets is needed: factor all certified
    parents, retaining everything else as a residual precondition.
    """
    by_id = {c['id']: c for c in candidates}
    groups = defaultdict(list)
    for c in candidates:
        groups[c['family']].append(c)
    edges = []
    for c in candidates:
        options = []
        for target in groups[c['family']]:
            wirings = parent_wirings(c, mapping_to(c, target), by_id)
            families = sorted({w['parent']['family'] for w in wirings})
            for paths in itertools.product(*(groups[f] for f in families)):
                chosen = {p['family']: p for p in paths}
                opt = lower_option(c, target, wirings, chosen)
                opt['id'] = f'impl_{c["id"]}_{len(options)}'
                options.append(opt)
        edges.append(dict(id=c['id'], observed_parents=(c['origin'].get('composition') or {}).get('parts', []),
                          options=options))
    return edges


def option_rule(opt):
    return f'(rule ({" ".join(opt["body"])}) ({" ".join(opt["head"])}) :ruleset Spine-Run :name "{opt["id"]}")'


def price_options(edges, work):
    """Batch native parsing counts nested lookups/writes, not just root atoms."""
    options = {o['id']: o for e in edges for o in e['options']}
    path = work/'implementations.egg'
    path.write_text('(ruleset Spine-Run)\n' + '\n'.join(option_rule(o) for o in options.values()))
    data = ast(path, work/'implementations.json')
    for rule in data['rules']:
        opt = options[rule['name']]
        reads = set()
        for fact in rule['body']:
            roots = fact['args'] if fact['kind'] == 'eq' else [fact['expr']]
            reads.update(text(e) for root in roots for e in walk(root) if e['kind'] == 'call')
        writes = {text(e) for root in expressions(rule['head']) for e in walk(root) if e['kind'] == 'call'}
        unions = sum(a['kind'] == 'union' for a in rule['head'])
        opt.update(search_atoms=len(reads) + opt['equality_guards'], update_actions=len(writes) + unions,
                   lookup_terms=sorted(reads), write_terms=sorted(writes), union_actions=unions)


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


def summary_fields(c):
    return [f'r{i}' for i in range(len(c['roots']))] + list(c['params'].values())


def canonical_subtree_lenses(chosen):
    """A selected spine entails child seeds at its actual canonical positions.

    This is constructor-pattern unification over selected spines, not a search
    of all rows in a saturated closure. Closed equality guards are checked
    before authorizing retirement or inherited consumers.
    """
    result=[]
    for owner in chosen.values():
        owner_params={raw:takeover.var(name) for raw,name in owner['params'].items()}
        forest=[takeover.fork(e,owner_params) for e in owner['roots']]
        nodes={text(e):e for root in forest for e in walk(root)}
        fields={name:parse_param(owner,raw) for raw,name in owner['params'].items()}
        def native(e):
            if e['kind']=='var':return fields[e['name']]
            if e['kind']=='call':return dict(e,head=e['head'][6:],args=[native(a) for a in e['args']])
            return e
        def match(pattern,value,binding):
            if pattern['kind']=='var':
                name=pattern['name']
                if name in binding:return binding if binding[name]==value else None
                return dict(binding,**{name:value})
            if pattern['kind']!=value['kind']:return None
            if pattern['kind']=='literal':return binding if pattern==value else None
            if pattern['head']!=value['head'] or len(pattern['args'])!=len(value['args']):return None
            for a,b in zip(pattern['args'],value['args']):
                binding=match(a,b,binding)
                if binding is None:return None
            return binding
        for child in chosen.values():
            if child['family']==owner['family']:continue
            params={raw:takeover.var(name) for raw,name in child['params'].items()}
            roots=[takeover.fork(e,params) for e in child['roots']]
            bindings=[{}]
            for root in roots:
                bindings=[b for previous in bindings for node in nodes.values()
                          if (b:=match(root,node,previous)) is not None]
            for binding in bindings:
                def instantiate(e):return takeover.replace(takeover.fork(e,params),binding)
                guards=[a for a in child['acts'] if a['kind']=='union']
                if any(len({resolve(owner['snapshot'],native(instantiate(e))) for e in a['args']})!=1 for a in guards):continue
                args=[takeover.replace(e,binding) for e in roots]+[binding[name] for name in child['params'].values()]
                result.append(dict(owner=owner['family'],child=child['family'],args=args,
                    owner_fields=summary_fields(owner),child_fields=summary_fields(child),
                    evidence=[['canonical_spine',owner['id'],child['id']]]))
    return result


def takeover_lenses(candidates, chosen):
    direct, by_id = [], {c['id']:c for c in candidates}
    for c in candidates:
        owner = chosen[c['family']]
        params = {raw:takeover.var(name) for raw,name in mapping_to(c,owner).items()}
        for w in parent_wirings(c,mapping_to(c,owner),by_id):
            parent = w['parent']; child = chosen[parent['family']]
            renamed = mapping_to(parent,child)
            slots = {renamed[raw]:takeover.fork(e,params) for raw,e in w['native_subst'].items()}
            binding = {raw:slots[name] for raw,name in child['params'].items()}
            args = [takeover.fork(e,binding) for e in child['roots']] + [binding[raw] for raw in child['params']]
            direct.append(dict(owner=owner['family'],child=child['family'],args=args,
                owner_fields=summary_fields(owner),child_fields=summary_fields(child),evidence=[[c['id'],parent['id']]]))
    direct += canonical_subtree_lenses(chosen)
    lenses, blocked = takeover.ancestors(direct)
    supports = {}
    for family, owner in chosen.items():
        fields = summary_fields(owner)
        native_fields = dict(zip(fields,owner['roots']+[parse_param(owner,raw) for raw in owner['params']]))
        def native(e):
            if e['kind']=='var': return native_fields[e['name']]
            if e['kind']=='call': return dict(e,head=e['head'].removeprefix('Spine-'),args=[native(a) for a in e['args']])
            return e
        def cls(e):
            try:
                return resolve(owner['snapshot'],native(e))
            except (KeyError, StopIteration) as error:
                raise ValueError(f'Cell {family}: inherited projection is absent from its certified closure: {text(e)}') from error
        known = {cls(takeover.var(name)):takeover.var(name) for name in fields}
        def project(e):
            value = cls(e)
            if value in known: return known[value]
            if e['kind']=='call': return dict(e,args=[project(a) for a in e['args']])
            return e
        extra, representatives = [], dict(known)
        for lens in lenses:
            if lens['owner'] != family: continue
            for arg in lens['args']:
                for e in walk(arg):
                    if e['kind'] != 'call': continue
                    value = cls(e)  # Fails closed if a projected fact is not in the certified closure.
                    if value in representatives and e != representatives[value]:
                        extra.append(dict(kind='union',args=[e,representatives[value]]))
                    else:
                        extra.append(dict(kind='expr',expr=e)); representatives[value] = e
            lens['args'] = [project(e) for e in lens['args']]
        supports[family] = fields,extra
    definitions = {}
    for family, c in chosen.items():
        fields = summary_fields(c)
        params = {raw:takeover.var(name) for raw,name in c['params'].items()}
        roots = [takeover.fork(e,params) for e in c['roots']]
        exprs = list(c['roots']) + [parse_param(c,raw) for raw in c['params']]
        classes = [resolve(c['snapshot'],e) for e in exprs]
        materialized = [takeover.fork(e,params) for root in expressions(c['acts']) for e in walk(root) if e['kind']=='call' and text(e) not in params]
        materialized += [e for a in supports[family][1] for e in expressions([a])]
        definitions[f'Spine-Summary-{family}'] = dict(fields=fields,roots=roots,materialized=materialized,
            aliases=[(i,j) for i in range(len(classes)) for j in range(i) if classes[i]==classes[j]])
    return lenses, blocked, supports, definitions


def prepare_runtime(candidates, plan, edges, work, destination=None):
    chosen = {c['family']: c for c in candidates if c['id'] in plan['selected']}
    emitted, transitions = set(), []
    for edge in edges:
        opt = next(o for o in edge['options'] if o['id'] in plan['implementations'])
        key = (tuple(opt['body']), tuple(opt['head']))
        if key in emitted:
            continue
        emitted.add(key)
        rule = option_rule(opt)
        transitions.append(dict(**opt, rule=rule))
    lenses, blocked, supports, definitions = takeover_lenses(candidates,chosen)
    parsed = {r['name']:r for r in json.loads((work/'implementations.json').read_text())['rules']}
    lowered, retire, redundant = takeover.lower([(t,parsed[t['id']]) for t in transitions],lenses,supports,definitions)
    destination=destination or work
    destination.mkdir(parents=True,exist_ok=True)
    path=destination/'lowered-consumers.egg';path.write_text('\n'.join([r['rule'] for r in lowered]+retire))
    consumers=ast(path,destination/'lowered-consumers.json')['rules']
    from .representation import extend_layouts
    return extend_layouts(dict(chosen=chosen,transitions=transitions,lenses=lenses,blocked=blocked,lowered=lowered,
                retire=retire,redundant=redundant,consumers=consumers))


def make_library(candidates, plan, edges, environment, signatures, work, prepared=None):
    prepared=prepared or prepare_runtime(candidates,plan,edges,work)
    chosen=prepared['chosen'];transitions=prepared['transitions'];consumers=prepared['consumers']
    selected_layouts={c['family']:next(l for l in c['marker_layouts'] if l['id'] in plan['marker_layouts']) for c in chosen.values()}
    out=environment + fork_definitions(signatures) + markers.definitions(signatures,fork_definitions(signatures)) + ['(sort Spine-Cell)']
    for family,c in chosen.items():
        types=markers.signature(selected_layouts[family],c)
        out.append(f'(constructor Spine-Summary-{family} ({" ".join("Spine-"+s for s in types)}) Spine-Cell)')
    runtime_rules={r['name']:[markers.lower(r,selected_layouts,signatures)] for r in consumers}
    out += [line for group in runtime_rules.values() for line in group]
    emitted_path=work/'marker-runtime.egg'
    emitted_path.write_text('\n'.join(line for group in runtime_rules.values() for line in group))
    gate_audit=[]
    for r in ast(emitted_path,work/'marker-runtime.json')['rules']:
        reads={text(e):e['head'] for f in r['body'] for root in (f['args'] if f['kind']=='eq' else [f['expr']])
               for e in walk(root) if e['kind']=='call'}
        writes={text(e):e['head'] for a in r['head'] for root in ([a['expr']] if a['kind'] in ('expr','let') else a['args'])
                for e in walk(root) if e['kind']=='call'}
        owners=sum(h.startswith('Spine-Summary-') for h in reads.values())
        canonical_reads=sum(h.startswith('Spine-Cano-') for h in reads.values())
        external_reads=sum(h.startswith('Spine-') and h[6:] in signatures for h in reads.values())
        if canonical_reads and not owners:raise ValueError(f'{r["name"]}: canonical lookup without an active marker')
        if any(h.startswith('Spine-') and h[6:] in signatures for h in writes.values()):
            raise ValueError(f'{r["name"]}: a runtime rule may not recreate raw ingress constructors')
        gate_audit.append(dict(rule=r['name'],kind='transition' if owners else 'ingress',marker_reads=owners,
                              canonical_lookups=canonical_reads,external_lookups=external_reads,
                              canonical_writes=sum(h.startswith('Spine-Cano-') for h in writes.values())))
    export_path=work/'marker-exports.egg';export_path.write_text('\n'.join(closure_export(c) for c in chosen.values()))
    exported=ast(export_path,work/'marker-exports.json')['rules']
    out += [markers.lower(r,selected_layouts,signatures,export=True) for r in exported]
    retirement=dict(lenses=prepared['lenses'],cyclic_lenses_retained=prepared['blocked'],consumers=prepared['lowered'],retirement_rules=prepared['retire'],
        redundant_inherited_rules_removed=prepared['redundant'],runtime_rules=runtime_rules,marker_layouts=selected_layouts,gate_audit=gate_audit,
        raw_contracts=prepared.get('raw_contracts',{}),costed_transitions=transitions,scope='active canonical markers; raw constructors are ingress only; no closure-read relations')
    return '\n'.join(out)+'\n', chosen, prepared['lowered'], retirement


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
    lines += [markers.RUNTIME_SCHEDULE]
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
    p.add_argument('--mode',choices=('auto','symbolic','stages'),default='auto')
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
        run(['cargo','build','--offline','--release','--bin','egg_layout','--example','lhs_ast','--example','spine_validate','--example','spine_stages','--example','native_math_baseline'])
    data=ast(a.source,work/'input.json')
    report=dict(schema='cell-spine/v1',source=str(a.source),source_sha256=digest(a.source))
    if a.mode=='stages':
        from .stages import compile_stages
        report.update(compile_stages(a.source,a.output,ROOT,'explicit snapshot archive export',weights))
        (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps({'status':report['status']})); return
    problem=compatible_program(data)
    if problem:
        raise ValueError('Cell Bake unsupported: '+problem+'. No snapshot fallback was produced; use the explicit snapshot export only for debugging.')
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
    cs,rejected=load_candidates(analysis,work)
    if not cs:
        raise ValueError('Cell Bake found no supported completed Cell paths; snapshot export is a separate explicit operation')
    for c in cs: c["marker_layouts"]=markers.layouts(c)
    edges=transition_options(cs)
    price_options(edges,work)
    # Definitions preserve the source rules. Only opaque parameter constructors are added.
    sig=dict(data['signatures']);environment=commands(data,{'declaration','rule'})
    for c in cs:
        for name,s in c['signatures'].items():
            if name not in sig:
                environment.append(f'(constructor {name} ({" ".join(s["input"])}) {s["output"]})')
                sig[name]=s
    from .pricing import RuntimePricing,audit
    pricing=RuntimePricing(lambda plan,dest:prepare_runtime(cs,plan,edges,work,dest),sig,work/'pricing',weights)
    plan=solve(cs,edges,a.output,weights,pricing=pricing)
    prepared,priced=pricing.optimize(plan)
    lib,chosen,transitions,retirement=make_library(cs,plan,edges,environment,sig,work,prepared)
    emitted=json.loads((work/'marker-runtime.json').read_text())['rules']
    audit(emitted,priced['rule_costs'])
    retirement['cost_audit']=dict(status='exact',rule_costs=priced['rule_costs'],scope='Native parser agrees with pre-solver lowered AST costs')
    runtime,restore=cellify(data,lib)
    cases=[]
    for c in cs:
        # Check the actual staged native entry against flattened spine export.
        src=Path(c['source_folder'])/'entry.egg'
        seed=(work/f'path-{c["id"]}.json')
        ds=json.loads(seed.read_text())
        compact='\n'.join(actions([x['action'] for x in ds['actions']],True,sig))+'\n'+markers.RUNTIME_SCHEDULE+'\n'
        ruleset=c['rulesets'][0] if c['rulesets'] else ''
        finish='(run-schedule (saturate Spine-Export))\n'
        cases.append(dict(name=f'path-{c["id"]}',reference_path=str(src),reference=f'\n(run-schedule (saturate (run {ruleset})))',compressed=compact,restore=finish,
                          expected_summary=f'Spine-Summary-{c["family"]}'))
    cases.append(dict(name='source',reference_path=str(a.source),reference='',compressed=runtime.removeprefix(lib),restore=restore))
    manifest=dict(constructors=sorted(sig),cases=cases,library=lib)
    (work/'validation.json').write_text(json.dumps(manifest))
    run([ROOT/'target/release/examples/spine_validate',work/'validation.json',a.output/'validation.json'])
    measured=next(c for c in json.loads((a.output/'validation.json').read_text())['cases'] if c['case']=='source')
    # Publish executable output only after every native differential check.
    (a.output/'transitions.json').write_text(json.dumps(edges,indent=2)+'\n')
    (a.output/'library.egg').write_text(lib)
    (a.output/'runtime.egg').write_text(runtime)
    (a.output/'restore.egg').write_text(runtime+restore)
    report.update(status='compiled',cellified=True,representation='cell_spine',analysis=str(analysis),plan=plan,rejected=rejected,
                  library_bytes=len(lib.encode()), runtime_bytes=len(runtime.encode()),
                  source_storage={k:measured[k] for k in ('spine_constructor_rows','native_total_rows_before_export','native_tables_before_export','original_native_total_rows')},
                  storage_scope='Native table rows, including auxiliary relations and subsumed rows; not bytes or RSS. Cellification does not imply compression.',
                  transitions=transitions,retirement=retirement,runtime_schedule=markers.RUNTIME_SCHEDULE,
                  paths=[dict(id=c['id'],family=c['family'],nodes=c['nodes'],lhs_atoms=c['lhs_atoms'],source_folder=c['source_folder']) for c in cs],
                  guarantees='exact finite constructor closure at explicit export from cached snapshots; original rules do not run on compressed path',
                  boundary='baked path whitelist only; no general hidden-fact matcher or automatic dynamic rebake')
    (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ('status','compressed','reason') if k in report}))


if __name__=='__main__':
    main()
