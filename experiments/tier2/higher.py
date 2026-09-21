"""Feed native power folding with closed unary steps already in tier-1.
The stable update is the resolved source-role Extension, not raw slot equality.
"""
import json,sys
from pathlib import Path
OUT=Path(__file__).parent;T1=OUT.parent/'tier1_extract';sys.path.insert(0,str(T1))
from lower import Lower
m=json.loads((OUT/'math.json').read_text());exts={x['name']:x for x in m['extensions']};extraction=json.loads((T1/'extraction.json').read_text());names={x['eclass']:x['name'] for x in extraction['definitions']}
i=json.loads((T1/'native_interfaces.json').read_text());engine=Lower(json.loads((T1/'native_templates.json').read_text()),i,i['rules'])
text=['(include "experiments/tier1_extract/existing_combs.egg")','(include "experiments/tier2/math.egg")','(include "experiments/tier2/higher_ir.egg")']
for c,n in names.items():text.append(f'(CombName {n} "{n}")')
for e in exts:text.append(f'(ExtensionName ${e} "{e}")')
rows=[]
for o in m['occurrences']:
    e=exts[o['extension']]
    if e['schema']['kind']!='SmoothRuleComposition' or len(o['parents'])!=1:continue
    # Every interface route must stay within this one parent (including facts).
    def closed(r):
        if r[0]=='ParentRole':return r[1]==0
        if r[0]=='Construct':return all(closed(x) for x in r[2])
        return False
    if not all(closed(r) for r in e['routes']):continue
    template=engine.node(o['template']);binding_class=engine.nodes[template['children'][2]]['eclass']
    # Print the selected existing binding, never synthesize a replacement port.
    def egg(c):
        n=engine.node(c)
        if c.split('-',1)[0] in ('String','i64'):return n['op']
        return '('+n['op']+(' '+' '.join(egg(engine.nodes[x]['eclass']) for x in n['children']) if n['children'] else '')+')'
    p=engine.occ[o['parents'][0]]['template'];b=egg(binding_class)
    text.append(f'(UnaryStep {names[p]} {names[o["template"]]} ${o["extension"]} {b})')
    rows.append({'event':o['event'],'parent':names[p],'out':names[o['template']],'extension':o['extension'],'binding':b})
text.append('(run-schedule (saturate (run higher)))')
(OUT/'higher.egg').write_text('\n'.join(text)+'\n');(OUT/'higher_steps.json').write_text(json.dumps(rows,indent=2)+'\n')
