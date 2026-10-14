"""Native Bake -> exact catalog -> MIP selection -> multi-node spine .egg.

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
import time

from .terms import actions, expressions, text, walk
from .planner import solve
from . import takeover, markers, names
from .interfaces import is_interface

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
    from .composition import positive_environment
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
                boundary_positive=positive_environment(data),
                value_map=mappings[cid], source_folder=str(folder),
                rulesets=r['ripen']['rulesets'], marker=marker, markers=markers))
        except (ValueError, KeyError, FileNotFoundError) as e:
            rejected.append(dict(unit=cid, reason=str(e)))
    return candidates, rejected


def fork_definitions(signatures):
    sorts = sorted({s['output'] for s in signatures.values()})
    out = ['(ruleset Run!)', '(ruleset Export!)']
    out += [f'(sort {s}!)' for s in sorts]
    for op, sig in signatures.items():
        inputs = ' '.join(names.raw(t) if t in sorts else t for t in sig['input'])
        out.append(f'(constructor {op}! ({inputs}) {sig["output"]}!)')
    for s in sorts:
        out.append(f'(function Reified-{s}! ({s}!) {s} :merge old)')
    for op, sig in signatures.items():
        xs = [f'x{i}' for i in range(len(sig['input']))]
        ys = [f'y{i}' if t in sorts else x for i,(t,x) in enumerate(zip(sig['input'],xs))]
        body = [f'(= s ({op}! {" ".join(xs)}))']
        body += [f'(= {y} (Reified-{t}! {x}))' for t,x,y in zip(sig['input'],xs,ys) if t in sorts]
        original = f'({op} {" ".join(ys)})'
        reified = f'(Reified-{sig["output"]}! s)'
        out.append(f'(rule ({" ".join(body)}) ((set {reified} {original})) :ruleset Export!)')
        # Retaining old in the function is only a representative choice. Union
        # every alternate constructor denotation explicitly; none may be lost.
        out.append(f'(rule ({" ".join(body)} (= previous {reified})) ((union previous {original})) :ruleset Export!)')
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
    return f'(Summary-{c["family"]}! {" ".join(args)})'


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
            stored=c['origin'].get('bound_parent_substitutions',{}).get(str(pid))
            native_subst=stored if stored is not None else {raw:child_exprs[token_terms[native_tokens[raw]]] for raw in parent['params']}
            if set(native_subst)!=set(parent['params']):continue
            subst = {raw:text(e,True,c['signatures'],params) for raw,e in native_subst.items()}
            if any(parent['param_sorts'][raw] != c['signatures'][native_subst[raw]['head']]['output']
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
                           native_subst=native_subst))
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
            term = '('+' '.join([names.raw(row['op'])]+[known[arg] for arg in row['args']])+')'
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
    # A source-only fact supplied by a certified closure contributes its value,
    # not another physical row. Target terms still use the materialized spine
    # recipe, and are explicitly unioned to those source values when needed.
    for path, binding in [(c, residual_params), (target, target_params)]:
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


def representation_choices(candidates):
    """Quotient identical physical spines, NOT history entries or bindings.

    Compare typed seeds, ordered roots/ports and their certified closure values
    in one family's coordinates. No commutativity, literal erasure, support
    frequency, or approximate graph matching is used here.
    """
    anchors={};groups={};aliases={};representatives=[]
    for c in candidates:
        anchor=anchors.setdefault(c['family'],c)
        binding=mapping_to(c,anchor)
        key=json.dumps((c['family'],actions(c['acts'],True,c['signatures'],binding),
            [text(r,True,c['signatures'],binding) for r in c['roots']],
            [(binding[p],c['param_sorts'][p]) for p in c['params']],
            [c['value_map'][resolve(c['snapshot'],r)] for r in c['roots']]),sort_keys=True)
        if key not in groups:
            groups[key]=c['id'];representatives.append(c)
        aliases[c['id']]=groups[key]
    return representatives,aliases


def transition_options(candidates, representations=None, *, recompose=False):
    """All representation tuples on observed hyperedges; no arbitrary pairs.

    Parent instances are never deduplicated by family. Two occurrences of one
    family share its representation choice, but retain separate port mappings.
    No powerset search over parent subsets is needed: factor all certified
    parents, retaining everything else as a residual precondition.
    """
    by_id = {c['id']: c for c in candidates}
    groups = defaultdict(list)
    for c in candidates if representations is None else representations:
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
    if recompose:
        from .composition import extend
        extend(candidates,candidates if representations is None else representations,edges)
    return edges


def option_rule(opt):
    return f'(rule ({" ".join(opt["body"])}) ({" ".join(opt["head"])}) :ruleset Run! :name "{opt["id"]}")'


def price_options(edges, work):
    """Batch native parsing counts nested lookups/writes, not just root atoms."""
    options = {o['id']: o for e in edges for o in e['options']}
    path = work/'implementations.egg'
    path.write_text('(ruleset Run!)\n' + '\n'.join(option_rule(o) for o in options.values()))
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
        body.append(f'(= {actual} (Reified-{c["param_sorts"][raw]}! {p}))')
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
        head.append(f'(set (Reified-{sort}! {root}) {v})')
    return f'(rule ({" ".join(body)}) ({" ".join(head)}) :ruleset Export!)'


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
            if e['kind']=='call':return dict(e,head=e['head'].removesuffix('!'),args=[native(a) for a in e['args']])
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


def takeover_lenses(candidates, chosen, cuts=()):
    direct, by_id = [], {c['id']:c for c in candidates}
    additional=defaultdict(list)
    for edge in cuts:
        if edge.get('kind') in ('certified_seed_cut','certified_boundary_cut'):additional[edge['target_unit']].extend(edge['seed_cut']['parents'])
    for c in candidates:
        owner = chosen[c['family']]
        params = {raw:takeover.var(name) for raw,name in mapping_to(c,owner).items()}
        ws=parent_wirings(c,mapping_to(c,owner),by_id)
        if additional[c['id']]:
            from .composition import wiring
            ws+= [wiring(c,d,mapping_to(c,owner),by_id) for d in additional[c['id']]]
        for w in ws:
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
            if e['kind']=='call': return dict(e,head=e['head'].removesuffix('!'),args=[native(a) for a in e['args']])
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
    # Distinct proofs may expose the same stored interface after projection.
    unique={}
    for lens in lenses:
        key=(lens['owner'],lens['child'],tuple(lens['owner_fields']),tuple(lens['child_fields']),tuple(map(text,lens['args'])))
        if key not in unique:unique[key]=dict(lens,evidence=list(lens['evidence']))
        else:
            for evidence in lens['evidence']:
                if evidence not in unique[key]['evidence']:unique[key]['evidence'].append(evidence)
    lenses=list(unique.values())
    definitions = {}
    for family, c in chosen.items():
        fields = summary_fields(c)
        params = {raw:takeover.var(name) for raw,name in c['params'].items()}
        roots = [takeover.fork(e,params) for e in c['roots']]
        exprs = list(c['roots']) + [parse_param(c,raw) for raw in c['params']]
        classes = [resolve(c['snapshot'],e) for e in exprs]
        materialized = [takeover.fork(e,params) for root in expressions(c['acts']) for e in walk(root) if e['kind']=='call' and text(e) not in params]
        materialized += [e for a in supports[family][1] for e in expressions([a])]
        definitions[f'Summary-{family}!'] = dict(fields=fields,roots=roots,materialized=materialized,
            equalities=[[takeover.fork(e,params) for e in a['args']] for a in c['acts'] if a['kind']=='union']
                + [a['args'] for a in supports[family][1] if a['kind']=='union'],
            aliases=[(i,j) for i in range(len(classes)) for j in range(i) if classes[i]==classes[j]])
    return lenses, blocked, supports, definitions


def prepare_runtime(candidates, plan, edges, work, destination=None, *, interface_mode='auto'):
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
    lenses, blocked, supports, definitions = takeover_lenses(candidates,chosen,edges)
    parsed = {r['name']:r for r in json.loads((work/'implementations.json').read_text())['rules']}
    lowered, retire, redundant = takeover.lower([(t,parsed[t['id']]) for t in transitions],lenses,supports,definitions)
    destination=destination or work
    destination.mkdir(parents=True,exist_ok=True)
    path=destination/'lowered-consumers.egg';path.write_text('\n'.join([r['rule'] for r in lowered]+retire))
    consumers=ast(path,destination/'lowered-consumers.json')['rules']
    from .representation import extend_layouts
    from .interfaces import extend
    return extend(extend_layouts(dict(chosen=chosen,transitions=transitions,lenses=lenses,blocked=blocked,lowered=lowered,
                retire=retire,redundant=redundant,consumers=consumers,supports=supports)),interface_mode)


def runtime_header(chosen, selected_layouts, environment, signatures):
    from .interfaces import declarations
    out=environment + fork_definitions(signatures) + markers.definitions(signatures,fork_definitions(signatures)) + ['(sort Cell!)']
    for family,c in chosen.items():
        types=markers.signature(selected_layouts[family],c)
        out.append(f'(constructor Summary-{family}! ({" ".join(names.raw(s) for s in types)}) Cell!)')
    out+=declarations(selected_layouts,signatures)
    return out


def make_library(candidates, plan, edges, environment, signatures, work, prepared=None):
    prepared=prepared or prepare_runtime(candidates,plan,edges,work)
    chosen=prepared['chosen'];transitions=prepared['transitions'];consumers=prepared['consumers']
    selected_layouts={c['family']:next(l for l in c['marker_layouts'] if l['id'] in plan['marker_layouts']) for c in chosen.values()}
    out=runtime_header(chosen,selected_layouts,environment,signatures)
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
        owners=sum(names.is_summary(h) for h in reads.values())
        canonical_reads=sum(names.is_canonical(h) for h in reads.values())
        interface_reads=sum(is_interface(h) for h in reads.values())
        external_reads=sum(names.is_raw(h,signatures) for h in reads.values())
        if canonical_reads and not owners:raise ValueError(f'{r["name"]}: canonical lookup without an active marker')
        if interface_reads and not owners:raise ValueError(f'{r["name"]}: shared interface without an active marker')
        if any(names.is_raw(h,signatures) for h in writes.values()):
            raise ValueError(f'{r["name"]}: a runtime rule may not recreate raw ingress constructors')
        gate_audit.append(dict(rule=r['name'],kind='transition' if owners else 'ingress',marker_reads=owners,
                              canonical_lookups=canonical_reads,interface_lookups=interface_reads,external_lookups=external_reads,
                              canonical_writes=sum(names.is_canonical(h) for h in writes.values())))
    export_path=work/'marker-exports.egg';export_path.write_text('\n'.join(closure_export(c) for c in chosen.values()))
    exported=ast(export_path,work/'marker-exports.json')['rules']
    out += [markers.lower(r,selected_layouts,signatures,export=True) for r in exported]
    retirement=dict(lenses=prepared['lenses'],cyclic_lenses_retained=prepared['blocked'],consumers=prepared['lowered'],retirement_rules=prepared['retire'],
        redundant_inherited_rules_removed=prepared['redundant'],runtime_rules=runtime_rules,marker_layouts=selected_layouts,gate_audit=gate_audit,
        raw_contracts=prepared.get('raw_contracts',{}),costed_transitions=transitions,
        interfaces=dict(mode=prepared.get('interface_mode','inline'),selected=[f for f,l in selected_layouts.items() if l.get('interface')=='shared'],
            scope='shared seed decoders; every consumer retains its active owner marker; rows may remain after retirement'),
        scope='active canonical markers; raw constructors are ingress only; optional shared seed decoders')
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
    if any(name.endswith('!') or name.startswith(('Spine-','Reified-')) or names.is_summary(names.raw(name)) or is_interface(names.raw(name))
           for name in data['signatures']) or any(
        d.get('name','').endswith('!') or d.get('name')=='Cell' for d in data['declarations']) or any(
        r.endswith('!') for r in data['rule_rulesets']) or any(
        a['action'].get('name','').endswith('!') for a in data['actions']):
        return 'source name collides with the reserved Cell namespace (! suffix, Summary-N, Interface-N, Reified-, Cell or legacy Spine-)'
    first = min((a['command_ordinal'] for a in data['actions']), default=cut)
    if any(c['kind'] in ('declaration','rule') and c['ordinal']>first for c in data['commands']):
        return 'interleaved declarations/rules may capture earlier global bindings'
    if any(a['command_ordinal']>cut for a in data['actions']):
        return 'late external injection must retain its original stage'
    return None


def cellify(data, library, *, initial=None):
    sig = data['signatures']
    acts = [a['action'] for a in data['actions']]
    lines = [library, '; Solver-selected input spine initialization.' if initial is not None else
             '; Automatic constructor-level ingress; no scan of a materialized original graph.']
    lines += actions(acts, True, sig) if initial is None else initial
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
            return f'(Reified-{types[e["name"]]}! {e["name"]})'
        if e['kind']=='call': return '('+' '.join([e['head']]+[reify(a) for a in e['args']])+')'
        return text(e)
    restore = ['(run-schedule (saturate Export!))']
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
    p.add_argument('--knowledge-library',type=Path,
                   help='reuse native-validated Cell contracts and learn merge families across fact programs under the same rules')
    p.add_argument('--mode',choices=('auto','symbolic','stages'),default='auto')
    p.add_argument('--storage-weight',type=float,default=1.)
    p.add_argument('--search-weight',type=float,default=1.)
    p.add_argument('--update-weight',type=float,default=1.)
    p.add_argument('--storage-cost',choices=('instances','structural'),default='structural',
                   help='price current-input retained rows, or retain the inexpensive structural-only objective (default)')
    p.add_argument('--solver',choices=('scip','gurobi'),default=os.environ.get('EGG_LAYOUT_SOLVER','scip'))
    p.add_argument('--layout-pricing',choices=('enumerate','source-cache','projected'),default='projected',
                   help='exact layout pricing: original enumeration, source reuse, or source reuse plus per-transition projection (default)')
    p.add_argument('--cell-composition',choices=('observed','seed-cuts'),default=os.environ.get('EGG_LAYOUT_CELL_COMPOSITION','seed-cuts'),
                   help='observed parents only, or additionally factor certified constructor boundaries into small Cells (default)')
    p.add_argument('--cell-interfaces',choices=('inline','auto','shared'),default=os.environ.get('EGG_LAYOUT_CELL_INTERFACES','auto'),
                   help='inline seed queries, jointly selected shared decoders (default), or force shared where decoding is needed')
    p.add_argument('--keep-duplicate-spines',action='store_true',help='ablation: retain distinct variables for identical physical representations')
    p.add_argument('--skip-build',action='store_true')
    a=p.parse_args()
    from . import solver
    solver.select(a.solver)
    weights=(a.storage_weight,a.search_weight,a.update_weight)
    if not all(math.isfinite(w) and w>=0 for w in weights) or not any(weights):
        raise ValueError('weights must be finite, nonnegative and not all zero')
    a.source=a.source.resolve(); a.output=a.output.resolve()
    if a.output.exists() and any(a.output.iterdir()):
        raise ValueError('output must be empty; use a fresh output directory')
    a.output.mkdir(parents=True,exist_ok=True)
    work=a.output/'work';work.mkdir()
    phases={};phase_start=time.perf_counter()
    def phase(name):
        nonlocal phase_start
        now=time.perf_counter();phases[name]=now-phase_start;phase_start=now
    if not a.skip_build:
        run(['cargo','build','--offline','--release','--bin','egg_layout','--example','lhs_ast','--example','spine_validate','--example','spine_stages','--example','native_math_baseline'])
    phase('build')
    data=ast(a.source,work/'input.json')
    report=dict(schema='cell-spine/v1',source=str(a.source),source_sha256=digest(a.source))
    if a.mode=='stages':
        from .stages import compile_stages
        report.update(compile_stages(a.source,a.output,ROOT,'explicit snapshot archive export',weights))
        report['solver']=solver.statistics()
        (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps({'status':report['status']})); return
    problem=compatible_program(data)
    if problem:
        raise ValueError('Cell Bake unsupported: '+problem+'. No snapshot fallback was produced; use the explicit snapshot export only for debugging.')
    solver.module() # Fail before native analysis; never change backends silently.
    phase('source_prepare')
    analysis=a.analysis_dir.resolve() if a.analysis_dir else a.output/'analysis'
    if not a.analysis_dir:
        env=dict(os.environ,EGG_LAYOUT_GRAPHS='0')
        # Analyze defaults to additional history-window readings. Keep this
        # compiler's previous rooted capture policy for comparable workloads.
        # Full windows are supported with exact representation normalization;
        # explicit caller configuration and supplied catalogs are never truncated.
        env.setdefault('EGG_LAYOUT_CELL_WINDOWS','0')
        run([ROOT/'target/release/egg_layout','analyze','--source',a.source,'--output',analysis],env=env)
    else:
        captured=analysis/'source.egg'
        if not captured.exists(): captured=analysis/'input.egg'
        if not captured.exists() or captured.read_text().strip()!=a.source.read_text().strip():
            raise ValueError('analysis source does not match input')
    phase('tier0_capture')
    cs,rejected=load_candidates(analysis,work)
    if not cs:
        raise ValueError('Cell Bake found no supported completed Cell paths; snapshot export is a separate explicit operation')
    knowledge=None
    if a.knowledge_library:
        from .knowledge import Session
        knowledge=Session(a.knowledge_library,data,a.source,work,report['source_sha256'])
        cs=knowledge.augment(cs)
    for c in cs: c["marker_layouts"]=markers.layouts(c)
    representations,representation_aliases=representation_choices(cs)
    if a.keep_duplicate_spines:representations=cs;representation_aliases={c['id']:c['id'] for c in cs}
    edges=transition_options(cs,representations,recompose=a.cell_composition=='seed-cuts')
    price_options(edges,work)
    # Definitions preserve the source rules. Only opaque parameter constructors are added.
    sig=dict(data['signatures']);environment=commands(data,{'declaration','rule'})
    for c in cs:
        for name,s in c['signatures'].items():
            if name not in sig:
                environment.append(f'(constructor {name} ({" ".join(s["input"])}) {s["output"]})')
                sig[name]=s
    from .pricing import RuntimePricing,audit
    from .storage import InstanceStorage
    storage=InstanceStorage(data,cs,environment,sig,ROOT/'target/release/examples/native_math_baseline') if a.storage_cost=='instances' else None
    pricing=RuntimePricing(lambda plan,dest:prepare_runtime(cs,plan,edges,work,dest,interface_mode=a.cell_interfaces),sig,work/'pricing',weights,
                           storage=storage,layout_pricing=a.layout_pricing)
    phase('catalog_prepare')
    plan=solve(representations,edges,a.output,weights,pricing=pricing)
    prepared,priced=pricing.optimize(plan)
    phase('spine_solve')
    lib,chosen,transitions,retirement=make_library(cs,plan,edges,environment,sig,work,prepared)
    emitted=json.loads((work/'marker-runtime.json').read_text())['rules']
    audit(emitted,priced['rule_costs'])
    retirement['cost_audit']=dict(status='exact',rule_costs=priced['rule_costs'],scope='Native parser agrees with pre-solver lowered AST costs')
    phase('library_emit')
    from .input_spine import project
    if storage is None:
        initial,input_plan=project(data,cs,chosen,retirement['marker_layouts'],prepared,a.output)
    else:
        # Emit the very initializer whose storage affected selection. Re-solving
        # a tied input cover here could silently change the priced program.
        initial=priced['instance_storage']['initial'];input_plan=priced['instance_storage']['input_plan']
        import shutil
        for name in ('input-selection.lp','input-selection.sol'):
            saved=Path(priced['instance_storage']['folder'])/name
            if saved.exists():shutil.copyfile(saved,a.output/name)
    (a.output/'input-plan.json').write_text(json.dumps(input_plan,indent=2)+'\n')
    phase('input_solve')
    runtime,restore=cellify(data,lib,initial=initial)
    cases=[]
    for c in cs:
        # Check the actual staged native entry against flattened spine export.
        src=Path(c['source_folder'])/'entry.egg'
        seed=(work/f'path-{c["id"]}.json')
        ds=json.loads(seed.read_text())
        compact='\n'.join(actions([x['action'] for x in ds['actions']],True,sig))+'\n'+markers.RUNTIME_SCHEDULE+'\n'
        ruleset=c['rulesets'][0] if c['rulesets'] else ''
        finish='(run-schedule (saturate Export!))\n'
        cases.append(dict(name=f'path-{c["id"]}',reference_path=str(src),reference=f'\n(run-schedule (saturate (run {ruleset})))',compressed=compact,restore=finish,
                          expected_summary=f'Summary-{c["family"]}!'))
    cases.append(dict(name='source',reference_path=str(a.source),reference='',compressed=runtime.removeprefix(lib),restore=restore))
    manifest=dict(constructors=sorted(sig),cases=cases,library=lib)
    (work/'validation.json').write_text(json.dumps(manifest))
    phase('validation_prepare')
    run([ROOT/'target/release/examples/spine_validate',work/'validation.json',a.output/'validation.json'])
    phase('native_validation')
    measured=next(c for c in json.loads((a.output/'validation.json').read_text())['cases'] if c['case']=='source')
    if storage is not None:
        predicted=priced['instance_storage']['native']['retained']
        actual={k:v for k,v in measured['native_tables_before_export'].items() if v}
        if actual!={k:v['rows'] for k,v in predicted['tables'].items()}:
            raise ValueError('instance-priced and emitted programs retained different native table rows')
    # Publish executable output only after every native differential check.
    if digest(a.source)!=report['source_sha256']:
        raise ValueError('source changed during Bake; no executable output or knowledge was published')
    (a.output/'transitions.json').write_text(json.dumps(edges,indent=2)+'\n')
    (a.output/'library.egg').write_text(lib)
    (a.output/'runtime.egg').write_text(runtime)
    (a.output/'restore.egg').write_text(runtime+restore)
    learned=knowledge.commit(cs,edges,a.output) if knowledge else None
    if knowledge:phase('knowledge_update')
    report.update(status='compiled',cellified=True,representation='cell_spine',analysis=str(analysis),plan=plan,rejected=rejected,
                  knowledge=learned,
                  library_bytes=len(lib.encode()), runtime_bytes=len(runtime.encode()),
                  source_storage={k:measured[k] for k in ('spine_constructor_rows','native_total_rows_before_export','native_tables_before_export','original_native_total_rows')},
                  storage_scope='Native table rows, including auxiliary relations and subsumed rows; not bytes or RSS. Cellification does not imply compression.',
                  transitions=transitions,retirement=retirement,runtime_schedule=markers.RUNTIME_SCHEDULE,
                  input_plan=input_plan,paths=[dict(id=c['id'],family=c['family'],nodes=c['nodes'],lhs_atoms=c['lhs_atoms'],source_folder=c['source_folder']) for c in cs],
                  storage_cost=a.storage_cost,
                  representation_normalization=dict(history_paths=len(cs),representations=len(representations),
                      current_history_paths=len(knowledge.current_ids) if knowledge else len(cs),
                      imported_recipes=len(knowledge.imported) if knowledge else 0,
                      aliases=representation_aliases,scope='exact typed seed/root/interface identity; history_paths is all admitted recipes, including imported historical contracts when a knowledge library is used; every recipe remains a validation case'),
                  cell_composition=dict(mode=a.cell_composition,observed_interfaces=len(cs),
                      seed_cut_interfaces=sum(e.get('kind')=='certified_seed_cut' for e in edges),
                      boundary_interfaces=sum(e.get('kind')=='certified_boundary_cut' for e in edges),
                      scope='additional certified initial-fact decompositions; not new observed events or newly inferred closed states'),
                  pricing_work=dict(mode=pricing.layout_pricing,prepared_schemes=pricing.schemes,layout_evaluations=pricing.evaluations,
                      original_cartesian_tuples=pricing.cartesian_tuples,source_preparations=pricing.source_preparations,
                      source_reuses=pricing.evaluations-pricing.source_preparations,
                      layout_choices=pricing.layout_choices,projected_choices=pricing.projected_choices,
                      normalization_builds=pricing.normalization_builds,
                      normalization_reuses=pricing.evaluations-pricing.normalization_builds,
                      equivalent_model_hits=pricing.equivalent_model_hits,
                      preparation_seconds=pricing.preparation_seconds,layout_profile_seconds=pricing.profile_seconds,
                      scope='exact structural-model reuse; no candidate pruning; native instance pricing excluded'),
                  instance_storage=dict(selected=priced['instance_storage'],evaluations=storage.evaluations,seconds=storage.seconds,
                      audit='all emitted pre-export table row counts agree with native pricing') if storage is not None else None,
                  guarantees='exact finite constructor closure at explicit export from cached snapshots; original rules do not run on compressed path',
                  boundary='baked path whitelist only; no general hidden-fact matcher or automatic dynamic rebake')
    phase('output_write')
    report.update(phase_seconds=phases,compiler_seconds=sum(phases.values()),build_included=not a.skip_build,
                  solver=solver.statistics(),
                  capture_mode='reused' if a.analysis_dir else 'fresh',
                  phase_scope='sequential wall-clock stages; spine_solve includes preparation, pricing and MIP solving; solver API build and solve times are subsets, not additive stages; final report serialization and process startup excluded')
    (a.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:report[k] for k in ('status','compressed','reason') if k in report}))


if __name__=='__main__':
    main()
