"""Extract exact vec_add evidence from the already validated raytrace output."""
import hashlib
import json
import re
from pathlib import Path


def export(repo,out):
    ir=repo/'out/boundary-loop/extraction/gpu.egg'
    rust=Path('/Users/mineralsteins/Repos/egg_related/check_statewalk/benchmarks/passing/raytrace/raytrace.rs')
    source=ir.read_text(); a=source.index('; Function #3\n');b=source.index('; Function #4\n',a)
    block=source[a:b]
    assert '(Function "vec_add" __tmp7123 __tmp7121 __tmp7116)' in block
    bindings={m.group(1):m.group(2) for m in re.finditer(r'\(let (__tmp\d+) (.*)\)',block)}
    expected={
        '__tmp7061':'(Get __tmp7060 2)', '__tmp7062':'(Load)',
        '__tmp7068':'(Bop __tmp7062 __tmp7067 __tmp7061)',
        '__tmp7069':'(Get __tmp7068 1)',
        '__tmp7074':'(Bop __tmp7062 __tmp7073 __tmp7069)',
        '__tmp7075':'(Get __tmp7074 1)',
        '__tmp7100':'(Get __tmp7068 0)',
        '__tmp7094':'(Alloc 0 __tmp7093 __tmp7089 __tmp7091)',
        '__tmp7097':'(Get __tmp7094 0)',
        '__tmp7103':'(Top __tmp7096 __tmp7101 __tmp7102 __tmp7095)',
        '__tmp7108':'(Top __tmp7096 __tmp7104 __tmp7107 __tmp7103)',
        '__tmp7113':'(Top __tmp7096 __tmp7109 __tmp7112 __tmp7108)',
    }
    for key,value in expected.items():assert bindings[key]==value,(key,bindings.get(key))
    steps=[('__tmp7068','b[0]'),('__tmp7074','a[1]'),('__tmp7077','b[1]'),('__tmp7082','a[2]'),('__tmp7085','b[2]'),('__tmp7088','a[0]')]
    previous='__tmp7061';pointers=[]
    assert bindings['__tmp7063']=='(PtrAdd)'
    assert bindings['__tmp7096']=='(Write)'
    assert bindings['__tmp7098']=='(FAdd)'
    for root,label in steps:
        args=bindings[root][1:-1].split();assert args[:2]==['Bop','__tmp7062'] and args[-1]==previous
        ptr=bindings[args[2]][1:-1].split();assert ptr[:2]==['Bop','__tmp7063']
        argument=bindings[ptr[2]];assert argument in ('(Get __tmp7060 0)','(Get __tmp7060 1)')
        index_const=bindings[ptr[3]][1:-1].split();assert index_const[0]=='Const'
        index=re.fullmatch(r'\(Int ([012])\)',bindings[index_const[1]]).group(1)
        actual=('a' if argument.endswith('0)') else 'b')+'['+index+']';assert actual==label
        pointers.append(args[2]);previous=next(k for k,v in bindings.items() if v==f'(Get {root} 1)')
    assert previous=='__tmp7089'
    for result,left,right in [('__tmp7102','__tmp7088','__tmp7068'),('__tmp7107','__tmp7074','__tmp7077'),('__tmp7112','__tmp7082','__tmp7085')]:
        args=bindings[result][1:-1].split();assert args[:2]==['Bop','__tmp7098']
        assert bindings[args[2]]==f'(Get {left} 0)' and bindings[args[3]]==f'(Get {right} 0)'
    consumers=[k for k,v in bindings.items() if '__tmp7097' in v]
    assert consumers==['__tmp7101','__tmp7104','__tmp7109','__tmp7115']
    chosen=['__tmp7061','__tmp7062','__tmp7068','__tmp7069','__tmp7074','__tmp7075','__tmp7100']
    lines=[]
    for key in chosen:
        line=next((i,l.strip()) for i,l in enumerate(source.splitlines(),1) if l.strip().startswith('(let '+key+' '))
        lines.append(dict(line=line[0],text=line[1],name=key))
    rust_source=rust.read_text();start=rust_source.index('fn vec_add(');end=rust_source.index('\n}',start)+2
    meta=dict(function='vec_add',ir_path=str(ir),ir_sha256=hashlib.sha256(ir.read_bytes()).hexdigest(),
              rust_path=str(rust),rust_sha256=hashlib.sha256(rust.read_bytes()).hexdigest(),rust=rust_source[start:end],
              first_line=rust_source[:start].count('\n')+1,lines=lines,loads=[dict(node=n,label=l) for n,l in steps],
              alloc='__tmp7094',writes=['__tmp7103','__tmp7108','__tmp7113'],shared_pointer='__tmp7097',pointer_consumers=consumers,
              note='Exact chosen Tiger DAG, not all pre-extraction alternatives. Source operator lets and node IDs remain unchanged.')
    (out/'ir-demo.json').write_text(json.dumps(meta,ensure_ascii=False,indent=2)+'\n')
    prefix=source[:source.index('; Function #1\n')]
    (out/'vec_add-extracted.egg').write_text(prefix+block+'(run reconstruction 1)\n')
    (out/'vec_add-source.rs').write_text(meta['rust']+'\n')
    return meta
