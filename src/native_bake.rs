//! Offline library learning and frozen, incremental matching. No tier0 mutation
//! is skipped by matching; certified value queries have a separate entry point.
use super::*;
use serde::{Deserialize, Serialize};

#[path = "bake_format.rs"]
mod format;

const VERSION: u32 = 1;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sample {
    name: String,
    source: String,
    rounds: Option<usize>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    samples: Vec<Sample>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Step {
    signature: Json,
    source: String,
    label: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Parameter {
    member: usize,
    input: Json,
}
#[derive(Clone, Serialize, Deserialize)]
struct Contract {
    dag: String,
    parameters: Vec<Option<Parameter>>,
    outputs: Vec<Json>,
    #[serde(default)]
    expressions: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Support {
    sample: String,
    instances: usize,
    max_depth: usize,
    witnesses: Vec<Json>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Template {
    id: String,
    kind: String,
    entry: usize,
    ports: Vec<usize>,
    support: Vec<Support>,
    contract: Option<Contract>,
    array_summary: Option<Json>,
}
#[derive(Serialize, Deserialize)]
struct Library {
    version: u32,
    algebra: String,
    scope: String,
    /// Name of the single datatype the samples declare. Older libraries predate
    /// this field and were all named `Math`.
    #[serde(default = "default_datatype_name")]
    datatype: String,
    samples: Vec<Json>,
    steps: Vec<Step>,
    templates: Vec<Template>,
}

struct Model {
    rule: Json,
    names: BTreeMap<String, usize>,
}
fn model(rule: &Rule, schema: &BTreeMap<String, Vec<String>>) -> Result<Model> {
    fn expr(e: &Expr, names: &mut BTreeMap<String, usize>, ops: &mut BTreeSet<String>) -> Json {
        match e {
            Expr::Var(_, n) => {
                let next = names.len();
                json!(["var", *names.entry(n.clone()).or_insert(next)])
            }
            Expr::Lit(_, v) => json!(["literal", v.to_string()]),
            Expr::Call(_, op, xs) => {
                ops.insert(op.clone());
                json!([
                    "call",
                    op,
                    xs.iter().map(|x| expr(x, names, ops)).collect::<Vec<_>>()
                ])
            }
        }
    }
    let mut names = BTreeMap::new();
    let mut ops = BTreeSet::new();
    let mut body = vec![];
    let mut head = vec![];
    for f in &rule.body {
        body.push(match f {
            Fact::Eq(_, a, b) => json!([
                "eq",
                expr(a, &mut names, &mut ops),
                expr(b, &mut names, &mut ops)
            ]),
            Fact::Fact(e) => json!(["fact", expr(e, &mut names, &mut ops)]),
        });
    }
    for a in &rule.head.0 {
        head.push(match a {
            Action::Expr(_, e) => json!(["expr", expr(e, &mut names, &mut ops)]),
            Action::Union(_, a, b) => json!([
                "union",
                expr(a, &mut names, &mut ops),
                expr(b, &mut names, &mut ops)
            ]),
            Action::Let(_, n, e) => {
                let rhs = expr(e, &mut names, &mut ops);
                if names.contains_key(n) {
                    return Err("Bake does not support redefined action bindings".into());
                }
                let next = names.len();
                names.insert(n.clone(), next);
                json!(["let", next, rhs])
            }
            _ => return Err(
                "Bake supports constructor expressions, lets and unions, not mutable table actions"
                    .into(),
            ),
        });
    }
    let primitives = [
        "+", "-", "*", "/", "%", "<", ">", "<=", ">=", "=", "!=", "min", "max",
    ];
    if ops
        .iter()
        .any(|op| !schema.contains_key(op) && !primitives.contains(&op.as_str()))
    {
        return Err("Bake encountered an undeclared/custom primitive".into());
    }
    let used: BTreeMap<_, _> = ops
        .iter()
        .filter_map(|op| schema.get(op).map(|s| (op, s)))
        .collect();
    Ok(Model {
        rule: json!({"body":body,"head":head,"schema":used}),
        names,
    })
}
struct Models {
    rules: Vec<Option<Model>>,
}
impl Models {
    fn new(c: &Captured) -> Result<Self> {
        let commands = EGraph::default().parse_program(None, &c.datatype)?;
        let Command::Datatype { name, variants, .. } = &commands[0] else {
            return Err("Bake requires exactly one declared datatype".into());
        };
        let schema = variants
            .iter()
            .map(|v| {
                let mut ty = v.types.clone();
                ty.push(name.clone());
                (v.name.clone(), ty)
            })
            .collect();
        Ok(Self {
            rules: c
                .rules
                .iter()
                .map(|r| model(&r.rule, &schema).ok())
                .collect(),
        })
    }
    fn input(&self, c: &Captured, r: &Record, i: usize) -> Option<Json> {
        let m = self.rules[r.rule].as_ref()?;
        Some(match &r.inputs[i] {
            Input::Var(n) => json!(["var", m.names.get(n)?]),
            Input::Read(s) => json!(["read", c.rules[r.rule].calls.get(s)?.0]),
        })
    }
    fn output(&self, c: &Captured, r: &Record, i: usize) -> Option<Json> {
        let m = self.rules[r.rule].as_ref()?;
        Some(match &r.outputs[i] {
            Output::Var(n) => json!(["var", m.names.get(n)?]),
            Output::Row(s) => json!(["row", c.rules[r.rule].calls.get(s)?.0]),
            Output::Column(s, col) => json!(["column", c.rules[r.rule].calls.get(s)?.0, col]),
        })
    }
    fn step(&self, c: &Captured, index: usize, refined: bool) -> Option<Step> {
        let r = &c.records[index];
        let m = self.rules[r.rule].as_ref()?;
        if r.coarse || r.parents.len() != 1 {
            return None;
        }
        let mut inputs = vec![];
        let mut outs = vec![];
        for i in 0..r.inputs.len() {
            inputs.push((self.input(c, r, i)?, i));
        }
        inputs.sort_by_key(|(role, _)| role.to_string());
        let mut externals = BTreeMap::new();
        let mut input_desc = vec![];
        let mut tokens = vec![];
        for (role, i) in inputs {
            let route = match &r.ports[i] {
                Port::Parent(p, j) => {
                    let parent = &c.records[r.parents[*p]];
                    json!([
                        "parent",
                        p,
                        self.rules[parent.rule].as_ref()?.rule,
                        self.output(c, parent, *j)?
                    ])
                }
                Port::External(k) => {
                    let next = externals.len();
                    json!(["external", *externals.entry(*k).or_insert(next)])
                }
            };
            input_desc.push(json!([role, route, c.pool.values[r.wanted[i]].sort]));
            tokens.push(r.wanted[i]);
        }
        for i in 0..r.outputs.len() {
            outs.push((self.output(c, r, i)?, i));
        }
        outs.sort_by_key(|(role, _)| role.to_string());
        let output_desc: Vec<_> = outs
            .iter()
            .map(|(role, i)| {
                tokens.push(r.values[*i]);
                json!([role, c.pool.values[r.values[*i]].sort])
            })
            .collect();
        let aliases = if refined {
            let mut ids = BTreeMap::new();
            let mut alias = |v: usize| {
                let next = ids.len();
                *ids.entry(v).or_insert(next)
            };
            let values: Vec<_> = tokens.into_iter().map(&mut alias).collect();
            let mut writes: Vec<_> = r
                .produced
                .iter()
                .map(|v| json!([c.pool.values[*v].sort, alias(*v)]))
                .collect();
            writes.sort_by_key(Json::to_string);
            let mut unions: Vec<_> = r
                .unions
                .iter()
                .map(|(a, b)| {
                    let (a, b) = (alias(*a), alias(*b));
                    json!([a.min(b), a.max(b)])
                })
                .collect();
            unions.sort_by_key(Json::to_string);
            json!({"values":values,"writes":writes,"unions":unions})
        } else {
            Json::Null
        };
        Some(Step {
            signature: json!({"rule":m.rule,"inputs":input_desc,"outputs":output_desc,"aliases":aliases}),
            source: c.rules[r.rule].rule.to_string(),
            label: c.rules[r.rule].rule.name.clone(),
        })
    }
}
pub(super) fn default_datatype_name() -> String {
    "Math".into()
}

fn validate_source(source: &Path) -> Result {
    let commands = EGraph::default().parse_program(None, &std::fs::read_to_string(source)?)?;
    if commands.iter().any(|c| {
        matches!(
            c,
            Command::Function { .. }
                | Command::Constructor { .. }
                | Command::Relation { .. }
                | Command::Sort { .. }
                | Command::Datatypes { .. }
                | Command::Include(..)
                | Command::Rewrite(_, _, true)
        )
    }) {
        return Err("Bake v1 requires a self-contained datatype and built-in primitives; custom table/sort definitions are not supported".into());
    }
    let globals: BTreeSet<_> = commands
        .iter()
        .filter_map(|c| {
            if let Command::Action(Action::Let(_, n, _)) = c {
                Some(n.clone())
            } else {
                None
            }
        })
        .collect();
    fn has_global(e: &Expr, globals: &BTreeSet<String>) -> bool {
        match e {
            Expr::Var(_, n) => globals.contains(n),
            Expr::Call(_, _, xs) => xs.iter().any(|e| has_global(e, globals)),
            _ => false,
        }
    }
    for (i, c) in commands.into_iter().enumerate() {
        if let Command::Rule { rule } = crate::visual_rule::normalize(c, i) {
            let used = rule.body.iter().any(|f| match f {
                Fact::Eq(_, a, b) => has_global(a, &globals) || has_global(b, &globals),
                Fact::Fact(e) => has_global(e, &globals),
            }) || rule.head.0.iter().any(|a| match a {
                Action::Expr(_, e) => has_global(e, &globals),
                Action::Let(_, n, e) => globals.contains(n) || has_global(e, &globals),
                Action::Union(_, a, b) => has_global(a, &globals) || has_global(b, &globals),
                _ => true,
            });
            if used {
                return Err("Bake requires rule-local bindings; globals referenced by rules are not portable inputs".into());
            }
        }
    }
    Ok(())
}
fn write_new(path: &Path, value: &impl Serialize) -> Result {
    use std::io::Write;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".partial");
    let partial = std::path::PathBuf::from(name);
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let result = (|| -> Result {
        let mut out = std::io::BufWriter::new(file);
        serde_json::to_writer_pretty(&mut out, value)?;
        out.flush()?;
        // Atomic publication without replacing an existing frozen library/result.
        std::fs::hard_link(&partial, path)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(&partial);
    result
}

fn used_parameters(e: &Expr, depth: usize, out: &mut BTreeSet<usize>) -> Result {
    if let Expr::Call(_, op, args) = e {
        if op == "BLet" {
            if args.len() != 2 {
                return Err("invalid BLet".into());
            }
            used_parameters(&args[0], depth, out)?;
            used_parameters(&args[1], depth + 1, out)?;
        } else if op == "BParam" {
            let [Expr::Lit(_, Literal::Int(i))] = args.as_slice() else {
                return Err("invalid BParam".into());
            };
            let i = usize::try_from(*i).map_err(|_| "negative BParam")?;
            if i >= depth {
                out.insert(i - depth);
            }
        } else {
            for a in args {
                used_parameters(a, depth, out)?;
            }
        }
    }
    Ok(())
}
fn contract(c: &Captured, models: &Models, members: &[usize], value: &Json) -> Option<Contract> {
    let dag = value["local_dag"].as_str()?.to_owned();
    let count = value["parameter_count"].as_u64()? as usize;
    let mut parameters = vec![None; count];
    for (key, name) in value["boundary"].as_object()? {
        let p = name.as_str()?.strip_prefix('p')?.parse::<usize>().ok()?;
        for (member, &index) in members.iter().enumerate() {
            let r = &c.records[index];
            for (i, port) in r.ports.iter().enumerate() {
                let actual = match port {
                    Port::Parent(parent, col) => {
                        format!("parent:{}:{col}", c.records[r.parents[*parent]].id)
                    }
                    Port::External(k) => format!("external:{}:{k}", r.id),
                };
                if &actual == key {
                    parameters[p] = Some(Parameter {
                        member,
                        input: models.input(c, r, i)?,
                    });
                    break;
                }
            }
            if parameters[p].is_some() {
                break;
            }
        }
    }
    let outputs = members
        .iter()
        .enumerate()
        .flat_map(|(member, index)| {
            let r = &c.records[*index];
            (0..r.outputs.len()).map(move |i| (member, r, i))
        })
        .map(|(m, r, i)| models.output(c, r, i).map(|role| json!([m, role])))
        .collect::<Option<Vec<_>>>()?;
    let expression = egglog::ast::Parser::default()
        .get_expr_from_string(None, &dag)
        .ok()?;
    let mut used = BTreeSet::new();
    used_parameters(&expression, 0, &mut used).ok()?;
    if used
        .iter()
        .any(|i| parameters.get(*i).and_then(Option::as_ref).is_none())
    {
        return None;
    }
    Some(Contract {
        dag,
        parameters,
        outputs,
        expressions: value["endpoint_outputs"]
            .as_array()?
            .iter()
            .map(|v| v.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()?,
    })
}
// A deliberately narrow proof schema, independent of rule/constructor names:
// C(i,end,rest) with i<end -> C(i+1,end,rest), with both counters typed i64.
// Successful updates cannot overflow: i+1 <= end <= i64::MAX. Other laws remain
// observational and cannot be queried without tier0 execution.
fn constructor_binding(fact: &Json) -> Option<&Json> {
    if fact[0] != "eq" {
        return None;
    }
    let (root, call) = if fact[1][0] == "var" && fact[2][0] == "call" {
        (&fact[1], &fact[2])
    } else if fact[2][0] == "var" && fact[1][0] == "call" {
        (&fact[2], &fact[1])
    } else {
        return None;
    };
    if call[2].as_array()?.iter().any(|arg| arg == root) {
        return None;
    }
    Some(call)
}
fn counted_law(step: &Step, datatype: &str) -> Option<Json> {
    let r = &step.signature["rule"];
    let body = r["body"].as_array()?;
    let head = r["head"].as_array()?;
    if body.len() != 2 || head.len() != 1 || head[0][0] != "expr" || body[0][0] != "eq" {
        return None;
    }
    let lhs = constructor_binding(&body[0])?;
    let rhs = &head[0][1];
    let guard = &body[1][1];
    if body[1][0] != "fact"
        || guard[0] != "call"
        || guard[1] != "<"
        || lhs[0] != "call"
        || rhs[0] != "call"
        || lhs[1] != rhs[1]
    {
        return None;
    }
    let args = lhs[2].as_array()?;
    if args
        .iter()
        .map(Json::to_string)
        .collect::<BTreeSet<_>>()
        .len()
        != args.len()
    {
        return None;
    }
    let next = rhs[2].as_array()?;
    let g = guard[2].as_array()?;
    if g.len() != 2
        || args.len() != next.len()
        || g[0][0] != "var"
        || g[1][0] != "var"
        || g[0] == g[1]
    {
        return None;
    }
    let index = args.iter().position(|v| v == &g[0])?;
    let limit = args.iter().position(|v| v == &g[1])?;
    let schema = r["schema"][lhs[1].as_str()?].as_array()?;
    if schema.len() != args.len() + 1
        || schema.last()? != datatype
        || schema.get(index)? != "i64"
        || schema.get(limit)? != "i64"
    {
        return None;
    }
    for (i, (a, b)) in args.iter().zip(next).enumerate() {
        if a[0] != "var" {
            return None;
        }
        if i == index {
            if b[0] != "call"
                || b[1] != "+"
                || !(b[2] == json!([a, ["literal", "1"]]) || b[2] == json!([["literal", "1"], a]))
            {
                return None;
            }
        } else if a != b {
            return None;
        }
    }
    Some(
        json!({"kind":"bounded_unit_stride","index_field":index,"limit_field":limit,"proof":"strict integer guard, unit positive update, unchanged limit and other fields; successful update is <= limit so cannot overflow","sequence":"start+1, ..., limit when start < limit; otherwise empty","dissipation":"remaining=max(0,limit-start); decreases by one per successful update","scope":"successor scalar payloads, not equivalence to all tier0 mutations"}),
    )
}
// Complete radix branches tile a contiguous frontier. This is checked against
// the source AST/schema, not inferred from a few observed cardinalities.
fn radix_law(t: &Template, steps: &[Step], datatype: &str) -> Option<Json> {
    if t.kind != "recursive_dag" || t.ports.len() < 2 {
        return None;
    }
    let r = &steps[t.entry].signature["rule"];
    let body = r["body"].as_array()?;
    let head = r["head"].as_array()?;
    if body.len() != 2 || body[0][0] != "eq" || body[1][0] != "fact" || head.len() != t.ports.len()
    {
        return None;
    }
    let lhs = constructor_binding(&body[0])?;
    if lhs[0] != "call" || lhs[2].as_array()?.len() != 1 || lhs[2][0][0] != "var" {
        return None;
    }
    let n = &lhs[2][0];
    if body[1][1] != json!(["call", ">", [n, ["literal", "0"]]]) {
        return None;
    }
    let radix = head.len();
    let mut residues = BTreeSet::new();
    let mut branch = None;
    for action in head {
        let rhs = &action[1];
        if action[0] != "expr" || rhs[0] != "call" || rhs[2].as_array()?.len() != 1 {
            return None;
        }
        if branch.as_ref().is_some_and(|b| b != &rhs[1]) {
            return None;
        }
        branch = Some(rhs[1].clone());
        let value = &rhs[2][0];
        if value[0] != "call" || value[1] != "+" || value[2].as_array()?.len() != 2 {
            return None;
        }
        let mul = &value[2][0];
        let offset = &value[2][1];
        if mul != &json!(["call", "*", [n, ["literal", radix.to_string()]]])
            || offset[0] != "literal"
        {
            return None;
        }
        residues.insert(offset[1].as_str()?.parse::<usize>().ok()?);
    }
    if residues != (0..radix).collect() {
        return None;
    }
    let branch = branch?;
    if r["schema"][lhs[1].as_str()?] != json!(["i64", datatype])
        || r["schema"][branch.as_str()?] != json!(["i64", datatype])
    {
        return None;
    }
    for port in &t.ports {
        let b = &steps[*port].signature["rule"];
        let body = b["body"].as_array()?;
        let head = b["head"].as_array()?;
        if body.len() != 1 || head.len() != 1 || body[0][0] != "eq" || head[0][0] != "expr" {
            return None;
        }
        let l = constructor_binding(&body[0])?;
        if l[0] != "call"
            || l[1] != branch
            || l[2].as_array()?.len() != 1
            || l[2][0][0] != "var"
            || head[0][1] != json!(["call", lhs[1], [l[2][0]]])
        {
            return None;
        }
        if b["schema"][branch.as_str()?] != json!(["i64", datatype])
            || b["schema"][lhs[1].as_str()?] != json!(["i64", datatype])
        {
            return None;
        }
    }
    Some(
        json!({"kind":"radix_frontier","radix":radix,"sequence":"at depth d: start*radix^d + j, 0 <= j < radix^d","proof":"all residues 0..radix-1 appear exactly once; return rule copies payload; induction partitions the next contiguous frontier","requires":"start > 0 and (start+1)*radix^depth-1 <= i64::MAX","scope":"last-frontier scalar payloads, not intermediate rows or effects; no extension through overflow"}),
    )
}
fn summarize_array(root: &Path, start: &str, count: &str) -> Result<Json> {
    summarize_range(root, &format!("(EAdd {start} (EInt 1))"), count)
}
fn summarize_range(root: &Path, first: &str, count: &str) -> Result<Json> {
    let mut eg = EGraph::default();
    load(&mut eg, &root.join("rules/tier2.egg"), root)?;
    let array = format!("(RampArray (FiniteDomain {count}) {first} (EInt 1))");
    let term = format!("(ReduceArray (SumReduction) {array})");
    eg.parse_and_run_program(None,&format!("{term}\n(run-schedule (saturate (seq (run endpoint-reduce) (run array-shape) (run array-laws) (run array-eval))))"))?;
    let expr = egglog::ast::Parser::default().get_expr_from_string(None, &term)?;
    let (sort, value) = eg.eval_expr(&expr)?;
    let (result, cost) = eg.extract_value_to_string(&sort, value)?;
    Ok(
        json!({"array":array,"sum":result,"cost":cost,"element_queries":eg.get_size("ArrayAt"),"expanded_prefixes":eg.get_size("ReducePrefix")}),
    )
}

pub fn train(root: &Path, manifest: &Path, out: &Path) -> Result<Json> {
    let manifest = manifest.canonicalize()?;
    let input: Manifest = serde_json::from_slice(&std::fs::read(&manifest)?)?;
    if input.version != VERSION || input.samples.len() < 2 {
        return Err("Bake manifest v1 requires at least two named samples".into());
    }
    if out.exists() {
        return Err("Bake output directory already exists".into());
    }
    let mut names = BTreeSet::new();
    for s in &input.samples {
        if !names.insert(s.name.clone()) {
            return Err("duplicate Bake sample name".into());
        }
    }
    std::fs::create_dir_all(out)?;
    let started = Instant::now();
    let mut samples = vec![];
    let mut steps = Vec::<Step>::new();
    let mut step_ids = BTreeMap::<String, usize>::new();
    let mut families = BTreeMap::<String, Template>::new();
    let mut datatype: Option<String> = None;
    let worker = rayon::ThreadPoolBuilder::new().num_threads(1).build()?;
    for sample in input.samples {
        eprintln!("[bake] {}", sample.name);
        let source = manifest
            .parent()
            .unwrap()
            .join(&sample.source)
            .canonicalize()?;
        validate_source(&source)?;
        let begin = Instant::now();
        let mut c = capture(&source, sample.rounds, None)?;
        match &datatype {
            None => datatype = Some(c.datatype_name.clone()),
            Some(name) if *name != c.datatype_name => {
                return Err(format!(
                    "Bake samples must declare the same datatype ({} vs {})",
                    name, c.datatype_name
                )
                .into())
            }
            Some(_) => {}
        }
        let report=worker.install(||->std::result::Result<Json,String>{
            (||->Result<Json>{
                let mut eg=EGraph::default();build_tier1(&mut c,&mut eg,root,&mut(0,0))?;
                let(x,_)=build_tier2(&mut c,&mut eg,root)?;
                let linear=fractal::build(&c,&mut eg,&x,root)?;
                let discovered=recursive::build(&c,&x,&mut eg,root,&linear)?;
                let models=Models::new(&c)?;
                let by_id:BTreeMap<_,_>=c.records.iter().enumerate().map(|(i,r)|(r.id,i)).collect();
                let mut accepted=0;
                for p in discovered["patterns"].as_array().ok_or("missing discovered patterns")? {
                    let kind=p["kind"].as_str().ok_or("missing pattern kind")?;
                    let refined=kind=="recursive_dag";
                    let Some(start)=p["learning_witnesses"].as_array().and_then(|a|a.first()).and_then(Json::as_u64).and_then(|id|by_id.get(&id)).copied() else {continue};
                    let Some(entry)=models.step(&c,start,refined) else {continue};
                    let mut members=vec![start];
                    if refined {
                        let node=p["instances"].as_array().unwrap().iter().flat_map(|i|i["nodes"].as_array().unwrap()).find(|n|n["entry_event"].as_u64()==Some(c.records[start].id));
                        let Some(node)=node else {continue};
                        let Some(children)=node["ports"].as_array().unwrap().iter().map(|p|p["observed_child"].as_u64().and_then(|id|by_id.get(&id).copied())).collect::<Option<Vec<_>>>() else {continue};
                        members.extend(children);
                    }
                    let Some(port_specs)=members[1..].iter().map(|i|models.step(&c,*i,true)).collect::<Option<Vec<_>>>() else {continue};
                    let definition=json!({"kind":kind,"entry":entry.signature,"ports":port_specs.iter().map(|s|&s.signature).collect::<Vec<_>>()}).to_string();
                    let mut intern=|step:Step| {let key=step.signature.to_string();*step_ids.entry(key).or_insert_with(||{let id=steps.len();steps.push(step);id})};
                    let entry_id=intern(entry);let ports:Vec<_>=port_specs.into_iter().map(&mut intern).collect();
                    let f=call("FractalComb",vec![call("Depth",vec![num(1)]),call("ImportedExtension",vec![num(c.records[start].extension as u64)]),cref(c.records[c.records[start].parents[0]].id),fractal::binding(&c,&c.records[start])]);
                    let exported=binding::reduce(&c,&members,&[],"bake-local",&mut eg,&f).ok().and_then(|v|contract(&c,&models,&members,&v));
                    let instances=p["instances"].as_array().unwrap();
                    let support=Support{sample:sample.name.clone(),instances:instances.len(),max_depth:instances.iter().filter_map(|i|i["depth"].as_u64()).max().unwrap_or(0) as usize,witnesses:members.iter().map(|i|json!({"sample":sample.name,"event":c.records[*i].id,"parents":c.records[*i].parents.iter().map(|p|c.records[*p].id).collect::<Vec<_>>(),"inputs":c.records[*i].wanted.iter().map(|v|json!([c.pool.values[*v].sort,c.pool.values[*v].label()])).collect::<Vec<_>>(),"outputs":c.records[*i].values.iter().map(|v|json!([c.pool.values[*v].sort,c.pool.values[*v].label()])).collect::<Vec<_>>()})).collect()};
                    let family=families.entry(definition).or_insert_with(||Template{id:String::new(),kind:kind.into(),entry:entry_id,ports,support:vec![],contract:exported.clone(),array_summary:None});
                    if family.contract.is_none(){family.contract=exported;}
                    family.support.push(support);accepted+=1;
                }
                Ok(json!({"name":sample.name,"source":source,"source_program":std::fs::read_to_string(&source)?,"events":c.events,"eligible":c.records.len(),"excluded":c.rejected,"discovered_families":discovered["pattern_count"],"exported_families":accepted,"seconds":begin.elapsed().as_secs_f64()}))
            })().map_err(|e|e.to_string())
        }).map_err(|e|->Box<dyn std::error::Error>{e.into()})?;
        samples.push(report);
    }
    let mut templates: Vec<_> = families.into_values().collect();
    // Every sample declared the same datatype; the array-law recognizers name it
    // instead of assuming `Math`.
    let datatype = datatype.unwrap_or_else(default_datatype_name);
    for (i, t) in templates.iter_mut().enumerate() {
        t.id = format!("fractal_{i:04}");
        if t.kind == "linear_extension" {
            if let Some(mut law) = counted_law(&steps[t.entry], &datatype) {
                law["dsl"] = summarize_array(root, "(ESymbol \"start\")", "(ECount \"steps\")")?;
                t.array_summary = Some(law);
            }
        } else if let Some(mut law) = radix_law(t, &steps, &datatype) {
            let count = format!("(EPow (EInt {}) (ECount \"depth\"))", law["radix"]);
            law["dsl"] =
                summarize_range(root, &format!("(EMul (ESymbol \"start\") {count})"), &count)?;
            t.array_summary = Some(law);
        }
    }
    let library=Library{version:VERSION,algebra:std::env::var("EGG_LAYOUT_BINDING_ALGEBRA").unwrap_or_else(|_|"integer-safe".into()),datatype,scope:"Frozen observed FractalRule families. Exact portable source/binding/effect fingerprints; no general induction proof and no automatic tier0 mutation replacement. Only separately checked array laws permit no-tier0 value queries.".into(),samples,steps,templates};
    let summary = json!({"samples":library.samples.len(),"templates":library.templates.len(),"step_definitions":library.steps.len(),"multi_sample_templates":library.templates.iter().filter(|t|t.support.iter().map(|s|&s.sample).collect::<BTreeSet<_>>().len()>1).count(),"array_summaries":library.templates.iter().filter(|t|t.array_summary.is_some()).count(),"families":library.templates.iter().map(|t|json!({"id":t.id,"kind":t.kind,"entry_rule":library.steps[t.entry].label,"samples":t.support.iter().map(|s|&s.sample).collect::<Vec<_>>(),"array_law":t.array_summary.as_ref().map(|s|&s["kind"]),"symbolic_outputs":t.contract.as_ref().map(|c|&c.expressions)})).collect::<Vec<_>>(),"seconds":started.elapsed().as_secs_f64()});
    // Inspectable native DSL recipes; these do not assert tier0 effect equivalence.
    let mut dsl = "; Baked local contracts and checked scalar-array recipes.
; Supply contract arguments explicitly; these are not tier0 replacement rules.
(include \"rules/tier2.egg\")
(relation BakedContract (String BindingDag))
(relation BakedSequence (String Array))
(relation BakedSummary (String EndpointExpr))
"
    .to_owned();
    for t in &library.templates {
        let id = serde_json::to_string(&t.id)?;
        if let Some(c) = &t.contract {
            dsl += &format!("(BakedContract {id} {})\n", c.dag);
        }
        if let Some(a) = &t.array_summary {
            let expression = a["dsl"]["array"].as_str().ok_or("missing array recipe")?;
            dsl += &format!(
                "(BakedSequence {id} {expression})\n(BakedSummary {id} (ReduceArray (SumReduction) {expression}))\n"
            );
        }
    }
    dsl += "(run-schedule (saturate (seq (run endpoint-reduce) (run array-shape) (run array-laws) (run array-eval))))\n";
    {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(out.join("baked.egg"))?;
        file.write_all(dsl.as_bytes())?;
    }
    write_library(&out.join("library.egg"), &library)?;
    write_new(&out.join("bake.json"), &summary)?;
    Ok(summary)
}

fn load_library(path: &Path) -> Result<Library> {
    let text = std::fs::read_to_string(path)?;
    let lib: Library = if text.trim_start().starts_with('{') {
        serde_json::from_str(&text)?
    } else {
        format::parse(&text)?
    };
    if lib.version != VERSION {
        return Err("unsupported Bake library version".into());
    }
    if !["integer-safe", "math"].contains(&lib.algebra.as_str()) {
        return Err("invalid Bake algebra".into());
    }
    let keys: BTreeSet<_> = lib.steps.iter().map(|s| s.signature.to_string()).collect();
    if keys.len() != lib.steps.len() {
        return Err("duplicate Bake step definitions".into());
    }
    for step in &lib.steps {
        let commands = EGraph::default().parse_program(None, &step.source)?;
        let [Command::Rule { rule }] = commands.as_slice() else {
            return Err("invalid baked source rule".into());
        };
        let schema: BTreeMap<String, Vec<String>> =
            serde_json::from_value(step.signature["rule"]["schema"].clone())?;
        if model(rule, &schema)?.rule != step.signature["rule"] {
            return Err("baked source does not match its canonical definition".into());
        }
    }
    let mut ids = BTreeSet::new();
    for t in &lib.templates {
        if !ids.insert(&t.id)
            || t.entry >= lib.steps.len()
            || t.ports.iter().any(|p| *p >= lib.steps.len())
        {
            return Err("invalid Bake template references".into());
        }
        if !["linear_extension", "recursive_dag"].contains(&t.kind.as_str())
            || (t.kind == "recursive_dag" && t.ports.is_empty())
        {
            return Err("invalid Bake family kind".into());
        }
        if t.ports.iter().collect::<BTreeSet<_>>().len() != t.ports.len() {
            return Err("duplicate Bake branch ports".into());
        }
        if let Some(summary) = &t.array_summary {
            let checked = if t.kind == "linear_extension" {
                counted_law(&lib.steps[t.entry], &lib.datatype)
            } else {
                radix_law(t, &lib.steps, &lib.datatype)
            };
            if checked.as_ref().is_none_or(|c| {
                ["kind", "index_field", "limit_field", "radix"]
                    .iter()
                    .any(|key| c[*key] != summary[*key])
            }) {
                return Err("invalid array certificate".into());
            }
            // Array/sum declarations are cached consequences of the checked law.
            // Reject stale visible recipes instead of silently ignoring edits.
            let (count, first) = if summary["kind"] == "bounded_unit_stride" {
                (
                    "(ECount \"steps\")".to_owned(),
                    "(EAdd (ESymbol \"start\") (EInt 1))".to_owned(),
                )
            } else {
                let count = format!("(EPow (EInt {}) (ECount \"depth\"))", summary["radix"]);
                (count.clone(), format!("(EMul (ESymbol \"start\") {count})"))
            };
            let expected_array = format!("(RampArray (FiniteDomain {count}) {first} (EInt 1))");
            let expected_sum = format!(
                "(EAdd (EMul {count} {first}) (EDiv (EMul {count} (ESub {count} (EInt 1))) (EInt 2)))"
            );
            for (field, expected) in [("array", expected_array), ("sum", expected_sum)] {
                let actual = summary["dsl"][field]
                    .as_str()
                    .ok_or("missing array recipe")?;
                let mut parser = egglog::ast::Parser::default();
                if parser.get_expr_from_string(None, actual)?.to_string()
                    != parser.get_expr_from_string(None, &expected)?.to_string()
                {
                    return Err(
                        "array recipe disagrees with its checked law; rebake the library".into(),
                    );
                }
            }
        }
        if let Some(contract) = &t.contract {
            let mut eg = EGraph::default();
            // Parse expressions, never execute arbitrary library commands.
            let expr = egglog::ast::Parser::default().get_expr_from_string(None, &contract.dag)?;
            fn pure(e: &Expr) -> bool {
                match e {
                    Expr::Lit(..) => true,
                    Expr::Var(..) => false,
                    Expr::Call(_, op, args) => {
                        [
                            "BLet", "BDag", "BParam", "BInt", "BText", "BAdd", "BSub", "BMul",
                            "BDiv", "BPow", "BIAdd", "BISub", "BIMul", "BApply", "BRow", "BColumn",
                            "BXCons", "BXNil",
                        ]
                        .contains(&op.as_str())
                            && args.iter().all(pure)
                    }
                }
            }
            let mut used = BTreeSet::new();
            used_parameters(&expr, 0, &mut used)?;
            if used.iter().any(|i| {
                contract
                    .parameters
                    .get(*i)
                    .and_then(Option::as_ref)
                    .is_none()
            }) {
                return Err("unmapped baked contract parameter".into());
            }
            if !pure(&expr)
                || contract
                    .parameters
                    .iter()
                    .flatten()
                    .any(|p| p.member > t.ports.len())
            {
                return Err("invalid baked binding contract".into());
            }
            // Check the serialized term is an expression with no trailing commands.
            let parsed = eg.parse_program(None, &contract.dag)?;
            if parsed.len() != 1
                || !matches!(&parsed[0],Command::Action(Action::Expr(_,e)) if e==&expr)
            {
                return Err("invalid contract expression".into());
            }
        }
    }
    Ok(lib)
}
#[derive(Default)]
struct UnitMatch {
    template: usize,
    entry: usize,
    members: Vec<Vec<usize>>,
    returns: Vec<Vec<usize>>,
}
struct Frozen<'a> {
    library: &'a Library,
    models: Option<Models>,
    step_ids: BTreeMap<String, usize>,
    entries: BTreeMap<usize, Vec<usize>>,
    roles: BTreeMap<usize, Vec<(usize, usize)>>,
    units: Vec<UnitMatch>,
    anchors: BTreeMap<(usize, usize), usize>,
    membership: BTreeMap<usize, Vec<(usize, usize)>>,
    linear: Vec<BTreeMap<usize, (usize, usize)>>,
    terminals: Vec<BTreeSet<usize>>,
    seen: usize,
    covered: BTreeSet<usize>,
    recognized: BTreeSet<usize>,
    complete: usize,
    batches: Vec<Json>,
    seconds: f64,
    step_probes: usize,
    role_checks: usize,
    misses: BTreeMap<String, usize>,
}
impl<'a> Frozen<'a> {
    fn new(library: &'a Library) -> Self {
        let step_ids = library
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| (s.signature.to_string(), i))
            .collect();
        let mut entries = BTreeMap::<_, Vec<_>>::new();
        let mut roles = BTreeMap::<_, Vec<_>>::new();
        for (i, t) in library.templates.iter().enumerate() {
            entries.entry(t.entry).or_default().push(i);
            for (port, step) in t.ports.iter().enumerate() {
                roles.entry(*step).or_default().push((i, port));
            }
        }
        Self {
            library,
            models: None,
            step_ids,
            entries,
            roles,
            units: vec![],
            anchors: BTreeMap::new(),
            membership: BTreeMap::new(),
            linear: vec![BTreeMap::new(); library.templates.len()],
            terminals: vec![BTreeSet::new(); library.templates.len()],
            seen: 0,
            covered: BTreeSet::new(),
            recognized: BTreeSet::new(),
            complete: 0,
            batches: vec![],
            seconds: 0.0,
            step_probes: 0,
            role_checks: 0,
            misses: BTreeMap::new(),
        }
    }
    fn update(&mut self, c: &Captured) -> Result {
        if self.seen == c.records.len() {
            return Ok(());
        }
        let begin = Instant::now();
        if self.models.is_none() {
            self.models = Some(Models::new(c)?);
        }
        let models = self.models.as_ref().unwrap();
        for i in self.seen..c.records.len() {
            let r = &c.records[i];
            let exact = models.step(c, i, true).and_then(|s| {
                self.step_probes += 1;
                self.step_ids.get(&s.signature.to_string()).copied()
            });
            let base = models.step(c, i, false).and_then(|s| {
                self.step_probes += 1;
                self.step_ids.get(&s.signature.to_string()).copied()
            });
            if exact.is_some() || base.is_some() {
                self.recognized.insert(i);
            }
            if exact.is_none() && base.is_none() {
                *self
                    .misses
                    .entry(if r.coarse {
                        "coarse_or_trigger_boundary".into()
                    } else {
                        format!("uncovered_rule:{}", c.rules[r.rule].rule.name)
                    })
                    .or_default() += 1;
            }
            if let Some(step) = exact {
                // Link already-known library return ports, never enumerate new families.
                for p in &r.parents {
                    if let Some(links) = self.membership.get(p) {
                        for &(u, slot) in links {
                            if self.library.templates[self.units[u].template].entry == step {
                                self.units[u].returns[slot].push(i);
                            }
                        }
                    }
                }
                if let Some(roles) = self.roles.get(&step) {
                    for &(template, slot) in roles {
                        for p in &r.parents {
                            self.role_checks += 1;
                            if let Some(&u) = self.anchors.get(&(template, *p)) {
                                let before = self.units[u].members.iter().all(|m| m.len() == 1);
                                self.units[u].members[slot].push(i);
                                let after = self.units[u].members.iter().all(|m| m.len() == 1);
                                self.complete =
                                    self.complete + usize::from(after) - usize::from(before);
                                self.membership.entry(i).or_default().push((u, slot));
                            }
                        }
                    }
                }
                if let Some(entries) = self.entries.get(&step) {
                    for &template in entries {
                        if self.library.templates[template].kind != "recursive_dag" {
                            continue;
                        }
                        let n = self.library.templates[template].ports.len();
                        let u = self.units.len();
                        self.units.push(UnitMatch {
                            template,
                            entry: i,
                            members: vec![vec![]; n],
                            returns: vec![vec![]; n],
                        });
                        self.anchors.insert((template, i), u);
                    }
                }
            }
            if let Some(step) = base {
                if let Some(entries) = self.entries.get(&step) {
                    for &t in entries {
                        if self.library.templates[t].kind != "linear_extension" {
                            continue;
                        }
                        let (length, start) = r
                            .parents
                            .first()
                            .and_then(|p| self.linear[t].get(p))
                            .map(|(n, s)| (n + 1, *s))
                            .unwrap_or((1, i));
                        self.linear[t].insert(i, (length, start));
                        self.terminals[t].insert(i);
                        if length > 1 {
                            self.terminals[t].remove(&r.parents[0]);
                            self.covered.insert(r.parents[0]);
                            self.covered.insert(i);
                        }
                    }
                }
            }
        }
        self.seen = c.records.len();
        let complete = self.complete;
        self.batches.push(json!({"records":self.seen,"entry_candidates":self.units.len(),"locally_complete_units":complete,"pending_units":self.units.len()-complete}));
        self.seconds += begin.elapsed().as_secs_f64();
        Ok(())
    }
    fn finish(mut self, c: &Captured) -> Json {
        let mut matches = vec![];
        let mut instantiated = 0;
        let models = self.models.as_ref();
        for u in &self.units {
            let t = &self.library.templates[u.template];
            let ambiguous =
                u.members.iter().any(|m| m.len() > 1) || u.returns.iter().any(|m| m.len() > 1);
            let complete = !ambiguous && u.members.iter().all(|m| m.len() == 1);
            let state = if ambiguous {
                "ambiguous"
            } else if complete {
                "local_complete"
            } else {
                "pending"
            };
            let mut members = vec![u.entry];
            if complete {
                members.extend(u.members.iter().map(|m| m[0]));
                self.covered.extend(&members);
            }
            let bindings = if complete {
                t.contract.as_ref().and_then(|contract|{
                contract.parameters.iter().map(|p|match p {
                    None=>Some(Json::Null),
                    Some(p)=>{let r=&c.records[*members.get(p.member)?];let models=models?;let slot=(0..r.inputs.len()).find(|i|models.input(c,r,*i).as_ref()==Some(&p.input))?;Some(json!({"event":r.id,"input":slot,"sort":c.pool.values[r.wanted[slot]].sort,"value":c.pool.values[r.wanted[slot]].label()}))}
                }).collect::<Option<Vec<_>>>()
            })
            } else {
                None
            };
            if bindings.is_some() {
                instantiated += 1;
            }
            matches.push(json!({"template":t.id,"entry_event":c.records[u.entry].id,"state":state,"ports":u.members.iter().zip(&u.returns).enumerate().map(|(p,(m,r))|json!({"port":p,"members":m.iter().map(|i|c.records[*i].id).collect::<Vec<_>>(),"returns":r.iter().map(|i|c.records[*i].id).collect::<Vec<_>>()})).collect::<Vec<_>>(),"contract_bindings":bindings}));
        }
        for (t, ends) in self.terminals.iter().enumerate() {
            for end in ends {
                let (length, start) = self.linear[t][end];
                if length < 2 {
                    continue;
                }
                matches.push(json!({"template":self.library.templates[t].id,"state":"linear_run","entry_event":c.records[start].id,"end_event":c.records[*end].id,"depth":length,"array_summary_available":self.library.templates[t].array_summary.is_some()}));
            }
        }
        let mut uncovered_reasons = BTreeMap::<String, usize>::new();
        let uncovered: Vec<_> = (0..c.records.len())
            .filter(|i| !self.covered.contains(i))
            .map(|i| {
                let reason = if c.records[i].coarse {
                    "trigger_or_external_boundary"
                } else if self.recognized.contains(&i) {
                    "known_step_without_complete_context"
                } else {
                    "no_library_step"
                };
                *uncovered_reasons.entry(reason.into()).or_default() += 1;
                c.records[i].id
            })
            .collect();
        json!({"uncovered_events":uncovered,"uncovered_reasons":uncovered_reasons,"scope":"Frozen library matching at capture boundaries. No candidate discovery, no new templates, no tier1/tier2 reconstruction. Tier0 still runs its complete original schedule; contracts are linked to observed inputs, not executed as replacement mutations.","templates_in_library":self.library.templates.len(),"templates_added":0,"discovery_calls":0,"tier1_nodes_built":0,"tier2_search_runs":0,"step_key_probes":self.step_probes,"role_checks":self.role_checks,"covered_events":self.covered.iter().map(|i|c.records[*i].id).collect::<Vec<_>>(),"eligible_events":c.records.len(),"raw_match_events":c.events,"excluded_events":c.rejected,"contract_instances":instantiated,"matches":matches,"capture_batches":self.batches,"misses":self.misses,"fixed_match_seconds":self.seconds})
    }
}

pub fn apply(
    _root: &Path,
    library_path: &Path,
    source: &Path,
    out: &Path,
    save_history: bool,
) -> Result<Json> {
    let library = load_library(library_path)?;
    validate_source(source)?;
    if out.exists() {
        return Err("frozen-use output directory already exists".into());
    }
    std::fs::create_dir_all(out)?;
    let start = Instant::now();
    let mut matcher = Frozen::new(&library);
    let c = capture_with_sink(source, None, None, Some(&mut |c| matcher.update(c)))?;
    if save_history {
        history::save(&out.join("history.json"), source, &c)?;
    }
    let mut report = matcher.finish(&c);
    report["wall_seconds"] = json!(start.elapsed().as_secs_f64());
    report["capture_total_seconds"] = json!(c.trace_seconds);
    report["history_saved"] = json!(save_history);
    write_new(&out.join("use.json"), &report)?;
    Ok(report)
}
pub fn query(
    root: &Path,
    library_path: &Path,
    id: &str,
    start: i64,
    limit: i64,
    out: &Path,
) -> Result<Json> {
    let lib = load_library(library_path)?;
    let t = lib
        .templates
        .iter()
        .find(|t| t.id == id)
        .ok_or("unknown baked template")?;
    if t.kind != "linear_extension"
        || t.array_summary.is_none()
        || counted_law(&lib.steps[t.entry], &lib.datatype).is_none()
    {
        return Err(
            "this template has no certified counted-array law; tier0-free queries are not allowed"
                .into(),
        );
    }
    let count = (i128::from(limit) - i128::from(start)).max(0);
    let count = i64::try_from(count).map_err(|_| "count exceeds the concrete i64 index domain")?;
    let begin = Instant::now();
    let value = summarize_array(root, &format!("(EInt {start})"), &format!("(EInt {count})"))?;
    let report = json!({"scope":"Certified successor-payload sum for this isolated bounded unit-stride recurrence. Not whole-program execution and not preservation/materialization of tier0 effects.","template":id,"start":start,"limit":limit,"steps":count,"tier0_rule_applications":0,"discovery_calls":0,"result":value,"seconds":begin.elapsed().as_secs_f64()});
    write_new(out, &report)?;
    Ok(report)
}

/// Query the complete frontier of a structurally checked radix recurrence.
pub fn query_frontier(
    root: &Path,
    library_path: &Path,
    id: &str,
    start: i64,
    depth: u32,
    out: &Path,
) -> Result<Json> {
    let lib = load_library(library_path)?;
    let t = lib
        .templates
        .iter()
        .find(|t| t.id == id)
        .ok_or("unknown baked template")?;
    let law =
        radix_law(t, &lib.steps, &lib.datatype).ok_or("no certified radix frontier for this template")?;
    if t.array_summary.is_none() || start <= 0 {
        return Err("radix query requires a baked summary and positive start".into());
    }
    let radix = law["radix"].as_u64().unwrap() as i128;
    let count = radix
        .checked_pow(depth)
        .ok_or("frontier size overflows the supported domain")?;
    let first = i128::from(start)
        .checked_mul(count)
        .ok_or("frontier overflows")?;
    let last = first.checked_add(count - 1).ok_or("frontier overflows")?;
    if last > i128::from(i64::MAX) {
        return Err("requested depth crosses checked-i64 overflow; no complete frontier exists under this certificate".into());
    }
    let count = i64::try_from(count).map_err(|_| "frontier index domain too large")?;
    let result = summarize_range(root, &format!("(EInt {first})"), &format!("(EInt {count})"))?;
    let report = json!({"scope":"Certified last-frontier payload query only; no tier0 intermediate facts or effects are materialized.","template":id,"start":start,"depth":depth,"radix":radix as u64,"elements":count,"theoretical_basic_applies_for_full_unfolding":(((i128::from(count)-1)/(radix-1))*(radix+1)) as u64,"first":first as i64,"last":last as i64,"tier0_rule_applications":0,"discovery_calls":0,"result":result});
    write_new(out, &report)?;
    Ok(report)
}

#[cfg(test)]
mod proof_tests {
    use super::*;
    fn step(source: &str) -> Step {
        let commands = EGraph::default().parse_program(None, source).unwrap();
        let Command::Rule { rule } = &commands[0] else {
            panic!("rule")
        };
        let schema = BTreeMap::from([
            ("A".into(), vec!["i64".into(), "i64".into(), "Math".into()]),
            ("D".into(), vec!["i64".into(), "i64".into(), "Math".into()]),
        ]);
        Step {
            signature: json!({"rule":model(rule,&schema).unwrap().rule}),
            source: source.into(),
            label: "test".into(),
        }
    }
    #[test]
    fn counted_schema_does_not_drop_extra_requirements() {
        assert!(
            counted_law(&step(
                "(rule ((= root (A i end)) (< i end)) ((A (+ i 1) end)))",
            ), "Math")
            .is_some()
        );
        assert!(
            counted_law(&step(
                "(rule ((= (A i end) (D i end)) (< i end)) ((A (+ i 1) end)))",
            ), "Math")
            .is_none()
        );
        assert!(
            counted_law(
                &step(
                    "(rule ((= root (A i end)) (<= i end)) ((A (+ i 1) end)))"
                ),
                "Math",
            )
            .is_none()
        );
        assert!(
            counted_law(
                &step(
                    "(rule ((= root (A i end)) (< i end)) ((A (+ i 2) end)))"
                ),
                "Math",
            )
            .is_none()
        );
    }
}

fn write_library(path: &Path, lib: &Library) -> Result {
    use std::io::Write;
    let text = format::render(lib)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let mut name = path.as_os_str().to_os_string();
    name.push(".partial");
    let partial = std::path::PathBuf::from(name);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&partial)?;
    let result = (|| -> Result {
        file.write_all(text.as_bytes())?;
        file.flush()?;
        std::fs::hard_link(&partial, path)?;
        Ok(())
    })();
    let _ = std::fs::remove_file(partial);
    result
}
/// Convert old JSON libraries without rerunning tier0 or discovery.
pub fn convert_library(input: &Path, output: &Path) -> Result {
    write_library(output, &load_library(input)?)
}
/// Read either library representation into its validated inspection model.
pub fn inspect_library(input: &Path) -> Result<Json> {
    Ok(serde_json::to_value(load_library(input)?)?)
}
