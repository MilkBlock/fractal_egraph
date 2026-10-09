#!/usr/bin/env python3
"""Compile the actual CICM LHS and compare native binding sets at every event."""
import argparse
from copy import deepcopy
import hashlib
import json
from pathlib import Path
import subprocess
import run
from lhs_compiler import compile_rule, body_text

ROOT=run.ROOT


def call(args,log):
    p=subprocess.run(list(map(str,args)),capture_output=True,text=True)
    log.write_text(p.stdout+p.stderr)
    if p.returncode:raise RuntimeError(f'{args[0]} failed: {p.stderr[-1200:]}')


def event_text(event,rows):
    if event is None:return ''
    kind,*args=event
    if kind=='size':return f'(set (Expr-size {run.CICM.symbol(args[0])}) {args[1]})'
    if kind=='pure':return f'(ExprIsPure {run.CICM.symbol(args[0])})'
    if kind=='pending':return f'(DelayedSubstUnion {run.CICM.value(args[0])} {run.CICM.value(args[1])})'
    if kind=='commit':return '(run commit 1)'
    if kind=='union':return f'(union {run.CICM.value(args[0])} {run.CICM.value(args[1])})'
    if kind=='subsume':
        row=next(r for r in rows if r['name']==args[0])
        return f'(subsume (Uop {run.CICM.value(row["op"])} {run.CICM.value(row["operand"])}))'
    raise ValueError(event)


def run_variant(rule,signatures,setup,events,folder,oracle):
    folder.mkdir(parents=True,exist_ok=True)
    compilation=compile_rule(rule,signatures)
    variables=compilation.metadata['named_variables']
    sorts=[compilation.metadata['variable_sorts'][name] for name in variables]
    manifest={'setup':str(setup.resolve()),'stages':[]}
    for stage,(label,actions) in enumerate(events):
        prefix=f'touch{stage}'
        view=compilation.emit(prefix)
        original=f'Original{stage}';rewritten=f'Rewritten{stage}'
        declarations='\n'.join(f'(relation {name} ({" ".join(sorts)}))' for name in [original,rewritten])
        head=lambda name:f'(({name} {" ".join(variables)}))'
        text='\n'.join([actions,view['declarations'],view['producer_rules'],
                        f'(run {view["ruleset"]} 1)',declarations,
                        f'(ruleset original{stage})',f'(ruleset rewritten{stage})',
                        f'(rule {body_text(rule["body"])} {head(original)} :ruleset original{stage})',
                        f'(rule {view["rewritten_body"]} {head(rewritten)} :ruleset rewritten{stage})',
                        f'(run original{stage} 1)',f'(run rewritten{stage} 1)'])+'\n'
        path=folder/f'stage-{stage}.egg';path.write_text(text)
        manifest['stages'].append(dict(label=label,source=str(path.resolve()),original=original,rewritten=rewritten,
                                      touches=[f'{prefix}_cell_{i}' for i in range(len(compilation.templates))]))
    mp=folder/'manifest.json';mp.write_text(json.dumps(manifest,indent=2)+'\n')
    call([oracle,mp,folder/'native.json'],folder/'native.log')
    native=json.loads((folder/'native.json').read_text())
    return dict(compiler=compilation.metadata,templates=compilation.templates,instances=compilation.instances,
                stages=native['stages'])


def main():
    p=argparse.ArgumentParser()
    p.add_argument('--output',type=Path,default=ROOT/'out/lhs-touchpoints')
    p.add_argument('--parser',type=Path,default=ROOT/'target/release/examples/lhs_ast')
    p.add_argument('--oracle',type=Path,default=ROOT/'target/release/examples/lhs_equivalence')
    args=p.parse_args();out=args.output;out.mkdir(parents=True,exist_ok=True)
    rows,frames,updates=run.CICM.fixture()
    setup=out/'setup.egg'
    setup.write_text(run.CICM.native_program(run.CICM.lhs_source(),rows,frames,[],[],False))
    parser_input=out/'parser-input.egg'
    parser_input.write_text(setup.read_text()+'\n'+run.CICM.SOURCE.read_text())
    call([args.parser,parser_input,out/'ast.json'],out/'parser.log')
    parsed=json.loads((out/'ast.json').read_text())
    rule=next(r for r in parsed['rules'] if r['ruleset']=='cicm')
    events=[('initial','')]+[(str(event),event_text(event,rows)) for event in updates]
    events += [
      ('same Expr in both branches','(ContextOf $e_L2 (InIf false (Leaf "p0") (Leaf "i0")))'),
      ('another Uop witness in the same eclass','(union $e_L2 (Uop (Op "abs") (Leaf "extra_operand")))'),
      ('operand union','(union (Leaf "extra_operand") (Leaf "arg_e:L2"))'),
      ('hide only the alternative Uop','(subsume (Uop (Op "abs") (Leaf "extra_operand")))'),
      ('ExtractedExpr merge produces unresolved Smaller','(set (ExtractedExpr $e_R1) (TCPair (T "A") 2))'),
      ('resolve Smaller to selected TCPair','(union (Smaller (TCPair (T "A") 50) (TCPair (T "A") 2)) (TCPair (T "A") 2))'),
      ('two TCPair cost witnesses in one output eclass','(union (TCPair (T "A") 2) (TCPair (T "A") 3))'),
    ]
    variants={'source':rule}
    guarded=deepcopy(rule)
    guarded['body'].append({'kind':'fact','expr':{'kind':'call','head':'<=','args':[
        {'kind':'var','name':'c1'},{'kind':'var','name':'c2'}]}})
    variants['cross_side_guard']=guarded
    results={name:run_variant(r,parsed['signatures'],setup,events,out/name,args.oracle) for name,r in variants.items()}
    # Observable checks catch wrong stage order, lost aliases or hidden internal witnesses.
    counts=[s['binding_count'] for s in results['source']['stages']]
    assert counts[6]<counts[7], 'delayed union must enable new matches only at commit'
    assert counts[10]<counts[9], 'subsumed old Uop must stop matching'
    assert counts[13]>counts[12], 'same eclass on both sides is legal'
    assert counts[14]>counts[13], 'multiple Uop witnesses must be retained'
    assert counts[16]<counts[15], 'hiding one witness must remove only its bindings'
    assert counts[17]<counts[16] and counts[18]==counts[16], 'Smaller phases must remain observable'
    assert counts[19]>counts[18], 'different costs are separate named bindings'
    assert results['source']['templates']==results['cross_side_guard']['templates'], 'cross-side guard must not change local shared template'
    # Human-readable single-snapshot library and rewritten query.
    c=compile_rule(rule,parsed['signatures']);emitted=c.emit('AutoTouch')
    variables=c.metadata['named_variables'];sorts=[c.metadata['variable_sorts'][v] for v in variables]
    (out/'generated.egg').write_text('\n'.join([
        '; Requires the original declarations and current facts.',
        '; Rebuild this view at an observation boundary; do not reuse stale rows.',
        emitted['declarations'],emitted['producer_rules'],f'(run {emitted["ruleset"]} 1)',
        f'(relation AutoBinding ({" ".join(sorts)}))','(ruleset AutoConsume)',
        f'(rule {emitted["rewritten_body"]} ((AutoBinding {" ".join(variables)})) :ruleset AutoConsume)',
        '(run AutoConsume 1)'])+'\n')
    # Preserve the original action program for inspection; do not execute it here.
    transformed='\n'.join([
        '; Install with the original declarations. Original rule is replaced, not retained.',
        '; Refresh the touch view before one consumer snapshot. Do not reuse after mutation.',
        '; Original RHS is preserved; its execution is outside this binding-equivalence test.',
        emitted['declarations'],emitted['producer_rules'],'(ruleset AutoApply)',
        f'(rule {emitted["rewritten_body"]} {rule["head_text"]} :ruleset AutoApply)',
    ])+'\n'
    (out/'rewritten-rule.egg').write_text(transformed)
    call([args.parser,out/'rewritten-rule.egg',out/'rewritten-ast.json'],out/'rewritten-parser.log')
    transformed_ast=json.loads((out/'rewritten-ast.json').read_text())
    rewritten_rule=next(r for r in transformed_ast['rules'] if r['ruleset']=='AutoApply')
    assert rewritten_rule['head_text']==rule['head_text'], 'original action sequence changed'
    report=dict(scope='Automatic source LHS decomposition; actual native same-snapshot binding equivalence, no benchmark',
                source=str(run.CICM.SOURCE),source_sha256=hashlib.sha256(run.CICM.SOURCE.read_bytes()).hexdigest(),
                parser_sha256=hashlib.sha256(args.parser.read_bytes()).hexdigest(),
                oracle_sha256=hashlib.sha256(args.oracle.read_bytes()).hexdigest(),
                actual_source_rule=rule['body_text'],variants=results,original_rhs_preserved=True,
                rewritten_source_conjuncts=len(rewritten_rule['body']),shared_template_across_variants=True,
                producer_cost='View construction still executes its original six-condition Join; no total Join elimination claimed',
                visibility='Fresh relation names at each event boundary; not a production incremental view-maintenance implementation')
    (out/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({name:[s['binding_count'] for s in r['stages']] for name,r in results.items()},indent=2))


if __name__=='__main__':main()
