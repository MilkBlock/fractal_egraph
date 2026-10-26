"""Run-boundary regressions for batched device execution (not benchmarks)."""
import argparse
import json
import subprocess
import sys
from pathlib import Path
from run import run
from compare_runs import compare

HERE = Path(__file__).resolve().parent

def tests(build, out):
    out.mkdir(parents=True, exist_ok=True)
    results = {}
    for name in ['typed-merge-index', 'merge-index-refresh', 'merge-rebuild', 'sets', 'custom-merge',
                 'deferred-union', 'subsume-union', 'snapshot-subsume', 'set-prediction',
                 'ordered-effects', 'repeated-custom-set', 'seed-pivot-guards']:
        source = HERE / 'fixtures' / (name + '.egg')
        before, after = out / (name + '-serial'), out / (name + '-batch')
        a = run(source, build, before)
        b = run(source, build, after, rebuild_batch=8, parallel_rhs=True,merge_target_index=True)
        assert a['steps'] == b['steps'], name
        results[name] = compare(before, after, device_states=True)
        results[name]['rebuild'] = b['rebuild_dispatches']

    # Crossed unions have shared identities despite independent bindings.
    # Exact atomics must converge to the same partition, while ordinary Run
    # consumers cannot see those unions until the next published snapshot.
    source = out / 'parallel-unions.egg'
    facts = ' '.join(f'(Link (N {i}) (N {i//2})) (Box (N {i}))' for i in range(1, 512))
    source.write_text('''(datatype T (N i64) (Box T))
(relation Link (T T)) (relation Seen (T))
(ruleset connect) (ruleset observe)
(rule ((Link x y)) ((union x y)) :ruleset connect)
(rule ((= x (N 511)) (= x (N 0))) ((Seen x)) :ruleset observe)
''' + facts + '\n(run observe 1) (run connect 1) (run observe 1) (check (Seen (N 0)))\n')
    a = run(source, build, out / 'unions-serial')
    b = run(source, build, out / 'unions-parallel', rebuild_batch=8, parallel_rhs=True,merge_target_index=True)
    assert b['parallel_rhs_rules']
    assert a['steps'] == b['steps']
    assert b['steps'][0]['matched'] == 0
    results['parallel-unions'] = compare(out / 'unions-serial', out / 'unions-parallel', True)

    source=HERE/'fixtures/context-joins.egg'
    original=run(source,build,out/'contexts-original')
    split=run(source,build,out/'contexts-split',split_matches=True,rebuild_batch=8,parallel_rhs=True,context_training=True)
    assert split['contexts']['emitted']>0 and split['query_context_plans']>0
    assert original['steps']==split['steps']
    results['context-joins']=compare(out/'contexts-original',out/'contexts-split',True)
    program=json.loads((out/'contexts-split/program.json').read_text())
    info=program['context_ridge']
    for value,name in [(0.0,'negative'),(1.0,'positive')]:
        weights=[[value]+[0.0]*(info['feature_width']-1) for _ in range(info['heads'])]
        model=out/(name+'.json')
        model.write_text(json.dumps({'schema':'egglog-context-ridge/v1',
                         'layout_sha256':info['layout_sha256'],'coefficients':weights}))
        predicted=run(source,build,out/('contexts-'+name),split_matches=True,context_model=model)
        assert predicted['context_ridge']['mode']=='ridge'
        assert predicted['steps']==original['steps']
        results['all-'+name]=compare(out/'contexts-original',out/('contexts-'+name),True)
    bad=out/'bad-model.json';bad.write_text(json.dumps({'schema':'egglog-context-ridge/v1','layout_sha256':'stale','coefficients':[]}))
    try:run(source,build,out/'stale-model',split_matches=True,context_model=bad)
    except ValueError as error:assert 'layout mismatch' in str(error)
    else:raise AssertionError('accepted a stale context model')
    results['reject-stale-model']=True

    source=out/'context-overflow.egg'
    source.write_text('(relation P (i64 i64)) (relation Q (i64 i64)) (relation R (i64 i64)) '
        '(relation Out (i64 i64)) (ruleset q) '
        '(rule ((P a b) (Q b c) (R c d) (< d 0)) ((Out a d)) :ruleset q) '+
        ' '.join(f'(P {i} 0) (Q 0 {i}) (R {i} 0)' for i in range(16))+' (run q 1)')
    a=run(source,build,out/'overflow-original',match_capacity=8)
    b=run(source,build,out/'overflow-contexts',match_capacity=8,split_matches=True)
    assert b['contexts']['fallback_runs']>0 and a['steps']==b['steps']
    results['overflow-completion']=compare(out/'overflow-original',out/'overflow-contexts',True)
    (out / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
    return results

if __name__ == '__main__':
    p = argparse.ArgumentParser()
    p.add_argument('--build', type=Path, required=True)
    p.add_argument('--output', type=Path, required=True)
    a = p.parse_args()
    print('passed', len(tests(a.build, a.output)), 'context execution cases')
