#!/usr/bin/env python3
"""Actual baked Cell -> source-derived affine operators -> native effect check."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
import sys

from algebra import interface_basis,shared_affine_program
from artifacts import ROOT,DEFAULT_CELLS,FAMILY,load,prepare_probe_source,compact_evidence
from derive import Graph,Affine,extract_equations,multiply,contains,exprs,TRACKED,PRIMITIVES
sys.path.insert(0,str(ROOT/'experiments/cell_kernel'))
from lhs_compiler import body_text


def call(args,log):
    p=subprocess.run(list(map(str,args)),text=True,capture_output=True)
    log.write_text(p.stdout+p.stderr)
    if p.returncode:raise RuntimeError(f'{args[0]} failed: {p.stderr[-1400:]}')


def field_label(token):return f'{token[0]}:{token[1]}'


def parse_cell(cell,folder,parser):
    folder.mkdir(parents=True,exist_ok=True)
    source,probes=prepare_probe_source(cell)
    inp=folder/'parser-input.egg';inp.write_text(source)
    call([parser,inp,folder/'ast.json'],folder/'parser.log')
    ast=json.loads((folder/'ast.json').read_text());graph=Graph(cell['snapshot'])
    actions={a['action']['name']:a['action']['expr'] for a in ast['actions'] if a['action']['kind']=='let'}
    typed={};roots=[];term_by_value={}
    for name,proof in probes.items():
        expression=actions[name]
        if proof['kind']=='typed_token':
            value=graph.resolve(expression)
            assert graph.values[value]['sort']==proof['sort']
            typed[(proof['sort'],proof['token'])]=value
            term_by_value[value]=proof['term']
        elif ast['signatures'].get(expression.get('head'),{}).get('output')=='Expr':
            roots.append(graph.resolve(expression))
    assert len(set(roots))==1,'root stage must identify one Expr constructor output'
    root=roots[0]
    root_tokens=[t for t,v in typed.items() if v==root and t[0]=='Expr']
    assert len(root_tokens)==1
    eqs,seeds,certificates,blocked=extract_equations(graph,ast,cell['rulesets'])
    affine=Affine(eqs,seeds)
    full=affine.field(root)
    inverse={v:t for t,v in typed.items() if t[0]=='Expr'}
    if len(inverse)!=sum(t[0]=='Expr' for t in typed):
        raise ValueError('aliased typed Expr tokens require an explicit port quotient')
    assert all(k is None or k in inverse for k in full),'unanchored scalar boundary'
    # Capture source intermediate definedness; final min cannot excuse overflow.
    intermediates=[affine.expr(e) for cert in certificates for e in cert['intermediates']]
    for form in intermediates:
        assert all(v>=0 for v in form.values()),'this certificate uses a nonnegative input domain'
    # Native overapproximate effect isolation, on complete tables, not observed projection.
    isolation=['(ruleset CellOperatorIsolation)']
    for ordinal in blocked:
        rule=next(r for r in ast['rules'] if r['ordinal']==ordinal)
        facts=[f for f in rule['body'] if not any(contains(e,TRACKED|PRIMITIVES) for e in exprs(f))]
        isolation += [f'(relation CellOperatorForbidden{ordinal} ())',
                      f'(rule {body_text(facts)} ((CellOperatorForbidden{ordinal})) :ruleset CellOperatorIsolation)']
    isolation.append('(run CellOperatorIsolation 1)')
    isolation += [f'(fail (check (CellOperatorForbidden{i})))' for i in blocked]
    isolation_path=folder/'isolation.egg';isolation_path.write_text('\n'.join(isolation)+'\n')
    return dict(cell=cell,graph=graph,ast=ast,typed=typed,inverse=inverse,root=root,root_token=root_tokens[0],
                terms=term_by_value,equations=eqs,seeds=seeds,full=full,affine=affine,
                certificates=certificates,intermediates=intermediates,blocked=blocked,isolation=str(isolation_path.resolve()))


def global_form(item,form):
    return {'1' if k is None else field_label(item['inverse'][k]):v for k,v in form.items()}


def scalar_environment(item):
    return sorted(r['rule_text'] for r in item['ast']['rules'] if any(a['kind']=='set' and a['head']=='Expr-size' for a in r['head']))


def compose_family(items):
    """Use real recorded parents and typed-token port maps, never literal equality."""
    composed={};certs=[]
    for cid,item in items.items():
        parent_ids=item['cell']['origin']['composition']['parts']
        assert all(p in items and p in composed for p in parent_ids),'parent closure is missing or unordered'
        cuts={};links=[]
        for pid in parent_ids:
            parent=items[pid];token=parent['root_token']
            assert token in item['typed'],'parent root has no typed child coordinate'
            local=item['typed'][token]
            # The same original history supplies parent members and child local references.
            assert set(parent['cell']['origin']['members'])<=set(item['cell']['origin']['members'])
            assert scalar_environment(parent)==scalar_environment(item),'scalar rule environment differs'
            if local in cuts and composed[cuts[local]]!=composed[pid]:
                raise ValueError('aliased parent roots disagree on their scalar transfer')
            cuts[local]=pid
            links.append(dict(parent=pid,typed_token=field_label(token),parent_slot=parent['root'],child_slot=local))
        local=Affine(item['equations'],item['seeds'],cuts).field(item['root'])
        columns=sorted(k for k in local if k is not None)
        expressions=[]
        for slot in columns:
            if slot in cuts:expression=composed[cuts[slot]]
            else:expression={field_label(item['inverse'][slot]):1}
            expressions.append(expression)
        # Keep every parent coordinate, including ones that cancel in the child.
        all_inputs=sorted(({k for e in expressions for k in e}|
                           set(global_form(item,item['full'])))-{'1'})
        basis=all_inputs+['1']
        mapping=[[expression.get(k,0) for k in basis] for expression in expressions]
        mapping.append([int(k=='1') for k in basis])
        arithmetic=[[local[k] for k in columns]+[local.get(None,0)]]
        result=multiply(arithmetic,mapping)[0]
        calculated={name:value for name,value in zip(basis,result) if value}
        assert calculated==global_form(item,item['full']),'composed and direct symbolic derivations differ'
        composed[cid]=calculated
        certs.append(dict(cell=cid,parent_links=links,input_columns=[field_label(item['inverse'][k]) for k in columns]+['1'],
                          source_arithmetic_matrix=arithmetic,parent_binding_matrix=mapping,
                          final_basis=basis,composed_row=result,formula=calculated))
    return composed,certs


def verify_composition_certificate(items,claimed):
    # Re-derive from immutable source/snapshot inputs; never trust fitted rows.
    _,expected=compose_family(items)
    if claimed!=expected:raise ValueError('Cell port or arithmetic certificate differs from source derivation')


def check_tampering(items,certs):
    verify_composition_certificate(items,certs)
    bad=deepcopy(certs)
    # Use an EXISTING but wrong typed value, rather than merely an absent ID.
    last=items[bad[-1]['cell']]
    wrong=next(t for t in last['typed'] if t[0]=='Expr' and t!=items[bad[-1]['parent_links'][0]['parent']]['root_token'])
    bad[-1]['parent_links'][0]['typed_token']=field_label(wrong)
    try:verify_composition_certificate(items,bad)
    except ValueError:pass
    else:raise AssertionError('wrong Cell port accepted')
    bad=deepcopy(certs);bad[-1]['composed_row']=[min(1,v) for v in bad[-1]['composed_row']]
    try:verify_composition_certificate(items,bad)
    except ValueError:pass
    else:raise AssertionError('Booleanized operator accepted')
    root=certs[-1]['composed_row'];binary=[min(1,v) for v in root]
    assert sum(root)!=sum(binary)
    return dict(wrong_existing_typed_port_rejected=True,non01_counterexample_input=1,
                exact_value=sum(root),booleanized_value=sum(binary),
                dropped_parent_transition_value=1+1,
                note='Dropping the parent scalar transition feeds raw boundary1 into final Uop, producing2 instead of6.')


def native_negative_checks(manifest,oracle,folder):
    """The native oracle must independently reject a wrong computed effect."""
    cases=[]
    bad=deepcopy(manifest)
    bad['inputs']=[bad['domain_max']+1]
    # Failure must precede any attempt to read or execute this invalid source.
    bad['cells'][0]['source']=str(folder/'must-not-be-read.egg')
    cases.append(('out-of-domain',bad,'outside [0,'))
    bad=deepcopy(manifest);bad['cells']=bad['cells'][:1];bad['inputs']=[1]
    bad['cells'][0]['outputs'][0]['coefficients'][1]+=1
    cases.append(('wrong-affine-effect',bad,'native versus compiled: tables differ'))
    verified=[]
    for name,bad,expected in cases:
        path=folder/f'negative-{name}.json';path.write_text(json.dumps(bad)+'\n')
        result=subprocess.run([str(oracle),str(path),str(folder/f'negative-{name}-result.json')],
                              text=True,capture_output=True)
        (folder/f'negative-{name}.log').write_text(result.stdout+result.stderr)
        assert result.returncode!=0 and expected in result.stderr,result.stderr
        verified.append(name)
    return verified


def main():
    p=argparse.ArgumentParser()
    p.add_argument('--cells',type=Path,default=DEFAULT_CELLS)
    p.add_argument('--output',type=Path,default=ROOT/'out/real-cell-operators')
    p.add_argument('--parser',type=Path,default=ROOT/'target/release/examples/lhs_ast')
    p.add_argument('--oracle',type=Path,default=ROOT/'target/release/examples/cell_operator_verify')
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    items={cid:parse_cell(load(cid,args.cells),args.output/str(cid),args.parser) for cid in FAMILY}
    composed,certs=compose_family(items)
    main_cells=[cid for cid,item in items.items() if any(k is not None for k in item['full'])]
    boundaries={t for cid in main_cells for t in composed[cid] if t!='1'}
    assert len(boundaries)==1,'prototype verifier needs one scalar boundary'
    boundary=next(iter(boundaries));kind,number=boundary.split(':');token=(kind,int(number))
    domain_max=(1<<63)-1
    for cid in main_cells:
        item=items[cid]
        for form in item['intermediates']:
            # Even 0*size(x) requires size(x) to exist in the original rule.
            # Algebraic cancellation must not silently remove a required input.
            if any(field_label(item['inverse'][k])!=boundary for k in form if k is not None):
                raise ValueError('an eliminated scalar input still needs a presence gate')
            assert 0<=form.get(None,0)<=(1<<63)-1,'undefined constant arithmetic intermediate'
            slope=sum(v for k,v in form.items() if k is not None)
            if slope:domain_max=min(domain_max,((1<<63)-1-form.get(None,0))//slope)
    inputs=[0,1,2,4,9,27,1000,domain_max]
    native_cells=[];compiled_bases={}
    for cid in main_cells:
        item=items[cid];input_id=item['typed'][token]
        outputs=[]
        for target in sorted(item['equations']):
            if target in item['seeds']:continue
            form=item['affine'].field(target)
            assert set(form)<={None,input_id}
            assert target in item['terms']
            outputs.append(dict(term=item['terms'][target],coefficients=[form.get(input_id,0),form.get(None,0)]))
            if target==item['root']:
                assert outputs[-1]['coefficients']==[composed[cid].get(boundary,0),composed[cid].get('1',0)]
        ruleset=f'CellOperatorCompiled{cid}'
        lets,expressions,compiled_bases[cid]=shared_affine_program([o['coefficients'] for o in outputs])
        actions=lets+[f'(set (Expr-size {o["term"]}) {expression})' for o,expression in zip(outputs,expressions)]
        shortcut=args.output/str(cid)/'operator.egg'
        shortcut.write_text('\n'.join([
            f'; Generated from actual Cell {cid}; requires its frozen structural contract.',
            '; Not a general global-graph rule. Original size rules are not executed by this ruleset.',
            f'(ruleset {ruleset})',
            f'(rule ((= cell_size (Expr-size {item["terms"][input_id]})) (>= cell_size 0) (<= cell_size {domain_max}))',
            '      ('+'\n       '.join(actions)+')',
            f'      :ruleset {ruleset})'])+'\n')
        native_cells.append(dict(id=cid,source=item['cell']['paths']['ripened'],rulesets=item['cell']['rulesets'],
                                 input_term=item['terms'][input_id],outputs=outputs,isolation_source=item['isolation'],
                                 compiled_source=str(shortcut.resolve()),compiled_ruleset=ruleset))
    manifest=dict(cells=native_cells,inputs=inputs,domain_max=domain_max)
    mp=args.output/'manifest.json';mp.write_text(json.dumps(manifest,indent=2)+'\n')
    call([args.oracle,mp,args.output/'native.json'],args.output/'native.log')
    native=json.loads((args.output/'native.json').read_text())
    native_rejections=native_negative_checks(manifest,args.oracle,args.output)
    basis=interface_basis([c['composed_row'] for c in certs if c['cell'] in main_cells])
    basis['cell_order']=main_cells
    basis['retained_runtime_outputs']=[main_cells[i] for i in basis['owners'] if isinstance(i,int)]
    report=dict(scope='Real completed Cell observations; exact source-derived size slice, not full Cell admission',
                cells=[compact_evidence(item['cell']) for item in items.values()],
                operators=certs,interface_basis=basis,compiled_bases=compiled_bases,
                domain=dict(input=boundary,min=0,max=domain_max),
                certificates={cid:item['certificates'] for cid,item in items.items()},
                effect_isolation_rules={cid:item['blocked'] for cid,item in items.items()},
                counterexamples=check_tampering(items,certs),native=native,
                native_rejections=native_rejections,
                provenance=dict(
                    base_commit=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip(),
                    parser_sha256=hashlib.sha256(args.parser.read_bytes()).hexdigest(),
                    oracle_sha256=hashlib.sha256(args.oracle.read_bytes()).hexdigest(),
                    local_kernel_diff_sha256=hashlib.sha256(subprocess.check_output(['git','diff','HEAD','--','egglog'],cwd=ROOT)).hexdigest(),
                    note='Local modified runtime, not pristine egglog. This experiment changes no engine files.'),
                limitations=['Saved exports remain observation_only and are not union certificates.',
                             'All-table differential tests are sample checks; symbolic arithmetic certificate is for the declared fixed-shape slice.',
                             'No timing benchmark, full workload acceleration, or general union/subsume operator is claimed.'])
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'operators':certs,'domain_max':domain_max,'native':native},indent=2))


if __name__=='__main__':main()
