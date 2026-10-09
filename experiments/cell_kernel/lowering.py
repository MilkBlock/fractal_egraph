"""One typed Cell program supplies observation, updates, CPU and Metal lowering."""
from program import Program

FIELDS=['id','a','a_alias','b','c','old_left','old_right','flags',
        'term_left','term_right','op_left','op_right','type_left','type_right',
        'pred_left','pred_right','input_left','input_right']


def cicm_program():
    p=Program()
    x={name:p.input(name) for name in FIELDS}
    one=p.const(1)
    bl=p.add(p.mul_const(2,x['a']),one)
    br=p.add(x['b'],x['c'],one)
    left=p.minimum(x['old_left'],p.add(bl,one))
    right=p.minimum(x['old_right'],p.add(br,one))
    p.name_cell('size',dict(bopLeft=bl,bopRight=br,left=left,right=right))
    gates=[p.lt(left,p.const(10)),p.lt(right,p.const(10)),p.bit_all(x['flags'],15)]
    gates += [p.eq(x[FIELDS[i]],x[FIELDS[i+1]]) for i in range(8,18,2)]
    selected=p.all(*gates)
    p.name_cell('selector',dict(eligible=selected))
    roots=dict(id=x['id'],bopLeft=bl,left=left,bopRight=br,right=right,eligible=selected)
    p.name_cell('CICM',roots)
    return p,roots


def lower_statements(p,roots,language):
    """Emit only the observation's dependency slice, with uint32 overflow proof.

    The general evaluator accepts signed integers; this backend deliberately
    supports nonnegative inputs <=2^30 and only provably safe intermediates.
    """
    assert language in ['swift','metal']
    upper={};lines=[]
    def number(n):
        if not 0<=n<2**32:raise ValueError('unsigned backend cannot encode constant')
        return f'UInt32({n})' if language=='swift' else f'{n}u'
    def fold(name,args):
        out=args[0]
        for arg in args[1:]:out=f'{name}({out}, {arg})'
        return out
    for i in p.reachable_topological(list(roots.values())):
        n=p.nodes[i];args=[f'v{j}' for j in n.args]
        bounds=[upper[j] for j in n.args]
        if n.op=='input':
            if n.kind!='int':raise ValueError('batch ABI only accepts integer input slots')
            index=FIELDS.index(n.value)
            expression=f'inputs[i + {index}]' if language=='swift' else f'p[{index}]'
            bound=2**30
        elif n.op=='const':
            expression=str(n.value).lower() if n.kind=='bool' else number(n.value)
            bound=int(n.value)
        elif n.op=='add':expression=' + '.join(args);bound=sum(bounds)
        elif n.op=='scale':expression=f'{number(n.value)} * {args[0]}';bound=n.value*bounds[0]
        elif n.op=='min':expression=fold('min',args);bound=min(bounds)
        elif n.op in ['eq','lt']:
            expression=f'{args[0]} {"==" if n.op=="eq" else "<"} {args[1]}';bound=1
        elif n.op=='and':expression=' && '.join(args);bound=1
        elif n.op=='bit_all':expression=f'({args[0]} & {number(n.value)}) == {number(n.value)}';bound=1
        else:raise ValueError(f'unsupported operator {n.op}')
        if n.kind=='int' and not 0<=bound<2**32:
            raise ValueError(f'node {i} can overflow uint32')
        upper[i]=bound
        ty='Bool' if n.kind=='bool' else 'UInt32'
        lines.append(f'let v{i}: {ty} = {expression}' if language=='swift' else
                     f'{"bool" if n.kind=="bool" else "uint"} v{i} = {expression};')
    for column,node in enumerate(roots.values()):
        expression=f'v{node}'
        if p.nodes[node].kind=='bool':expression=f'{expression} ? {number(1)} : {number(0)}'
        lines.append(f'outputs[o + {column}] = {expression}' if language=='swift' else f'o[{column}] = {expression};')
    return '\n'.join(lines)


def metal_source(p,roots):
    body=lower_statements(p,roots,'metal')
    return '''#include <metal_stdlib>
using namespace metal;
kernel void cell_kernel(device const uint* input [[buffer(0)]],
                        device uint* output [[buffer(1)]],
                        constant uint& count [[buffer(2)]],
                        uint id [[thread_position_in_grid]]) {
    if (id >= count) return;
    device const uint* p = input + id * 18;
    device uint* o = output + id * OUTPUT_WIDTH;
'''.replace('OUTPUT_WIDTH',str(len(roots)))+body+'\n}\n'


def swift_source(template,p,roots):
    begin=template.index('        // CELL_BODY_BEGIN')
    end=template.index('        // CELL_BODY_END',begin)
    body='''        // CELL_BODY_BEGIN
        for row in 0..<count {
            let i = row * inputWidth
            let o = row * outputWidth
'''+ '\n'.join('            '+line for line in lower_statements(p,roots,'swift').splitlines())+'\n        }\n'
    result=template[:begin]+body+template[end:]
    return result.replace('private let outputWidth = 6',f'private let outputWidth = {len(roots)}')
