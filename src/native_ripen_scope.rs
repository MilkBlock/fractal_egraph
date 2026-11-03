//! Conservative table reachability for local closure certification. This never
//! treats a failed match, numeric guard, or one observed binding as impossibility.
use super::*;
use crate::closure_contract::LocalDependencies;

pub(super) struct Scope {
    pub dependencies: LocalDependencies,
    pub rules: BTreeSet<usize>,
    pub types: BTreeSet<String>,
    pub blocked: Vec<Json>,
    rule_environment: Vec<String>,
}

fn rule_environment(rules: &[RuleInfo]) -> Vec<String> {
    let mut rules: Vec<_> = rules
        .iter()
        .map(|r| {
            let mut rule = r.rule.clone();
            rule.name.clear();
            rule.to_string()
        })
        .collect();
    rules.sort();
    rules.dedup();
    rules
}

/// Shared by sessions with the same prepared environment: no parsing or rule
/// serialization on each weak-dominate query.
pub(super) struct QueryEnvironment {
    rules: Vec<String>,
    runtime: Option<Json>,
    declarations: BTreeSet<String>,
}
impl QueryEnvironment {
    pub fn new(source: &str, declarations: &[Command], rules: &[RuleInfo]) -> Result<Self> {
        Ok(Self {
            rules: rule_environment(rules),
            runtime: crate::structural_key::source::declaration(source)?
                .map(|h| serde_json::from_str(&h[crate::structural_key::source::HEADER.len()..]))
                .transpose()?,
            declarations: declarations
                .iter()
                .filter(|d| sorts::is_declaration(d))
                .map(ToString::to_string)
                .collect(),
        })
    }
}

/// A positive membership query may inspect a smaller/different slice of the
/// same environment. Its closure-absence guards are not query preconditions.
/// This does not authorize substitution or compare two completed closures.
pub(super) fn compatible_query(
    eg: &EGraph,
    environment: &QueryEnvironment,
    pattern: &crate::saturated_rule_composition::SaturatedRuleComposition,
) -> Result<bool> {
    let contract = &pattern.scope["dependency_contract"];
    if contract["schema"] != "local-closure-dependencies/v1"
        || !contract["rule_environment"]
            .as_array()
            .is_some_and(|rules| {
                rules.len() == environment.rules.len()
                    && rules
                        .iter()
                        .zip(&environment.rules)
                        .all(|(a, b)| a.as_str() == Some(b.as_str()))
            })
    {
        return Ok(false);
    }
    if pattern.scope.get("native_runtime") != environment.runtime.as_ref() {
        return Ok(false);
    }
    if pattern.scope["tables"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|t| {
            t.as_str()
                .is_none_or(|t| !environment.declarations.contains(t))
        })
    {
        return Ok(false);
    }
    for row in &pattern.rows {
        let Some(f) = eg.get_function(&row.op) else {
            return Ok(false);
        };
        let schema = f.schema();
        if schema.input.len() != row.args.len()
            || schema.output.name() != pattern.values[row.result].sort
            || schema
                .input
                .iter()
                .zip(&row.args)
                .any(|(s, i)| s.name() != pattern.values[*i].sort)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

fn calls(e: &Expr, tables: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    if let Expr::Call(_, op, args) = e {
        if tables.contains(op) {
            out.insert(op.clone());
        }
        for a in args {
            calls(a, tables, out);
        }
    }
}
fn reads(r: &Rule, tables: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for f in &r.body {
        match f {
            Fact::Eq(_, a, b) => {
                calls(a, tables, &mut out);
                calls(b, tables, &mut out);
            }
            Fact::Fact(e) => calls(e, tables, &mut out),
        }
    }
    out
}
fn effects(r: &Rule, tables: &BTreeSet<String>) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for a in &r.head.0 {
        match a {
            Action::Expr(_, e) | Action::Let(_, _, e) => calls(e, tables, &mut out),
            Action::Union(_, a, b) => {
                calls(a, tables, &mut out);
                calls(b, tables, &mut out);
            }
            Action::Set(_, op, args, value) => {
                out.insert(op.clone());
                for e in args {
                    calls(e, tables, &mut out);
                }
                calls(value, tables, &mut out);
            }
            Action::Change(_, _, op, args) => {
                out.insert(op.clone());
                for e in args {
                    calls(e, tables, &mut out);
                }
            }
            _ => {}
        }
    }
    out
}
impl Scope {
    pub fn discover(
        eg: &EGraph,
        c: &Captured,
        declarations: &[Command],
        ports: &[(String, egglog::ArcSort, Value)],
    ) -> Result<Self> {
        let tables: BTreeSet<_> = sorts::constructors(declarations)
            .into_iter()
            .map(|(_, v)| v.name)
            .chain(declarations.iter().filter_map(|d| match d {
                Command::Relation { name, .. } | Command::Function { name, .. } => {
                    Some(name.clone())
                }
                _ => None,
            }))
            .collect();
        let mut populated = BTreeSet::new();
        for table in &tables {
            let mut present = false;
            // Include subsumed rows: visibility can change, and exported state
            // still contains their identities and equality evidence.
            eg.function_for_each(table, |_| present = true)?;
            if present {
                populated.insert(table.clone());
            }
        }
        let lhs: Vec<_> = c.rules.iter().map(|r| reads(&r.rule, &tables)).collect();
        let rhs: Vec<_> = c.rules.iter().map(|r| effects(&r.rule, &tables)).collect();
        let merges: BTreeMap<_, _> = declarations
            .iter()
            .filter_map(|d| match d {
                Command::Function {
                    name,
                    merge: Some(expr),
                    ..
                } => {
                    let mut deps = BTreeSet::new();
                    calls(expr, &tables, &mut deps);
                    Some((name.clone(), deps))
                }
                _ => None,
            })
            .collect();
        let mut possible = populated.clone();
        let mut rules = BTreeSet::new();
        loop {
            let size = possible.len();
            let count = rules.len();
            for (i, required) in lhs.iter().enumerate() {
                if required.is_subset(&possible) {
                    rules.insert(i);
                    possible.extend(rhs[i].iter().cloned());
                }
            }
            // Callback reads/constructions participate transitively even if
            // their rows are currently absent. Never certify around a callback.
            for (table, deps) in &merges {
                if possible.contains(table) {
                    possible.extend(deps.iter().cloned());
                }
            }
            if possible.len() == size && rules.len() == count {
                break;
            }
        }
        let mut empty_frontier = BTreeSet::new();
        let mut blocked = vec![];
        for (i, required) in lhs.iter().enumerate() {
            if rules.contains(&i) {
                continue;
            }
            let missing: BTreeSet<_> = required.difference(&possible).cloned().collect();
            empty_frontier.extend(missing.iter().cloned());
            // Retain the whole excluded rule, so a different environment
            // cannot reuse this absence argument just because its name agrees.
            let mut rule = c.rules[i].rule.clone();
            rule.name.clear();
            blocked.push(json!({"rule":rule.to_string(),"missing_tables":missing}));
        }
        blocked.sort_by_key(|v| v.to_string());
        blocked.dedup();
        let mut types = BTreeSet::new();
        fn add_type(sort: &egglog::ArcSort, types: &mut BTreeSet<String>) {
            if types.insert(sort.name().to_owned()) && sort.is_container_sort() {
                for inner in sort.inner_sorts() {
                    add_type(&inner, types);
                }
            }
        }
        for t in &possible {
            let f = eg.get_function(t).ok_or("missing local dependency table")?;
            for s in f
                .schema()
                .input
                .iter()
                .chain(std::iter::once(&f.schema().output))
            {
                add_type(s, &mut types);
            }
        }
        for (_, s, _) in ports {
            add_type(s, &mut types);
        }
        Ok(Self {
            dependencies: LocalDependencies {
                tables: possible,
                empty_frontier,
                rulesets: c.rules.iter().map(|r| r.rule.ruleset.clone()).collect(),
            },
            rules,
            types,
            blocked,
            rule_environment: rule_environment(&c.rules),
        })
    }
    pub fn contract(&self) -> Json {
        json!({"schema":"local-closure-dependencies/v1","dependencies":self.dependencies,"types":self.types,"rule_environment":self.rule_environment,
            "blocked_rules":self.blocked,
            "admission":"all relational premises may be present; binding and numeric guards never used to discard a rule",
            "invalidation":{"table_change":"insert, update, delete or subsume in dependency/frontier tables requires revalidation",
                "union":"any equality identity of the instance, including internal/non-root values; aliases must be followed",
                "environment":"rule definitions, merge callbacks, native primitives or allowed schedule change resets the scope",
                "binding":"new binding, alias partition or primitive parameter value requires a new instance check"},
            "authorization":"isolated closure comparison only; no automatic live tier0 substitution"})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn environment(c: &Captured) -> QueryEnvironment {
        let mut parser = measured_engine();
        QueryEnvironment::new(
            &c.preview_source,
            &parser.parse_program(None, &c.datatype).unwrap(),
            &c.rules,
        )
        .unwrap()
    }
    #[test]
    fn membership_queries_require_matching_rules_types_and_merge_definitions() {
        let source =
            "(datatype E (A) (B)) (function F (E) i64 :merge (min old new)) (rewrite (A) (B)) (A)";
        let c = capture_text_with_sink("scope.egg", source, None, None).unwrap();
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, source).unwrap();
        let declarations = eg.parse_program(None, &c.datatype).unwrap();
        let local = Scope::discover(&eg, &c, &declarations, &[]).unwrap();
        let function = declarations
            .iter()
            .find(|d| matches!(d, Command::Function { .. }))
            .unwrap()
            .to_string();
        let mut pattern = crate::saturated_rule_composition::SaturatedRuleComposition {
            version: 2,
            local_ids: vec![],
            scope: json!({"dependency_contract":local.contract(),"tables":[function]}),
            values: vec![crate::saturated_rule_composition::Vertex {
                sort: "E".into(),
                literal: None,
            }],
            rows: vec![crate::saturated_rule_composition::Row {
                op: "A".into(),
                args: vec![],
                result: 0,
            }],
            subsumed_rows: vec![],
            ports: BTreeMap::new(),
        };
        assert!(compatible_query(&eg, &environment(&c), &pattern).unwrap());
        let different = capture_text_with_sink(
            "merge.egg",
            &source.replace("min old new", "max old new"),
            None,
            None,
        )
        .unwrap();
        assert!(!compatible_query(&eg, &environment(&different), &pattern).unwrap());
        let different = capture_text_with_sink(
            "rule.egg",
            &source.replace("(rewrite (A) (B))", "(rewrite (B) (A))"),
            None,
            None,
        )
        .unwrap();
        assert!(!compatible_query(&eg, &environment(&different), &pattern).unwrap());
        pattern.values[0].sort = "Other".into();
        assert!(!compatible_query(&eg, &environment(&c), &pattern).unwrap());
    }
}
