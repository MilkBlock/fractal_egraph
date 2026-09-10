#!/usr/bin/env python3
"""Render a self-contained offline inspector from exported block snapshots.

Optionally inline a rule-combine candidate export (`rule_prepare` output) as a
fourth view via `--prepare prepared.json`.
"""
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

def render(source, output, prepare=None):
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
    assert template.count('__PREPARE_DATA__')==1
    html=template.replace('__BLOCK_DATA__',payload)
    if prepare:
        prep=json.loads(Path(prepare).read_text())
        prep_payload=json.dumps(prep,ensure_ascii=False).replace('<','\\u003c')
    else:
        prep_payload='null'
    html=html.replace('__PREPARE_DATA__',prep_payload)
    Path(output).parent.mkdir(parents=True,exist_ok=True)
    Path(output).write_text(html)

if __name__=='__main__':
    p=argparse.ArgumentParser();p.add_argument('source');p.add_argument('--output',default='results/block_viewer/index.html');p.add_argument('--prepare',default=None);a=p.parse_args();render(a.source,a.output,a.prepare)
