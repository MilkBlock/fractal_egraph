#!/usr/bin/env python3
"""Correctness only: one program, late observation, two lowerings, native effects."""
import argparse
from dataclasses import asdict
import json
from pathlib import Path
import struct
import subprocess
import run
from lowering import FIELDS, cicm_program, metal_source, swift_source
from touchpoints import discover
from touch_backend import TouchRequest


def command(args,log):
    result=subprocess.run(list(map(str,args)),capture_output=True,text=True)
    log.write_text(result.stdout+result.stderr)
    if result.returncode:raise RuntimeError(f'{args[0]} failed: {result.stderr[-1500:]}')
    return result.stdout


def build(p,roots,folder):
    folder.mkdir(parents=True,exist_ok=True)
    (folder/'kernel.metal').write_text(metal_source(p,roots))
    template=(run.ROOT/'experiments/cell_kernel/backend.swift').read_text()
    swift=folder/'generated.swift';swift.write_text(swift_source(template,p,roots))
    binary=folder/'backend'
    command(['swiftc','-O','-module-cache-path','/tmp/cell-program-module-cache',swift,'-o',binary],folder/'build.log')
    return binary


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--output',type=Path,default=run.ROOT/'out/cell-shared-program')
    parser.add_argument('--gpu',action='store_true')
    args=parser.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    rules,lhs=run.compile_plan(args.output)
    p,roots=cicm_program()
    rows=run.records(32)
    expected=[]
    for row in rows:
        inst=p.instance(dict(zip(FIELDS,row)))
        out=inst.observe(roots)
        assert list(out.values())==run.reference(row)
        expected.append(run.reference(row))
    modes=['cpu']+(['gpu'] if args.gpu else [])
    binary=build(p,roots,args.output/'original')
    input_path=args.output/'input.bin';input_path.write_bytes(b''.join(struct.pack('<18q',*r) for r in rows))
    outputs={}
    for mode in modes:
        path=args.output/f'original-{mode}.bin'
        cmd=[binary,mode,input_path,path]
        if mode=='gpu':cmd+=['--metal-source',args.output/'original/kernel.metal']
        command(cmd,args.output/f'original-{mode}.log')
        actual=[list(r) for r in struct.iter_unpack('<6q',path.read_bytes())]
        assert actual==expected
        outputs[mode]=actual
    # Add a consumer of an existing internal port. Prior program nodes stay intact.
    before=tuple(p.nodes)
    internal=p.port('size','bopLeft')
    observer=p.lt(internal,p.const(8))
    p.name_cell('external',{'small_internal':observer})
    assert tuple(p.nodes[:len(before)])==before
    extended={**roots,'small_internal':observer}
    extended_expected=[]
    for row,base in zip(rows,expected):
        inst=p.instance(dict(zip(FIELDS,row)))
        value=inst.observe(extended)
        assert list(value.values())==base+[base[1]<8]
        extended_expected.append(base+[int(base[1]<8)])
    observed_binary=build(p,extended,args.output/'observed')
    for mode in modes:
        path=args.output/f'observed-{mode}.bin'
        cmd=[observed_binary,mode,input_path,path]
        if mode=='gpu':cmd+=['--metal-source',args.output/'observed/kernel.metal']
        command(cmd,args.output/f'observed-{mode}.log')
        assert [list(r) for r in struct.iter_unpack('<7q',path.read_bytes())]==extended_expected
    # Compile the observer alone: no obligation to export/recompute whole Cell.
    observer_roots={'small_internal':observer}
    reachable=set(p.reachable_topological(observer_roots.values()))
    assert roots['right'] not in reachable and roots['eligible'] not in reachable
    observer_binary=build(p,observer_roots,args.output/'observer-only')
    for mode in modes:
        path=args.output/f'observer-only-{mode}.bin'
        cmd=[observer_binary,mode,input_path,path]
        if mode=='gpu':cmd+=['--metal-source',args.output/'observer-only/kernel.metal']
        command(cmd,args.output/f'observer-only-{mode}.log')
        assert [r[0] for r in struct.iter_unpack('<q',path.read_bytes())]==[r[-1] for r in extended_expected]
    # Automatically choose shared interfaces for three consumer requirements.
    touch_plan=discover(p,{
        'size_export':{name:roots[name] for name in ['bopLeft','left','bopRight','right']},
        'selector':{'eligible':roots['eligible']},
        'internal_policy':{'small':observer},
    })
    assert touch_plan.fragments
    live=[p.instance(dict(zip(FIELDS,row))) for row in rows]
    requests=[TouchRequest.capture(touch_plan,inst,row[0]) for inst,row in zip(live,rows)]
    touch_roots=requests[0].outputs
    touch_binary=build(p,touch_roots,args.output/'automatic-touchpoints')
    (args.output/'automatic-touchpoints/plan.json').write_text(json.dumps({
        'consumers':{name:dict(ports) for name,ports in touch_plan.consumers.items()},
        'fragments':[asdict(f) for f in touch_plan.fragments],
        'compiled_outputs':touch_roots,
        'program':p.to_dict(list(touch_roots.values())),
    },indent=2)+'\n')
    for mode in modes:
        path=args.output/f'automatic-touchpoints-{mode}.bin'
        cmd=[touch_binary,mode,input_path,path]
        if mode=='gpu':cmd+=['--metal-source',args.output/'automatic-touchpoints/kernel.metal']
        command(cmd,args.output/f'automatic-touchpoints-{mode}.log')
        data=list(struct.iter_unpack(f'<{len(touch_roots)}q',path.read_bytes()))
        assert len(data)==len(rows)
        for row,inst,request,values in zip(rows,live,requests,data):
            values={node:bool(value) if p.nodes[node].kind=='bool' else value
                    for node,value in zip(touch_roots,values)}
            cache_before=dict(inst.cache)
            packets=request.finish(inst,values)
            assert inst.cache==cache_before, 'compiled touch reconstruction evaluated the old Cell'
            reference_packets=touch_plan.observe(p.instance(dict(zip(FIELDS,row))),row[0])['packets']
            assert packets==reference_packets
    # Patch exact canonical boundary values. Compare live instance with fresh full eval.
    patch_checks=0
    instance=p.instance(dict(zip(FIELDS,rows[0])))
    instance.observe(extended)
    env=dict(zip(FIELDS,rows[0]))
    for patch in [{'a':1,'a_alias':1},{'old_left':2},{'old_left':3},
                  {'term_right':999},{'term_right':env['term_left']},{'flags':0},{'flags':15}]:
        env.update(patch);instance.patch(patch)
        assert instance.observe(extended)==p.instance(env).observe(extended)
        patch_checks+=1
    # Compare actual native effects, not just CPU/GPU arrays.
    oracle=run.ROOT/'target/release/examples/cell_kernel_oracle'
    native=[]
    for mode in ['native',*modes]:
        folder=args.output/f'commit-{mode}';folder.mkdir(exist_ok=True)
        setup=folder/'setup.egg';setup.write_text(run.setup(rows,rules,lhs,mode=='native'))
        action=folder/'actions.egg'
        action.write_text('(run-schedule (saturate (run always-run)))\n(run select 1)\n(run result 1)\n' if mode=='native' else run.commit(rows,outputs[mode],folder))
        checks=folder/'checks.egg'
        checks.write_text('\n'.join(f'(check (FullBinding {run.binding(r)}))' if o[5] else f'(fail (check (Selected {r[0]})))' for r,o in zip(rows,expected))+'\n')
        command([oracle,setup,action,folder/'native.json',checks],folder/'native.log')
        report=json.loads((folder/'native.json').read_text())
        assert report['rows']['KernelResult']==[r[:5] for r in expected]
        assert report['rows']['Selected']==[[r[0]] for r in expected if r[5]]
        native.append(mode)
    report=dict(scope='Correctness and representation checks only; no performance benchmark',
                batch_rows=len(rows),native_effects_equal=native,compiled_backends=modes,
                original_operator_nodes=len(before),nodes_after_observer=len(p.nodes),
                old_nodes_unchanged=True,late_observer_outputs=7,observer_only_nodes=len(reachable),patch_checks=patch_checks,
                automatic_touch_fragments=len(touch_plan.fragments),
                automatic_touch_operator_sizes=[len(f.nodes) for f in touch_plan.fragments],
                automatic_touch_outputs=len(touch_roots),compiled_packets_equal=True,
                integration='Source-checked CICM pilot with automatic shared-touch discovery and packet reconstruction',
                limits='Consumer roots still supplied by source lowering; automatic shared cuts are over pure scalar Program, not arbitrary egraph pattern discovery or general union/subsume')
    (args.output/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report,indent=2))


if __name__=='__main__':main()
