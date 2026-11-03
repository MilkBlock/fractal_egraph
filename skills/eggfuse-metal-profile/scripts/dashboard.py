#!/usr/bin/env python3
"""Render saved resident summaries offline. Does not execute or profile a worker."""
import argparse
from html import escape as esc
import json
from pathlib import Path
from resident_full import sha


def render(p, hardware):
    total = p['encoder_duration_sum_seconds']
    def number(x):
        return '不可用' if x is None else f'{x:,.3f}'
    def table(head, rows, cls=''):
        return '<table class="'+cls+'"><thead><tr>'+''.join('<th>'+esc(str(x))+'</th>' for x in head)+'</tr></thead><tbody>'+''.join('<tr>'+''.join('<td>'+esc(str(x))+'</td>' for x in row)+'</tr>' for row in rows)+'</tbody></table>'
    groups=''.join('<div class="bar"><code>'+esc(name)+'</code><div><i style="width:'+str(100*v['seconds']/total)+'%"></i></div><span>'+number(v['seconds'])+' s · '+f'{100*v["seconds"]/total:.2f}%</span></div>' for name,v in sorted(p['groups'].items(),key=lambda x:-x[1]['seconds']))
    phases=table(['Encoder','累计 s','发出次数','零时间次数','p50 µs','p95 µs','p99 µs','max µs'],[[name,number(v['seconds']),v['issued_encoders'],v.get('zero_duration_encoders','—'),*[number(v.get(k)) for k in ['p50_us','p95_us','p99_us','max_us']]] for name,v in sorted(p['phases'].items(),key=lambda x:-x[1]['seconds'])], 'phases')
    wins=[]
    for i,w in enumerate(hardware.get('windows',[])):
        t=w['timeline'];c=w['counters'];a=c['attribution'];g=c['groups']['all_target_active'];coord=w['coordinates']
        rows=[[name,number(v['mean']),number(v['peak']),v['samples'],number(v['overlap_seconds'])] for name,v in sorted(g['counters'].items())]
        wins.append(f'<article><h3>窗口 {i}</h3><p>外层 command：{coord["start"]["outer_command"]} → {coord["end"]["outer_command"]}；完成 Run：{esc(str(coord["start"]["completed_runs"]))} → {esc(str(coord["end"]["completed_runs"]))}</p><p>GPU active {number(t["gpu_active_seconds"])} / span {number(t["active_span_seconds"])} s；其他进程重叠 {100*a["other_process_overlap_fraction"]:.2f}%；目标 shader 样本覆盖 {100*a.get("target_shader_sample_coverage_fraction",0):.4f}%。</p><details><summary>全部硬件计数器（Apple 原始单位）</summary>'+table(['指标','加权均值','峰值','样本数','覆盖 s'],rows)+'</details><details><summary>计数器定义、单位及 CPU 样本</summary><pre>'+esc(json.dumps(dict(metadata=c['counter_metadata'],cpu_samples=t.get('cpu_samples')),ensure_ascii=False,indent=2))+'</pre></details></article>')
    accounting=table(['口径','数值'],[
        ['复用的无插桩推理基线',number(p['reference_inference_seconds'])+' s'],
        ['本次插桩推理',number(p['profiled_inference_seconds'])+' s'],
        ['插桩扰动',f'{100*(p["timing_perturbation_ratio"]-1):+.2f}%'],
        ['全部已登记 command GPU 累计时间',number(p['all_command_gpu_seconds'])+' s'],
        ['外层 / 全部 command 数',str(p['verification']['outer_command_buffers'])+' / '+str(p['registered_command_buffers'])],
        ['encoder 时长和 / 区间并集',number(total)+' / '+number(p['encoder_interval_union_seconds'])+' s'],
        ['encoder 重叠时间',number(p['overlapping_encoder_seconds'])+' s'],
        ['未归属间隔（不是纯屏障时间）',number(p['unattributed_command_seconds'])+' s'],
        ['主线程 wait 调用 / 累计墙钟',str(p['cpu_wait_calls'])+' / '+number(p['cpu_wait_wall_seconds'])+' s'],
        ['CPU command 编码 / counter 解析',number(p['cpu_command_encoding_wall_seconds'])+' / '+number(p['counter_resolution_wall_seconds'])+' s']])
    return '''<!doctype html><html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><title>Eggfuse · Resident profile</title><style>
body{font:16px/1.6 system-ui,sans-serif;max-width:1250px;margin:36px auto;padding:0 24px;background:#f4f6f8;color:#192b3d}h1{font-size:32px}h2{margin-top:36px}article,.panel{background:white;border:1px solid #d9e2e9;border-radius:12px;padding:20px;margin:18px 0}.notice{border-left:5px solid #b86d19;padding:12px 18px;background:#fff6df}table{width:100%;border-collapse:collapse;overflow:auto;display:table;font-variant-numeric:tabular-nums}th,td{text-align:left;border-bottom:1px solid #e1e7eb;padding:8px}th{background:#e9eff3;position:sticky;top:0}code{font-size:13px}pre{white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px}.bar{display:grid;grid-template-columns:215px 1fr 185px;align-items:center;gap:12px;margin:10px 0}.bar div{background:#e5edf2;height:18px;border-radius:3px}.bar i{display:block;height:100%;background:#227b9e;border-radius:3px}.bar span{font-size:13px}input{padding:10px;width:min(400px,90%)}details{margin:12px 0}summary{cursor:pointer}footer{color:#53697b;margin:24px 0}.scroll{overflow:auto;max-height:700px}@media(max-width:700px){.bar{grid-template-columns:1fr}.bar span{margin-bottom:10px}}</style>
<h1>Eggfuse · Resident GPU profile</h1><p>真实 Raytrace Pass 3 · 全程计时、局部硬件计数器、完整状态复核</p>
<p class="notice">阶段占比只针对插桩诊断，不是无插桩基线的精确分解。CPU 等待包含 GPU 工作，不能相加。硬件计数器为设备级数据，不是逐 kernel 独占利用率；峰值不代表持续利用率。</p>
<details><summary>实际执行配置（标明有无源代码 marker）</summary><pre>'''+esc(json.dumps(p.get('configuration'),ensure_ascii=False,indent=2))+'''</pre></details><h2>计时口径</h2><div class="panel">'''+accounting+'''</div><h2>阶段工作在哪里</h2><p>分母：所有采样 encoder 的时长和；区间并集另列，重叠不冒充额外墙钟。</p><div class="panel">'''+groups+'''</div><h2>逐 encoder 耗时与长尾</h2><p>issued 包含无工作派发；非零耗时也不等于有效计算。</p><input id="filter" placeholder="按 kernel 名称筛选"><div class="scroll">'''+phases+'''</div><h2>硬件窗口</h2><p>两个窗口不同，不用于互算加速比；计数器组覆盖秒数可能不同。</p>'''+''.join(wins)+'''<h2>验证与真实工作量</h2><details open><summary>同执行器复核；不是独立的 CPU 语义证明</summary><pre>'''+esc(json.dumps(dict(verification=p['verification'],work=p.get('work')),ensure_ascii=False,indent=2))+'''</pre></details><footer>本页面仅读取已有 JSON；不联网、不重新运行 benchmark。原始数据见同目录 data.json。Apple replay 缺失的寄存器与 spill 不填零。</footer><script>document.querySelector('#filter').addEventListener('input',e=>{for(const r of document.querySelectorAll('.phases tbody tr'))r.hidden=!r.cells[0].textContent.toLowerCase().includes(e.target.value.toLowerCase())})</script></html>'''


def main():
    a=argparse.ArgumentParser(description=__doc__)
    a.add_argument('--profile',type=Path,required=True);a.add_argument('--hardware',type=Path,required=True);a.add_argument('--output',type=Path,required=True)
    args=a.parse_args();p=json.loads(args.profile.read_text());h=json.loads(args.hardware.read_text())
    if not p['verification']['full_state_and_history_equal'] or not h['verification']['full_state_and_history_equal']:
        raise ValueError('refusing to display unverified diagnostics as accepted results')
    args.output.mkdir(parents=True,exist_ok=False)
    (args.output/'index.html').write_text(render(p,h))
    (args.output/'data.json').write_text(json.dumps(dict(profile=p,hardware=h,sources=[dict(path=str(x.resolve()),sha256=sha(x)) for x in [args.profile,args.hardware]]),indent=2)+'\n')
    print(args.output/'index.html')


if __name__=='__main__':main()
