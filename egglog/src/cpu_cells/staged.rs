//! Stage-gated local RHS patches. Native queries authorize every publication.
//! Preparation never allocates global identities or changes source tables.
use super::*;
use egglog_bridge::{TableAction, UnionAction};
use std::sync::Mutex;

type Ref = usize;
#[derive(Clone)]
enum Arg {
    Slot(usize),
    Const(Value, ColumnTy),
}
#[derive(Clone)]
enum Op {
    Copy(usize, Arg),
    Construct(usize, usize, Vec<Arg>),
    Read(usize, usize, Vec<Arg>),
    Compute(usize, ExternalFunctionId, ColumnTy, Vec<Arg>),
    Set(usize, Vec<Arg>, Arg),
    Union(Arg, Arg),
}
#[derive(Clone)]
enum Merge {
    Old,
    New,
    Const(Value, ColumnTy),
    Call(usize, Vec<Merge>),
    Compute(ExternalFunctionId, ColumnTy, Vec<Merge>),
    Union,
    Unknown,
}
struct Table {
    name: String,
    id: FunctionId,
    action: TableAction,
    schema: Vec<ColumnTy>,
    merge: Merge,
}
struct Program {
    id: RuleId,
    name: String,
    inputs: Vec<ResolvedAtomTerm>,
    key_inputs: Vec<usize>,
    types: Vec<ColumnTy>,
    ops: Vec<Op>,
    ports: Vec<(usize, usize, Vec<Arg>)>,
    guards: Vec<(usize, ExternalFunctionId, ColumnTy, Vec<Arg>)>,
}
#[derive(Clone)]
enum Node {
    Value(Value, ColumnTy),
    Unknown(ColumnTy),
    Construct(usize, Vec<Ref>, Option<Value>),
    Read(usize, Vec<Ref>),
    Compute(
        ExternalFunctionId,
        ColumnTy,
        Vec<Ref>,
        Option<(Vec<Value>, Value)>,
    ),
}
#[derive(Clone)]
enum Effect {
    Construct(Ref),
    Compute(Ref),
    Read(Ref),
    Set(usize, Vec<Ref>, Ref),
    Union(Ref, Ref),
}
#[derive(Clone)]
enum Link {
    Slot(usize),
    Const(Value, ColumnTy),
}
struct Step {
    program: usize,
    mapping: Vec<Link>,
    receipts: Vec<(usize, usize, usize)>,
}
struct Template {
    slots: Vec<ColumnTy>,
    steps: Vec<Step>,
}
#[derive(Clone)]
struct Patch {
    program: usize,
    inputs: Vec<Ref>,
    effects: Vec<Effect>,
    done: bool,
}
struct Frame {
    nodes: Vec<Node>,
    patches: Vec<Patch>,
    born: u64,
}
#[derive(Default)]
struct Counters {
    native_matches: usize,
    prepared_steps: usize,
    reused_steps: usize,
    fresh_steps: usize,
    local_nodes: usize,
    materializations: usize,
    numeric_reuse: usize,
    sets: usize,
    ordered_conflicts: usize,
    preview_resolved: usize,
    preview_unresolved: usize,
    peak_frames: usize,
    live_nodes: usize,
    peak_nodes: usize,
    retired_frames: usize,
}
struct Write {
    table: usize,
    key: Vec<Value>,
    proposed: Value,
    before: Option<Ref>,
    preview: Ref,
    ordinal: usize,
}
#[derive(Default)]
struct Overlay {
    nodes: Vec<Node>,
    latest: BTreeMap<(usize, Vec<Value>), Ref>,
    writes: Vec<Write>,
}
struct Runtime {
    programs: Vec<Program>,
    tables: Vec<Table>,
    templates: BTreeMap<usize, Template>,
    frames: Vec<Option<Frame>>,
    ready: BTreeMap<(usize, Vec<Value>), Vec<(usize, usize)>>,
    unions: UnionAction,
    overlay: Overlay,
    counters: Counters,
    stage: u64,
    current: J,
    events: Vec<J>,
    audit: bool,
}
/// A graph-bound CPU runtime using native matching, hashcons, merge and rebuild.
/// Local patches are prepared early but only a native match in the current Run
/// may publish them. This first staged path does not claim to bypass Free Join.
pub struct StagedCpuCells {
    runtime: Arc<Mutex<Runtime>>,
    variants: BTreeMap<RuleId, RuleId>,
    rulesets: BTreeMap<String, Vec<RuleId>>,
    executions: usize,
    archive: Option<super::archive::StageArchive>,
}

fn column(eg: &EGraph, s: &J) -> Result<ColumnTy> {
    let sort = eg
        .type_info
        .get_sort_by_name(string(&s["name"])?)
        .ok_or_else(|| err("staged slot sort"))?;
    if sort.is_container_sort() {
        return Err(err("container patch remains native"));
    }
    Ok(sort.column_ty(&eg.backend))
}
fn literal(eg: &EGraph, j: &J) -> Result<(Value, ColumnTy)> {
    let l = match string(&j["sort"]["name"])? {
        "i64" => Literal::Int(
            j["literal"]
                .as_i64()
                .ok_or_else(|| err("integer literal"))?,
        ),
        "String" => Literal::String(string(&j["literal"])?.into()),
        "Unit" => Literal::Unit,
        "bool" => Literal::Bool(match j["literal"].as_i64() {
            Some(0) => false,
            Some(1) => true,
            _ => return Err(err("boolean literal")),
        }),
        _ => return Err(err("unsupported patch literal")),
    };
    Ok((literal_to_value(&eg.backend, &l), column(eg, &j["sort"])?))
}
fn pure(p: &core::SpecializedPrimitive) -> bool {
    p.supports_context(crate::Context::Pure)
        && p.input()
            .iter()
            .chain([p.output()])
            .all(|s| matches!(s.name(), "i64" | "bool" | "Unit"))
        && matches!(
            p.name(),
            "+" | "-"
                | "*"
                | "/"
                | "%"
                | "min"
                | "max"
                | "<"
                | ">"
                | "<="
                | ">="
                | "!="
                | "not"
                | "and"
                | "or"
                | "xor"
                | "=>"
                | "bool-="
                | "bool-<"
                | "bool->"
                | "bool-<="
                | "bool->="
        )
}
fn merge_expr(eg: &EGraph, e: &ResolvedExpr, tids: &BTreeMap<String, usize>) -> Merge {
    match e {
        GenericExpr::Var(_, v) if v.name == "old" => Merge::Old,
        GenericExpr::Var(_, v) if v.name == "new" => Merge::New,
        GenericExpr::Lit(_, l) => Merge::Const(
            literal_to_value(&eg.backend, l),
            e.output_type().column_ty(&eg.backend),
        ),
        GenericExpr::Call(_, ResolvedCall::Func(f), aa)
            if eg.functions[&f.name].is_constructor() =>
        {
            Merge::Call(
                tids[&f.name],
                aa.iter().map(|e| merge_expr(eg, e, tids)).collect(),
            )
        }
        GenericExpr::Call(_, ResolvedCall::Primitive(p), aa) if pure(p) => Merge::Compute(
            p.external_id(crate::Context::Write),
            p.output().column_ty(&eg.backend),
            aa.iter().map(|e| merge_expr(eg, e, tids)).collect(),
        ),
        _ => Merge::Unknown,
    }
}
fn compile_program(
    eg: &EGraph,
    r: &ResolvedCoreRule,
    id: RuleId,
    spec: &J,
    tids: &BTreeMap<String, usize>,
) -> Result<Program> {
    let slots = array(&spec["slots"])?;
    let names = slots
        .iter()
        .enumerate()
        .map(|(i, s)| Ok((string(&s["name"])?.to_owned(), i)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let types = slots
        .iter()
        .map(|s| column(eg, &s["sort"]))
        .collect::<Result<Vec<_>>>()?;
    let mut native_variables = BTreeMap::new();
    for a in &r.body.atoms {
        for t in &a.args {
            if let T::Var(_, v) = t {
                native_variables.insert(v.name.clone(), v.sort.name().to_owned());
            }
        }
    }
    for a in &r.head.0 {
        if let A::Let(_, v, ..) | A::LetAtomTerm(_, v, ..) = a {
            native_variables.insert(v.name.clone(), v.sort.name().to_owned());
        }
    }
    if names.len() != slots.len()
        || native_variables.len() != slots.len()
        || slots.iter().any(|s| {
            native_variables
                .get(s["name"].as_str().unwrap_or(""))
                .map(String::as_str)
                != s["sort"]["name"].as_str()
        })
    {
        return Err(err("patch variable layout differs from native source"));
    }
    let arg = |t: &ResolvedAtomTerm| -> Result<Arg> {
        match t {
            T::Var(_, v) => Ok(Arg::Slot(
                *names
                    .get(&v.name)
                    .ok_or_else(|| err("missing variable slot"))?,
            )),
            T::Literal(_, l) => Ok(Arg::Const(
                literal_to_value(&eg.backend, l),
                t.output().column_ty(&eg.backend),
            )),
            _ => Err(err("unresolved global")),
        }
    };
    let mut query_vars = BTreeMap::new();
    for a in &r.body.atoms {
        for x in &a.args {
            if let T::Var(_, v) = x {
                query_vars.insert(v.name.clone(), x.clone());
            }
        }
    }
    let mut inputs = vec![];
    for s in slots.iter().take_while(|s| s["definition"] == "query") {
        inputs.push(
            query_vars
                .remove(string(&s["name"])?)
                .ok_or_else(|| err("query slot is not native binding"))?,
        );
    }
    if !query_vars.is_empty() {
        return Err(err("incomplete native query binding"));
    }
    let key_inputs = r
        .body
        .atoms
        .iter()
        .filter(|a| matches!(a.head, ResolvedCall::Func(_)))
        .flat_map(|a| a.args.iter())
        .filter_map(|t| {
            if let T::Var(_, v) = t {
                Some(names[&v.name])
            } else {
                None
            }
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut ports = vec![];
    for atom in &r.body.atoms {
        if let ResolvedCall::Func(f) = &atom.head {
            if !eg.functions[&f.name].is_constructor() {
                if let T::Var(_, v) = atom.args.last().unwrap() {
                    ports.push((
                        names[&v.name],
                        tids[&f.name],
                        atom.args[..atom.args.len() - 1]
                            .iter()
                            .map(arg)
                            .collect::<Result<_>>()?,
                    ));
                }
            }
        }
    }
    let mut guards = vec![];
    for a in &r.body.atoms {
        if let ResolvedCall::Primitive(p) = &a.head {
            if !pure(p) {
                return Err(err("opaque query primitive remains native"));
            }
            if let T::Var(_, v) = a.args.last().unwrap() {
                guards.push((
                    names[&v.name],
                    p.external_id(crate::Context::Read),
                    p.output().column_ty(&eg.backend),
                    a.args[..a.args.len() - 1]
                        .iter()
                        .map(arg)
                        .collect::<Result<_>>()?,
                ));
            }
        }
    }
    let mut ops = vec![];
    for a in &r.head.0 {
        ops.push(match a {
            A::LetAtomTerm(_, v, x) => Op::Copy(names[&v.name], arg(x)?),
            A::Let(_, v, ResolvedCall::Func(f), aa) => {
                let args = aa.iter().map(arg).collect::<Result<_>>()?;
                if eg.functions[&f.name].is_constructor() {
                    Op::Construct(names[&v.name], tids[&f.name], args)
                } else {
                    Op::Read(names[&v.name], tids[&f.name], args)
                }
            }
            A::Let(_, v, ResolvedCall::Primitive(p), aa) if pure(p) => Op::Compute(
                names[&v.name],
                p.external_id(crate::Context::Write),
                p.output().column_ty(&eg.backend),
                aa.iter().map(arg).collect::<Result<_>>()?,
            ),
            A::Set(_, ResolvedCall::Func(f), aa, v) => Op::Set(
                tids[&f.name],
                aa.iter().map(arg).collect::<Result<_>>()?,
                arg(v)?,
            ),
            A::Union(_, a, b) => Op::Union(arg(a)?, arg(b)?),
            _ => return Err(err("partial/opaque action remains native")),
        });
    }
    Ok(Program {
        id,
        name: string(&spec["metadata"]["name"])?.into(),
        inputs,
        key_inputs,
        types,
        ops,
        guards,
        ports,
    })
}
fn push(nodes: &mut Vec<Node>, n: Node) -> Ref {
    let i = nodes.len();
    nodes.push(n);
    i
}
fn arg_ref(nodes: &mut Vec<Node>, slots: &[Ref], a: &Arg) -> Ref {
    match a {
        Arg::Slot(i) => slots[*i],
        Arg::Const(v, t) => push(nodes, Node::Value(*v, *t)),
    }
}
fn node_type(n: &Node, tables: &[Table]) -> ColumnTy {
    match n {
        Node::Value(_, t) | Node::Unknown(t) | Node::Compute(_, t, ..) => *t,
        Node::Construct(t, ..) | Node::Read(t, _) => *tables[*t].schema.last().unwrap(),
    }
}
fn resolved_all(nodes: &[Node], eg: &EGraph, tables: &[Table]) -> Vec<Option<Value>> {
    let mut values: Vec<Option<Value>> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let v = match node {
            Node::Value(v, _) => Some(*v),
            Node::Unknown(_) => None,
            Node::Construct(t, aa, _) | Node::Read(t, aa) => aa
                .iter()
                .map(|i| values.get(*i).copied().flatten())
                .collect::<Option<Vec<_>>>()
                .and_then(|args| eg.backend.lookup_id(tables[*t].id, &args)),
            Node::Compute(_, _, aa, known) => aa
                .iter()
                .map(|i| values.get(*i).copied().flatten())
                .collect::<Option<Vec<_>>>()
                .and_then(|args| {
                    known
                        .as_ref()
                        .filter(|(old, _)| *old == args)
                        .map(|(_, v)| *v)
                }),
        };
        values.push(v.map(|v| eg.backend.get_canon_repr(v, node_type(node, tables))));
    }
    values
}
fn in_state(nodes: &[Node], i: Ref, state: &ExecutionState, tables: &[Table]) -> Option<Value> {
    match &nodes[i] {
        Node::Value(v, _) => Some(*v),
        Node::Unknown(_) => None,
        Node::Construct(_, _, Some(v)) => Some(*v),
        Node::Construct(t, aa, None) | Node::Read(t, aa) => {
            let args = aa
                .iter()
                .map(|i| in_state(nodes, *i, state, tables))
                .collect::<Option<Vec<_>>>()?;
            tables[*t].action.lookup(state, &args)
        }
        Node::Compute(_, _, aa, known) => {
            let args = aa
                .iter()
                .map(|i| in_state(nodes, *i, state, tables))
                .collect::<Option<Vec<_>>>()?;
            known
                .as_ref()
                .filter(|(old, _)| *old == args)
                .map(|(_, v)| *v)
        }
    }
}
fn materialize(
    nodes: &mut Vec<Node>,
    i: Ref,
    state: &mut ExecutionState,
    tables: &[Table],
    counts: &mut Counters,
) -> Option<Value> {
    match nodes[i].clone() {
        Node::Value(v, _) => Some(v),
        Node::Unknown(_) => None,
        Node::Construct(_, _, Some(v)) => Some(v),
        Node::Construct(t, aa, None) => {
            let args = aa
                .iter()
                .map(|i| materialize(nodes, *i, state, tables, counts))
                .collect::<Option<Vec<_>>>()?;
            let v = tables[t].action.lookup_or_insert(state, &args)?;
            nodes[i] = Node::Construct(t, aa, Some(v));
            counts.materializations += 1;
            Some(v)
        }
        Node::Read(t, aa) => {
            let args = aa
                .iter()
                .map(|i| materialize(nodes, *i, state, tables, counts))
                .collect::<Option<Vec<_>>>()?;
            tables[t].action.lookup(state, &args)
        }
        Node::Compute(p, ty, aa, old) => {
            let args = aa
                .iter()
                .map(|i| materialize(nodes, *i, state, tables, counts))
                .collect::<Option<Vec<_>>>()?;
            let v = if let Some((before, v)) = old.filter(|(a, _)| *a == args) {
                let _ = before;
                counts.numeric_reuse += 1;
                v
            } else {
                state.call_external_func(p, &args)?
            };
            nodes[i] = Node::Compute(p, ty, aa, Some((args, v)));
            Some(v)
        }
    }
}
impl Frame {
    fn step(&mut self, pid: usize, program: &Program, mut slots: Vec<Ref>) -> (usize, Vec<Ref>) {
        // Each stage owns a distinct function read port. Consuming B's old
        // value must not freeze C's future read of the same function key.
        for (dst, t, args) in &program.ports {
            let aa = args
                .iter()
                .map(|a| arg_ref(&mut self.nodes, &slots, a))
                .collect();
            slots[*dst] = push(&mut self.nodes, Node::Read(*t, aa));
        }
        for (dst, p, ty, args) in &program.guards {
            if matches!(self.nodes[slots[*dst]], Node::Unknown(_)) {
                let aa = args
                    .iter()
                    .map(|a| arg_ref(&mut self.nodes, &slots, a))
                    .collect();
                slots[*dst] = push(&mut self.nodes, Node::Compute(*p, *ty, aa, None));
            }
        }
        let inputs = slots[..program.inputs.len()].to_vec();
        let mut effects = vec![];
        for op in &program.ops {
            match op {
                Op::Copy(dst, a) => slots[*dst] = arg_ref(&mut self.nodes, &slots, a),
                Op::Construct(dst, t, args) | Op::Read(dst, t, args) => {
                    let aa = args
                        .iter()
                        .map(|a| arg_ref(&mut self.nodes, &slots, a))
                        .collect();
                    let n = if matches!(op, Op::Construct(..)) {
                        Node::Construct(*t, aa, None)
                    } else {
                        Node::Read(*t, aa)
                    };
                    slots[*dst] = push(&mut self.nodes, n);
                    effects.push(if matches!(op, Op::Construct(..)) {
                        Effect::Construct(slots[*dst])
                    } else {
                        Effect::Read(slots[*dst])
                    });
                }
                Op::Compute(dst, p, ty, args) => {
                    let aa = args
                        .iter()
                        .map(|a| arg_ref(&mut self.nodes, &slots, a))
                        .collect();
                    slots[*dst] = push(&mut self.nodes, Node::Compute(*p, *ty, aa, None));
                    effects.push(Effect::Compute(slots[*dst]));
                }
                Op::Set(t, args, v) => {
                    let aa = args
                        .iter()
                        .map(|a| arg_ref(&mut self.nodes, &slots, a))
                        .collect();
                    let value = arg_ref(&mut self.nodes, &slots, v);
                    effects.push(Effect::Set(*t, aa, value));
                }
                Op::Union(a, b) => {
                    let a = arg_ref(&mut self.nodes, &slots, a);
                    let b = arg_ref(&mut self.nodes, &slots, b);
                    effects.push(Effect::Union(a, b));
                }
            }
        }
        let index = self.patches.len();
        self.patches.push(Patch {
            program: pid,
            inputs,
            effects,
            done: false,
        });
        (index, slots)
    }
}
impl Overlay {
    fn expression(&mut self, m: &Merge, old: Ref, new: Ref, tables: &[Table]) -> Ref {
        match m {
            Merge::Old => old,
            Merge::New => new,
            Merge::Const(v, t) => push(&mut self.nodes, Node::Value(*v, *t)),
            Merge::Call(t, args) => {
                let aa = args
                    .iter()
                    .map(|a| self.expression(a, old, new, tables))
                    .collect();
                push(&mut self.nodes, Node::Construct(*t, aa, None))
            }
            Merge::Compute(p, ty, args) => {
                let aa = args
                    .iter()
                    .map(|a| self.expression(a, old, new, tables))
                    .collect();
                push(&mut self.nodes, Node::Compute(*p, *ty, aa, None))
            }
            Merge::Union => new, // The proposed identity is canonicalized after native union.
            Merge::Unknown => {
                let ty = node_type(&self.nodes[new], tables);
                push(&mut self.nodes, Node::Unknown(ty))
            }
        }
    }
    fn set(
        &mut self,
        t: usize,
        key: Vec<Value>,
        new: Value,
        state: &mut ExecutionState,
        tables: &[Table],
        counts: &mut Counters,
    ) {
        let start = self.nodes.len();
        let ty = *tables[t].schema.last().unwrap();
        let old = self.latest.get(&(t, key.clone())).copied().or_else(|| {
            tables[t]
                .action
                .lookup(state, &key)
                .map(|v| push(&mut self.nodes, Node::Value(v, ty)))
        });
        let proposed = push(&mut self.nodes, Node::Value(new, ty));
        let result = old
            .map(|old| {
                counts.ordered_conflicts += 1;
                if matches!(tables[t].merge, Merge::Call(..))
                    && in_state(&self.nodes, old, state, tables) == Some(new)
                {
                    old
                } else {
                    self.expression(&tables[t].merge, old, proposed, tables)
                }
            })
            .unwrap_or(proposed);
        // Evaluate only pure numeric preview nodes. Constructors remain local.
        for i in start..self.nodes.len() {
            if let Node::Compute(p, ty, aa, None) = self.nodes[i].clone() {
                if let Some(args) = aa
                    .iter()
                    .map(|i| in_state(&self.nodes, *i, state, tables))
                    .collect::<Option<Vec<_>>>()
                {
                    if let Some(v) = state.call_external_func(p, &args) {
                        self.nodes[i] = Node::Compute(p, ty, aa, Some((args, v)));
                    }
                }
            }
        }
        self.latest.insert((t, key.clone()), result);
        self.writes.push(Write {
            table: t,
            key,
            proposed: new,
            before: old,
            preview: result,
            ordinal: self.writes.len(),
        });
    }
}
impl Runtime {
    fn frame(&self, pid: usize, values: &[Value]) -> Frame {
        let p = &self.programs[pid];
        let mut f = Frame {
            nodes: vec![],
            patches: vec![],
            born: self.stage,
        };
        if let Some(t) = self.templates.get(&pid) {
            let mut globals = t
                .slots
                .iter()
                .map(|ty| push(&mut f.nodes, Node::Unknown(*ty)))
                .collect::<Vec<_>>();
            for (i, v) in values.iter().enumerate() {
                globals[i] = push(&mut f.nodes, Node::Value(*v, p.types[i]));
            }
            for step in &t.steps {
                let slots = step
                    .mapping
                    .iter()
                    .map(|m| match m {
                        Link::Slot(i) => globals[*i],
                        Link::Const(v, ty) => push(&mut f.nodes, Node::Value(*v, *ty)),
                    })
                    .collect();
                let (pi, out) = f.step(step.program, &self.programs[step.program], slots);
                for (mapping, value) in step.mapping.iter().zip(out) {
                    if let Link::Slot(i) = mapping {
                        globals[*i] = value;
                    }
                }
                for (effect, before, actual) in &step.receipts {
                    if let Effect::Set(table, key, _) = &f.patches[pi].effects[*effect] {
                        // A function reply is a port, never its proposed value.
                        let key = key.clone();
                        let table = *table;
                        globals[*before] = push(
                            &mut f.nodes,
                            Node::Unknown(*self.tables[table].schema.last().unwrap()),
                        );
                        globals[*actual] = push(&mut f.nodes, Node::Read(table, key));
                    }
                }
            }
        } else {
            let slots = p
                .types
                .iter()
                .enumerate()
                .map(|(i, t)| {
                    push(
                        &mut f.nodes,
                        if i < values.len() {
                            Node::Value(values[i], *t)
                        } else {
                            Node::Unknown(*t)
                        },
                    )
                })
                .collect();
            f.step(pid, p, slots);
        }
        f
    }
    fn apply(&mut self, pid: usize, values: &[Value], state: &mut ExecutionState) -> Option<()> {
        self.counters.native_matches += 1;
        let key = self.programs[pid]
            .key_inputs
            .iter()
            .map(|i| values[*i])
            .collect::<Vec<_>>();
        let cached = loop {
            let next = self
                .ready
                .get_mut(&(pid, key.clone()))
                .and_then(|x| x.pop());
            match next {
                Some((fi, si))
                    if self
                        .frames
                        .get(fi)
                        .and_then(Option::as_ref)
                        .is_some_and(|f| !f.patches[si].done && f.patches[si].program == pid) =>
                {
                    break Some((fi, si));
                }
                Some(_) => continue,
                None => break None,
            }
        };
        let (mut frame, step, slot) = if let Some((fi, si)) = cached {
            let frame = self.frames[fi].take()?;
            self.counters.live_nodes -= frame.nodes.len();
            self.counters.reused_steps += 1;
            (frame, si, Some(fi))
        } else {
            self.counters.fresh_steps += 1;
            let frame = self.frame(pid, values);
            self.counters.prepared_steps += frame.patches.len().saturating_sub(1);
            self.counters.local_nodes += frame.nodes.len();
            (frame, 0, None)
        };
        // Exact native binding is the activation authority, including guards.
        let patch = frame.patches[step].clone();
        for (i, (r, v)) in patch.inputs.iter().zip(values).enumerate() {
            frame.nodes[*r] = Node::Value(*v, self.programs[pid].types[i]);
        }
        for effect in &patch.effects {
            match effect {
                Effect::Construct(i) | Effect::Compute(i) | Effect::Read(i) => {
                    materialize(
                        &mut frame.nodes,
                        *i,
                        state,
                        &self.tables,
                        &mut self.counters,
                    )?;
                }
                Effect::Set(t, aa, value) => {
                    let key = aa
                        .iter()
                        .map(|i| {
                            materialize(
                                &mut frame.nodes,
                                *i,
                                state,
                                &self.tables,
                                &mut self.counters,
                            )
                        })
                        .collect::<Option<Vec<_>>>()?;
                    let value = materialize(
                        &mut frame.nodes,
                        *value,
                        state,
                        &self.tables,
                        &mut self.counters,
                    )?;
                    self.overlay.set(
                        *t,
                        key.clone(),
                        value,
                        state,
                        &self.tables,
                        &mut self.counters,
                    );
                    self.tables[*t]
                        .action
                        .insert(state, key.into_iter().chain([value]));
                    self.counters.sets += 1;
                }
                Effect::Union(a, b) => {
                    let a = materialize(
                        &mut frame.nodes,
                        *a,
                        state,
                        &self.tables,
                        &mut self.counters,
                    )?;
                    let b = materialize(
                        &mut frame.nodes,
                        *b,
                        state,
                        &self.tables,
                        &mut self.counters,
                    )?;
                    self.unions.union(state, a, b);
                }
            }
        }
        frame.patches[step].done = true;
        // Preparing future expressions is pure: no constructor lookup-or-insert.
        for i in 0..frame.nodes.len() {
            if let Node::Compute(p, ty, aa, None) = frame.nodes[i].clone() {
                if let Some(args) = aa
                    .iter()
                    .map(|i| in_state(&frame.nodes, *i, state, &self.tables))
                    .collect::<Option<Vec<_>>>()
                {
                    if let Some(v) = state.call_external_func(p, &args) {
                        frame.nodes[i] = Node::Compute(p, ty, aa, Some((args, v)));
                    }
                }
            }
        }
        if frame.patches.iter().all(|p| p.done) {
            self.counters.retired_frames += 1;
        } else {
            self.counters.live_nodes += frame.nodes.len();
            self.counters.peak_nodes = self.counters.peak_nodes.max(self.counters.live_nodes);
            let index = slot.unwrap_or_else(|| {
                self.frames.push(None);
                self.frames.len() - 1
            });
            self.frames[index] = Some(frame);
            self.counters.peak_frames = self
                .counters
                .peak_frames
                .max(self.frames.iter().flatten().count());
        }
        Some(())
    }
}
impl StagedCpuCells {
    pub fn compile(eg: &mut EGraph, core: &J, plan: &J, audit: bool) -> Result<Self> {
        if eg.are_proofs_enabled() || eg.program_trace.is_some() || !eg.seminaive {
            return Err(err(
                "staged patches require native seminaive mode without tracing/proofs",
            ));
        }
        if plan["schema"] != "eggfuse-cpu-cells/v1" || plan["core"] != *core {
            return Err(err("staged artifact differs from source"));
        }
        let sets = array(&core["rulesets"])?
            .iter()
            .map(|s| string(&s["name"]).map(str::to_owned))
            .collect::<Result<Vec<_>>>()?;
        let mut actual = eg.export_rule_program(&sets).map_err(err)?;
        actual["schedule"] = core["schedule"].clone();
        if actual != *core {
            return Err(err("staged native rule environment differs"));
        }
        let tids = eg
            .functions
            .keys()
            .enumerate()
            .map(|(i, n)| (n.clone(), i))
            .collect::<BTreeMap<_, _>>();
        let tables = eg
            .functions
            .values()
            .map(|f| Table {
                name: f.decl.name.clone(),
                id: f.backend_id,
                action: TableAction::new(&eg.backend, f.backend_id),
                schema: f
                    .schema
                    .input
                    .iter()
                    .chain([&f.schema.output])
                    .map(|s| s.column_ty(&eg.backend))
                    .collect(),
                merge: if f.is_constructor() {
                    Merge::Union
                } else {
                    f.decl
                        .merge
                        .as_ref()
                        .map(|e| merge_expr(eg, e, &tids))
                        .unwrap_or(Merge::Unknown)
                },
            })
            .collect::<Vec<_>>();
        let mut programs = vec![];
        let mut native = vec![];
        let mut spec_ids = BTreeMap::new();
        for (ri, spec) in array(&plan["recipe"]["rules"])?.iter().enumerate() {
            let set = string(&spec["metadata"]["source_set"])?;
            let name = string(&spec["metadata"]["name"])?;
            let Some(Ruleset::Rules(rs)) = eg.rulesets.get(set) else {
                return Err(err("staged source ruleset"));
            };
            let (r, id) = rs.get(name).ok_or_else(|| err("staged source rule"))?;
            // Query and action semantics come exclusively from the native AST.
            if let Ok(p) = compile_program(eg, r, *id, spec, &tids) {
                spec_ids.insert(ri, programs.len());
                native.push(r.clone());
                programs.push(p);
            }
        }
        let mut templates = BTreeMap::new();
        if let Some(ts) = plan["staged_preparation"]["templates"].as_array() {
            for t in ts {
                let raw = array(&t["steps"])?;
                if raw.is_empty() {
                    continue;
                }
                let Some(&seed) = spec_ids.get(&index(&raw[0]["recipe_rule"])?) else {
                    continue;
                };
                if raw.iter().any(|s| {
                    s["recipe_rule"]
                        .as_u64()
                        .is_none_or(|i| !spec_ids.contains_key(&(i as usize)))
                }) {
                    continue;
                }
                let width = index(&t["input_width"])?;
                if width != programs[seed].inputs.len() {
                    return Err(err("staged seed width"));
                }
                let slots = array(&t["slots"])?
                    .iter()
                    .map(|s| column(eg, &s["sort"]))
                    .collect::<Result<Vec<_>>>()?;
                let mut steps = vec![];
                for (si, s) in raw.iter().enumerate() {
                    let pid = spec_ids[&index(&s["recipe_rule"])?];
                    let p = &programs[pid];
                    let mapping = array(&s["mapping"])?
                        .iter()
                        .map(|v| {
                            if v.get("slot").is_some() {
                                let i = index(&v["slot"])?;
                                if i >= slots.len() {
                                    return Err(err("staged mapping bounds"));
                                }
                                Ok(Link::Slot(i))
                            } else {
                                let (v, ty) = literal(eg, v)?;
                                Ok(Link::Const(v, ty))
                            }
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if mapping.len() != p.types.len() {
                        return Err(err("staged mapping width"));
                    }
                    for (i, m) in mapping.iter().enumerate() {
                        let ty = match m {
                            Link::Slot(i) => slots[*i],
                            Link::Const(_, t) => *t,
                        };
                        if ty != p.types[i] {
                            return Err(err("staged mapping type"));
                        }
                        if si == 0 && i < width && !matches!(m,Link::Slot(j) if *j==i) {
                            return Err(err("staged seed wiring"));
                        }
                    }
                    let mut receipts = vec![];
                    for r in s["receipts"].as_array().into_iter().flatten() {
                        let ei = index(&r["effect"])?;
                        // Receipt positions refer to the native action program;
                        // artifact effect text is never an execution authority.
                        let Some(Op::Set(table, _, _)) = p.ops.get(ei) else {
                            return Err(err("staged receipt effect"));
                        };
                        let effect = p.ops[..ei]
                            .iter()
                            .filter(|e| !matches!(e, Op::Copy(..)))
                            .count();
                        let before = index(&r["before"]["slot"])?;
                        let returned = index(&r["actual"]["slot"])?;
                        if before >= slots.len() || returned >= slots.len() {
                            return Err(err("staged receipt bounds"));
                        }
                        let ty = *tables[*table].schema.last().unwrap();
                        if slots[before] != ty || slots[returned] != ty {
                            return Err(err("staged receipt type"));
                        }
                        receipts.push((effect, before, returned));
                    }
                    steps.push(Step {
                        program: pid,
                        mapping,
                        receipts,
                    });
                }
                if steps.len() > 1 {
                    templates.entry(seed).or_insert(Template { slots, steps });
                }
            }
        }
        let mut rulesets = BTreeMap::new();
        for name in &sets {
            let mut rr = vec![];
            collect(eg, name, &mut rr)?;
            rulesets.insert(name.clone(), rr.iter().map(|r| r.3).collect());
        }
        let runtime = Arc::new(Mutex::new(Runtime {
            programs,
            tables,
            templates,
            frames: vec![],
            ready: BTreeMap::new(),
            unions: UnionAction::new(&eg.backend),
            overlay: Overlay::default(),
            counters: Counters::default(),
            stage: 0,
            current: J::Null,
            events: vec![],
            audit,
        }));
        let mut variants = BTreeMap::new();
        for (pid, r) in native.iter().enumerate() {
            let (id, inputs, name) = {
                let rt = runtime.lock().unwrap();
                let p = &rt.programs[pid];
                (p.id, p.inputs.clone(), p.name.clone())
            };
            let shared = runtime.clone();
            let unit = eg.backend.base_values().get(());
            let func = eg
                .backend
                .register_external_func(Box::new(make_external_func(move |state, values| {
                    shared
                        .lock()
                        .unwrap()
                        .apply(pid, values, state)
                        .map(|_| unit)
                })));
            let seminaive = eg.backend.device_rule_is_seminaive(id);
            let mut tr = BackendRule::new(
                eg.backend
                    .new_rule(&format!("eggfuse.staged.{name}"), seminaive),
                &eg.functions,
                &eg.type_info,
                seminaive,
            );
            tr.query(&r.body, false);
            let args = tr.args(&inputs);
            let ty = tr.rb.egraph().base_values().get_ty::<()>();
            tr.rb
                .call_external_func(func, &args, ColumnTy::Base(ty), || {
                    "staged RHS failed".to_owned()
                });
            variants.insert(id, tr.build());
        }
        Ok(Self {
            runtime,
            variants,
            rulesets,
            executions: 0,
            archive: None,
        })
    }
    fn begin(&mut self, eg: &EGraph, set: &str, path: &[usize], iterations: &[usize]) {
        let mut rt = self.runtime.lock().unwrap();
        rt.stage += 1;
        rt.current = json!({"ordinal":rt.stage,"ruleset":set,"path":path,"iterations":iterations});
        rt.ready.clear();
        rt.overlay = Overlay::default();
        let active = self.rulesets[set].iter().copied().collect::<BTreeSet<_>>();
        let mut ready = BTreeMap::<_, Vec<_>>::new();
        let rt_stage = rt.stage;
        let Runtime {
            frames,
            tables,
            programs,
            ..
        } = &mut *rt;
        for (fi, frame) in frames.iter_mut().enumerate() {
            if let Some(f) = frame {
                let values = resolved_all(&f.nodes, eg, tables);
                for i in 0..f.nodes.len() {
                    if let Node::Value(v, ty) = &mut f.nodes[i] {
                        *v = eg.backend.get_canon_repr(*v, *ty);
                    }
                    if let Node::Construct(t, args, _) = f.nodes[i].clone() {
                        let v = values[i];
                        f.nodes[i] = Node::Construct(t, args, v);
                    }
                }
                for (pi, p) in f.patches.iter().enumerate() {
                    if !p.done && f.born < rt_stage && active.contains(&programs[p.program].id) {
                        if let Some(values) = programs[p.program]
                            .key_inputs
                            .iter()
                            .map(|i| values[p.inputs[*i]])
                            .collect::<Option<Vec<_>>>()
                        {
                            ready.entry((p.program, values)).or_default().push((fi, pi));
                        }
                    }
                }
            }
        }
        rt.ready = ready;
    }
    fn finish(&mut self, eg: &EGraph) -> Result<()> {
        let mut rt = self.runtime.lock().unwrap();
        let mut audit = vec![];
        let mut matches = 0;
        let mut unresolved = 0;
        let preview_values = resolved_all(&rt.overlay.nodes, eg, &rt.tables);
        for w in &rt.overlay.writes {
            let key = w
                .key
                .iter()
                .zip(&rt.tables[w.table].schema)
                .map(|(v, t)| eg.backend.get_canon_repr(*v, *t))
                .collect::<Vec<_>>();
            let actual = eg.backend.lookup_id(rt.tables[w.table].id, &key).map(|v| {
                eg.backend
                    .get_canon_repr(v, *rt.tables[w.table].schema.last().unwrap())
            });
            let preview = preview_values[w.preview];
            // Per-write preview is intermediate, while readback is the final
            // value of that key for this stage. Compare only the last write.
            let last = rt.overlay.latest.get(&(w.table, w.key.clone())) == Some(&w.preview);
            if last {
                if actual.is_some() && actual == preview {
                    matches += 1;
                } else {
                    unresolved += 1;
                }
            }
            if rt.audit {
                audit.push(json!({"ordinal":w.ordinal,"table":rt.tables[w.table].name,"key":key.iter().map(|v|format!("{v:?}")).collect::<Vec<_>>(),"proposed":format!("{:?}",w.proposed),"before_local":w.before,"preview_local":w.preview,"actual":actual.map(|v|format!("{v:?}")),"preview":preview.map(|v|format!("{v:?}")),"last_for_key":last}));
            }
        }
        rt.counters.preview_resolved += matches;
        rt.counters.preview_unresolved += unresolved;
        if rt.audit {
            let nodes=rt.overlay.nodes.iter().enumerate().map(|(i,n)| match n {
                Node::Value(v,_)=>json!({"local":i,"kind":"value","value":format!("{v:?}")}),
                Node::Unknown(_)=>json!({"local":i,"kind":"unresolved"}),
                Node::Construct(t,args,_)=>json!({"local":i,"kind":"constructor","table":rt.tables[*t].name,"args":args}),
                Node::Read(t,args)=>json!({"local":i,"kind":"port","table":rt.tables[*t].name,"args":args}),
                Node::Compute(_,_,args,v)=>json!({"local":i,"kind":"primitive","args":args,"prepared":v.as_ref().map(|(_,v)|format!("{v:?}"))}),
            }).collect::<Vec<_>>();
            let row_counts = eg
                .functions
                .iter()
                .map(|(name, f)| (name.clone(), eg.backend.table_size(f.backend_id)))
                .collect::<BTreeMap<_, _>>();
            let e = json!({"stage":rt.current,"writes":audit,"merge_nodes":nodes,"source_table_rows":row_counts,"source_unions":eg.native_union_count(),"live_frames":rt.frames.iter().flatten().count(),"prepared_steps":rt.counters.prepared_steps,"reused_steps":rt.counters.reused_steps});
            rt.events.push(e);
        }
        // Ready patches not accepted by the native query are stale for this
        // activation. Unready ports wait; none of these hints suppress inference.
        let discard = rt
            .ready
            .values()
            .flat_map(|xs| xs.iter().map(|(f, _)| *f))
            .collect::<BTreeSet<_>>();
        for fi in discard {
            if let Some(f) = rt.frames[fi].take() {
                rt.counters.live_nodes -= f.nodes.len();
                rt.counters.retired_frames += 1;
            }
        }
        rt.ready.clear();
        rt.frames.retain(Option::is_some);
        rt.overlay = Overlay::default();
        Ok(())
    }
    /// Opt-in diagnostic recording. Never enabled by normal inference/bench.
    pub fn archive(&mut self, eg: &EGraph, path: &std::path::Path, core: &J) -> Result<()> {
        self.archive = Some(super::archive::StageArchive::new(eg, path, core)?);
        Ok(())
    }
    pub fn invalidate(&mut self) {
        let mut rt = self.runtime.lock().unwrap();
        rt.counters.retired_frames += rt.frames.iter().flatten().count();
        rt.frames.clear();
        rt.ready.clear();
        rt.counters.live_nodes = 0;
    }
    pub fn run(&mut self, eg: &mut EGraph, s: &Schedule) -> Result<RunReport> {
        self.executions += 1;
        self.walk(eg, s, &mut vec![self.executions - 1], &mut vec![])
    }
    fn walk(
        &mut self,
        eg: &mut EGraph,
        s: &Schedule,
        path: &mut Vec<usize>,
        iterations: &mut Vec<usize>,
    ) -> Result<RunReport> {
        match s {
            Schedule::Run(_, r) => {
                // Native until checks retain their exact entry-snapshot meaning.
                let resolved = eg.resolve_command(Command::RunSchedule(s.clone()))?;
                if let Some(ResolvedNCommand::RunSchedule(ResolvedSchedule::Run(span, cfg))) =
                    resolved.desugared.first()
                {
                    if cfg
                        .until
                        .as_ref()
                        .is_some_and(|f| eg.check_facts(span, f).is_ok())
                    {
                        return Ok(RunReport::default());
                    }
                }
                self.begin(eg, &r.ruleset, path, iterations);
                let stage = if self.archive.is_some() {
                    Some(self.runtime.lock().unwrap().current.clone())
                } else {
                    None
                };
                if let Some(archive) = &mut self.archive {
                    archive.begin(eg, stage.as_ref().unwrap())?;
                }
                let pairs = self.rulesets[&r.ruleset]
                    .iter()
                    .map(|id| (*id, *self.variants.get(id).unwrap_or(id)))
                    .collect::<Vec<_>>();
                let result = eg
                    .backend
                    .run_rule_variants(&pairs, &[])
                    .map_err(|e| err(e.to_string()));
                if result.is_err() {
                    self.invalidate();
                }
                let iteration = result?;
                if let Some(archive) = &mut self.archive {
                    archive.finish(eg, stage.as_ref().unwrap(), &iteration)?;
                }
                self.finish(eg)?;
                let report = RunReport::singleton(&r.ruleset, iteration);
                eg.overall_run_report.union(report.clone());
                Ok(report)
            }
            Schedule::Sequence(_, ss) => {
                let mut report = RunReport::default();
                for (i, s) in ss.iter().enumerate() {
                    path.push(i);
                    let r = self.walk(eg, s, path, iterations)?;
                    path.pop();
                    report.union(r);
                }
                Ok(report)
            }
            Schedule::Repeat(_, n, s) => {
                let mut report = RunReport::default();
                for i in 0..*n {
                    iterations.push(i);
                    let r = self.walk(eg, s, path, iterations)?;
                    iterations.pop();
                    let stop = r.can_stop;
                    report.union(r);
                    if stop {
                        break;
                    }
                }
                Ok(report)
            }
            Schedule::Saturate(_, s) => {
                let mut report = RunReport::default();
                let mut i = 0;
                loop {
                    iterations.push(i);
                    let r = self.walk(eg, s, path, iterations)?;
                    iterations.pop();
                    let stop = !r.updated;
                    report.union(r);
                    if stop {
                        break;
                    }
                    i += 1;
                }
                Ok(report)
            }
        }
    }
    pub fn report(&self) -> J {
        let rt = self.runtime.lock().unwrap();
        let c = &rt.counters;
        json!({"schema":"eggfuse-cpu-stage-patches/v1","execution":"native queries authorize stage-local publication; precomputed patches never authorize matches","compiled_rules":rt.programs.len(),"preparation_templates":rt.templates.len(),"stages":rt.stage,"native_matches":c.native_matches,"prepared_steps":c.prepared_steps,"reused_steps":c.reused_steps,"fresh_steps":c.fresh_steps,"local_nodes_prepared":c.local_nodes,"constructor_requests":c.materializations,"numeric_reuse":c.numeric_reuse,"sets":c.sets,"ordered_conflicts":c.ordered_conflicts,"merge_previews_resolved":c.preview_resolved,"merge_previews_unresolved":c.preview_unresolved,"live_frames":rt.frames.iter().flatten().count(),"live_local_nodes":c.live_nodes,"peak_frames":c.peak_frames,"peak_local_nodes":c.peak_nodes,"retired_frames":c.retired_frames,"stage_events":rt.events})
    }
}
