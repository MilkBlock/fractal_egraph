"""Compare the scheduler and the whole optional Spine package on one compiled program."""
import argparse
import json
import os
import subprocess
from pathlib import Path
from experiments.cell_runtime.suite import ROOT, fingerprint, timed
from experiments.cell_runtime.refutation_bench import audit

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('reference', type=Path)
    parser.add_argument('output', type=Path)
    group=parser.add_mutually_exclusive_group()
    group.add_argument('--storage',action='store_true',help='Cross reference storage and node scheduling with Spine off')
    group.add_argument('--join-policy', action='store_true', help='Cross Sieve and refinement on the node-only path')
    parser.add_argument('--plain-storage',action='store_true',help='Use ordinary storage in the join-policy matrix')
    args = parser.parse_args()
    if args.plain_storage and not args.join_policy:
        parser.error('--plain-storage requires --join-policy')
    saved = json.loads(args.reference.read_text())
    path = Path(saved['program'])
    engine = ROOT/'engine/cell-runtime/target/release/eggfuse-cell-runtime'
    expected = saved['lanes']['on']['measurement']
    assert fingerprint(engine) == expected['binary_sha256']
    assert fingerprint(path) == expected['program_sha256']
    assert not any(k.startswith('CELL_CPU_') for k in os.environ)
    os.environ.update(expected['configuration'])
    if args.plain_storage:
        os.environ['CELL_CPU_REFERENCE_VIEWS']='0'
    lanes = {'core': (0,0), 'nodes': (0,1), 'spine': (1,0), 'both': (1,1)}
    switches = ('CELL_CPU_STAGE_SPINE', 'CELL_CPU_SHARED_NODE_SCHEDULE')
    if args.join_policy:
        os.environ.update(CELL_CPU_STAGE_SPINE='0', CELL_CPU_SHARED_NODE_SCHEDULE='1')
        switches = ('CELL_CPU_SIEVE', 'CELL_CPU_JOIN_INTERSECTION')
    if args.storage:
        os.environ.update(CELL_CPU_STAGE_SPINE='0')
        switches = ('CELL_CPU_REFERENCE_VIEWS','CELL_CPU_SHARED_NODE_SCHEDULE')
    orders = [('core','nodes','both','spine'), ('nodes','spine','core','both'),
              ('spine','both','nodes','core'), ('both','core','spine','nodes')]
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    data = []
    reference_state = json.loads((args.reference.parent/'on/state.json').read_text())
    def ordered(state):
        if args.storage or args.plain_storage:
            state['rows'].sort(key=lambda row: json.dumps(row,sort_keys=True))
        return state
    reference_state = ordered(reference_state)
    for block, order in enumerate(orders, 1):
        for name in order:
            for switch, value in zip(switches, lanes[name]):
                os.environ[switch] = str(value)
            directory = out/f'{block}-{name}'
            m, r = timed(path, engine, directory)
            assert ordered(json.loads((directory/'state.json').read_text())) == reference_state
            checks = (r['stats']['direct_candidates'] + r['stats']['residual_candidates']
                      + r['multi_stage']['stats']['merge_candidate_rows']
                      + r['stage_spine']['stats']['pivot_candidates']
                      + r['entry_schedule']['stats']['predicate_checks'])
            label = {'core':'nodes', 'nodes':'refine', 'spine':'sieve', 'both':'sieve_refine'}[name] if args.join_policy else name
            if args.storage:
                label={'core':'plain', 'nodes':'plain_nodes','spine':'views','both':'views_nodes'}[name]
            data.append(dict(block=block,lane=label,ms=m['inference_seconds']*1000,
                             rss_bytes=m['peak_rss_bytes'],checks=checks,
                             runs=len(r['steps']),bindings=r['stats']['bindings']))
    (out/'samples.json').write_text(json.dumps(data,indent=2))
    streamer = engine.parent/'examples/project_stages'
    if args.join_policy:
        # Use the existing exact stage comparator for a coupled two-switch lane.
        wrapper=out/'join-streamer'
        wrapper.write_text('#!/bin/sh\nexport CELL_CPU_SIEVE="$REVIEW_JOIN" CELL_CPU_JOIN_INTERSECTION="$REVIEW_JOIN"\nexec "$REVIEW_STREAMER" "$@"\n')
        wrapper.chmod(0o755)
        os.environ['REVIEW_STREAMER']=str(streamer)
        checked=audit(path,json.loads(path.read_text()),out/'audit',wrapper,'REVIEW_JOIN')
        checked['underlying_streamer_sha256']=fingerprint(streamer)
        (out/'audits.json').write_text(json.dumps(checked,indent=2))
        if not args.plain_storage:
            from experiments.cell_runtime.local_sat_bench import profile_existing
            receipt_dir=out/'depth-input'
            receipt_dir.mkdir()
            receipt={k:saved[k] for k in ('program','binary_sha256','program_sha256')}
            receipt['lanes']={}
            for label,first in [('off','1-core'),('on','1-both')]:
                lane=out/first
                (receipt_dir/label).symlink_to(lane,target_is_directory=True)
                source_label='nodes' if label=='off' else 'sieve_refine'
                checks=next(row['checks'] for row in data if row['lane']==source_label)
                receipt['lanes'][label]=dict(measurement=json.loads((lane/'measurement.json').read_text()),all_candidate_visits=checks)
            receipt_path=receipt_dir/'results.json'
            receipt_path.write_text(json.dumps(receipt))
            (out/'depth').mkdir()
            profile_existing(receipt_path,out/'depth')
        return
    if args.storage:
        os.environ['CELL_CPU_SHARED_NODE_SCHEDULE']='1'
        os.environ['CELL_CPU_REFERENCE_VIEWS']='1'
        checked=json.loads(subprocess.check_output([engine,'--program',path,'--verify-reference-views'],text=True))
        (out/'audits.json').write_text(json.dumps(checked,indent=2))
        return
    program = json.loads(path.read_text())
    os.environ['CELL_CPU_STAGE_SPINE']='0'
    comparisons={'core_vs_nodes':audit(path,program,out/'core-vs-nodes',streamer,'CELL_CPU_SHARED_NODE_SCHEDULE')}
    os.environ['CELL_CPU_SHARED_NODE_SCHEDULE']='1'
    comparisons['nodes_vs_both']=audit(path,program,out/'nodes-vs-both',streamer,'CELL_CPU_STAGE_SPINE')
    (out/'audits.json').write_text(json.dumps(comparisons,indent=2))

if __name__ == '__main__':
    main()
