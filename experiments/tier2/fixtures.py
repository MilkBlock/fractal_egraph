from pathlib import Path
OUT=Path(__file__).parent/'fixtures';OUT.mkdir(exist_ok=True)
for name,step,weight in [('unit',1,1),('triple',1,3),('stride',2,1),('changing',1,1),('warmup',1,1)]:
    base='(datatype E (F i64) (Const i64) (Add E E))\n(relation Edge (i64 i64 i64))\n(ruleset unfold)\n'
    cases=[('(> n 0)',weight)] if name!='changing' else [('(> n 12)',1),('(> n 0) (<= n 12)',2)]
    if name=='warmup':cases=[('(> n 22)',7),('(> n 0) (<= n 22)',1)]
    for j,(guard,w) in enumerate(cases):
        base+=f'(rule ((= root (F n)) {guard}) ((union root (Add (F (- n {step})) (Const {w}))) (Edge n (- n {step}) {w})) :ruleset unfold :name "unfold-{j}")\n'
    base+='(F 24)\n(run unfold 20)\n'
    (OUT/(name+'.egg')).write_text(base)
