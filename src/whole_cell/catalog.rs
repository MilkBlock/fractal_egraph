//! Analyze catalogs supply observed coarse contexts. Native saturation and
//! independent delta replay recheck them; no hand-authored transition plan.
use super::*;
use std::path::Path;
const HEADER: &str = "; @ripsat-certificates ";

fn source_environment(source: &str) -> Result<(String, String)> {
    let mut parser = EGraph::default();
    let cs = parser.parse_program(None, source)?;
    let mut sets = BTreeSet::new();
    let mut env = vec![];
    for c in cs {
        match &c {
            Command::Rule { rule } => {
                sets.insert(rule.ruleset.clone());
                env.push(c.to_string());
            }
            Command::Rewrite(set, ..) | Command::BiRewrite(set, ..) => {
                sets.insert(set.clone());
                env.push(c.to_string());
            }
            Command::Datatype { .. }
            | Command::Constructor { .. }
            | Command::Sort { .. }
            | Command::AddRuleset(..) => env.push(c.to_string()),
            Command::Action(_) | Command::RunSchedule(_) | Command::Check(..) => (),
            _ => {
                return Err(
                    "finite certificate importer requires a positive declaration/rule environment"
                        .into(),
                );
            }
        }
    }
    if sets.len() != 1 {
        return Err("finite certificate needs one complete positive ruleset; parameterized schemes use transfers.egg".into());
    }
    let env = env.join("\n");
    let set = sets.pop_first().unwrap();
    environment(&env, &set)?;
    Ok((env, set))
}
fn entry(origin: &Json, name: String) -> Result<Stage> {
    if origin["excluded_seed_rules"]
        .as_array()
        .is_some_and(|x| !x.is_empty())
    {
        return Err("entry omitted original rules".into());
    }
    if origin["comb_members"].as_array().is_none_or(Vec::is_empty) {
        return Err("entry lacks observed rule composition".into());
    }
    let marker_sorts = origin["parameter_markers"]
        .as_object()
        .ok_or("missing typed opaque parameter declarations")?
        .iter()
        .map(|(sort, name)| {
            Ok((
                name.as_str().ok_or("invalid parameter marker")?.to_string(),
                sort.clone(),
            ))
        })
        .collect::<Result<BTreeMap<_, _>>>()?;
    let mut parser = EGraph::default();
    let mut actions = vec![];
    for c in parser.parse_program(
        None,
        origin["initial_source"]
            .as_str()
            .ok_or("missing initial source")?,
    )? {
        if let Command::Action(a) = c {
            if matches!(&a,Action::Expr(_,Expr::Call(_,f,_)) if marker_sorts.contains_key(f)) {
                continue;
            }
            actions.push(a);
        }
    }
    for stage in origin["staged_injections"]
        .as_array()
        .ok_or("missing seed forest")?
    {
        for fact in stage["facts"].as_array().ok_or("missing injected fact")? {
            for c in parser.parse_program(None, fact.as_str().ok_or("invalid fact")?)? {
                if let Command::Action(a) = c {
                    actions.push(a)
                } else {
                    return Err("non-fact in seed forest".into());
                }
            }
        }
    }
    let mut params = BTreeMap::new();
    let mut parameters = vec![];
    let mut bind = |e: Expr| {
        e.visit_exprs(&mut |e| {
            if let Expr::Call(s, f, _) = &e {
                if let Some(sort) = marker_sorts.get(f) {
                    let key = e.to_string();
                    let next = params.len();
                    let name = params
                        .entry(key)
                        .or_insert_with(|| {
                            let n = format!("p{next}");
                            parameters.push(Parameter {
                                name: n.clone(),
                                sort: sort.clone(),
                            });
                            n
                        })
                        .clone();
                    return Expr::Var(s.clone(), name);
                }
            }
            e
        })
    };
    let mut premises = vec![];
    let mut graft = None;
    for mut a in actions {
        match &mut a {
            Action::Expr(_, e) => {
                *e = bind(e.clone());
                if graft.is_none() && matches!(e, Expr::Call(..)) {
                    graft = Some(e.to_string())
                }
            }
            Action::Union(_, a, b) => {
                *a = bind(a.clone());
                *b = bind(b.clone());
                if graft.is_none() && matches!(a, Expr::Call(..)) {
                    graft = Some(a.to_string())
                }
            }
            _ => {
                return Err("seed requires an effect other than constructor facts/equality".into());
            }
        }
        let text = a.to_string();
        if !premises.contains(&text) {
            premises.push(text)
        }
    }
    Ok(Stage {
        name,
        parent: None,
        parameters,
        graft: graft.ok_or("no constructor seed")?,
        premises,
    })
}

impl Library {
    pub fn from_analysis(dir: &Path) -> Result<(Self, Json)> {
        let (env, ruleset) = source_environment(&std::fs::read_to_string(dir.join("source.egg"))?)?;
        let catalog: Json =
            serde_json::from_slice(&std::fs::read(dir.join("catalog/catalog.json"))?)?;
        let mut stages = vec![];
        let mut rejected = vec![];
        let mut origins = vec![];
        let mut seen = BTreeMap::new();
        for (i, trigger) in catalog["triggers"]
            .as_array()
            .ok_or("missing Analyze triggers")?
            .iter()
            .enumerate()
        {
            let result = (|| -> Result<Stage> {
                let state = trigger["saturated_rule_composition"]
                    .as_u64()
                    .ok_or("missing completed state")?;
                let body: State = crate::saturated_rule_composition::read(
                    &dir.join(format!("catalog/states/state-{state:04}.json")),
                )?;
                body.validate()?;
                if body.scope["semantics"] != "positive-constructor-equality/v1"
                    || body.scope["observation_only"] == true
                    || !body.subsumed_rows.is_empty()
                    || body
                        .scope
                        .get("symbolic_size_conditions")
                        .is_some_and(|v| !v.is_null() && v != &Json::Bool(false))
                {
                    return Err("state is not a complete positive symbolic closure".into());
                }
                entry(&trigger["binding_origin"], format!("observed_{i}"))
            })();
            match result {
                Ok(stage) => {
                    let signature = serde_json::to_string(&(
                        stage.parameters.clone(),
                        &stage.graft,
                        &stage.premises,
                    ))?;
                    let name = seen
                        .entry(signature)
                        .or_insert_with(|| {
                            let name = stage.name.clone();
                            stages.push(stage);
                            name
                        })
                        .clone();
                    origins.push(json!({"stage":name,"trigger":i,"candidate_id":trigger["binding_origin"]["candidate_id"],"catalog_state":trigger["saturated_rule_composition"]}));
                }
                Err(e) => rejected.push(json!({"trigger":i,"reason":e.to_string()})),
            }
        }
        if stages.is_empty() {
            return Err(format!("no admitted finite RipSat certificates: {rejected:?}").into());
        }
        let plan = Plan { ruleset, stages };
        let lib = Self::bake(&env, &plan)?;
        for o in &mut origins {
            o["certificate"] = json!(
                lib.steps
                    .iter()
                    .find(|s| s.name == o["stage"].as_str().unwrap())
                    .unwrap()
                    .target
            );
        }
        // A closed graph can have several distinguished roots/port orders. Keep
        // those executable interfaces separate, but independently verify every
        // old catalog family before the spine solver shares its denotation.
        let mut family = BTreeMap::new();
        for o in &origins {
            let id = o["certificate"].as_u64().unwrap() as usize;
            let old = o["catalog_state"].as_u64().unwrap();
            let mut state = lib.states[id].graph().clone();
            state.ports.clear();
            if let Some(previous) = family.get(&old) {
                if !matches!(
                    graph::compare(previous, &state)?,
                    Comparison::Equivalent { .. }
                ) {
                    return Err(
                        "Analyze family contradicts independently rechecked closure graphs".into(),
                    );
                }
            } else {
                family.insert(old, state);
            }
        }
        let evidence = json!({"source":"native Analyze catalog","origins":origins,"rejected":rejected,"unique_entries":lib.steps.len(),"family_equivalence_verified":true,
            "verification":"fresh native saturation plus independent delta replay; catalog flags alone do not authorize a shortcut"});
        Ok((lib, evidence))
    }
    pub fn certificate_text(&self, evidence: Json) -> Result<String> {
        let plan = Plan {
            ruleset: self.ruleset.clone(),
            stages: self
                .steps
                .iter()
                .map(|s| Stage {
                    name: s.name.clone(),
                    parent: s.source.map(|source| {
                        self.steps
                            .iter()
                            .find(|p| p.target == source)
                            .unwrap()
                            .name
                            .clone()
                    }),
                    parameters: s.parameters.clone(),
                    graft: s.graft.clone(),
                    premises: s.premises.clone(),
                })
                .collect(),
        };
        let states = self
            .states
            .iter()
            .map(|s| {
                let mut s = s.graph().clone();
                s.scope = Json::Null;
                s
            })
            .collect::<Vec<_>>();
        Ok(format!(
            "{HEADER}{}\n{}",
            serde_json::to_string(
                &json!({"version":1,"kind":"finite","plan":plan,"states":states,"evidence":evidence})
            )?,
            self.environment
        ))
    }
    /// Verification is a one-time cold-load operation. It does not run the
    /// original queries at each marker hit. No unchecked serialized macro loads.
    pub fn load_certificate(path: &Path) -> Result<Self> {
        Self::parse_certificate(&std::fs::read_to_string(path)?)
    }
    pub fn parse_certificate(text: &str) -> Result<Self> {
        let (line, env) = text
            .split_once('\n')
            .ok_or("missing certificate environment")?;
        let meta: Json = serde_json::from_str(
            line.strip_prefix(HEADER)
                .ok_or("missing certificate header")?,
        )?;
        if meta["version"] != 1 || meta["kind"] != "finite" {
            return Err("unsupported finite certificate version".into());
        }
        let plan: Plan = serde_json::from_value(meta["plan"].clone())?;
        let lib = Self::bake(env, &plan)?;
        let states: Vec<State> = serde_json::from_value(meta["states"].clone())?;
        if states.len() != lib.states.len() {
            return Err("certificate state count differs".into());
        }
        for (mut expected, actual) in states.into_iter().zip(&lib.states) {
            expected.scope = actual.graph().scope.clone();
            if !matches!(
                graph::compare(&expected, actual.graph())?,
                Comparison::Equivalent { .. }
            ) {
                return Err("certificate body does not follow from seed/rules".into());
            }
        }
        Ok(lib)
    }
}

pub fn bake_file(analysis: &Path, output: &Path) -> Result<Json> {
    if output.exists() {
        return Err("certificate output exists".into());
    }
    if analysis.join("transfers.egg").is_file() {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let b = cell_assisted_egglog::parameterized_cell::artifact::Bundle::load(
                &analysis.join("transfers.egg"),
            )?;
            let r = b.instantiate()?;
            std::fs::write(output, b.text()?)?;
            return Ok(
                json!({"kind":"parameterized","library":b.report(),"certificates":r.report()["certificates"]}),
            );
        }
    }
    let (lib, evidence) = Library::from_analysis(analysis)?;
    std::fs::write(output, lib.certificate_text(evidence.clone())?)?;
    Ok(json!({"kind":"finite","evidence":evidence,"certificates":lib.certificates.report()}))
}

pub fn run_file(library: &Path, input: &Path, out: &Path) -> Result<Json> {
    let text = std::fs::read_to_string(library)?;
    if text
        .lines()
        .next()
        .is_some_and(|s| s.contains("\"kind\":\"parameterized\""))
        || text.contains("; @cell-transfer ")
    {
        #[cfg(not(target_arch = "wasm32"))]
        return crate::native_analyze::transfer::run_file(library, input, out);
        #[cfg(target_arch = "wasm32")]
        return Err("parameterized certificates require native execution".into());
    }
    let start = Instant::now();
    let lib = Library::parse_certificate(&text)?;
    let checked_load_seconds = start.elapsed().as_secs_f64();
    let mut runtime = automatic::AutomaticRuntime::new(lib, "")?;
    let commands = runtime.engine().clone().parse_program(
        Some(input.display().to_string()),
        &std::fs::read_to_string(input)?,
    )?;
    std::fs::create_dir(out)?;
    let mut boundaries = vec![];
    for c in commands {
        match &c {
            Command::Action(_) => {
                runtime.commit(&c.to_string())?;
            }
            Command::RunSchedule(_) => {
                boundaries.push(runtime.run(&c.to_string())?);
            }
            Command::Check(..) => {
                runtime.engine().clone().run_program(vec![c])?;
            }
            Command::Fail(_, inner) if matches!(**inner, Command::Check(..)) => {
                runtime.engine().clone().run_program(vec![c])?;
            }
            _ => {
                return Err("certificate input accepts facts, checks and original schedules".into());
            }
        }
    }
    let report = json!({"passed":true,"kind":"finite","checked_load_seconds":checked_load_seconds,
        "load_verification":"native saturation and delta replay; charged once, never on marker hits",
        "boundaries":boundaries,"runtime":runtime.report()});
    crate::pipeline::write_json(&out.join("results.json"), &report)?;
    Ok(report)
}
