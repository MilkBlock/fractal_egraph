"""Check native sizes against the upstream fixture and summarize native rule counts."""
from pathlib import Path
import json,re,hashlib
root=Path(__file__).resolve().parents[2]
base=Path(__file__).parent
result=json.loads((base/'native.json').read_text())
assert result['error'] is None
sizes=next(o for o in result['outputs'] if o.startswith('((Add '))
parsed={n:int(v) for n,v in re.findall(r'\((\w+) (\d+)\)',sizes)}
snapshot=(root/'egglog/tests/snapshots/files__shared_snapshot_math_microbenchmark.snap').read_text()
expected={n:int(v) for n,v in re.findall(r'\((\w+) (\d+)\)',snapshot)}
assert parsed==expected,(parsed,expected)
stats=next(o for o in result['outputs'] if o.startswith('Overall statistics:'))
rules=[]
for line in stats.splitlines():
 m=re.fullmatch(r'Rule (.*): search and apply ([0-9.]+)s, num matches (\d+)',line)
 if m:rules.append({'rule':m[1],'search_apply_seconds_rounded':float(m[2]),'native_num_matches':int(m[3])})
assert len(rules)==sum(line.startswith('Rule ') for line in stats.splitlines())
rules.sort(key=lambda r:(-r['native_num_matches'],r['rule']))
time=(base/'native.time.txt').read_text()
rss=int(re.search(r'(\d+)\s+maximum resident set size',time)[1])
summary={'source_sha256':hashlib.sha256((root/'egglog/tests/math-microbenchmark.egg').read_bytes()).hexdigest(),'native_rounds':11,'build':'release','trace':False,'native_elapsed_seconds':result['elapsed_seconds'],'max_rss_bytes_macos_time':rss,'sizes_match_checked_in_snapshot':True,'table_rows':parsed,'total_table_rows':sum(parsed.values()),'rule_counts':rules,'count_semantics':'native aggregate matches, not committed mutations or combined rule usage'}
(base/'summary.json').write_text(json.dumps(summary,indent=2)+'\n')
lines=['# Math microbenchmark 原生运行结果','',f"完整原文件 11 轮，release，无 dependency trace。函数行数逐项匹配仓库快照；合计 {sum(parsed.values()):,} 行。",'',f"单次程序内计时 {result['elapsed_seconds']:.3f} 秒；macOS time -l 最大 RSS {rss:,} 字节。不是多次基准平均值，也不是有 trace/组合器的耗时。",'', '| 规则 | 原生 num matches |','|---|---:|']
for r in rules:lines.append(f"| `{r['rule']}` | {r['native_num_matches']} |")
lines+=['','这些匹配计数来自原生 print-stats，不是已提交变更，也不是 combined rule 使用次数。本轮没有执行全量 dependency tracing 或生成 math 的组合见证文件。','','复现：','```sh','CARGO_INCREMENTAL=0 cargo build --release --bin run_egg','/usr/bin/time -l target/release/run_egg egglog/tests/math-microbenchmark.egg experiments/math_microbenchmark/native.json 2> experiments/math_microbenchmark/native.time.txt','python3 experiments/math_microbenchmark/summarize.py','```']
(base/'README.md').write_text('\n'.join(lines)+'\n')
print(json.dumps({'rows':sum(parsed.values()),'rules':len(rules),'top':rules[:5]},indent=2))
