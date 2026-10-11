//! Concrete stage Bake for native commands that have no parameterized closure
//! certificate. Snapshots retain every user table, named value and visibility.
//! A stage is NOT a saturated Cell. Each restored checkpoint is independently
//! compared with the native state; original scheduled rules never run on export.
use egg_layout::{
    pipeline::Result,
    saturated_rule_composition::{self as graph, Row, SaturatedRuleComposition, Vertex},
};
use egglog::{
    ArcSort, EGraph, Value,
    ast::{Action, Command, Parser},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

#[derive(Default)]
struct Snapshot {
    keys: BTreeMap<(String, Value), usize>,
    vertices: Vec<Vertex>,
    rows: Vec<Row>,
    hidden: Vec<usize>,
    expressions: Vec<String>,
    containers: Vec<Row>,
    setup: Vec<String>,
    eqsorts: BTreeSet<String>,
    namespace: String,
}
impl Snapshot {
    fn unit(&mut self) -> usize {
        if let Some(i) = self
            .vertices
            .iter()
            .position(|v| v.sort == "Unit" && v.literal.as_deref() == Some("()"))
        {
            return i;
        }
        let id = self.vertices.len();
        self.vertices.push(Vertex {
            sort: "Unit".into(),
            literal: Some("()".into()),
        });
        self.expressions.push("()".into());
        id
    }
    fn intern(&mut self, eg: &EGraph, sort: &ArcSort, v: Value) -> Result<usize> {
        if sort.name() == "Unit" {
            return Ok(self.unit());
        }
        let v = eg.get_canonical_value(v, sort);
        let key = (sort.name().to_string(), v);
        if let Some(id) = self.keys.get(&key) {
            return Ok(*id);
        }
        let id = self.vertices.len();
        self.keys.insert(key, id);
        self.vertices.push(Vertex {
            sort: sort.name().into(),
            literal: None,
        });
        self.expressions.push(String::new());
        if sort.is_eq_sort() {
            self.eqsorts.insert(sort.name().into());
            self.expressions[id] = format!("{}-v-{id}", self.namespace);
            self.setup.push(format!(
                "(let {}-v-{id} ({}-Ref-{} {id}))",
                self.namespace,
                self.namespace,
                sort.name()
            ));
            return Ok(id);
        }
        let mut children: Vec<(ArcSort, Value)> = vec![];
        let kind;
        let mut function_name = None;
        if sort.value_type() == Some(std::any::TypeId::of::<egglog::sort::VecContainer>()) {
            let c = eg
                .value_to_container::<egglog::sort::VecContainer>(v)
                .ok_or("missing Vec payload")?;
            kind = "vec-of";
            let s = sort.inner_sorts()[0].clone();
            children.extend(c.data.iter().map(|v| (s.clone(), *v)));
        } else if sort.value_type() == Some(std::any::TypeId::of::<egglog::sort::SetContainer>()) {
            kind = "set-of";
            let c = eg
                .value_to_container::<egglog::sort::SetContainer>(v)
                .ok_or("missing Set payload")?;
            let s = sort.inner_sorts()[0].clone();
            children.extend(c.data.iter().map(|v| (s.clone(), *v)));
        } else if sort.value_type() == Some(std::any::TypeId::of::<egglog::sort::MapContainer>()) {
            kind = "map";
            let c = eg
                .value_to_container::<egglog::sort::MapContainer>(v)
                .ok_or("missing Map payload")?;
            let sorts = sort.inner_sorts();
            for (k, v) in &c.data {
                children.push((sorts[0].clone(), *k));
                children.push((sorts[1].clone(), *v));
            }
        } else if sort.value_type() == Some(std::any::TypeId::of::<egglog::sort::PairContainer>()) {
            kind = "pair";
            let c = eg
                .value_to_container::<egglog::sort::PairContainer>(v)
                .ok_or("missing Pair payload")?;
            let ss = sort.inner_sorts();
            children = vec![(ss[0].clone(), c.first), (ss[1].clone(), c.second)];
        } else if sort.value_type()
            == Some(std::any::TypeId::of::<egglog::sort::MultiSetContainer>())
        {
            kind = "multiset-of";
            let c = eg
                .value_to_container::<egglog::sort::MultiSetContainer>(v)
                .ok_or("missing MultiSet payload")?;
            let s = sort.inner_sorts()[0].clone();
            children.extend(c.data.iter().map(|v| (s.clone(), *v)));
        } else if sort.value_type()
            == Some(std::any::TypeId::of::<egglog::sort::FunctionContainer>())
        {
            kind = "unstable-fn";
            let c = eg
                .value_to_container::<egglog::sort::FunctionContainer>(v)
                .ok_or("missing function payload")?;
            function_name = Some(c.2.clone());
            children = c.1.clone();
        } else {
            if sort.is_container_sort() {
                return Err(format!("no lossless codec for container {}", sort.name()).into());
            }
            let (literal, _) = eg.extract_value_to_string(sort, v)?;
            self.vertices[id].literal = Some(literal.clone());
            self.expressions[id] = literal;
            return Ok(id);
        }
        let ids = children
            .iter()
            .map(|(s, v)| self.intern(eg, s, *v))
            .collect::<Result<Vec<_>>>()?;
        let args = ids
            .iter()
            .map(|i| self.expressions[*i].clone())
            .collect::<Vec<_>>();
        self.expressions[id] = match kind {
            "map" => args.chunks(2).fold("(map-empty)".into(), |m, p| {
                format!("(map-insert {m} {} {})", p[0], p[1])
            }),
            "unstable-fn" => format!(
                "(unstable-fn {} {})",
                serde_json::to_string(&function_name.as_ref().unwrap())?,
                args.join(" ")
            ),
            _ => format!("({kind} {})", args.join(" ")),
        };
        // Incidence representation retains container aliases and order. Sets
        // and maps use unordered member/entry edges; vectors retain positions.
        self.containers.push(Row {
            op: format!(
                "@container:{}:{kind}:{}",
                sort.name(),
                function_name.unwrap_or_default()
            ),
            args: vec![],
            result: id,
        });
        if kind == "set-of" || kind == "multiset-of" {
            let mut multiplicity = BTreeMap::new();
            for c in ids {
                *multiplicity.entry(c).or_insert(0usize) += 1;
            }
            for (c, n) in multiplicity {
                self.containers.push(Row {
                    op: format!("@member:{n}"),
                    args: vec![c],
                    result: id,
                });
            }
        } else if kind == "map" {
            for pair in ids.chunks(2) {
                self.containers.push(Row {
                    op: "@entry".into(),
                    args: pair.to_vec(),
                    result: id,
                });
            }
        } else {
            self.containers.push(Row {
                op: "@ordered".into(),
                args: ids,
                result: id,
            });
        }
        Ok(id)
    }
}

fn snapshot(
    eg: &mut EGraph,
    globals: &BTreeSet<String>,
    namespace: &str,
) -> Result<(SaturatedRuleComposition, Vec<String>, Vec<String>)> {
    let mut s = Snapshot {
        namespace: namespace.into(),
        ..Default::default()
    };
    let mut schemas = BTreeMap::new();
    let mut emit = vec![];
    let mut hide = vec![];
    let mut names = eg.get_function_names();
    names.sort();
    for name in names {
        let f = eg.get_function(&name).unwrap();
        if f.is_let_binding()
            || name.starts_with('@')
            || (name.starts_with(&format!("{namespace}-Ref-"))
                || name.starts_with(&format!("{namespace}-Global-")))
        {
            continue;
        }
        let constructor = f.is_constructor();
        let schema = f.schema().clone();
        let relation = constructor && schema.output.name().starts_with('@');
        schemas.insert(name.clone(),json!({"kind":if relation {"relation"} else if constructor {"constructor"} else {"function"},"inputs":schema.input.iter().map(|s|s.name()).collect::<Vec<_>>(),"output":if relation {"Unit"} else {schema.output.name()}}));
        let mut raw = vec![];
        eg.function_for_each(&name, |r| raw.push((r.vals.to_vec(), r.subsumed)))?;
        for (values, hidden) in raw {
            let mut ids = vec![];
            for (v, sort) in values.iter().zip(schema.input.iter()) {
                ids.push(s.intern(eg, sort, *v)?);
            }
            let result = if relation {
                s.unit()
            } else {
                s.intern(eg, &schema.output, *values.last().ok_or("empty row")?)?
            };
            let call = format!(
                "({name} {})",
                ids.iter()
                    .map(|i| s.expressions[*i].clone())
                    .collect::<Vec<_>>()
                    .join(" ")
            );
            if relation {
                emit.push(call.clone());
            } else if constructor {
                emit.push(format!("(union {call} {})", s.expressions[result]));
            } else {
                emit.push(format!("(set {call} {})", s.expressions[result]));
            }
            if hidden {
                hide.push(format!("(subsume {call})"));
                s.hidden.push(s.rows.len());
            }
            s.rows.push(Row {
                op: name.clone(),
                args: ids,
                result,
            });
        }
    }
    let mut ports = BTreeMap::new();
    let mut binds = vec![];
    let mut extra_declarations = vec![];
    let mut inspector = eg.clone();
    for name in globals {
        let expr = Parser::default().get_expr_from_string(None, name)?;
        if let Ok((sort, v)) = inspector.eval_expr(&expr) {
            let i = s.intern(eg, &sort, v)?;
            ports.insert(name.clone(), i);
            let helper = format!("{namespace}-Global-{name}");
            extra_declarations.push(format!("(function {helper} () {} :no-merge)", sort.name()));
            binds.push(format!("(set ({helper}) {})", s.expressions[i]));
            binds.push(format!("(let {name} ({helper}))"));
        }
    }
    let mut declarations: Vec<_> = s
        .eqsorts
        .iter()
        .map(|ty| format!("(constructor {namespace}-Ref-{ty} (i64) {ty} :unextractable)"))
        .collect();
    declarations.extend(extra_declarations);
    s.setup.extend(emit);
    s.setup.extend(hide);
    s.setup.extend(binds);
    s.rows.extend(s.containers);
    Ok((
        SaturatedRuleComposition {
            version: 2,
            local_ids: vec![],
            scope: json!({"semantics":"exact-native-stage/v1","schemas":schemas}),
            values: s.vertices,
            rows: s.rows,
            subsumed_rows: s.hidden,
            ports,
        },
        declarations,
        s.setup,
    ))
}
fn restore_header(
    events: &[String],
    declarations: &[String],
    actions: &[String],
    ns: &str,
) -> Result<(EGraph, Vec<String>, Vec<String>)> {
    use egglog::ast::Expr;
    let mut eg = EGraph::default();
    let mut header = vec![];
    let mut ordinary = vec![];
    let mut globals: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut slots = vec![];
    let mut helpers = declarations.to_vec();
    for line in actions {
        let cmd = Parser::default()
            .get_program_from_string(None, line)?
            .into_iter()
            .next()
            .ok_or("empty action")?;
        match cmd {
            Command::Action(Action::Let(_, name, _)) if name.starts_with(&format!("{ns}-v-")) => {
                slots.push(line.clone())
            }
            Command::Action(Action::Set(_, name, _, _))
                if name.starts_with(&format!("{ns}-Global-")) =>
            {
                globals
                    .entry(name[format!("{ns}-Global-").len()..].into())
                    .or_default()
                    .push(line.clone())
            }
            Command::Action(Action::Let(_, name, Expr::Call(_, head, _)))
                if head.starts_with(&format!("{ns}-Global-")) =>
            {
                globals.entry(name).or_default().push(line.clone())
            }
            _ => ordinary.push(line.clone()),
        }
    }
    // Install helper declarations as soon as their sorts exist, and reference
    // values as soon as their constructors exist. Actual user facts come later.
    fn ready(
        eg: &mut EGraph,
        helpers: &mut Vec<String>,
        slots: &mut Vec<String>,
        header: &mut Vec<String>,
    ) -> Result {
        let mut pending = vec![];
        for line in std::mem::take(helpers) {
            let c = Parser::default()
                .get_program_from_string(None, &line)?
                .remove(0);
            let available = match &c {
                Command::Constructor { schema, .. } | Command::Function { schema, .. } => schema
                    .input
                    .iter()
                    .chain(std::iter::once(&schema.output))
                    .all(|t| eg.get_sort_by_name(t).is_some()),
                _ => false,
            };
            if available {
                eg.run_program(vec![c])?;
                header.push(line);
            } else {
                pending.push(line)
            }
        }
        *helpers = pending;
        let mut pending = vec![];
        for line in std::mem::take(slots) {
            let c = Parser::default()
                .get_program_from_string(None, &line)?
                .remove(0);
            let available = matches!(&c,Command::Action(Action::Let(_,_,Expr::Call(_,op,_))) if eg.get_function(op).is_some());
            if available {
                eg.run_program(vec![c])?;
                header.push(line)
            } else {
                pending.push(line)
            }
        }
        *slots = pending;
        Ok(())
    }
    ready(&mut eg, &mut helpers, &mut slots, &mut header)?;
    for event in events {
        if let Some(name) = event.strip_prefix("; stage-global ") {
            for line in globals
                .remove(name)
                .ok_or_else(|| format!("global {name} is unavailable"))?
            {
                eg.parse_and_run_program(None, &line)?;
                header.push(line);
            }
        } else {
            eg.parse_and_run_program(None, event)?;
            header.push(event.clone());
        }
        ready(&mut eg, &mut helpers, &mut slots, &mut header)?;
    }
    if !helpers.is_empty() || !slots.is_empty() || !globals.is_empty() {
        return Err("unresolved stage header dependencies".into());
    }
    Ok((eg, header, ordinary))
}

fn declaration(c: &Command) -> bool {
    matches!(
        c,
        Command::Datatype { .. }
            | Command::Datatypes { .. }
            | Command::Sort { .. }
            | Command::Constructor { .. }
            | Command::Function { .. }
            | Command::Relation { .. }
    )
}
fn expand(
    eg: &mut EGraph,
    commands: Vec<Command>,
    active: &mut BTreeSet<std::path::PathBuf>,
) -> Result<Vec<Command>> {
    let mut result = vec![];
    for c in commands {
        if let Command::Include(_, file) = c {
            let path = Path::new(&file).canonicalize()?;
            if !active.insert(path.clone()) {
                return Err("include cycle".into());
            }
            let children = eg.parse_program(Some(file), &fs::read_to_string(&path)?)?;
            result.extend(expand(eg, children, active)?);
            active.remove(&path);
        } else {
            result.push(c)
        }
    }
    Ok(result)
}
fn verify_archive(path: &Path) -> Result {
    let report: serde_json::Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    let ns = report["namespace"]
        .as_str()
        .ok_or("missing snapshot namespace")?;
    for frame in report["frames"].as_array().ok_or("missing frames")? {
        let expected: SaturatedRuleComposition = serde_json::from_value(frame["state"].clone())?;
        let globals = expected.ports.keys().cloned().collect();
        let mut eg = EGraph::default();
        for field in ["environment", "slot_declarations"] {
            for line in frame[field].as_array().ok_or("missing declarations")? {
                let commands =
                    eg.parse_program(None, line.as_str().ok_or("invalid declaration")?)?;
                if commands
                    .iter()
                    .any(|c| !declaration(c) && !matches!(c, Command::Action(_)))
                {
                    return Err(
                        "snapshot header may contain declarations/value initializers only".into(),
                    );
                }
                eg.run_program(commands)?;
            }
        }
        let actions = frame["actions"].as_array().ok_or("missing actions")?;
        let count = actions
            .len()
            .checked_sub(usize::from(frame["observable"] == true))
            .ok_or("missing observable action")?;
        for line in &actions[..count] {
            let commands = eg.parse_program(None, line.as_str().ok_or("invalid action")?)?;
            if commands.iter().any(|c| !matches!(c, Command::Action(_))) {
                return Err("snapshot may not execute rules".into());
            }
            eg.run_program(commands)?;
        }
        let actual = snapshot(&mut eg, &globals, ns)?.0;
        if !matches!(
            graph::compare(&expected, &actual)?,
            graph::Comparison::Equivalent { .. }
        ) {
            return Err(format!("snapshot {} differs from its native witness", frame["id"]).into());
        }
    }
    println!("verified snapshot archive");
    Ok(())
}
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: spine_stages SOURCE.egg OUTPUT_DIR".into());
    }
    if args[1] == "--verify" {
        return verify_archive(Path::new(&args[2]));
    }
    let out = Path::new(&args[2]);
    fs::create_dir_all(out)?;
    let source = fs::read_to_string(&args[1])?;
    let mut eg = EGraph::default();
    let parsed = eg.parse_program(Some(args[1].clone()), &source)?;
    let commands = expand(&mut eg, parsed, &mut BTreeSet::new())?;
    let sets: BTreeSet<_> = commands
        .iter()
        .filter_map(|c| match c {
            Command::Rule { rule } => Some(rule.ruleset.clone()),
            Command::Rewrite(r, ..) | Command::BiRewrite(r, ..) => Some(r.clone()),
            _ => None,
        })
        .collect();
    if sets.len() > 1 {
        return Err("requires a single rule-bearing ruleset after expanding includes".into());
    }
    let all_text = commands
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let mut namespace = "Stage".to_string();
    while all_text.contains(&namespace) {
        namespace.push('X');
    }
    let command_count = commands.len();
    let mut environment = vec![];
    let mut globals = BTreeSet::new();
    let mut stack = vec![];
    let mut frames = vec![];
    for (ordinal, c) in commands.into_iter().enumerate() {
        if matches!(
            &c,
            Command::Output { .. }
                | Command::UserDefined(..)
                | Command::Prove(..)
                | Command::ProveExists(..)
        ) {
            return Err(format!("command requires a separate external-effect codec: {c}").into());
        }
        if declaration(&c) {
            environment.push(c.to_string());
        }
        if let Command::Action(Action::Let(_, name, _)) = &c {
            globals.insert(name.clone());
            environment.push(format!("; stage-global {name}"));
        }
        let observable = matches!(
            &c,
            Command::Check(..)
                | Command::Extract(..)
                | Command::Fail(..)
                | Command::PrintFunction(..)
                | Command::PrintSize(..)
        );
        // Observables execute in the original position on the captured prestate.
        let pre = if observable {
            Some(snapshot(&mut eg, &globals, &namespace)?)
        } else {
            None
        };
        let native_outputs = eg.run_program(vec![c.clone()])?;
        let output = native_outputs
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        match &c {
            Command::Push(n) => {
                for _ in 0..*n {
                    stack.push((environment.clone(), globals.clone()));
                }
            }
            Command::Pop(_, n) => {
                for _ in 0..*n {
                    let (e, g) = stack.pop().ok_or("unbalanced pop")?;
                    environment = e;
                    globals = g;
                }
            }
            _ => {}
        }
        if (declaration(&c)
            || matches!(
                &c,
                Command::Rule { .. }
                    | Command::Rewrite(..)
                    | Command::BiRewrite(..)
                    | Command::AddRuleset(..)
                    | Command::Push(..)
            ))
            && ordinal + 1 < command_count
        {
            continue;
        }
        // Only observation/schedule boundaries and the final state need a
        // checkpoint; do not create a quadratic pile for seed insertion loops.
        if matches!(&c, Command::Action(_) | Command::Input { .. }) && ordinal + 1 < command_count {
            continue;
        }
        let (state, slots, mut setup) = if let Some(pre) = pre {
            pre
        } else {
            snapshot(&mut eg, &globals, &namespace)?
        };
        let (mut restored, header, remaining) =
            restore_header(&environment, &slots, &setup, &namespace)?;
        setup = remaining;
        restored.parse_and_run_program(None, &setup.join("\n"))?;
        let restored_state = snapshot(&mut restored, &globals, &namespace)?.0;
        let compare = graph::compare(&state, &restored_state)?;
        if !matches!(compare, graph::Comparison::Equivalent { .. }) {
            return Err(format!(
                "checkpoint {ordinal} reconstruction differs: {}",
                serde_json::to_string(&compare)?
            )
            .into());
        }
        if observable {
            let restored_outputs = restored.run_program(vec![c.clone()])?;
            let got = restored_outputs
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let equivalent = native_outputs.len() == restored_outputs.len()
                && native_outputs.iter().zip(&restored_outputs).all(|(a, b)| {
                    use egglog::CommandOutput as O;
                    match (a, b) {
                        (O::ExtractBest(_, a, _), O::ExtractBest(_, b, _)) => a == b,
                        (O::PrintAllFunctionsSize(a), O::PrintAllFunctionsSize(b)) => {
                            let filtered: Vec<_> = b
                                .iter()
                                .filter(|(name, _)| !name.starts_with(&format!("{namespace}-")))
                                .cloned()
                                .collect();
                            a == &filtered
                        }
                        _ => a.to_string() == b.to_string(),
                    }
                });
            if !equivalent {
                return Err(format!(
                    "observable output differs at {ordinal}: {got:?} != {output:?}"
                )
                .into());
            }
            let expected_after = snapshot(&mut eg, &globals, &namespace)?.0;
            let actual_after = snapshot(&mut restored, &globals, &namespace)?.0;
            if !matches!(
                graph::compare(&expected_after, &actual_after)?,
                graph::Comparison::Equivalent { .. }
            ) {
                return Err(format!("observable effect differs at {ordinal}").into());
            }
            setup.push(c.to_string());
        }
        let id = frames.len();
        let program = format!("{}\n{}\n", header.join("\n"), setup.join("\n"));
        fs::write(out.join(format!("stage-{id:04}.egg")), program)?;
        frames.push(json!({"id":id,"command_ordinal":ordinal,"command":c.to_string(),"observable":observable,"output":output,"rows":state.rows.len(),"hidden_rows":state.subsumed_rows.len(),"ports":state.ports.len(),"environment":header,"slot_declarations":[],"actions":setup,"state":state,"comparison":compare,"original_rules_executed_on_restore":false}));
    }
    fs::write(
        out.join("stages.json"),
        serde_json::to_string_pretty(
            &json!({"schema":"native-stage-bake/v1","scope":"concrete source specialization, not parameterized closure equivalence","namespace":namespace,"frames":frames}),
        )? + "\n",
    )?;
    println!("validated {} concrete stage checkpoints", frames.len());
    Ok(())
}
