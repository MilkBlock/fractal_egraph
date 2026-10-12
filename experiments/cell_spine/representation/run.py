#!/usr/bin/env python3
"""Joint physical-representation selection on frozen catalogs, native single runs.

Reuse the joint_pricing and spine_ablation artifacts for inputs and old libraries.
Do not recapture tier-0, dump benchmark graphs, or average/cherry-pick timings.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import time

ROOT=Path(__file__).resolve().parents[3]
sys.path[:0]=[str(ROOT),str(ROOT/'tools/bench')]
from math_virtual import measure
from tools.cell_spine.compiler import digest,ast
from tools.cell_spine.terms import actions
from tools.cell_spine.markers import RUNTIME_SCHEDULE

RUNNER=ROOT/'target/release/examples/native_math_baseline'


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--catalog-root',type=Path,required=True)
    p.add_argument('--ablation-root',type=Path,required=True)
    p.add_argument('--output',type=Path,required=True)
    a=p.parse_args();out=a.output.resolve();out.mkdir(parents=True,exist_ok=False)
    os.environ['RAYON_NUM_THREADS']='1';os.environ.pop('EGGLOG_JOIN_WORK_RULES',None)
    original=json.loads((a.ablation_root/'report.json').read_text())
    results=dict(schema='solver-physical-representation/v1',binary_sha256=digest(RUNNER),
        sources={p:digest(ROOT/p) for p in ['tools/cell_spine/compiler.py','tools/cell_spine/markers.py','tools/cell_spine/representation.py','tools/cell_spine/pricing.py','tools/cell_spine/planner.py']},
        scope='Actual current egglog runtime, not pristine. Single timing per mode. Restored node/class counts only on 1000-instance workloads; tiny Bake admission checks remain separate.',cases=[])
    for name in ('long_spine_graft','assoc','eggcc_if'):
        prior=json.loads((a.catalog_root/name/'report.json').read_text())
        folder=out/name;folder.mkdir();bake=folder/'bake'
        start=time.perf_counter()
        subprocess.run([sys.executable,'-m','tools.cell_spine.compiler','--source',prior['source'],
            '--analysis-dir',prior['analysis'],'--output',bake,'--skip-build'],cwd=ROOT,check=True)
        compile_seconds=time.perf_counter()-start
        selected=json.loads((bake/'report.json').read_text())
        base=a.ablation_root/name;old=base/'joint/library.egg'
        assert digest(old)==original['training'][name]['variants']['joint']['library_sha256']
        expected=next(c['native_shape'] for c in original['cases'] if c['fixture']==name and c['label']=='fresh-1000')
        record=dict(fixture=name,source=prior['source'],compile_from_catalog_seconds=compile_seconds,solver={k:selected['plan'][k] for k in
            ('objective','bound','master','inner_peak','priced_schemes','costs')},
            markers={f:dict(id=l['id'],representation=l.get('representation','canonical'),columns=l['columns'])
                for f,l in selected['retirement']['marker_layouts'].items()},variants={})
        for mode,path in [('previous',old),('selected',bake/'library.egg')]:
            dest=folder/mode;dest.mkdir()
            source=json.loads((base/'fresh-1000/joint_export.json').read_text())
            source.update(environment=str(path.resolve()),counts=True)
            if mode=='selected':
                data=json.loads((bake/'work/input.json').read_text())
                initial=ast(base/'fresh-1000/native-input.egg',dest/'input.json')
                ingress=dest/'input.egg'
                ingress.write_text('\n'.join(actions([a['action'] for a in initial['actions']],True,data['signatures'])))
                source.update(input=str(ingress.resolve()),run=RUNTIME_SCHEDULE,export='(run-schedule (saturate Export!))')
            (dest/'counts.json').write_text(json.dumps(source))
            counts=measure([RUNNER,'bench-probes',dest/'counts.json'],dest/'counts')['result']['verification']
            assert counts['check_mode']=='counts_only'
            assert all(counts['restored'][k]==expected[k] for k in ('enodes','eclasses'))
            source.update(counts=False);source.pop('export',None)
            (dest/'run.json').write_text(json.dumps(source))
            timed=measure([RUNNER,'bench',dest/'run.json'],dest/'time')
            record['variants'][mode]=dict(library_sha256=digest(path),run_ms=1000*timed['result']['run_seconds'],
                total_ms=1000*timed['result']['seconds'],peak_rss_mib=timed['peak_rss_bytes']/2**20,
                restored=counts['restored'],canonical=counts['canonical_spine'],raw=counts['raw_spine'],
                physical_rows=counts['native_total_rows_before_export'],candidate_rows=counts['run_probe_counters']['candidate_rows'])
        native=json.loads((base/'fresh-1000/native.json').read_text());native['counts']=False
        (folder/'native.json').write_text(json.dumps(native))
        timed=measure([RUNNER,'bench',folder/'native.json'],folder/'native-time')
        record['variants']['native']=dict(run_ms=1000*timed['result']['run_seconds'],total_ms=1000*timed['result']['seconds'],peak_rss_mib=timed['peak_rss_bytes']/2**20)
        results['cases'].append(record)
        (out/'report.json').write_text(json.dumps(results,indent=2)+'\n')
        print(name,{k:round(v['run_ms'],3) for k,v in record['variants'].items()},flush=True)


if __name__=='__main__':main()
