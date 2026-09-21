#!/usr/bin/env python3
"""Build teaching figures from egglog's own DOT export, without hand-drawn topology.

The mini program marks executable prefixes. Each prefix runs in a fresh native
engine via the egglog CLI. JSON supplies semantic labels; DOT supplies every
cluster, node, and ordered child edge. We inline only String payloads into Var
labels, colour nodes relative to the entry snapshot, and let Graphviz lay out the
result. Counts and a before/after topology audit are recorded alongside the PDFs.
"""
import argparse
import hashlib
import html
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]
NODE = re.compile(r'^\s*"([^"]+)"\[label=')
EDGE = re.compile(r'^\s*"([^"]+)":(\d+):s -> "([^"]+)" \[lhead="([^"]+)"\]')


def semantics(graph):
    """Use child-class leaf sets to label this restricted, acyclic Add fragment.

    Runtime node/row IDs are not stable across executions. For these three
    distinct variables, a class is identified by its multiset of leaves. Check
    every representative agrees; this is not a general e-graph canonicalizer.
    """
    nodes = graph['nodes']
    classes = {}
    for nid, n in nodes.items():
        classes.setdefault(n['eclass'], []).append(nid)
    cache = {}

    def leaves(cid, stack=()):
        if cid in cache:
            return cache[cid]
        if cid in stack:
            raise ValueError('teaching fragment unexpectedly contains a cycle')
        choices = set()
        for nid in classes[cid]:
            n = nodes[nid]
            if n['op'] == 'Var':
                choices.add((json.loads(nodes[n['children'][0]]['op']),))
            elif n['op'] == 'Add':
                choices.add(tuple(sorted(x for child in n['children']
                                         for x in leaves(nodes[child]['eclass'], stack + (cid,)))))
            else:
                raise ValueError(f'unsupported Math operator {n["op"]}')
        if len(choices) != 1:
            raise ValueError('class representatives disagree on variable multiset')
        cache[cid] = choices.pop()
        return cache[cid]

    labels, keys = {}, {}
    for nid, n in nodes.items():
        if n['op'] not in ('Add', 'Var'):
            continue
        leaf = leaves(n['eclass'])
        labels[n['eclass']] = leaf[0] if len(leaf) == 1 else ('r' if len(leaf) == 3 else 'q_' + ''.join(leaf))
        keys[nid] = (n['op'], tuple(leaves(nodes[c]['eclass']) for c in n['children'])) if n['op'] == 'Add' else ('Var', leaf)
    return labels, keys


def style_dot(raw, graph, entry_keys):
    nodes = graph['nodes']
    labels, keys = semantics(graph)
    kept = set(keys)
    removed = set(nodes) - kept
    result, skip_depth = [], 0
    for line in raw.splitlines():
        if skip_depth:
            skip_depth += line.count('{') - line.count('}')
            continue
        # Match the whole balanced outer cluster, never a regex across nested braces.
        if re.match(r'\s*subgraph "outer_cluster_String-', line):
            skip_depth = line.count('{') - line.count('}')
            continue
        edge = EDGE.match(line)
        if edge and (edge[1] in removed or edge[3] in removed):
            continue
        node = NODE.match(line)
        if node:
            nid = node[1]
            if nid not in kept:
                raise ValueError(f'unexpected exported node {nid}')
            n = nodes[nid]
            colour = '#DCEBFA' if keys[nid] in entry_keys else '#FFE5C7'
            line = line.replace('BGCOLOR="white"', f'BGCOLOR="{colour}"')
            if n['op'] == 'Var':
                payload = nodes[n['children'][0]]['op']
                line = line.replace('>Var</td>', f'>Var({html.escape(payload)})</td>')
                line = line.replace('<TR><TD PORT="0"></TD></TR>', '')
            else:
                # Labels expose actual argument classes, including argument order.
                children = [labels[nodes[c]['eclass']] for c in n['children']]
                line = line.replace('>Add</td>', f'>Add({children[0]}, {children[1]})</td>')
                for slot in range(2):
                    line = line.replace(f'<TD PORT="{slot}"></TD>', f'<TD PORT="{slot}"><FONT POINT-SIZE="9">{slot}</FONT></TD>')
            result.append(line)
            continue
        cluster = re.match(r'\s*subgraph "cluster_(Math-[^"]+)" \{', line)
        if cluster:
            result.append(line)
            label = labels[cluster[1]]
            result.append(f'      label="{label}"; color="' + ('#267A68' if label == 'r' else '#7654A3') + '"; fillcolor="white"; penwidth=1.2;')
            continue
        # Replace native global styling; keep the exported graph statements.
        if line.strip().startswith('label=<<TABLE'):
            continue  # native let-binding table is replaced by the root label above
        replacements = {'fontname=helvetica': 'fontname="Times-Roman"',
                        'fontsize=9': 'fontsize=12', 'margin=3': 'margin=0.10',
                        'nodesep=0.05': 'nodesep=0.22', 'ranksep=0.6': 'ranksep=0.65',
                        'colorscheme=set312': 'bgcolor="white"',
                        'graph[style="dashed,rounded,filled"]': 'graph[style="dashed,rounded",fontname="Times-Roman"]'}
        stripped = line.strip()
        if stripped.startswith('fillcolor='):
            continue
        line = '  ' + replacements[stripped] if stripped in replacements else line
        result.append(line)
    if skip_depth:
        raise ValueError('unbalanced native primitive cluster')
    styled = '\n'.join(result) + '\n'
    expected_edges = {(a, str(i), c, 'cluster_' + nodes[c]['eclass'])
                      for a in kept for i, c in enumerate(nodes[a]['children']) if c in kept}
    actual_edges = {m.groups() for line in styled.splitlines() if (m := EDGE.match(line))}
    actual_nodes = {m[1] for line in styled.splitlines() if (m := NODE.match(line))}
    if expected_edges != actual_edges or kept != actual_nodes:
        raise ValueError('styled DOT changed Math nodes or child-class references')
    return styled, {'math_nodes': len(kept), 'math_classes': len(labels),
                    'add_nodes': sum(nodes[n]['op'] == 'Add' for n in kept),
                    'var_nodes': sum(nodes[n]['op'] == 'Var' for n in kept),
                    'math_child_edges': len(expected_edges), 'inlined_string_nodes': len(removed),
                    'new_math_nodes': sum(keys[n] not in entry_keys for n in kept),
                    'topology_check': 'all Math nodes and ordered child edges preserved'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--egglog', type=Path, default=ROOT / 'egglog/target/debug/egglog')
    args = parser.parse_args()
    source = ROOT / 'docs/papers/examples/math-mini-trace.egg'
    output = ROOT / 'docs/papers/figures/native'
    work = ROOT / 'out/paper-native'
    output.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    prefix, programs = [], {}
    for line in source.read_text().splitlines():
        if line.startswith('; SNAPSHOT '):
            programs[line.split()[-1]] = '\n'.join(prefix) + '\n'
        prefix.append(line)
    # Run the complete source as well, so the final equality check is not skipped.
    subprocess.run([str(args.egglog), str(source)], check=True)
    entry_keys, records = None, {}
    expected = {'entry': (2, 3, 5), 'assoc': (4, 3, 6), 'saturated': (12, 3, 7)}
    for name, program in programs.items():
        path = work / f'{name}.egg'
        path.write_text(program)
        subprocess.run([str(args.egglog), '--to-dot', '--to-json', '--max-functions', '100',
                        '--max-calls-per-function', '1000', str(path)], check=True)
        graph = json.loads(path.with_suffix('.json').read_text())
        if entry_keys is None:
            entry_keys = set(semantics(graph)[1].values())
        raw = path.with_suffix('.dot').read_text()
        styled, record = style_dot(raw, graph, entry_keys)
        assert (record['add_nodes'], record['var_nodes'], record['math_classes']) == expected[name], record
        (output / f'{name}.raw.dot').write_text(raw)
        (output / f'{name}.json').write_text(json.dumps(graph, indent=2) + '\n')
        dot = output / f'{name}.dot'
        dot.write_text(styled)
        subprocess.run(['dot', '-Tpdf', str(dot), '-o', str(dot.with_suffix('.pdf'))], check=True)
        records[name] = record
    records['source_sha256'] = hashlib.sha256(source.read_bytes()).hexdigest()
    records['scope'] = 'native egglog; Var/Add only; colours relative to entry; String payloads inlined'
    (output / 'evidence.json').write_text(json.dumps(records, indent=2) + '\n')
    print(json.dumps(records, indent=2))


if __name__ == '__main__':
    main()
