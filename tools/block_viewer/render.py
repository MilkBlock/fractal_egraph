#!/usr/bin/env python3
"""Render a self-contained offline inspector from exported block snapshots."""
import argparse,json,shutil,subprocess
from pathlib import Path

def dot_to_svg(dot):
    """Convert a graphviz dot string to an inline SVG using the system `dot`.

    Returns None when `dot` is unavailable or the conversion fails, so the viewer
    can fall back to showing the dot source.
    """
    if not shutil.which('dot'):
        return None
    try:
        proc = subprocess.run(
            ['dot','-Tsvg'], input=dot, capture_output=True, text=True, timeout=30,
        )
    except (subprocess.SubprocessError, OSError):
        return None
    if proc.returncode != 0 or not proc.stdout:
        return None
    return proc.stdout

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
            # Inline the rendered egraph SVG next to the dot source, so the egraph
            # view shows an actual picture instead of raw dot text.
            dot = phase.get('egraph_dot')
            if dot and not phase.get('egraph_svg'):
                svg = dot_to_svg(dot)
                if svg:
                    phase['egraph_svg'] = svg
    payload=json.dumps(data,ensure_ascii=False).replace('<','\\u003c')
    template=Path(__file__).with_name('viewer.html').read_text()
    assert template.count('__BLOCK_DATA__')==1
    Path(output).parent.mkdir(parents=True,exist_ok=True)
    Path(output).write_text(template.replace('__BLOCK_DATA__',payload))

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('source');p.add_argument('--output',default='results/block_viewer/index.html');a=p.parse_args();render(a.source,a.output)
