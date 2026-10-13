"""Price the actual lowered runtime, including inheritance and subtree reuse.

Canonical path choices are fixed only inside one conditional subproblem. All
marker layouts are optimized together there. The outer solver may then change
those path choices. No layout-pair binary variables or candidate budgets.
"""
import json
import math
import time
from collections import defaultdict
from . import markers,names
from . import takeover
from .interfaces import storage_columns, is_interface
from .terms import actions,text,walk


def roots(action):
    result=[action['expr']] if action['kind'] in ('expr','let') else list(action['args'])
    if action['kind']=='set':result.append(action['value'])
    return result


def cost(rule):
    reads={text(e):e for f in rule['body'] for root in (f['args'] if f['kind']=='eq' else [f['expr']]) for e in walk(root) if e['kind']=='call'}
    writes={text(e):e for a in rule['head'] for root in roots(a) for e in walk(root) if e['kind']=='call'}
    guards=sum(f['kind']=='eq' and not any(e['kind']=='call' for r in f['args'] for e in walk(r)) for f in rule['body'])
    unions=sum(a['kind']=='union' for a in rule['head'])
    changes=sum(a['kind']=='change' for a in rule['head'])
    marker_columns=sum(len(e['args']) for e in writes.values() if names.is_summary(e['head']))
    marker_keys=sum(len(a['args']) for a in rule['head'] if a['kind']=='change' and names.is_summary(a['head']))
    interface_columns=sum(len(e['args']) for e in writes.values() if is_interface(e['head']))
    return dict(search_atoms=len(reads)+guards,update_actions=len(writes)+unions+changes+marker_columns+marker_keys+interface_columns,
        canonical_reads=sum(names.is_canonical(e['head']) for e in reads.values()),
        external_reads=sum(names.is_raw(e['head']) for e in reads.values()),
        marker_reads=sum(names.is_summary(e['head']) for e in reads.values()),
        canonical_writes=sum(names.is_canonical(e['head']) for e in writes.values()),
        marker_columns=marker_columns,marker_key_columns=marker_keys,unions=unions,changes=changes,
        interface_reads=sum(is_interface(e['head']) for e in reads.values()),interface_columns=interface_columns)


def audit(rules, expected):
    """Check the native-parsed emitted program against the solver's price."""
    names=[r['name'] for r in rules]
    if len(names)!=len(set(names)) or set(names)!=set(expected):
        raise ValueError('priced and emitted runtime rule sets differ')
    for rule in rules:
        actual=cost(rule)
        if any(actual[k]!=expected[rule['name']][k] for k in actual):
            raise ValueError(f'{rule["name"]}: priced and emitted runtime costs differ')


def families(rule):
    heads={e['head'] for f in rule['body'] for r in (f['args'] if f['kind']=='eq' else [f['expr']]) for e in walk(r) if e['kind']=='call'}
    heads.update(e['head'] for a in rule['head'] for r in roots(a) for e in walk(r) if e['kind']=='call')
    heads.update(a['head'] for a in rule['head'] if a['kind']=='change')
    return sorted(names.family(n) for n in heads if names.is_summary(n))


def rule_recipe(rule):
    # Cache syntactic alpha-equivalence only. Unifying guards or deleting atoms
    # here could conceal different work performed before final normalization.
    expressions=[e for f in rule['body'] for e in (f['args'] if f['kind']=='eq' else [f['expr']])]
    expressions += [e for a in rule['head'] for e in ([takeover.var(a['name'])] if a['kind']=='let' else [])+roots(a)]
    names={}
    for root in expressions:
        for e in walk(root):
            if e['kind']=='var':names.setdefault(e['name'],takeover.var(f'v{len(names)}'))
    return (tuple(takeover.fact_text(takeover.remap_fact(f,names)) for f in rule['body']),
            tuple(actions([takeover.remap_action(a,names) for a in rule['head']])))


class Diagram:
    """Reduced multi-valued cost diagram. Labels are global marker-layout IDs."""
    def __init__(self):self.nodes=[];self.index={}
    def leaf(self,value):
        key=('leaf',float(value))
        if key not in self.index:
            self.index[key]=len(self.nodes);self.nodes.append(dict(value=float(value),bound=float(value)))
        return self.index[key]
    def branch(self,choices):
        if len(set(c for _,c in choices))==1:return choices[0][1]
        grouped=defaultdict(list)
        for label,child in choices:grouped[child].append(label)
        groups=tuple((child,tuple(sorted(labels))) for child,labels in sorted(grouped.items()))
        key=('branch',groups)
        if key not in self.index:
            self.index[key]=len(self.nodes)
            self.nodes.append(dict(groups=groups,bound=max(self.nodes[c]['bound'] for c,_ in groups)))
        return self.index[key]
    def encode(self,model,variables,roots):
        from . import solver as gp
        expressions={}
        def visit(i):
            if i in expressions:return expressions[i]
            node=self.nodes[i]
            if 'value' in node:result=node['value']
            else:
                result=model.addVar(lb=0,ub=node['bound'],name=f'cost_{i}')
                for child,labels in node['groups']:
                    value=visit(child);active=gp.quicksum(variables[k] for k in labels)
                    model.addConstr(result>=value-self.nodes[child]['bound']*(1-active))
            expressions[i]=result;return result
        return [visit(r) for r in roots],len(expressions)
    def relabel(self,root,labels):
        memo={}
        def visit(i):
            if i in memo:return memo[i]
            node=self.nodes[i]
            result=i if 'value' in node else self.branch([(labels[label],visit(child)) for child,group in node['groups'] for label in group])
            memo[i]=result;return result
        return visit(root)


class RuntimePricing:
    def __init__(self,prepare,signatures,output,weights,*,storage=None,layout_pricing='projected'):
        self.prepare=prepare;self.signatures=signatures;self.output=output;self.weights=weights
        if layout_pricing not in ('enumerate','source-cache','projected'):raise ValueError('invalid layout pricing mode')
        self.layout_pricing=layout_pricing
        self.storage=storage
        self.cache={};self.diagram=Diagram();self.profiles={};self.evaluations=0;self.schemes=0
        self.equivalent_models={};self.equivalent_model_hits=0
        self.preparation_seconds=0.;self.profile_seconds=0.
        self.cartesian_tuples=0;self.source_preparations=0
        self.layout_choices=0;self.projected_choices=0
        self.normalization_builds=0
    @staticmethod
    def key(plan):return tuple(sorted(plan['selected'])),tuple(sorted(plan['implementations']))
    def profile(self,rule,chosen):
        roles=markers.layout_roles(rule)
        all_families=families(rule)
        sources=[f for f in all_families if 'read' in roles.get(f,())]
        # Keep one source snapshot on the recursive stack for all target choices.
        scope=sources+[f for f in all_families if f not in sources]
        domains=[chosen[f]['marker_layouts'] for f in scope]
        ids=tuple(tuple(l['id'] for l in ls) for ls in domains)
        # Path IDs and variable spelling do not change emitted operation
        # counts. Reuse only identical typed lowering recipes, not merely
        # equal costs observed on a few samples. Relabel decision variables.
        recipe_fields=('roots','fields','terms','decode','anchor','anchor_field','columns','representation','read_storage','stored_roots','interface','interface_types','interface_columns')
        recipes=tuple(tuple(json.dumps({k:l.get(k) for k in recipe_fields},sort_keys=True) for l in ls) for ls in domains)
        key=(rule_recipe(rule),'interface_family' in rule,recipes)
        if key in self.profiles:
            root,previous=self.profiles[key]
            return self.diagram.relabel(root,{old:new for olds,news in zip(previous,ids) for old,new in zip(olds,news)})
        self.cartesian_tuples+=math.prod(map(len,domains))
        grouped=[]
        for family,domain in zip(scope,domains):
            groups={}
            for l in domain:
                projection=markers.layout_projection(l,roles.get(family,())) if self.layout_pricing=='projected' else l['id']
                groups.setdefault(projection,[]).append(l)
            grouped.append(list(groups.values()))
            self.layout_choices+=len(domain);self.projected_choices+=len(groups)
        assignment={}
        def build(depth,source=None,normalizations=None):
            if self.layout_pricing!='enumerate' and depth==len(sources):
                source=markers.prepare_source(rule,assignment,self.signatures);self.source_preparations+=1
                normalizations={}
            if depth==len(scope):
                if self.layout_pricing=='enumerate':
                    lowered=markers.lower_ast(rule,assignment,self.signatures);self.source_preparations+=1
                    self.normalization_builds+=1
                else:
                    before=len(normalizations)
                    lowered=markers.resume_source(rule,assignment,self.signatures,source,normalizations=normalizations)
                    self.normalization_builds+=len(normalizations)-before
                c=cost(lowered);self.evaluations+=1
                return self.diagram.leaf(self.weights[1]*c['search_atoms']+self.weights[2]*c['update_actions'])
            choices=[]
            for group in grouped[depth]:
                assignment[scope[depth]]=group[0]
                child=build(depth+1,source,normalizations)
                choices.extend((layout['id'],child) for layout in group)
            del assignment[scope[depth]]
            return self.diagram.branch(choices)
        root=build(0);self.profiles[key]=root,ids
        return root
    def prices(self,bundle,selected):
        result={}
        for rule in bundle['consumers']:
            lowered=markers.lower_ast(rule,selected,self.signatures)
            if lowered.get('disabled'):continue
            result[rule['name']]=dict(cost(lowered),reused_subtrees=lowered['reused_subtrees'])
        return result
    def price_below(self,plan,upper_bound):
        return self.optimize(plan,upper_bound=upper_bound)
    def optimize(self,plan,*,upper_bound=None):
        from . import solver as gp
        key=self.key(plan)
        if key in self.cache:return self.cache[key]
        folder=self.output/f'scheme-{self.schemes:04d}';self.schemes+=1;folder.mkdir(parents=True,exist_ok=True)
        tick=time.perf_counter()
        bundle=self.prepare(plan,folder);chosen=bundle['chosen']
        preparation_seconds=time.perf_counter()-tick
        self.preparation_seconds+=preparation_seconds
        # Reuse a complete conditional minimum only for identical normalized
        # rule programs and entire layout domains. IDs are labels, not semantics.
        # Concrete input/storage pricing is deliberately excluded from this key.
        semantic_key=None
        if self.storage is None:
            semantic_key=json.dumps(([rule_recipe(r) for r in bundle['consumers']],
                [(f,[{k:v for k,v in l.items() if k not in ('id','path')} for l in c['marker_layouts']])
                 for f,c in sorted(chosen.items())]),sort_keys=True)
            if semantic_key in self.equivalent_models:
                previous,indices=self.equivalent_models[semantic_key]
                selected={f:chosen[f]['marker_layouts'][i] for f,i in indices.items()}
                prices=self.prices(bundle,selected)
                k=sum(storage_columns(l) for l in selected.values())
                q=sum(c['search_atoms'] for c in prices.values());w=sum(c['update_actions'] for c in prices.values())
                value=self.weights[0]*k+self.weights[1]*q+self.weights[2]*w
                if not math.isclose(value,previous['value'],rel_tol=1e-9,abs_tol=1e-7):
                    raise ValueError('equivalent conditional model changed its optimum')
                result=dict(previous,marker_layouts=[l['id'] for l in selected.values()],rule_costs=prices,
                    folder=str(folder),evaluated_layout_tuples=0,preparation_seconds=preparation_seconds,profile_seconds=0.,
                    isomorphic_pricing_of=previous['folder'],
                    certificate='identical alpha-normalized rules and full layout domains; structural cost only')
                self.equivalent_model_hits+=1
                (folder/'pricing.json').write_text(json.dumps(result,indent=2)+'\n')
                self.cache[key]=(bundle,result)
                return bundle,result
        before=self.evaluations
        tick=time.perf_counter()
        profiles=[self.profile(r,chosen) for r in bundle['consumers']]
        profile_seconds=time.perf_counter()-tick
        self.profile_seconds+=profile_seconds
        with gp.Model('cell_marker_value') as model:
            variables={l['id']:model.addVar(vtype=gp.BINARY,name=l['id']) for c in chosen.values() for l in c['marker_layouts']}
            for family,c in chosen.items():model.addConstr(gp.quicksum(variables[l['id']] for l in c['marker_layouts'])==1,name=f'layout_{family}')
            work,nodes=self.diagram.encode(model,variables,profiles)
            columns=gp.quicksum(storage_columns(l)*variables[l['id']] for c in chosen.values() for l in c['marker_layouts'])
            storage_bound=self.storage.lower_bound(bundle) if hasattr(self.storage,'lower_bound') else 0.
            retained=model.addVar(lb=storage_bound,name='instance_storage') if self.storage is not None else 0.
            model.setObjective(self.weights[0]*(columns+retained)+gp.quicksum(work))
            # Native storage is nonnegative. Exact-layout value cuts refine
            # this lower bound; no layout is sampled, discarded or budgeted.
            best=None;history=[];seen=set()
            while True:
                model.optimize()
                if model.Status!=gp.OPTIMAL:raise RuntimeError(f'marker value function not solved: {model.Status}')
                bound=model.ObjVal
                if best is not None and (bound>=best[0] or math.isclose(bound,best[0],rel_tol=1e-9,abs_tol=1e-7)):break
                if upper_bound is not None and bound>=upper_bound:
                    # A lower-bound certificate, NOT a measured feasible plan.
                    # The outer solver may safely discard it against its incumbent.
                    result=dict(value=bound,lower_bound_only=True,variables=model.NumVars,
                                constraints=model.NumConstrs,folder=str(folder),
                                preparation_seconds=preparation_seconds,profile_seconds=profile_seconds)
                    (folder/'pricing.json').write_text(json.dumps(result,indent=2)+'\n')
                    return bundle,result
                selected={f:next(l for l in c['marker_layouts'] if model.value(variables[l['id']])>.5) for f,c in chosen.items()}
                ids=tuple(sorted(l['id'] for l in selected.values()))
                if ids in seen:raise ValueError('priced layout returned below its certified cost')
                seen.add(ids)
                sample=self.storage(bundle,selected,folder/f'instance-{len(history):04d}') if self.storage is not None else None
                storage_value=sample['value'] if sample is not None else 0.
                if storage_value<storage_bound:raise ValueError('mandatory input storage bound exceeds native measurement')
                prices=self.prices(bundle,selected)
                q=sum(c['search_atoms'] for c in prices.values());w=sum(c['update_actions'] for c in prices.values())
                k=sum(storage_columns(l) for l in selected.values())
                value=self.weights[0]*(k+storage_value)+self.weights[1]*q+self.weights[2]*w
                history.append(dict(layouts=ids,lower_bound=bound,storage_value=storage_value,value=value))
                if best is None or value<best[0]:best=value,selected,sample
                if self.storage is None:break
                model.addConstr(retained>=storage_value*(gp.quicksum(variables[i] for i in ids)-len(ids)+1),
                                name=f'instance_value_{len(history)}')
            value,selected,sample=best
            model.write(str(folder/'selection.lp'))
            for name,v in variables.items():model.fix(v,float(name in {l['id'] for l in selected.values()}))
            model.optimize()
            if model.Status!=gp.OPTIMAL:raise RuntimeError('priced marker solution is infeasible')
            prices=self.prices(bundle,selected)
            q=sum(c['search_atoms'] for c in prices.values());w=sum(c['update_actions'] for c in prices.values())
            k=sum(storage_columns(l) for l in selected.values())
            if not math.isclose(value,model.ObjVal,rel_tol=1e-8,abs_tol=1e-6):raise ValueError('cost diagram disagrees with emitted runtime')
            model.write(str(folder/'selection.sol'))
            result=dict(value=value,marker_layouts=[l['id'] for l in selected.values()],
                costs=dict(marker_columns=sum(l['columns'] for l in selected.values()),
                           interface_columns=sum(l.get('interface_columns',0) for l in selected.values()),
                           lookup_atoms_and_guards=q,write_atoms_and_unions=w),rule_costs=prices,
                variables=model.NumVars,constraints=model.NumConstrs,cost_diagram_nodes=nodes,
                evaluated_layout_tuples=self.evaluations-before,folder=str(folder),instance_storage=sample,
                storage_pricing_history=history if self.storage is not None else [],
                mandatory_input_value_slots=storage_bound,
                preparation_seconds=preparation_seconds,profile_seconds=profile_seconds,
                storage_cost_scope='checkpoint retained Value slots, including retired markers; not index bytes or RSS' if self.storage is not None else 'structural only')
            if sample is not None:result['costs']['instance_value_slots']=sample['value']
        (folder/'pricing.json').write_text(json.dumps(dict(result,selected=plan['selected'],implementations=plan['implementations']),indent=2)+'\n')
        self.cache[key]=(bundle,result)
        if semantic_key is not None:
            indices={f:next(i for i,l in enumerate(c['marker_layouts']) if l['id'] in result['marker_layouts']) for f,c in chosen.items()}
            self.equivalent_models[semantic_key]=(result,indices)
        return bundle,result
