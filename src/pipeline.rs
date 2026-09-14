//! Native adapters for the current tier-1/tier-2 pipeline.
//! Legacy executable names delegate here; there is one implementation per operation.
use egglog::ast::{Action, Command, Expr, Fact, Span};
use serde_json::{Value, json};
use std::collections::BTreeMap;
pub type Result<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Execute the tier-2 relations in native egglog, exporting witnessed repeats.
pub fn relations(input: &str, output: &str) -> Result {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(Some(input.to_owned()), &std::fs::read_to_string(input)?)?;
    let mut counts = serde_json::Map::new();
    for name in [
        "ParentRole",
        "Outside",
        "Construct",
        "End",
        "Next",
        "Schema",
        "Extend",
        "Translate",
        "Compose",
        "Power",
        "PreservesLinear",
        "At",
        "Before",
        "Repeat",
        "Fits",
        "Counterexample",
        "NeedsBoundary",
        "InjectionEdge",
    ] {
        let mut count = 0;
        eg.function_for_each(name, |r| {
            if !r.subsumed {
                count += 1
            }
        })?;
        counts.insert(name.into(), json!(count));
    }
    let mut repeats = vec![];
    eg.function_for_each("Repeat",|r|repeats.push(json!({"start":eg.value_to_base::<i64>(r.vals[0]),"end":eg.value_to_base::<i64>(r.vals[1]),"slot":eg.value_to_base::<i64>(r.vals[3]),"length":eg.value_to_base::<i64>(r.vals[4])})))?;
    repeats.sort_by_key(|r| (r["start"].as_i64(), r["end"].as_i64(), r["length"].as_i64()));
    let mut counterexamples = vec![];
    eg.function_for_each("Counterexample",|r|counterexamples.push(json!({"series":eg.value_to_base::<egglog::sort::S>(r.vals[0]).as_str(),"before":[eg.value_to_base::<i64>(r.vals[1]),eg.value_to_base::<i64>(r.vals[2])],"after":[eg.value_to_base::<i64>(r.vals[3]),eg.value_to_base::<i64>(r.vals[4])]})))?;
    std::fs::write(
        output,
        serde_json::to_string_pretty(
            &json!({"scope":"Native tier-2 relation evaluation; witnessed repeats and finite translation checks, no inferred equivalence unions.","counts":counts,"repeats":repeats,"counterexamples":counterexamples}),
        )? + "\n",
    )?;
    Ok(())
}

/// Extract FractalComb views derived by native power folding of tier-1 edges.
pub fn higher() -> Result {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        &std::fs::read_to_string("experiments/tier2/higher.egg")?,
    )?;
    let mut cn = BTreeMap::new();
    let mut en = BTreeMap::new();
    eg.function_for_each("CombName", |r| {
        cn.insert(
            r.vals[0],
            eg.value_to_base::<egglog::sort::S>(r.vals[1])
                .as_str()
                .to_owned(),
        );
    })?;
    eg.function_for_each("ExtensionName", |r| {
        en.insert(
            r.vals[0],
            eg.value_to_base::<egglog::sort::S>(r.vals[1])
                .as_str()
                .to_owned(),
        );
    })?;
    let mut depths = BTreeMap::new();
    eg.function_for_each("Depth", |r| {
        depths.insert(r.vals[1], eg.value_to_base::<i64>(r.vals[0]));
    })?;
    let mut hs = BTreeMap::new();
    eg.function_for_each("FractalComb",|r|{
  let k=depths[&r.vals[0]];let binding=eg.extract_value_to_string(eg.get_sort_by_name("RelativeBinding").unwrap(),r.vals[3]).unwrap().0;
  hs.insert(r.vals[4],json!({"count":k,"operator":en[&r.vals[1]],"ctx":cn[&r.vals[2]],"binding":binding,"egg":format!("(FractalComb (Depth {k}) ${} {} {binding})",en[&r.vals[1]],cn[&r.vals[2]]),"represents":[]}));
 })?;
    eg.function_for_each("Represents", |r| {
        hs.get_mut(&r.vals[0]).unwrap()["represents"]
            .as_array_mut()
            .unwrap()
            .push(json!(cn[&r.vals[1]]))
    })?;
    let mut values = hs.into_values().collect::<Vec<_>>();
    values.sort_by_key(|r| {
        (
            -(r["count"].as_i64().unwrap()),
            r["ctx"].as_str().unwrap().to_owned(),
            r["operator"].as_str().unwrap().to_owned(),
        )
    });
    std::fs::write(
        "experiments/tier2/higher_native.json",
        serde_json::to_string_pretty(
            &json!({"scope":"Native FractalComb(k, extension, ctx, initial binding) views of existing closed unary tier-1 paths. No new arbitrary-k rule applications and no full-effect equality claims beyond witnessed folding.","higher_rules":values}),
        )? + "\n",
    )?;
    Ok(())
}

fn collect(eg: &egglog::EGraph) -> BTreeMap<String, (String, usize)> {
    let mut out = BTreeMap::new();
    eg.function_for_each("ReduceExample", |r| {
        let name = eg
            .value_to_base::<egglog::sort::S>(r.vals[0])
            .as_str()
            .to_owned();
        let (term, cost) = eg
            .extract_value_to_string(eg.get_sort_by_name("EndpointExpr").unwrap(), r.vals[1])
            .unwrap();
        out.insert(name, (term, cost as usize));
    })
    .unwrap();
    out
}
/// Native cost extraction, before and after endpoint reduction.
pub fn reduce() -> Result {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(
        None,
        &std::fs::read_to_string("experiments/tier2/reduce.egg")?,
    )?;
    let before = collect(&eg);
    eg.parse_and_run_program(None, "(run-schedule (saturate (run endpoint-reduce)))")?;
    let after = collect(&eg);
    let mut rows = vec![];
    for (name, (term, cost)) in after {
        let closed = !term.contains("(Reduce ");
        let expected = matches!(
            name.as_str(),
            "constant-add" | "arithmetic-add" | "constant-multiply"
        );
        assert_eq!(closed, expected, "unexpected reduction for {name}");
        if expected {
            assert!(cost < before[&name].1);
        }
        rows.push(json!({"name":name,"before":before[&name].0,"before_cost":before[&name].1,"after":term,"after_cost":cost,"closed_form":closed}));
    }
    std::fs::write(
        "experiments/tier2/reduce.json",
        serde_json::to_string_pretty(
            &json!({"scope":"Native egglog constructor-cost extraction. Exact scalar endpoint identities; neither removal of intermediate runtime nodes nor automatic FractalComb accumulator discovery.","examples":rows}),
        )? + "\n",
    )?;
    Ok(())
}

/// Read recurrence coordinate transitions only after native tier-1 binding checks.
pub fn observations() -> Result {
    let path = "experiments/tier2/recurrence_tier1.egg";
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(Some(path.into()), &std::fs::read_to_string(path)?)?;
    let mut samples = vec![];
    eg.function_for_each("ObservedStep",|r|samples.push(json!({"series":eg.value_to_base::<egglog::sort::S>(r.vals[0]).as_str(),"depth":eg.value_to_base::<i64>(r.vals[1]),"before":[eg.value_to_base::<i64>(r.vals[2]),eg.value_to_base::<i64>(r.vals[3])],"after":[eg.value_to_base::<i64>(r.vals[4]),eg.value_to_base::<i64>(r.vals[5])]})))?;
    samples.sort_by_key(|s| {
        (
            s["series"].as_str().unwrap().to_owned(),
            s["depth"].as_i64().unwrap(),
        )
    });
    std::fs::write(
        "experiments/tier2/observations.json",
        serde_json::to_string_pretty(
            &json!({"scope":"Native tier-1 ObservedStep rows after all Binding checks. Coordinates are an additive-recursion adapter over checked native tier-0 edges, not a general automatic state discovery.","samples":samples}),
        )? + "\n",
    )?;
    Ok(())
}

/// Observe edges in a finite native recurrence run; check their endpoint equality.
pub fn fixture(input: &str, output: &str) -> Result {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(Some(input.to_owned()), &std::fs::read_to_string(input)?)?;
    let mut edges = vec![];
    eg.function_for_each("Edge", |r| {
        edges.push([
            eg.value_to_base::<i64>(r.vals[0]),
            eg.value_to_base::<i64>(r.vals[1]),
            eg.value_to_base::<i64>(r.vals[2]),
        ])
    })?;
    edges.sort();
    for [n, m, c] in &edges {
        eg.parse_and_run_program(
            None,
            &format!("(check (= (F {n}) (Add (F {m}) (Const {c}))))"),
        )?;
    }
    eg.parse_and_run_program(
        None,
        r#"
 (ruleset endpoint-normalize)
 (rewrite (Add (Add x y) z) (Add x (Add y z)) :ruleset endpoint-normalize)
 (rewrite (Add (Const x) (Const y)) (Const (+ x y)) :ruleset endpoint-normalize)
 (run-schedule (saturate (run endpoint-normalize)))
 "#,
    )?;
    let start = edges.iter().map(|e| e[0]).max().ok_or("no edges")?;
    let mut m = start;
    let mut total = 0i64;
    let mut path_checks = 0;
    let mut seen = std::collections::BTreeSet::new();
    while let Some(edge) = edges.iter().find(|e| e[0] == m) {
        assert!(seen.insert(m), "cyclic fixture path");
        m = edge[1];
        total = total.checked_add(edge[2]).ok_or("accumulator overflow")?;
        eg.parse_and_run_program(
            None,
            &format!("(check (= (F {start}) (Add (F {m}) (Const {total}))))"),
        )?;
        path_checks += 1;
    }
    let result = json!({"source":input,"scope":"Actual native egglog fixture. Edge instrumentation records recurrence steps; endpoint equalities checked. Accumulated path weight is derived later, not a recorded runtime binding.","edges":edges,"endpoint_checks":edges.len(),"multi_step_endpoint_checks":path_checks});
    std::fs::write(output, serde_json::to_string_pretty(&result)? + "\n")?;
    Ok(())
}

fn expr(e: &Expr) -> Value {
    match e {
        Expr::Var(_, v) => json!({"var":v}),
        Expr::Lit(_, l) => json!({"literal":l.to_string()}),
        Expr::Call(s, op, args) => {
            let span = if let Span::Egglog(s) = s {
                format!("{:?}:{}:{}", s.file.name, s.i, s.j)
            } else {
                String::new()
            };
            json!({"op":op,"args":args.iter().map(expr).collect::<Vec<_>>(),"span":span})
        }
    }
}
/// Export original rule ASTs with source positions using egglog's own parser.
pub fn schema(input: &str, output: &str) -> Result {
    let path = input;
    let mut eg = egglog::EGraph::default();
    let commands = eg.parse_program(Some(path.to_owned()), &std::fs::read_to_string(path)?)?;
    let mut rules = serde_json::Map::new();
    let mut index = 0;
    for c in commands {
        if let Command::Rule { rule } = crate::visual_rule::normalize(c, index) {
            let body: Vec<_> = rule
                .body
                .iter()
                .map(|f| match f {
                    Fact::Eq(_, a, b) => json!({"eq":[expr(a),expr(b)]}),
                    Fact::Fact(e) => json!({"fact":expr(e)}),
                })
                .collect();
            let head: Vec<_> = rule
                .head
                .0
                .iter()
                .map(|a| match a {
                    Action::Union(_, a, b) => json!({"union":[expr(a),expr(b)]}),
                    _ => json!({"unsupported":a.to_string()}),
                })
                .collect();
            rules.insert(rule.name.clone(), json!({"body":body,"head":head}));
            index += 1;
        }
    }
    std::fs::write(output, serde_json::to_string_pretty(&rules)? + "\n")?;
    Ok(())
}

/// Execute the native logical-array DSL and extract its named results. This is
/// a tier2 experiment, not tier0 capture or a GPU lowering path.
pub fn arrays(input: &str, output: &str) -> Result {
    let mut eg = egglog::EGraph::default();
    eg.parse_and_run_program(Some(input.to_owned()), &std::fs::read_to_string(input)?)?;
    let sort = eg
        .get_sort_by_name("EndpointExpr")
        .ok_or("input must load rules/tier2.egg")?;
    let extractor = egglog::extract::Extractor::compute_costs_from_rootsorts(
        Some(vec![sort.clone()]),
        &eg,
        egglog::extract::TreeAdditiveCostModel::default(),
    );
    let mut dag = egglog::TermDag::default();
    let mut examples = BTreeMap::new();
    eg.function_for_each("ArrayExample", |row| {
        let name = eg
            .value_to_base::<egglog::sort::S>(row.vals[0])
            .as_str()
            .to_owned();
        let value = extractor
            .extract_best(&eg, &mut dag, row.vals[1])
            .map(|(cost, term)| json!({"expression":dag.to_string(term),"cost":cost}));
        examples.insert(name, value);
    })?;
    let report = json!({"scope":"Native egglog tier2 logical scalar arrays. Symbolic rewrites and extraction; no tier0 trace capture, automatic Bake, GPU memory scheduling or FlashAttention search.","examples":examples,"element_queries":eg.get_size("ArrayAt"),"fold_prefixes":eg.get_size("FoldPrefix")+eg.get_size("ReducePrefix"),"block_views":eg.get_size("ReduceBlocks")});
    if let Some(parent) = std::path::Path::new(output)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    serde_json::to_writer_pretty(file, &report)?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
