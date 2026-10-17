"""Adapters from existing baked spine/depth and transition programs to the
shared matrix IR. These compile symbolic wiring, never concrete match answers.
The inputs retain their original finite-domain and supplied-boundary contracts.
"""
from experiments.cell_spine.tensor_interaction import demo as tensor
from experiments.cell_spine.layered_context import demo as layers
from experiments.cell_spine.marker_context.demo import MarkerPlan
from experiments.cell_spine.moment_compiler.compiler import Circuit,certify_commutative
from experiments.cell_spine.transition_program import demo as td

def nodes(c):
    return [dict(op=n['op'],args=n['args'],**({'slot':n['slot']} if n['op']=='port' else {})) for n in c.nodes]
def base(c,cells,channels,conditions,stages,evidence,templates):
    return dict(version=1,bits=c.bits,slots=c.slots,nodes=nodes(c),cells=cells,templates=templates,channels=channels,conditions=conditions,stages=stages,updates=[],evidence=evidence)
def root_program(library,prepared,old_cells,mode,marker,eager,ast):
    c=Circuit(4,library['width'],certify_commutative(ast,'Add'))
    ports,channels,_=layers.batch_layout(library,prepared,old_cells,mode)
    cells=[dict(template=tid,binding=[tensor.ALPHABET.index(v) for v in binding],partition=list(range(len(binding)))) for tid,binding in tensor.CASES]
    width=library['width'];slots={i:i for i in range(width)}
    if marker:
        plan=MarkerPlan(library,'pair');width=plan.width
        for m in plan.specs:
            if len(m['support'])!=2:raise ValueError('this importer requires exact binary-pair markers')
            _,node=c.compile(('Add',*[("Input",v) for v in m['support']]))
            slots[m['slot']]=node
        jobs=plan.candidates(dict(library,width=width),dict(prepared,cell_types=[x.template for x in old_cells]),channels)
    else:jobs=layers.candidates(library,prepared,channels)
    conditions=[];intern={};bindings=[]
    for j in jobs:
        alternatives=[]
        for branch in j['alternatives']:
            reqs=[]
            for depth,p,q in branch:
                left=(p//width,slots[p%width]);right=(q//width,slots[q%width]);key=tuple(sorted((left,right)))
                if key not in intern:intern[key]=len(conditions);conditions.append(dict(left=key[0],right=key[1]))
                reqs.append(dict(condition=intern[key],depth=0 if eager else depth))
            alternatives.append(reqs)
        bindings.append(dict(label=f"{j['left']}:{j['right']}",sources=sorted(set([channels[j['left']]['instance'],channels[j['right']]['instance']])),alternatives=alternatives,target=None))
    spec=base(c,cells,bindings,conditions,[list(range(len(bindings)))],dict(source='native baked math interfaces, imported from layered_context/marker_context',scope='all offered Add contexts in this validation fixture; not arbitrary runtime Cell pairing',marker=marker,mode=mode),library['templates'])
    return spec,channels,ports

def transition_program(p,c,jobs,ports,active):
    cells=[dict(template=x['template'],binding=td.decode([ports[i]])[0][:p.bake.templates[x['template']]['arity']] if active[i] else None,partition=list(range(p.bake.templates[x['template']]['arity']))) for i,x in enumerate(p.instances)]
    conditions=[];intern={};channels=[];stages=[]
    for job in jobs:
        stage=[]
        for t in job['transitions']:
            reqs=[]
            for qi,entry in t['conditions']:
                q=job['plan']['queries'][qi];n=len(q['wires']);left=tuple(q['wires'][entry//n]);right=tuple(q['wires'][entry%n]);key=tuple(sorted((left,right)))
                if key not in intern:intern[key]=len(conditions);conditions.append(dict(left=key[0],right=key[1]))
                reqs.append(dict(condition=intern[key],depth=0))
            stage.append(len(channels));channels.append(dict(label=str(t['branch']),sources=t['sources'],alternatives=[reqs],target=dict(cell=t['target'],bindings=t['bindings'])))
        stages.append(stage)
    return base(c,cells,channels,conditions,stages,dict(source='transition_program retained-context Bake',scope='supplied finite two-stage boundary connections; pending alias target is not rejection'),p.bake.templates)
