#!/usr/bin/env python3
"""Render a self-contained offline inspector from exported block snapshots."""
import argparse,json
from pathlib import Path

def render(source, output):
    data=json.loads(Path(source).read_text())
    if not isinstance(data.get('cases'),list) or not data['cases']:
        raise ValueError('expected nonempty cases list')
    for case in data['cases']:
        for phase in case['phases']:
            for block in phase['blocks']:
                ids={m['match'] for m in block['members']}
                for prefix in block['combined_prefixes']:
                    if 'member_matches' not in prefix:
                        raise ValueError('re-export with current dependency_blocks binary: member_matches missing')
                    if not set(prefix['member_matches']) <= ids:
                        raise ValueError('prefix member outside its block')
    payload=json.dumps(data,ensure_ascii=False).replace('<','\\u003c')
    template=Path(__file__).with_name('viewer.html').read_text()
    assert template.count('__BLOCK_DATA__')==1
    Path(output).parent.mkdir(parents=True,exist_ok=True)
    Path(output).write_text(template.replace('__BLOCK_DATA__',payload))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('source');p.add_argument('--output',default='results/block_viewer/index.html');a=p.parse_args();render(a.source,a.output)
