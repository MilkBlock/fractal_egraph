"""Do not count known-key interpolation as generalization to new bindings."""
import argparse
import json
from pathlib import Path
import numpy as np
import run
from memory import Memory,mix,interpolation_parts


def main():
    p=argparse.ArgumentParser();p.add_argument('--snapshot',required=True,type=Path)
    p.add_argument('--profile',required=True,type=Path);p.add_argument('--output',required=True,type=Path)
    a=p.parse_args();tables,header=run.response_run.load(a.snapshot,a.profile)
    bank=run.ResponseBank(header['queries'],tables).encode();groups,routes,_=run.prepare(bank)
    predictions={};scope={};held_count=0
    for t,g in groups.items():
        # Independent of the bucket-routing hash, so we withhold keys inside
        # buckets, not whole empty buckets.
        held=(mix(g['keys']^np.uint64(0x6a09e667f3bcc909))&np.uint64(7))==0
        held_count+=int(held.sum());keys=g['keys'][~held];payload=g['payload'][~held]
        m=Memory(keys,payload,256,interpolation_parts(keys,256),fit=True)
        predictions[t]=m.predict(g['probes'])
        scope[t]=np.isin(g['probes'],g['keys'][held])
    report={'scope':'withhold approximately 1/8 of previously known keys from Bake, then probe all original requests',
        'withheld_domain_keys':held_count,'total_domain_keys':sum(len(g['keys']) for g in groups.values()),
        'threshold':.5,'all_rule_root_pairs':run.assess(bank,groups,routes,predictions,.5),
        'rule_root_pairs_reading_a_withheld_key':run.assess(bank,groups,routes,predictions,.5,query_scope=scope),
        'constant_prior_on_withheld':run.assess(bank,groups,routes,predictions,.5,True,query_scope=scope),
        'warning':'Remaining missed positives are only observable here because an exact oracle is available; runtime needs real complement/fallback work.'}
    a.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k not in ('scope','warning')},indent=2))


if __name__=='__main__':main()
