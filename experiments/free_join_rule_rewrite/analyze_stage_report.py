#!/usr/bin/env python3
"""Summarize actual egglog stage-info without calling logical matches mutations."""
import argparse, collections, json
from pathlib import Path

def ns(t):
    return t.get('secs', 0) * 1_000_000_000 + t.get('nanos', 0)

def stage_kinds(obj, out):
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k in {'Intersect', 'FusedIntersect', 'FusedIntersectMat'}:
                out[k] += 1
            stage_kinds(v, out)
    elif isinstance(obj, list):
        for x in obj: stage_kinds(x, out)

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument('report', type=Path)
    ap.add_argument('-o','--output',type=Path)
    a=ap.parse_args(); data=json.loads(a.report.read_text())
    rules={}
    for iteration in data.get('iterations', []):
        rr=iteration.get('rule_set_report',{}).get('rule_reports',{})
        for name, invocations in rr.items():
            row=rules.setdefault(name, {'invocations':0,'matches':0,'search_ns':0,'stage_kinds':collections.Counter(),'distinct_plans':set()})
            for inv in invocations:
                row['invocations']+=1; row['matches']+=inv.get('num_matches',0); row['search_ns']+=ns(inv.get('search_and_apply_time',{}))
                kinds=collections.Counter(); stage_kinds(inv.get('plan',{}),kinds); row['stage_kinds'].update(kinds)
                row['distinct_plans'].add(json.dumps(inv.get('plan',{}),sort_keys=True,separators=(',',':')))
    result=[]
    for name,row in rules.items():
        result.append({'rule':name,'invocations':row['invocations'],'matches':row['matches'],'search_ms':row['search_ns']/1e6,
                       'matches_per_ms':row['matches']/(row['search_ns']/1e6) if row['search_ns'] else None,
                       'stage_kinds':dict(row['stage_kinds']),'distinct_plan_count':len(row['distinct_plans'])})
    result.sort(key=lambda x:x['search_ms'],reverse=True)
    out={'scope':'actual egglog 3.0 runtime stage-info; matches are logical matches, not committed mutations','rules':result}
    text=json.dumps(out,indent=2,ensure_ascii=False)+'\n'
    if a.output: a.output.write_text(text)
    else: print(text,end='')
if __name__=='__main__': main()
