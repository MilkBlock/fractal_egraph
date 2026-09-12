"""Attach symbolic interfaces to native tier-1 occurrences, outside Comb keys."""
import json
from pathlib import Path
OUT=Path(__file__).parent
r=json.loads((OUT/'extraction.json').read_text());names={x['eclass']:x['name'] for x in r['definitions']}
i=json.loads((OUT/'interfaces.json').read_text());rules=json.loads((OUT/'source_schema.json').read_text())
q=json.dumps
text=['(include "experiments/tier1_extract/existing_combs.egg")',
      '(relation SnapshotFingerprint (String))', '(SnapshotFingerprint '+q(i['native_sha256'])+')', '(relation OriginalClass (Comb String))','(relation InterfaceLayout (Instance String))','(relation SourceRuleAST (RuleId String))']
for cls,name in names.items():text.append(f'(OriginalClass {name} {q(cls)})')
for rule,ast in rules.items():text.append(f'(SourceRuleAST (Rule {q(rule)}) {q(q(ast))})')
for x in i['occurrences']:
    event=x['event'];text.append(f'(let $occ{event} (Occurrence {event} {names[x["template"]]}))')
    layout=q({k:x[k] for k in ['inputs','outputs']})
    text.append(f'(InterfaceLayout $occ{event} {q(layout)})')
for x in i['occurrences']:
    for slot,p in enumerate(x['parents']):text.append(f'(ParentAt $occ{x["event"]} {slot} $occ{p})')
text.append('(run-schedule (saturate (run tier1)))')
for x in i['occurrences']:
    for slot,p in enumerate(x['parents']):text.append(f'(check (LinkedParent $occ{x["event"]} {slot} $occ{p}))')
(OUT/'interfaces.egg').write_text('\n'.join(text)+'\n')
