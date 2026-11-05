use crate::value::{Value, literal};
use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::collections::BTreeMap;
use std::rc::Rc;
pub type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Arg {
    Slot(usize),
    Literal(Value),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Call {
    Table(usize),
    Primitive(String, String),
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Atom {
    pub call: Call,
    pub args: Vec<Arg>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Op {
    Copy(usize, Arg),
    Let(usize, Call, Vec<Arg>),
    Set(usize, Vec<Arg>, Arg),
    Union(Arg, Arg),
    Hide(usize, Vec<Arg>, bool),
    Panic,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Table {
    pub name: String,
    pub schema: Vec<J>,
    pub constructor: bool,
    pub virtual_fact: bool,
    pub merge: J,
    pub internal: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Template {
    pub name: String,
    pub sorts: Vec<J>,
    pub defs: BTreeMap<usize, usize>,
    pub ops: Rc<Vec<Op>>,
    pub parent_columns: BTreeMap<usize, usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Rule {
    #[serde(default)]
    pub(crate) constructor_only: bool,
    pub template: usize,
    pub query: Rc<Vec<Atom>>,
    pub inputs: Rc<Vec<usize>>,
    pub seminaive: bool,
    pub direct: Option<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Program {
    #[serde(skip)]
    pub(crate) view_blocks: Rc<Vec<crate::view_plan::Block>>,
    #[serde(skip)]
    pub(crate) neighbor_plans: Vec<Option<crate::neighbor::Plan>>,
    #[serde(skip)]
    pub(crate) context_cuts: crate::local_context::Cuts,
    #[serde(skip)]
    pub(crate) local_contexts: Vec<Option<Rc<crate::local_context::Plan>>>,
    #[serde(skip)]
    pub(crate) writes: Vec<Option<Rc<crate::row_kernel::Write>>>,
    #[serde(skip)]
    pub(crate) kernels: crate::row_kernel::Kernels,
    #[serde(default)]
    pub(crate) physical: Rc<crate::physical_plan::Plan>,
    #[serde(skip)]
    pub(crate) entry_join: Option<Rc<crate::entry_join::Plan>>,
    #[serde(default)]
    pub known_results: crate::known_results::Plans,
    #[serde(default)]
    pub merge_programs: crate::merge_programs::Programs,

    #[cfg(feature = "spine-blocks")]
    #[serde(skip)]
    pub spine_blocks: crate::spine_blocks::Catalog,
    #[cfg(feature = "iterative-bake")]
    #[serde(skip)]
    pub iterative: crate::iterative::Catalog,
    #[serde(default)]
    pub marker_tables: std::collections::BTreeSet<usize>,
    pub multi_stage: crate::multistage::MultiStage,
    pub entry_schedule: crate::entry_schedule::Plan,

    pub tables: Vec<Table>,
    pub templates: Vec<Template>,
    pub rules: Vec<Rule>,
    pub sets: BTreeMap<String, Vec<usize>>,
    pub control: Vec<J>,
    pub initial: Vec<J>,
    pub table_templates: Vec<usize>,
    pub artifact: J,
}
pub fn array(j: &J) -> Result<&Vec<J>> {
    j.as_array().ok_or_else(|| "expected array".into())
}
pub fn string(j: &J) -> Result<&str> {
    j.as_str().ok_or_else(|| "expected string".into())
}
pub fn arg(j: &J) -> Result<Arg> {
    if let Some(i) = j["slot"].as_u64() {
        Ok(Arg::Slot(i as usize))
    } else {
        Ok(Arg::Literal(literal(j)?))
    }
}
fn args(j: &J) -> Result<Vec<Arg>> {
    array(j)?.iter().map(arg).collect()
}
pub fn call(j: &J, names: &BTreeMap<String, usize>) -> Result<Call> {
    if let Some(t) = j["table"].as_str() {
        Ok(Call::Table(*names.get(t).ok_or("unknown operator")?))
    } else {
        Ok(Call::Primitive(
            string(&j["primitive"])?.into(),
            string(&j["output"])?.into(),
        ))
    }
}
impl Program {
    pub fn context_cut_counts(&self) -> (usize, usize) {
        (
            self.context_cuts.iter().filter(|ps| !ps.is_empty()).count(),
            self.context_cuts.iter().map(Vec::len).sum(),
        )
    }
    pub fn parse(j: J) -> Result<Self> {
        Self::load(j, false)
    }
    pub(crate) fn load(j: J, compiling: bool) -> Result<Self> {
        if j["schema"] != "eggfuse-cell-runtime/v1" {
            return Err("unsupported Cell runtime artifact".into());
        }
        let ir = &j["recipe"];
        if ir["schema"] != "eggfuse-recipe/v1" || ir["scope"] != "original_program" {
            return Err("expected original-program Recipe IR".into());
        }
        verify_source(&j)?;
        let mut tables = vec![];
        let mut names = BTreeMap::new();
        for t in array(&ir["tables"])? {
            let schema = array(&t["schema"])?.clone();
            if schema.is_empty() {
                return Err("empty schema".into());
            }
            for s in &schema {
                if !matches!(
                    s["kind"].as_str(),
                    Some(
                        "eq" | "i64"
                            | "bool"
                            | "Unit"
                            | "String"
                            | "set"
                            | "f64"
                            | "structural_key"
                    )
                ) {
                    return Err(format!("unsupported sort {s}"));
                }
            }
            let name = string(&t["name"])?.to_owned();
            if names.insert(name.clone(), tables.len()).is_some() {
                return Err("duplicate operator".into());
            }
            let constructor = t["kind"] == "constructor";
            let virtual_fact = constructor
                || (schema.last().unwrap()["kind"] == "Unit" && t["merge_expr"].is_null());
            tables.push(Table {
                name,
                schema,
                constructor,
                virtual_fact,
                merge: t["merge_expr"].clone(),
                internal: t["internal"].as_bool().unwrap_or(false),
            });
        }
        let mut templates = vec![];
        let mut rules = vec![];
        for r in array(&ir["rules"])? {
            let slots = array(&r["slots"])?;
            let sorts = slots.iter().map(|s| s["sort"].clone()).collect::<Vec<_>>();
            let inputs: Vec<usize> = slots
                .iter()
                .enumerate()
                .filter(|(_, s)| s["definition"] == "query")
                .map(|(i, _)| i)
                .collect();
            let mut query = vec![];
            let mut ops = vec![];
            let mut defs = BTreeMap::new();
            for a in array(&r["query"])? {
                query.push(Atom {
                    call: call(&a["call"], &names)?,
                    args: args(&a["args"])?,
                });
            }
            for e in array(&r["effects"])? {
                let dst = e["dst"].as_u64().unwrap_or(0) as usize;
                let op = match string(&e["op"])? {
                    "copy" => Op::Copy(dst, arg(&e["arg"])?),
                    "let" => Op::Let(dst, call(&e["call"], &names)?, args(&e["args"])?),
                    "set" => Op::Set(
                        *names
                            .get(string(&e["call"]["table"])?)
                            .ok_or("set operator")?,
                        args(&e["args"])?,
                        arg(&e["value"])?,
                    ),
                    "union" => {
                        let a = args(&e["args"])?;
                        if a.len() != 2 {
                            return Err("union arity".into());
                        }
                        Op::Union(a[0].clone(), a[1].clone())
                    }
                    "subsume" | "delete" => Op::Hide(
                        *names
                            .get(string(&e["call"]["table"])?)
                            .ok_or("visibility operator")?,
                        args(&e["args"])?,
                        e["op"] == "delete",
                    ),
                    "panic" => Op::Panic,
                    _ => return Err("unsupported effect".into()),
                };
                if matches!(op, Op::Copy(..) | Op::Let(..)) {
                    if dst >= sorts.len() || defs.insert(dst, ops.len()).is_some() {
                        return Err("invalid SSA definition".into());
                    }
                }
                ops.push(op);
            }
            let table_atoms = query
                .iter()
                .filter(|a| matches!(a.call, Call::Table(_)))
                .count();
            let direct = if table_atoms == 1 {
                query.iter().position(|a| matches!(a.call, Call::Table(_)))
            } else {
                None
            };
            let mut parent_columns = BTreeMap::new();
            if let Some(i) = direct {
                if let Call::Table(t) = query[i].call {
                    if tables[t].virtual_fact {
                        for (col, a) in query[i].args.iter().enumerate() {
                            if let Arg::Slot(slot) = a {
                                parent_columns.entry(*slot).or_insert(col);
                            }
                        }
                    }
                }
            }
            let constructor_only = ops.iter().all(|op| match op {
                Op::Copy(..) => true,
                Op::Let(_, Call::Table(t), _) => tables[*t].virtual_fact && tables[*t].constructor,
                _ => false,
            });
            rules.push(Rule {
                constructor_only,
                template: templates.len(),
                query: Rc::new(query),
                inputs: Rc::new(inputs),
                seminaive: r["metadata"]["seminaive"].as_bool().unwrap_or(true),
                direct,
            });
            templates.push(Template {
                name: r["metadata"]["name"].as_str().unwrap_or("rule").into(),
                sorts,
                defs,
                ops: Rc::new(ops),
                parent_columns,
            });
        }
        let mut table_templates = vec![];
        for (tid, t) in tables.iter().enumerate() {
            let dst = t.schema.len() - 1;
            let op = Op::Let(dst, Call::Table(tid), (0..dst).map(Arg::Slot).collect());
            table_templates.push(templates.len());
            templates.push(Template {
                name: format!("import/{}", t.name),
                sorts: t.schema.clone(),
                defs: BTreeMap::from([(dst, 0)]),
                ops: Rc::new(vec![op]),
                parent_columns: BTreeMap::new(),
            });
        }
        let mut sets = BTreeMap::new();
        for s in array(&ir["rulesets"])? {
            let ids = array(&s["rule_ids"])?
                .iter()
                .map(|i| {
                    i.as_u64()
                        .map(|x| x as usize)
                        .ok_or_else(|| "invalid rule ID".into())
                })
                .collect::<Result<Vec<_>>>()?;
            if ids.iter().any(|i| *i >= rules.len()) {
                return Err("rule ID out of bounds".into());
            }
            sets.insert(string(&s["name"])?.into(), ids);
        }
        let kernels = crate::row_kernel::derive(&rules);
        if let Some(raw) = j.get("row_kernels")
            && *raw != serde_json::json!(kernels)
        {
            return Err("invalid register join plan".into());
        }
        let mut p = Self {
            neighbor_plans: vec![],
            context_cuts: vec![],
            local_contexts: vec![],
            kernels,
            writes: vec![],
            physical: Default::default(),
            known_results: Default::default(),
            merge_programs: Default::default(),

            #[cfg(feature = "spine-blocks")]
            spine_blocks: Default::default(),
            #[cfg(feature = "iterative-bake")]
            iterative: Default::default(),
            view_blocks: Default::default(),
            marker_tables: Default::default(),
            multi_stage: crate::multistage::MultiStage::default(),
            entry_schedule: crate::entry_schedule::Plan::default(),
            entry_join: None,

            tables,
            templates,
            rules,
            sets,
            control: array(&ir["control"])?.clone(),
            initial: array(&j["initial"])?.clone(),
            table_templates,
            artifact: j,
        };
        p.validate()?;
        p.physical = Rc::new(crate::physical_plan::Plan::derive(&p));
        p.writes = crate::row_kernel::writes(&p);
        p.neighbor_plans = crate::neighbor::derive(&p);
        if let Some(raw) = p.artifact.get("neighbor_entries")
            && *raw != serde_json::json!(p.neighbor_plans)
        {
            return Err("invalid delta neighbor plan".into());
        }
        if let Some(raw) = p.artifact.get("single_writes")
            && *raw != serde_json::json!(p.writes)
        {
            return Err("invalid functional write plan".into());
        }
        if let Some(raw) = p.artifact.get("physical_plan") {
            let emitted: crate::physical_plan::Plan =
                serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            if emitted != *p.physical {
                return Err("physical plan differs from typed recipe".into());
            }
        }
        if compiling {
            return Ok(p);
        }
        if p.tables
            .iter()
            .any(|t| t.schema.iter().any(|s| s["kind"] == "structural_key"))
            && p.artifact["source_core"]["structural_key"]["abi"]
                != serde_json::from_str::<J>(eggfuse_structural_key::ABI).unwrap()
        {
            return Err("structural-key ABI differs".into());
        }
        if p.artifact.get("merge_contracts").is_some()
            && !p.artifact["merge_contracts"]["tables"].is_array()
        {
            return Err("merge contract schema".into());
        }
        if let Some(cs) = p.artifact["merge_contracts"]["tables"].as_array() {
            if p.artifact["merge_contracts"]["schema"] != "eggfuse-merge-contracts/v1"
                || cs.len() != p.tables.len()
            {
                return Err("merge contract schema".into());
            }
            for (t, c) in p.tables.iter().zip(cs) {
                let expected = crate::merge_contract::table(&p, t)?;
                if ["reads", "constructs"].iter().any(|k| c[k] != expected[k]) {
                    return Err("merge contract footprint differs".into());
                }
                if ["table", "expression", "kind", "equal_inputs"]
                    .iter()
                    .any(|k| c[k] != expected[k])
                {
                    return Err("merge contract differs from source".into());
                }
            }
        }
        if let Some(raw) = p.artifact.get("free_join_entry") {
            let mut emitted: crate::entry_join::Plan =
                serde_json::from_value(raw.clone()).map_err(|e| e.to_string())?;
            let expected = crate::entry_join::Plan::derive(&p);
            if raw.get("guards").is_none() {
                emitted.guards = expected.guards.clone();
            }
            if emitted != expected {
                return Err("Free Join entry plan differs from typed LHS".into());
            }
            p.entry_join = Some(Rc::new(emitted));
        }
        p.entry_schedule = crate::entry_schedule::Plan::parse(&p, &p.artifact["entry_schedule"])?;
        let marker_names = p
            .artifact
            .get("marker_tables")
            .unwrap_or(&p.artifact["reference_views"]["marker_tables"]);
        for name in marker_names.as_array().into_iter().flatten() {
            let t = p
                .tables
                .iter()
                .position(|t| name == &t.name)
                .ok_or("unknown marker table")?;
            if p.tables[t].constructor
                || p.tables[t].schema.last().unwrap()["kind"] != "Unit"
                || p.tables[t].merge != serde_json::json!({"var":"old"})
            {
                return Err("invalid marker relation".into());
            }
            p.marker_tables.insert(t);
        }
        if p.artifact["merge_programs"].is_null() {
            return Err("merge programs require re-emitting the CPU artifact".into());
        }
        p.merge_programs =
            crate::merge_programs::Programs::parse(&p, &p.artifact["merge_programs"])?;
        p.known_results = crate::known_results::Plans::parse(&p, &p.artifact["known_results"])?;
        p.multi_stage = crate::multistage::MultiStage::parse(&p, &p.artifact["multi_stage"])?;
        let spine = p
            .multi_stage
            .stage_entries
            .iter()
            .map(|(r, _, _)| *r)
            .collect();
        let blocks = crate::view_plan::derive(&p, &spine);
        if let Some(raw) = p.artifact.get("view_consumer_blocks")
            && *raw != serde_json::json!(&blocks)
        {
            return Err("view consumer block differs from typed recipe".into());
        }
        p.view_blocks = Rc::new(blocks);
        p.context_cuts = crate::local_context::derive_cuts(&p);
        if let Some(raw) = p.artifact.get("context_cuts")
            && *raw != serde_json::json!(crate::local_context::layout(&p.context_cuts))
        {
            return Err("invalid connected context cuts".into());
        }
        if let Some(raw) = p.artifact.get("context_admission") {
            let allowed = crate::context_admission::admitted(&p, raw)?;
            for (plans, keep) in p.context_cuts.iter_mut().zip(allowed) {
                if !keep {
                    plans.clear();
                }
            }
        }

        p.local_contexts =
            crate::local_context::derive(&p, &p.artifact["multi_stage"]["stage_entries"]);
        if let Some(raw) = p.artifact.get("local_contexts")
            && *raw != serde_json::json!(p.local_contexts)
        {
            return Err("invalid local context plan".into());
        }

        #[cfg(feature = "spine-blocks")]
        {
            p.spine_blocks = crate::spine_blocks::Catalog::parse(&p, &p.artifact["spine_blocks"])?;
        }
        #[cfg(feature = "iterative-bake")]
        {
            p.iterative = crate::iterative::Catalog::parse(&p, &p.artifact["iterative_bake"])?;
        }
        Ok(p)
    }
    fn validate(&self) -> Result<()> {
        for rule in &self.rules {
            let t = &self.templates[rule.template];
            let mut defined = rule
                .inputs
                .iter()
                .copied()
                .collect::<std::collections::BTreeSet<_>>();
            let check = |a: &Arg, defined: &std::collections::BTreeSet<usize>| -> Result<()> {
                if let Arg::Slot(i) = a {
                    if *i >= t.sorts.len() || !defined.contains(i) {
                        return Err("undefined slot".into());
                    }
                }
                Ok(())
            };
            for a in rule.query.iter() {
                for v in &a.args {
                    check(v, &defined)?;
                }
                if let Call::Table(i) = a.call {
                    if a.args.len() != self.tables[i].schema.len() {
                        return Err("query arity".into());
                    }
                    for (v, s) in a.args.iter().zip(&self.tables[i].schema) {
                        if let Arg::Slot(i) = v {
                            if t.sorts[*i] != *s {
                                return Err("query type mismatch".into());
                            }
                        }
                    }
                }
            }
            for op in t.ops.iter() {
                match op {
                    Op::Copy(dst, a) => {
                        check(a, &defined)?;
                        defined.insert(*dst);
                    }
                    Op::Let(dst, c, aa) => {
                        for a in aa {
                            check(a, &defined)?;
                        }
                        if let Call::Table(i) = c {
                            if aa.len() + 1 != self.tables[*i].schema.len()
                                || t.sorts[*dst] != *self.tables[*i].schema.last().unwrap()
                            {
                                return Err("construct/read type or arity".into());
                            }
                        }
                        defined.insert(*dst);
                    }
                    Op::Set(i, aa, v) => {
                        for a in aa {
                            check(a, &defined)?;
                        }
                        check(v, &defined)?;
                        if aa.len() + 1 != self.tables[*i].schema.len() {
                            return Err("set arity".into());
                        }
                    }
                    Op::Union(a, b) => {
                        check(a, &defined)?;
                        check(b, &defined)?;
                        let (Arg::Slot(a), Arg::Slot(b)) = (a, b) else {
                            return Err("union literals unsupported".into());
                        };
                        if t.sorts[*a] != t.sorts[*b] || t.sorts[*a]["kind"] != "eq" {
                            return Err("invalid union type".into());
                        }
                    }
                    Op::Hide(i, aa, _) => {
                        for a in aa {
                            check(a, &defined)?;
                        }
                        if aa.len() + 1 != self.tables[*i].schema.len() {
                            return Err("visibility arity".into());
                        }
                    }
                    Op::Panic => {}
                }
            }
        }
        Ok(())
    }
}

// Independently check decoded recipe wiring against the embedded native typed
// source, including stage control and initial facts. Python checks the checksummed
// manifest too; the Rust executable never trusts an entry-plan annotation.
fn verify_source(j: &J) -> Result<()> {
    let ir = &j["recipe"];
    let core = &j["source_core"];
    let mut unsupported = std::collections::BTreeSet::new();
    fn inspect(j: &J, spec: &J, bad: &mut std::collections::BTreeSet<String>) {
        match j {
            J::Array(xs) => {
                for x in xs {
                    inspect(x, spec, bad)
                }
            }
            J::Object(xs) => {
                if let Some(name) = j["primitive"]
                    .as_str()
                    .filter(|_| j["inputs"].is_array() && j["output"].is_string())
                {
                    let base = name.strip_prefix("bool-").unwrap_or(name);
                    let inputs = j["inputs"].as_array().cloned().unwrap_or_default();
                    let output = j["output"].as_str().unwrap_or("");
                    let supported = match base {
                        x if x.starts_with("cell-key-") => {
                            eggfuse_structural_key::signature(j, spec)
                        }
                        "+" | "-" | "*" | "/" | "%" | "min" | "max" => {
                            inputs == [J::from("i64"), J::from("i64")] && output == "i64"
                        }
                        "<" | "<=" | ">" | ">=" => {
                            inputs == [J::from("i64"), J::from("i64")]
                                && matches!(output, "Unit" | "bool")
                        }
                        "=" | "!=" => {
                            inputs.len() == 2
                                && inputs[0] == inputs[1]
                                && matches!(output, "Unit" | "bool")
                        }
                        "not" => inputs == [J::from("bool")] && output == "bool",
                        "and" | "or" | "xor" | "implies" => {
                            inputs == [J::from("bool"), J::from("bool")] && output == "bool"
                        }
                        "set-of" | "set-empty" | "set-insert" | "set-remove" | "set-union"
                        | "set-intersect" | "set-diff" | "set-contains" | "set-not-contains"
                        | "set-length" => true,
                        _ => false,
                    };
                    if !supported {
                        bad.insert(format!("{name} {inputs:?} -> {output}"));
                    }
                }
                for v in xs.values() {
                    inspect(v, spec, bad)
                }
            }
            _ => {}
        }
    }
    inspect(core, &core["structural_key"], &mut unsupported);
    if !unsupported.is_empty() {
        return Err(format!(
            "unsupported primitive capabilities before execution: {}",
            unsupported.into_iter().collect::<Vec<_>>().join("; ")
        ));
    }
    if ir["tables"] != core["tables"]
        || ir["control"] != core["schedule"]
        || j["initial"] != core["initial"]
    {
        return Err("source schema, schedule or initial facts differ".into());
    }
    fn decode(v: &J, slots: &[J]) -> Result<J> {
        if let Some(i) = v["slot"].as_u64() {
            let s = slots.get(i as usize).ok_or("invalid source slot")?;
            Ok(serde_json::json!({"var":s["name"],"sort":s["sort"]}))
        } else {
            Ok(v.clone())
        }
    }
    let rules = array(&ir["rules"])?;
    let mut decoded = vec![];
    for r in rules {
        let slots = array(&r["slots"])?;
        let mut body = vec![];
        let mut head = vec![];
        for atom in array(&r["query"])? {
            let mut a = atom.clone();
            a["args"] = array(&atom["args"])?
                .iter()
                .map(|v| decode(v, slots))
                .collect::<Result<Vec<_>>>()?
                .into();
            body.push(a);
        }
        for effect in array(&r["effects"])? {
            let mut a = effect.clone();
            a.as_object_mut().ok_or("effect object")?.remove("kind");
            if let Some(aa) = effect["args"].as_array() {
                a["args"] = aa
                    .iter()
                    .map(|v| decode(v, slots))
                    .collect::<Result<Vec<_>>>()?
                    .into();
            }
            for k in ["value", "arg"] {
                if !effect[k].is_null() {
                    a[k] = decode(&effect[k], slots)?;
                }
            }
            if let Some(dst) = effect["dst"].as_u64() {
                a["dst"] = slots.get(dst as usize).ok_or("definition slot")?["name"].clone();
            }
            head.push(a);
        }
        let mut out = r["metadata"].clone();
        out["body"] = body.into();
        out["head"] = head.into();
        decoded.push(out);
    }
    let sets = array(&ir["rulesets"])?;
    let source_sets = array(&core["rulesets"])?;
    if sets.len() != source_sets.len() {
        return Err("rulesets differ".into());
    }
    for (set, expected) in sets.iter().zip(source_sets) {
        let mut s = set.clone();
        s.as_object_mut().unwrap().remove("rule_ids");
        s["rules"] = array(&set["rule_ids"])?
            .iter()
            .map(|id| {
                decoded
                    .get(id.as_u64().ok_or("rule ID")? as usize)
                    .cloned()
                    .ok_or("rule missing".into())
            })
            .collect::<Result<Vec<_>>>()?
            .into();
        if s != *expected {
            return Err("recipe query/effect differs from source".into());
        }
    }
    Ok(())
}
