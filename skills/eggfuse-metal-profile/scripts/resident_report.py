#!/usr/bin/env python3
"""Summarize verified resident windows; stream large Apple counter XML.

Retain the original trace and small exports. No inference or GPU replay occurs.
Existing complete summaries are reused rather than exporting counters again.
"""
import argparse,json,subprocess,xml.etree.ElementTree as ET
from pathlib import Path
from profile import TABLES,issues,command
from report import summarize as timeline_summary,trace_problems
from counters import summarize as counter_summary
from resident_full import verify,sha
from artifacts import mirror_summaries


def analyze(window,pid):
    trace=window/'profile.trace';out=window/'exports';out.mkdir(exist_ok=True)
    if (window/'timeline.json').exists() and (window/'counters.json').exists():
        c=json.loads((window/'counters.json').read_text())
        if not c.get('rows'):raise ValueError('empty cached counters')
        return dict(reused=True,timeline=json.loads((window/'timeline.json').read_text()),counters=c)
    problems=issues(trace)
    if trace_problems(problems):raise ValueError('trace issues prevent attribution')
    toc=out/'toc.xml'
    if not toc.exists():command(['xcrun','xctrace','export','--input',trace,'--toc','--output',toc])
    schemas={x.get('schema') for x in ET.parse(toc).findall('.//table')};counts={}
    # Export shader intervals before counters so correlation can be streamed.
    for schema in TABLES:
        if schema=='metal-gpu-counter-intervals' or schema not in schemas:continue
        p=out/(schema+'.xml')
        if not p.exists():command(['xcrun','xctrace','export','--input',trace,'--xpath',f'/trace-toc/run[@number="1"]/data/table[@schema="{schema}"]','--output',p])
        n=0
        for _,e in ET.iterparse(p):
            if e.tag=='row':n+=1;e.clear()
        counts[schema]=n
    if 'metal-gpu-counter-intervals' not in schemas:raise ValueError('counter table not present')
    raw=out/'metal-gpu-counter-intervals.xml'
    if raw.exists():
        # Parse and validate the complete existing XML, never a growing partial.
        c=counter_summary(raw,out/'metal-gpu-intervals.xml',pid,top=12);streamed=False
    else:
        cmd=['xcrun','xctrace','export','--input',str(trace),'--xpath','/trace-toc/run[@number="1"]/data/table[@schema="metal-gpu-counter-intervals"]']
        (out/'counter-export-command.json').write_text(json.dumps(cmd,indent=2)+'\n')
        proc=None
        try:
            with (out/'counter-export.log').open('w') as log:
                proc=subprocess.Popen(cmd,stdout=subprocess.PIPE,stderr=log)
                c=counter_summary(proc.stdout,out/'metal-gpu-intervals.xml',pid,top=12)
                proc.wait(timeout=60)
                if proc.returncode:raise RuntimeError('counter export failed')
        finally:
            if proc is not None and proc.poll() is None:proc.terminate();proc.wait(timeout=30)
        streamed=True
    counts['metal-gpu-counter-intervals']=c['rows']
    (out/'availability.json').write_text(json.dumps(dict(rows=counts,trace_issues=problems,counter_xml_streamed=streamed),indent=2)+'\n')
    t=timeline_summary(out,pid)
    (window/'timeline.json').write_text(json.dumps(t,indent=2)+'\n');(window/'counters.json').write_text(json.dumps(c,indent=2)+'\n')
    return dict(reused=False,counter_xml_streamed=streamed,timeline=t,counters=c)


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--run',type=Path,required=True);a=p.parse_args();root=a.run.resolve()
    replay=json.loads((root/'replay.json').read_text());provenance=json.loads((root/'provenance.json').read_text())
    if not replay.get('full_state_and_history_equal'):raise ValueError('unverified window run')
    ref=Path(provenance['reference'])
    if sha(ref)!=provenance['reference_sha256']:raise ValueError('reference changed')
    verify(json.loads((root/'result.json').read_text()),json.loads(ref.read_text()))
    result=dict(schema='eggfuse-resident-hardware-windows/v1',verification=replay['verification'],windows=[])
    for i,coordinates in enumerate(replay['windows']):
        d=analyze(root/f'window-{i}',replay['worker_pid']);d['coordinates']=coordinates;result['windows'].append(d)
        print(i,'counter rows',d['counters']['rows'],'reused',d['reused'],flush=True)
    (root/'hardware-summary.json').write_text(json.dumps(result,indent=2)+'\n')
    print('local summaries',mirror_summaries(root))
if __name__=='__main__':main()
