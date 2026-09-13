"""Attach symbolic interfaces to native tier-1 occurrences, outside Comb keys."""
import json
from pathlib import Path
OUT=Path(__file__).parent

def commands(r,i,rules):
    names={x['eclass']:x['name'] for x in r['definitions']}
    q=json.dumps
    initial=['(include "experiments/tier1_extract/existing_combs.egg")',
          '(relation SnapshotFingerprint (String))', '(SnapshotFingerprint '+q(i['native_sha256'])+')', '(relation OriginalClass (Comb String))','(relation InterfaceLayout (Instance String))','(relation SourceRuleAST (RuleId String))']
    yield from initial
    for cls,name in names.items():yield (f'(OriginalClass {name} {q(cls)})')
    for rule,ast in rules.items():yield (f'(SourceRuleAST (Rule {q(rule)}) {q(q(ast))})')
    for x in i['occurrences']:
        event=x['event'];yield (f'(let $occ{event} (Occurrence {event} {names[x["template"]]}))')
        layout=q({k:x[k] for k in ['inputs','outputs']})
        yield (f'(InterfaceLayout $occ{event} {q(layout)})')
    for x in i['occurrences']:
        for slot,p in enumerate(x['parents']):yield (f'(ParentAt $occ{x["event"]} {slot} $occ{p})')
    yield ('(run-schedule (saturate (run tier1)))')
    for x in i['occurrences']:
        for slot,p in enumerate(x['parents']):yield (f'(check (LinkedParent $occ{x["event"]} {slot} $occ{p}))')

if __name__=='__main__':
    r=json.loads((OUT/'extraction.json').read_text())
    i=json.loads((OUT/'interfaces.json').read_text())
    rules=json.loads((OUT/'source_schema.json').read_text())
    (OUT/'interfaces.egg').write_text('\n'.join(commands(r,i,rules))+'\n')
