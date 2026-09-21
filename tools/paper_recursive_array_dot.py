#!/usr/bin/env python3
"""Native DOT figures for recursive DSL -> certified Bake arrays.

Run Bake on the four training samples, resolve template IDs by certified law,
query them, and insert the returned array expression into an executable .egg.
All e-nodes, class memberships and solid child edges come from egglog's DOT.
Array diagrams project away helper relations/arithmetic alternatives; the script
checks that all remaining child references still target the original e-class.
Dotted recurrence links are rule-instance annotations, NOT stored child edges or
claimed trace provenance. Raw DOT/JSON and the projection audit remain available.
"""
import argparse
import html
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
EXAMPLES = ROOT / 'docs/papers/examples/recursive-array'
OUT = ROOT / 'docs/papers/figures/native'
NODE = re.compile(r'^\s*"([^"]+)"\[label=')
EDGE = re.compile(r'^\s*"([^"]+)":(\d+):s -> "([^"]+)" \[lhead="([^"]+)"\]')
ALLOWED = {'RampArray', 'FiniteDomain', 'EInt', 'SumReduction', 'ReduceArray'}


def run(*args):
    result = subprocess.run([str(a) for a in args], cwd=ROOT, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stderr + result.stdout)
    return result.stdout


def project(raw, graph, kind):
    nodes = graph['nodes']
    classes = {}
    for nid, n in nodes.items():
        classes.setdefault(n['eclass'], []).append(nid)
    if kind == 'array':
        roots = [n['eclass'] for n in nodes.values() if n['op'] == 'ReduceArray']
        assert len(roots) == 1
        visited, keep, todo = set(), set(), list(roots)
        while todo:
            cid = todo.pop()
            if cid in visited:
                continue
            visited.add(cid)
            chosen = [nid for nid in classes[cid] if nodes[nid]['op'] in ALLOWED]
            if not chosen:
                raise ValueError(f'No printable representative for {cid}')
            keep.update(chosen)
            for nid in chosen:
                # Inline the actual i64 payload in EInt's label; all other
                # child-class references must survive the projection.
                if nodes[nid]['op'] != 'EInt':
                    todo.extend(nodes[c]['eclass'] for c in nodes[nid]['children'])
    else:
        keep = set(nodes)
        roots = []
    kept_classes = {nodes[n]['eclass'] for n in keep}
    representatives = {cid: min(n for n in keep if nodes[n]['eclass'] == cid) for cid in kept_classes}

    lines, skip_depth, edges = [], 0, []
    for line in raw.splitlines():
        if skip_depth:
            skip_depth += line.count('{') - line.count('}')
            continue
        outer = re.match(r'\s*subgraph "outer_cluster_([^"]+)" \{', line)
        if outer and outer[1] not in kept_classes:
            skip_depth = line.count('{') - line.count('}')
            continue
        edge = EDGE.match(line)
        if edge:
            src, slot, target, cluster = edge.groups()
            if src not in keep or (kind == 'array' and nodes[src]['op'] == 'EInt'):
                continue
            cid = nodes[target]['eclass']
            if cid not in kept_classes:
                raise ValueError('dangling projected child class')
            # Changing the chosen representative does not change the child class.
            target = target if target in keep else representatives[cid]
            line = f'  "{src}":{slot}:s -> "{target}" [lhead="cluster_{cid}"]'
            edges.append((src, int(slot), cid))
        node = NODE.match(line)
        if node:
            nid = node[1]
            if nid not in keep:
                continue
            n = nodes[nid]
            label, fill = n['op'], '#DCEBFA'
            if n['op'] == 'EInt':
                label = 'EInt(' + nodes[n['children'][0]]['op'] + ')'
                if n['eclass'] in roots:
                    fill = '#FFE5C7'
                line = re.sub(r'<TR><TD PORT="0"></TD></TR>', '', line)
            elif n['op'] in {'Cursor', 'Emit', 'Expand'}:
                values = [nodes[c]['op'] for c in n['children']]
                label += '(' + ', '.join(values) + ')'
                fill = '#DCEBFA' if values[0] in ('3', '4') and n['op'] != 'Emit' else '#FFE5C7'
            elif nid.startswith('primitive-'):
                fill = '#F1F1F1'
            line = line.replace('BGCOLOR="white"', f'BGCOLOR="{fill}"')
            line = line.replace('>' + html.escape(n['op']) + '</td>', '>' + html.escape(label) + '</td>')
            for i in range(len(n['children'])):
                line = line.replace(f'<TD PORT="{i}"></TD>', f'<TD PORT="{i}"><FONT POINT-SIZE="9">{i}</FONT></TD>')
        cluster = re.match(r'\s*subgraph "cluster_([^"]+)" \{', line)
        if cluster:
            lines.append(line)
            colour = '#267A68' if cluster[1] in roots else '#7654A3'
            label = 'sum: same e-class' if cluster[1] in roots else ''
            lines.append(f'      color="{colour}"; fillcolor="white"; label="{label}"; penwidth=1.2;')
            continue
        if line.strip().startswith(('fillcolor=', 'label=<<TABLE')):
            continue
        line = line.replace('colorscheme=set312', 'bgcolor="white"').replace('nodesep=0.05', 'nodesep=0.22').replace('margin=3', 'margin=0.12')
        lines.append(line)
    if skip_depth:
        raise ValueError('unbalanced cluster in native DOT')
    expected = {(nid, i, nodes[c]['eclass']) for nid in keep
                if not (kind == 'array' and nodes[nid]['op'] == 'EInt')
                for i, c in enumerate(nodes[nid]['children'])}
    if set(edges) != expected or len(edges) != len(expected):
        raise ValueError('projected edges disagree with native JSON')
    styled = '\n'.join(lines) + '\n'
    assert {m[1] for ln in styled.splitlines() if (m := NODE.match(ln))} == keep
    overlays = []
    if kind in ('tree', 'scan'):
        def values(nid):
            return tuple(int(nodes[c]['op']) for c in nodes[nid]['children'])
        operators = {}
        for nid in keep:
            if nodes[nid]['op'] in ('Expand', 'Emit', 'Cursor'):
                operators[(nodes[nid]['op'], values(nid))] = nid
        # An explicit overlay for these exact tiny rules. It is not a producer
        # certificate inferred from coincident values in an arbitrary program.
        for (op, v), nid in sorted(operators.items()):
            destinations = []
            if op == 'Expand':
                destinations = [('Emit', (3 * v[0] + r,), 'split-three') for r in range(3)]
            elif op == 'Emit':
                destinations = [('Expand', v, 'resume')]
            elif v[0] < v[1]:
                destinations = [('Cursor', (v[0] + 1, v[1]), 'scan-next')]
            for dest_op, dest, label in destinations:
                if (dest_op, dest) in operators:
                    other = operators[(dest_op, dest)]
                    overlays.append((nid, other, label))
        extra = '\n'.join(f'  "{a}" -> "{b}" [style=dashed,color="#267A68",fontsize=9,label="{label}"];' for a,b,label in overlays)
        pos = styled.rfind('}')
        styled = styled[:pos] + extra + '\n' + styled[pos:]
    return styled, {'native_nodes': len(nodes), 'visible_nodes': len(keep),
                    'visible_classes': len(kept_classes), 'solid_edges_checked': len(edges),
                    'omitted_nodes': sorted(set(nodes) - keep),
                    'rule_instance_overlay': overlays,
                    'audit': 'all visible nodes and class memberships native; solid edges preserve ordered child classes'}


def export(cli, work, name, source, kind):
    path = work / f'{name}.egg'
    path.write_text(source)
    run(cli, '--to-dot', '--to-json', '--max-functions', '1000', '--max-calls-per-function', '10000', path)
    raw = path.with_suffix('.dot').read_text()
    graph = json.loads(path.with_suffix('.json').read_text())
    styled, evidence = project(raw, graph, kind)
    (OUT / f'{name}.egg').write_text(source)
    (OUT / f'{name}.raw.dot').write_text(raw)
    (OUT / f'{name}.raw.json').write_text(json.dumps(graph, indent=2) + '\n')
    (OUT / f'{name}.dot').write_text(styled)
    run('dot', '-Tpdf', OUT / f'{name}.dot', '-o', OUT / f'{name}.pdf')
    return graph, evidence


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--library', type=Path, help='existing Bake output directory; otherwise train afresh')
    args = parser.parse_args()
    cli = ROOT / 'egglog/target/debug/egglog'
    bake = ROOT / 'target/debug/egg_layout'
    OUT.mkdir(parents=True, exist_ok=True)
    (ROOT / 'out').mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='paper-recursive-', dir=ROOT / 'out') as tmp:
        work = Path(tmp)
        library = args.library.resolve() if args.library else work / 'bake'
        if not args.library:
            run(bake, 'bake', EXAMPLES / 'manifest.json', library)
        summary = json.loads((library / 'bake.json').read_text())
        families = {f['array_law']: f['id'] for f in summary['families']}
        evidence = {'scope': 'native DSL snapshots and native array-query projections; no ripen feedback', 'figures': {}}
        for name, kind in [('recursive-tree', 'tree'), ('recursive-scan', 'scan')]:
            graph, info = export(cli, work, name, (EXAMPLES / f'{kind}-diagram.egg').read_text(), kind)
            counts = {op: sum(n['op'] == op for n in graph['nodes'].values()) for op in ('Expand','Emit','Cursor')}
            assert counts == ({'Expand':4,'Emit':3,'Cursor':0} if kind == 'tree' else {'Expand':0,'Emit':0,'Cursor':4}), counts
            info['constructor_counts'] = counts
            evidence['figures'][name] = info
        for name, law, query, expected in [
            ('recursive-array', 'radix_frontier', ['4','--depth','4'], 29484),
            ('recursive-scan-array', 'bounded_unit_stride', ['3','6'], 15)]:
            dest = work / f'{name}-query.json'
            run(bake, 'bake-eval', library / 'library.egg', families[law], *query, dest)
            result = json.loads(dest.read_text())
            assert result['result']['sum'] == f'(EInt {expected})'
            assert result['tier0_rule_applications'] == result['result']['element_queries'] == result['result']['expanded_prefixes'] == 0
            array = result['result']['array']
            # No hand-written array law: use exactly the certified query output.
            source = ('; Generated from Bake query output, not a training input.\n'
                      '(include "rules/tier2.egg")\n'
                      f'(let $array {array})\n(let $sum (ReduceArray (SumReduction) $array))\n'
                      '(run-schedule (saturate (seq (run endpoint-reduce) (run array-shape) (run array-laws) (run array-eval))))\n'
                      f'(check (= $sum (EInt {expected})))\n')
            graph, info = export(cli, work, name, source, 'array')
            reduces = [n for n in graph['nodes'].values() if n['op'] == 'ReduceArray']
            assert len(reduces) == 1
            assert any(n['op'] == 'EInt' and n['eclass'] == reduces[0]['eclass']
                       and graph['nodes'][n['children'][0]]['op'] == str(expected) for n in graph['nodes'].values())
            result.pop('seconds', None)
            info['bake_query'] = result
            evidence['figures'][name] = info
        (OUT / 'recursive-array-evidence.json').write_text(json.dumps(evidence, indent=2) + '\n')
        print('Four native figures generated; child-class audits and Bake result checks passed.')


if __name__ == '__main__':
    main()
