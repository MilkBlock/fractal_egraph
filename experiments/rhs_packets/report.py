"""Compare the standard Cargo suite against its retained historical controls."""
import argparse,json
from pathlib import Path

def read(p):return json.loads(Path(p).read_text())

def report(current,baseline):
    now=read(current);old=read(baseline);assert not now['failures'],now['failures'];assert now['configuration']==old['configuration']=='resident-core'
    refs={c['name']:c for c in old['cases']};rows=[]
    for c in now['cases']:
        ref=refs[c['name']];assert c['source_sha256']==ref['source_sha256'];assert c['marker_tables']==ref['source_marker_tables']==0
        folder=Path(c['clean']['attempt']);a=read(ref['baseline']);b=read(folder/'result.json');p=read(folder/'program.json')
        assert p['resident_writes']['schema']=='eggfuse-resident-writes/v2'
        assert len(a['instances'])==len(b['instances'])
        for x,y in zip(a['instances'],b['instances']):
            for field in ('completed','rows','parents','fact_parents','sets','watermarks','meta','history'):
                assert x[field]==y[field],(c['name'],field)
        for field in ('private_schedule_regions','private_schedule_runs','private_schedule_rows','private_schedule_forwarded_reads','private_schedule_binding_outputs'):
            assert a[field]==b[field],(c['name'],field)
        before=ref['clean_inference_seconds'];after=b['inference_seconds'];work=c['clean']['execution']
        rows.append(dict(name=c['name'],historical_seconds=before,seconds=after,reduction_percent=100*(before-after)/before,
            full_state_and_run_history_equal=True,source_sha256=c['source_sha256'],program_sha256=b['program_sha256'],worker_sha256=b['worker_sha256'],
            final_rows=sum(len(s['rows']) for s in b['instances']),runs=sum(len(s['history']) for s in b['instances']),
            packet_dispatch_quanta=b.get('packet_dispatch_quanta'),packet_readiness_polls=b.get('packet_readiness_polls'),
            packet_work={k:v for k,v in work.items() if k.startswith('packet_')},gpu_seconds=b['gpu_seconds'],
            baseline_artifact=ref['baseline'],candidate_artifact=str(folder/'result.json'),dashboard=c['dashboard']))
    return dict(schema='eggfuse-rhs-packets-pass3/v1',scope='three real complete Pass 3 programs; cached historical control, single new measurement per case; not Raytrace',
                suite_generated_unix=now['generated_unix'],control_kind='historical, not same-binary paired timing',cases=rows)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('--current',type=Path,default=Path('out/profile-suite/latest.json'));p.add_argument('--baseline',type=Path,default=Path('experiments/profile_suite/results.json'));p.add_argument('--output',type=Path,default=Path('experiments/rhs_packets/pass3.json'));a=p.parse_args()
    r=report(a.current,a.baseline);a.output.write_text(json.dumps(r,indent=2)+'\n')
    for c in r['cases']:print(c['name'],c['historical_seconds'],'->',c['seconds'],str(round(c['reduction_percent'],2))+'%',c['packet_work'])
