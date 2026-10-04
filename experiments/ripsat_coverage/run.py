#!/usr/bin/env python3
"""Freeze on training source, then audit held-out source without extending it."""
import argparse
import hashlib
import json
import pathlib
import subprocess
p=argparse.ArgumentParser()
p.add_argument('--train-source',type=pathlib.Path,required=True)
p.add_argument('--test-source',type=pathlib.Path,required=True)
p.add_argument('--output',type=pathlib.Path,required=True)
p.add_argument('--patterns',type=pathlib.Path,default=pathlib.Path('experiments/ripsat_coverage/patterns.json'))
p.add_argument('--binary',type=pathlib.Path,default=pathlib.Path('target/release/examples/ripsat_coverage'))
a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
def run(mode,source,library,dest):
    with (out/(mode+'.log')).open('w') as log:
        subprocess.run([str(a.binary.resolve()),mode,str(source.resolve()),str(library.resolve()),str(dest)],stdout=log,stderr=log,check=True)
    report=json.loads((dest/'report.json').read_text())
    if not report['same_final_state_as_native_control']:
        raise SystemExit(f'{mode}: native trace control differs; report is not accepted')
    return report
train=run('train',a.train_source,a.patterns,out/'train')
library=out/'train/library.json';digest=hashlib.sha256(library.read_bytes()).hexdigest()
test=run('eval',a.test_source,library,out/'test')
assert digest==hashlib.sha256(library.read_bytes()).hexdigest()
assert test['signature_count_before']==test['signature_count_after']==train['signature_count_after']
def rate(n,d):return None if not d else 100*n/d
def compact(r):
    s=r['structure'];m=r['matches'];e=r['effective_matches']
    hottest=sorted(r['rule_search_and_apply_seconds'],key=r['rule_search_and_apply_seconds'].get,reverse=True)[:10]
    return dict(structure=s,matches=m,effective_matches=e,
      expr_enode_coverage_percent=rate(s['covered_expr_enodes'],s['expr_enodes']),
      expr_eclass_coverage_percent=rate(s['covered_expr_eclasses'],s['expr_eclasses']),
      constructor_storage_coverage_percent=rate(s['covered_constructor_rows'],s['stored_constructor_rows']),
      complete_lhs_one_cell_percent=rate(m['all_reads_one_cell'],m['logical']),
      at_least_two_internal_atoms_percent=rate(m['multiple_internal_atoms'],m['logical']),
      successful_read_atom_coverage_percent=rate(m['covered_read_atoms'],m['read_atoms']),
      one_marker_atom_contraction_percent=rate(m['one_marker_atom_contraction'],m['read_atoms']),
      effective_signature_hit_percent=None if r['mode']=='train' else rate(e['known_effect_signature'],e['effective_insert_update_union']),
      anchored_effective_signature_hit_percent=None if r['mode']=='train' else rate(e['known_effect_signature'],e['any_cell']),
      native_steps=r['native_steps'],audit_seconds=r['audit_seconds'],native_traced_seconds=r['native_traced_seconds'],
      control_equal=r['same_final_state_as_native_control'],
      hottest_rules=[dict(rule=name,search_and_apply_seconds=r['rule_search_and_apply_seconds'][name],coverage=r['rules'].get(name)) for name in hottest])
summary=dict(scope='Frozen diagnostic closure library; original native rules and schedules. No candidate or embedding cutoff, no held-out library growth, no replacement runtime.',
 source_sha256=dict(train=hashlib.sha256(a.train_source.read_bytes()).hexdigest(),test=hashlib.sha256(a.test_source.read_bytes()).hexdigest()),
 library_sha256=digest,pattern_count=train['library_patterns'],unsupported_patterns=train['unsupported_patterns'],learned_effect_signatures=train['signature_count_after'],
 train=compact(train),held_out=compact(test),certified_state_transition_coverage=None,actual_join_savings=None,
 limitations=['Patterns are positive structural embeddings, not proofs of closed instances under arbitrary outside contexts.',
 'Signature hits retain relative cell slots, external/residual aliasing, source read positions, literals and committed effects, but have no certified target-state adapter.',
 'Only pre-step facts receive temporal credit; overlapping embeddings are deduplicated. Background-only facts receive no coverage credit.',
 'The contraction count replaces k successful LHS reads in one cell with one marker; it neither counts failed Join candidates nor bounds savings from suppressing whole internal rules.',
 'Native search-and-apply times include tracing and RHS work; they are not pure Join times and are not multiplied by coverage ratios.',
 'Effective mutation denominator covers insert/update/union receipts; subsume-only changes are outside this count.',
 'Native control uses identical dependency tracing. This does not claim tracing leaves order-sensitive untraced execution choices unchanged.'])
(out/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
print(json.dumps({k:summary['held_out'][k] for k in ['expr_enode_coverage_percent','expr_eclass_coverage_percent','complete_lhs_one_cell_percent','at_least_two_internal_atoms_percent','effective_signature_hit_percent']},indent=2))
