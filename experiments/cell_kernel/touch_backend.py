"""Capture a pure boundary snapshot; rebuild joint packets from compiled outputs.

This adapter never evaluates the operator DAG. The generated backend is trusted
for arithmetic; freshness, complete output shape and scalar types are checked.
"""
from dataclasses import dataclass
from program import Runtime, _scalar
from touchpoints import Packet


@dataclass(frozen=True)
class TouchRequest:
    plan: object
    instance_key: object
    bindings: tuple

    @classmethod
    def capture(cls,plan,runtime,instance_key):
        if not isinstance(runtime,Runtime) or runtime.program is not plan.program:
            raise ValueError('runtime belongs to a different Program')
        hash(instance_key)
        support=sorted({node for f in plan.fragments for node in f.input_support})
        try:bindings=tuple((node,runtime.cache[node]) for node in support)
        except KeyError as exc:raise ValueError('required boundary input is unbound') from exc
        return cls(plan,instance_key,bindings)

    @property
    def outputs(self):
        return {node:node for node in sorted({node for f in self.plan.fragments for node in f.outputs})}

    def finish(self,runtime,values):
        if not isinstance(runtime,Runtime) or runtime.program is not self.plan.program:
            raise ValueError('runtime belongs to a different Program')
        for node,value in self.bindings:
            now=runtime.cache.get(node)
            if type(now) is not type(value) or now!=value:
                raise ValueError('boundary changed during compiled observation')
        if set(values)!=set(self.outputs):raise ValueError('compiled outputs do not match touch schema')
        for node,value in values.items():_scalar(value,self.plan.program.nodes[node].kind)
        inputs=dict(self.bindings)
        def value(node):
            if node in values:return values[node]
            if node in inputs:return inputs[node]
            op=self.plan.program.nodes[node]
            if op.op=='const':return op.value
            raise ValueError('an imported operator value was not exported by its producer')
        return tuple(Packet(self.plan.program,self.instance_key,f.id,
                            tuple((node,value(node)) for node in f.inputs),
                            tuple((node,value(node)) for node in f.outputs),
                            tuple((node,inputs[node]) for node in f.input_support))
                     for f in self.plan.fragments)
