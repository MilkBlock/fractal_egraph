"""Small linear-MIP interface: explicit backend, no fallback or search budget.

SCIP frees its transformed problem before edits after a solve. Original variable
objects remain valid; solution access is forbidden until the next optimal solve.
API build timers exclude expression construction and Python pricing work.
"""
import importlib
import os
from pathlib import Path
import time

BINARY='B'
OPTIMAL='optimal'
_backend=os.environ.get('EGG_LAYOUT_SOLVER','scip')
_stats={}


def select(name):
    global _backend
    if name not in ('scip','gurobi'):raise ValueError(f'unknown solver: {name}')
    _backend=name
    _stats.clear()


def module():
    name='pyscipopt' if _backend=='scip' else 'gurobipy'
    try:return importlib.import_module(name)
    except ImportError as e:
        raise RuntimeError(f'{name} is required for --solver {_backend}; install the corresponding requirements file; no fallback was attempted') from e


def information():
    mod=module()
    return dict(backend=_backend,python_package_version=mod.__version__,
                optimality='solver OPTIMAL required; relative and absolute gaps zero; numerical tolerances apply',threads=1)


def statistics():return dict(information(),models={k:dict(v) for k,v in _stats.items()})


def quicksum(values):return module().quicksum(values)


class Model:
    def __init__(self,name='model'):
        tick=time.perf_counter();self.backend=_backend;self.ModelName=name
        self.api=module();self.native=self.api.Model(name);self.vars={};self.NumVars=0;self.NumBinVars=0;self.NumConstrs=0
        self.Status='not_solved';self._solved=False;self.solve_seconds=0.;self.solve_calls=0
        self.nonoptimal_solves=0
        if self.backend=='scip':
            self.native.hideOutput()
            self.native.setRealParam('limits/gap',0.)
            self.native.setRealParam('limits/absgap',0.)
            self.native.setIntParam('parallel/maxnthreads',1)
            self.native.setIntParam('lp/threads',1)
            self.version='.'.join(str(f()) for f in (self.native.getMajorVersion,self.native.getMinorVersion,self.native.getTechVersion))
        else:
            self.native.Params.OutputFlag=0;self.native.Params.MIPGap=0;self.native.Params.MIPGapAbs=0;self.native.Params.Threads=1
            self.version='.'.join(map(str,self.api.gurobi.version()))
        self.api_build_seconds=time.perf_counter()-tick

    def __enter__(self):return self
    def __exit__(self,*exc):
        r=_stats.setdefault(self.ModelName,dict(models=0,solve_calls=0,api_build_seconds=0.,solve_seconds=0.,
            peak_variables=0,peak_constraints=0,engine_version=self.version,nonoptimal_solves=0))
        r['models']+=1;r['solve_calls']+=self.solve_calls
        r['api_build_seconds']+=self.api_build_seconds;r['solve_seconds']+=self.solve_seconds
        r['peak_variables']=max(r['peak_variables'],self.NumVars);r['peak_constraints']=max(r['peak_constraints'],self.NumConstrs)
        r['nonoptimal_solves']+=self.nonoptimal_solves
        if self.backend=='scip':self.native.freeProb()
        else:self.native.dispose()
        self.Status='closed'
        for name in ('ObjVal','ObjBound'):
            if hasattr(self,name):delattr(self,name)

    def _edit(self):
        if self._solved and self.backend=='scip':self.native.freeTransform()
        self._solved=False;self.Status='not_solved'
        for name in ('ObjVal','ObjBound'):
            if hasattr(self,name):delattr(self,name)

    def addVar(self,*,name='',vtype='C',lb=0.,ub=None):
        tick=time.perf_counter();self._edit()
        name=name or f'v{self.NumVars}'
        if name in self.vars:raise ValueError(f'duplicate variable name {name}')
        if ub is None:ub=1. if vtype==BINARY else None
        if self.backend=='scip':v=self.native.addVar(name=name,vtype=vtype,lb=lb,ub=ub)
        else:v=self.native.addVar(name=name,vtype=vtype,lb=lb,ub=ub if ub is not None else self.api.GRB.INFINITY)
        self.vars[name]=v;self.NumVars+=1;self.NumBinVars+=int(vtype==BINARY)
        self.api_build_seconds+=time.perf_counter()-tick
        return v

    def addConstr(self,expr,name=''):
        tick=time.perf_counter();self._edit()
        c=self.native.addCons(expr,name=name) if self.backend=='scip' else self.native.addConstr(expr,name=name)
        self.NumConstrs+=1;self.api_build_seconds+=time.perf_counter()-tick
        return c

    def setObjective(self,expr):
        tick=time.perf_counter();self._edit();self.native.setObjective(expr)
        self.api_build_seconds+=time.perf_counter()-tick

    def getVarByName(self,name):return self.vars[name]

    def fix(self,var,value):
        tick=time.perf_counter();self._edit()
        if self.backend=='scip':
            self.native.chgVarLb(var,value);self.native.chgVarUb(var,value)
        else:var.LB=var.UB=value
        self.api_build_seconds+=time.perf_counter()-tick

    def optimize(self):
        if self.backend=='gurobi':
            tick=time.perf_counter();self.native.update();self.api_build_seconds+=time.perf_counter()-tick
        tick=time.perf_counter()
        try:self.native.optimize()
        except BaseException:
            self.Status='error';self.nonoptimal_solves+=1
            raise
        finally:self.solve_seconds+=time.perf_counter()-tick;self.solve_calls+=1
        self._solved=True
        self.Status=str(self.native.getStatus()) if self.backend=='scip' else (OPTIMAL if self.native.Status==self.api.GRB.OPTIMAL else f'gurobi:{self.native.Status}')
        self.nonoptimal_solves+=int(self.Status!=OPTIMAL)
        if self.Status==OPTIMAL:
            self.ObjVal=self.native.getObjVal() if self.backend=='scip' else self.native.ObjVal
            self.ObjBound=self.native.getDualbound() if self.backend=='scip' else self.native.ObjBound

    def value(self,var):
        if self.Status!=OPTIMAL:raise RuntimeError(f'no optimal solution: {self.Status}')
        return self.native.getSolVal(self.native.getBestSol(),var) if self.backend=='scip' else var.X

    def write(self,path):
        if self.backend=='gurobi':self.native.write(str(path));return
        if Path(path).suffix=='.sol':
            if self.Status!=OPTIMAL:raise RuntimeError('cannot export a non-optimal solution')
            self.native.writeBestSol(str(path),write_zeros=True)
        else:self.native.writeProblem(str(path),trans=False,verbose=False)
