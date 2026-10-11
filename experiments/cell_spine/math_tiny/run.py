"""Two tiny native math closures, their generated spines, and readable evidence."""
import argparse
import hashlib
import html
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
HERE = Path(__file__).resolve().parent


def run(*args, **kwargs):
    subprocess.run([str(x) for x in args], cwd=ROOT, check=True, **kwargs)


def dot(state, color):
    q = json.dumps
    groups = {}
    for i, row in enumerate(state['rows']):
        groups.setdefault(row['result'], []).append((i, row))
    lines = ['digraph G {', 'rankdir=TB; compound=true; bgcolor="transparent";',
             'graph [fontname="Arial",fontsize=11]; node [shape=box,fontname="Arial",fontsize=12,style=filled];',
             'edge [fontname="Arial",fontsize=9,color="#81909f"];']
    for cls, rows in groups.items():
        lines.append(f'subgraph cluster_{cls} {{ label="e{cls}"; color="#cbd5e1"; style=rounded;')
        for i, row in rows:
            lines.append(f'n{i} [label={q(row["op"])},fillcolor={q(color)}];')
        lines.append('}')
    for i, value in enumerate(state['values']):
        if i not in groups:
            lines.append(f'v{i} [label={q(value["literal"] or value["sort"])},fillcolor="#f1f5f9"];')
    for i, row in enumerate(state['rows']):
        for pos, child in enumerate(row['args']):
            target = f'n{groups[child][0][0]}' if child in groups else f'v{child}'
            cluster = f',lhead=cluster_{child}' if child in groups else ''
            lines.append(f'n{i} -> {target} [label="{pos}"{cluster}];')
    return '\n'.join(lines+['}'])


def total_matches(case, key):
    return sum(case[key].values())


def render(result, out):
    esc = html.escape
    cases = result['cases']
    rows = []
    for label, c in cases.items():
        v = c['validation']
        rows.append(f'<tr><td>{esc(c["seed"])}</td><td>{v["original_constructor_rows"]} / {v["original_constructor_eclasses"]}</td>'
                    f'<td>{v["spine_constructor_rows"]} / {v["spine_constructor_eclasses"]}</td><td>{v["summary_rows"]}</td>'
                    f'<td>{total_matches(v,"original_logical_matches")} → {total_matches(v,"compressed_logical_matches_before_export")}</td>'
                    f'<td>{c["library_bytes"]:,} B</td></tr>')
    parts = ['<!doctype html><html lang="zh"><meta charset="utf-8"><title>Math tiny · Cell spine</title>',
             '<style>body{font:16px system-ui;color:#152638;background:#f6f8fb;margin:30px auto;max-width:1100px;padding:0 20px}h1{font-size:30px}section{background:white;border:1px solid #dde4ed;border-radius:14px;padding:22px;margin:20px 0}table{width:100%;border-collapse:collapse}th,td{text-align:left;padding:12px;border-bottom:1px solid #dde4ed}pre{overflow:auto;background:#f1f5f9;padding:16px;border-radius:8px;font-size:13px}svg{max-width:100%;height:auto}small,p{line-height:1.7}summary{cursor:pointer;padding:10px 0}.bad{color:#9c4c00}.ok{color:#087767}</style>',
             '<h1>四条数学规则，两个起点，同一个闭包</h1>',
             '<p>加法交换、乘法交换、分配律、提取公因式。实际运行本仓库的 native egglog；不是 pristine 上游性能基准。</p>',
             '<section><table><tr><th>输入</th><th>原始节点 / 类</th><th>spine 节点 / 类</th><th>Summary 行</th><th>逻辑匹配次数</th><th>完整库文本</th></tr>'+''.join(rows)+'</table>',
             '<p>两边分别达到饱和；导出后均为 13 节点、7 个 Math e-class。原始与导出状态逐行、逐等价类对照通过，两个输入的最终状态也通过交叉对照。</p>',
             '<p class="bad">匹配次数不是提交变更次数，也不是 Join 工作量。节点、Summary 行宽不同，表中不能推出 RSS 压缩率或加速倍数。匹配次数不含 Bake、求解和显式导出。</p></section>']
    for label, c in cases.items():
        v = c['validation']
        tables = '\n'.join(f'{name}: {n}' for name,n in v['summary_tables'].items())
        parts += [f'<section><h2>{esc(c["seed"])}</h2>',
                  f'<p>选择 {len(c["plan"]["selected"])} 个 Cell 族；发出 {len(c["transitions"])} 条 spine 规则。含多个父 Summary 的规则实际匹配 <b>{c["multi_parent_matches"]}</b> 次。</p>',
                  '<details><summary>原始 .egg</summary><pre>'+esc(c['source'])+'</pre></details>',
                  '<details open><summary>生成的规则 · 实际匹配次数</summary>']
        for t in c['transitions']:
            m = v['compressed_logical_matches_before_export'].get(t['id'],0)
            parts += [f'<p><b>{esc(t["id"])}</b> · {len(t["parents"])} 个父实例 · {m} 次匹配</p><pre>{esc(t["rule"])}</pre>']
        parts += ['</details><details><summary>Summary 表</summary><pre>'+esc(tables)+'</pre></details>',
                  '<h3>实际存储的 spine</h3>'+ (out/f'{label}-spine.svg').read_text(), '</section>']
    parts += ['<section><h2>原始饱和 egraph</h2><p>圆角框内属于同一个 e-class，箭头指向参数所属的 e-class；数字为参数位置。</p>',
              (out/'original.svg').read_text(), '</section>',
              '<section><h2>这次直接暴露的限制</h2><p>因式形式仍直接查询 Add + Mul 来登记大 Cell，没有使用父 Summary。展开形式同时保留了 Summary 路径与完整 coarse 模式，两条路线都触发；还有已生成但未触发的转移。相同最终闭包尚未得到同样紧凑的 spine。</p>',
              '<p>结果文件：<a href="results.json">results.json</a>。每个子目录保存 selection.lp、transitions.json、runtime.egg 和 restore.egg。</p></section></html>']
    (out/'index.html').write_text('\n'.join(parts))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--skip-build', action='store_true')
    a = parser.parse_args()
    out = a.output.resolve()
    if out.exists() and any(out.iterdir()):
        raise ValueError('use a fresh output directory')
    out.mkdir(parents=True, exist_ok=True)
    if not a.skip_build:
        run('cargo','build','--offline','--release','--bin','egg_layout','--example','lhs_ast',
            '--example','spine_validate','--example','spine_stages','--example','native_math_baseline')
    upstream = ROOT/'egglog/tests/math-microbenchmark.egg'
    result = dict(scope='native constructor-state and logical-match comparison; not a speed/RSS benchmark',
                  provenance=dict(source=str(upstream.relative_to(ROOT)), sha256=hashlib.sha256(upstream.read_bytes()).hexdigest(),
                                  rule_lines=[19,20,33,34]), cases={})
    manifests = {}
    for label, file, seed in [('factored','source.egg','x × (y + z)'), ('expanded','expanded.egg','y × x + z × x')]:
        source = HERE/file
        folder = out/label
        run(sys.executable,'-m','tools.cell_spine.compiler','--source',source,'--mode','symbolic','--output',folder,'--skip-build')
        report = json.loads((folder/'report.json').read_text())
        assert report['status'] == 'compiled', 'must exercise actual spine transitions'
        m = json.loads((folder/'work/validation.json').read_text())
        m['cases'] = [next(c for c in m['cases'] if c['name']=='source')]
        m['dump_states'] = True
        manifests[label] = m
        (folder/'detail-input.json').write_text(json.dumps(m))
        run(ROOT/'target/release/examples/spine_validate',folder/'detail-input.json',folder/'detail.json')
        v = json.loads((folder/'detail.json').read_text())['cases'][0]
        states = v.pop('states')
        v['comparison'].pop('value_map',None)
        c = dict(seed=seed,source=source.read_text(),validation=v,plan=report['plan'],library_bytes=report['library_bytes'],
                 transitions=report['transitions'],multi_parent_matches=sum(v['compressed_logical_matches_before_export'].get(t['id'],0)
                     for t in report['transitions'] if len(t['parents'])>=2))
        result['cases'][label] = c
        diagrams = {f'{label}-spine':(states['spine'],'#dbeafe')}
        if label == 'factored': diagrams['original'] = (states['original'],'#ffedd5')
        for name,(state,color) in diagrams.items():
            path = out/f'{name}.dot';path.write_text(dot(state,color))
            run('dot','-Tsvg',path,'-o',out/f'{name}.svg')
    # Full cross-input comparison: factored reference vs expanded compiled run.
    cross = manifests['expanded']
    cross['cases'][0]['reference_path'] = str(HERE/'source.egg')
    cross['dump_states'] = False
    (out/'cross-input.json').write_text(json.dumps(cross))
    run(ROOT/'target/release/examples/spine_validate',out/'cross-input.json',out/'cross-validation.json')
    result['same_final_closure'] = json.loads((out/'cross-validation.json').read_text())['cases'][0]['comparison']['status']
    (out/'results.json').write_text(json.dumps(result,indent=2,ensure_ascii=False)+'\n')
    render(result,out)
    print(out/'index.html')


if __name__ == '__main__':
    main()
