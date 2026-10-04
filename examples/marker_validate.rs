//! Instance legality audit: marker entailment at consumer boundaries, source
//! candidate admissibility, and read-only membership of extracted function roots.
use egg_layout::pipeline::Result;
use egglog::{
    EGraph, Value,
    ast::{Command, Expr, Fact, GenericSchedule},
};
use serde_json::{Value as Json, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
fn rows(eg: &EGraph, name: &str) -> Result<Vec<Vec<Value>>> {
    let f = eg
        .get_function(name)
        .ok_or_else(|| format!("missing table {name}"))?;
    let sorts: Vec<_> = f
        .schema()
        .input
        .iter()
        .chain([&f.schema().output])
        .cloned()
        .collect();
    let mut result = vec![];
    eg.function_for_each(name, |r| {
        if !r.subsumed {
            result.push(
                r.vals
                    .iter()
                    .zip(&sorts)
                    .map(|(v, s)| eg.get_canonical_value(*v, s))
                    .collect(),
            );
        }
    })?;
    Ok(result)
}
fn marker_check(eg: &EGraph, m: &Json, complete: bool) -> Result<Json> {
    let outer = m["outer"].as_str().ok_or("outer")?;
    let inner = m["inner"].as_str().ok_or("inner")?;
    let marker = m["known_marker"].as_str().ok_or("marker")?;
    let slot = m["slot"].as_u64().ok_or("slot")? as usize;
    let inner_rows = rows(eg, inner)?;
    let mut by_result: BTreeMap<Value, Vec<Vec<Value>>> = BTreeMap::new();
    for row in inner_rows {
        by_result
            .entry(*row.last().unwrap())
            .or_default()
            .push(row[..row.len() - 1].to_vec());
    }
    let mut expected = BTreeSet::new();
    for row in rows(eg, outer)? {
        if let Some(inside) = by_result.get(&row[slot]) {
            for args in inside {
                let tuple = row[..row.len() - 1]
                    .iter()
                    .enumerate()
                    .flat_map(|(i, v)| if i == slot { args.clone() } else { vec![*v] })
                    .collect::<Vec<_>>();
                expected.insert(tuple);
            }
        }
    }
    let actual: BTreeSet<_> = rows(eg, marker)?
        .into_iter()
        .map(|mut r| {
            r.pop();
            r
        })
        .collect();
    let extra: Vec<_> = actual
        .difference(&expected)
        .map(|r| format!("{r:?}"))
        .collect();
    let missing: Vec<_> = expected
        .difference(&actual)
        .map(|r| format!("{r:?}"))
        .collect();
    Ok(
        json!({"expected":expected.len(),"actual":actual.len(),"invalid":extra.len(),"missing":missing.len(),"sound":extra.is_empty(),"complete":!complete||missing.is_empty(),"completeness_required":complete,"invalid_examples":extra.into_iter().take(3).collect::<Vec<_>>(),"missing_examples":missing.into_iter().take(3).collect::<Vec<_>>()}),
    )
}
struct Root {
    value: Value,
    term: String,
}
struct Audit {
    eg: EGraph,
    roots: BTreeMap<String, Root>,
    checks: Vec<Json>,
    consumers: BTreeSet<String>,
    marker: Option<Json>,
}
impl Audit {
    fn capture_roots(&mut self) -> Result {
        let sort = self
            .eg
            .get_sort_by_name("Expr")
            .ok_or("Expr missing")?
            .clone();
        for row in rows(&self.eg, "Function")? {
            let name = self.eg.value_to_base::<egglog::sort::S>(row[0]).to_string();
            let value = *row.last().unwrap();
            if let Some(old) = self.roots.get(&name) {
                if old.value != value {
                    return Err("ambiguous initialized function root".into());
                }
            } else {
                let term = self.eg.extract_value_to_string(&sort, value)?.0;
                self.roots.insert(name, Root { value, term });
            }
        }
        Ok(())
    }
    fn schedule(&mut self, s: &egglog::ast::Schedule) -> Result<(bool, bool)> {
        use GenericSchedule::*;
        match s {
            Run(_, r) => {
                if self.consumers.contains(&r.ruleset) {
                    self.checks
                        .push(marker_check(&self.eg, self.marker.as_ref().unwrap(), true)?);
                }
                let outputs = self.eg.run_program(vec![Command::RunSchedule(s.clone())])?;
                let report = outputs
                    .into_iter()
                    .find_map(|o| {
                        if let egglog::CommandOutput::RunSchedule(r) = o {
                            Some(r)
                        } else {
                            None
                        }
                    })
                    .ok_or("run report absent")?;
                if r.ruleset == "initialization" && self.roots.is_empty() {
                    self.capture_roots()?;
                }
                if let Some(m) = &self.marker {
                    self.checks.push(marker_check(&self.eg, m, false)?);
                }
                Ok((report.updated, report.can_stop))
            }
            Sequence(_, xs) => {
                let (mut updated, mut stop) = (false, true);
                for x in xs {
                    let (u, c) = self.schedule(x)?;
                    updated |= u;
                    stop &= c;
                }
                Ok((updated, stop))
            }
            Repeat(_, n, x) => {
                let (mut updated, mut stop) = (false, true);
                for _ in 0..*n {
                    let (u, c) = self.schedule(x)?;
                    updated |= u;
                    stop &= c;
                    if c {
                        break;
                    }
                }
                Ok((updated, stop))
            }
            Saturate(_, x) => {
                let (mut any, mut stop) = (false, true);
                loop {
                    let (u, c) = self.schedule(x)?;
                    any |= u;
                    stop &= c;
                    if !u {
                        break;
                    }
                }
                Ok((any, stop))
            }
        }
    }
    fn run(source: &str, marker: Option<Json>) -> Result<Self> {
        let mut eg = EGraph::default();
        let commands = egg_layout::visual_rule::surface_program(eg.parse_program(None, source)?);
        let mut consumers = BTreeSet::new();
        let mut combined = BTreeMap::new();
        if let Some(m) = &marker {
            let target = m["known_marker"].as_str().unwrap();
            for c in &commands {
                match c {
                    Command::Rule { rule } => {
                        if rule
                            .body
                            .iter()
                            .any(|f| matches!(f,Fact::Fact(Expr::Call(_,op,_)) if op==target))
                        {
                            consumers.insert(rule.ruleset.clone());
                        }
                    }
                    Command::UnstableCombinedRuleset(_, name, xs) => {
                        combined.insert(name.clone(), xs.clone());
                    }
                    _ => {}
                }
            }
            loop {
                let n = consumers.len();
                for (name, xs) in &combined {
                    if xs.iter().any(|x| consumers.contains(x)) {
                        consumers.insert(name.clone());
                    }
                }
                if n == consumers.len() {
                    break;
                }
            }
        }
        let mut a = Self {
            eg,
            roots: BTreeMap::new(),
            checks: vec![],
            consumers,
            marker,
        };
        for c in commands {
            match c {
                Command::RunSchedule(s) => {
                    a.schedule(&s)?;
                }
                other => {
                    a.eg.run_program(vec![other])?;
                }
            }
        }
        if a.roots.is_empty() {
            return Err("no initialized Function roots; expected an eggcc export".into());
        }
        Ok(a)
    }
    fn terms(&self) -> Result<BTreeMap<String, String>> {
        let sort = self.eg.get_sort_by_name("Expr").unwrap();
        self.roots
            .iter()
            .map(|(n, r)| Ok((n.clone(), self.eg.extract_value_to_string(sort, r.value)?.0)))
            .collect()
    }
}
fn hoist_check(eg: &EGraph) -> Result<Json> {
    if eg.get_function("to-hoist").is_none() {
        return Ok(Json::Null);
    }
    let invariants: BTreeSet<_> = rows(eg, "is-inv-Expr")?
        .into_iter()
        .map(|r| (r[0], r[1]))
        .collect();
    let loops: BTreeSet<_> = rows(eg, "DoWhile")?
        .into_iter()
        .map(|r| (r[0], r[1]))
        .collect();
    let base: BTreeSet<_> = rows(eg, "Base")?
        .into_iter()
        .map(|r| *r.last().unwrap())
        .collect();
    let typed: BTreeSet<_> = rows(eg, "HasType")?
        .into_iter()
        .filter(|r| base.contains(&r[1]))
        .map(|r| r[0])
        .collect();
    let sizes: BTreeMap<_, _> = rows(eg, "Expr-size")?
        .into_iter()
        .map(|r| (r[0], r[1]))
        .collect();
    let maxima: BTreeMap<_, _> = rows(eg, "to-hoist-size")?
        .into_iter()
        .map(|r| ((r[0], r[1]), r[2]))
        .collect();
    let mut failures = vec![];
    let mut choices = vec![];
    let mut count = 0;
    for r in rows(eg, "to-hoist")? {
        count += 1;
        choices.push(json!({"expression":eg.extract_value_to_string(eg.get_sort_by_name("Expr").unwrap(),r[2]).ok().map(|x|x.0),"size":sizes.get(&r[2]).map(|v|eg.value_to_base::<i64>(*v)),"maximum":maxima.get(&(r[0],r[1])).map(|v|eg.value_to_base::<i64>(*v))}));
        if !(loops.contains(&(r[0], r[1]))
            && invariants.contains(&(r[1], r[2]))
            && typed.contains(&r[2])
            && sizes
                .get(&r[2])
                .is_some_and(|s| Some(s) == maxima.get(&(r[0], r[1]))))
        {
            failures.push(format!("{r:?}"));
        }
    }
    Ok(
        json!({"entries":count,"currently_admissible":count-failures.len(),"failures":failures,"choices":choices,"scope":"final-state original selection predicate; does not establish historical write validity when premises later change"}),
    )
}
fn program(terms: &BTreeMap<String, String>) -> Result<String> {
    let main = terms.get("main").ok_or("no main function")?;
    let list = terms
        .iter()
        .filter(|(n, _)| n.as_str() != "main")
        .rev()
        .fold("(Nil)".to_string(), |tail, (_, t)| {
            format!("(Cons {t} {tail})")
        });
    Ok(format!("(Program {main} {list})"))
}
#[path = "../experiments/eggcc_cell_probe/snapshot.rs"]
mod snapshot;
fn main() -> Result {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("BASELINE.egg CANDIDATE.egg MANIFEST.json OUTPUT_DIR".into());
    }
    let out = Path::new(&args[4]);
    std::fs::create_dir(out)?;
    let manifest: Json = serde_json::from_slice(&std::fs::read(&args[3])?)?;
    let mut original = Audit::run(&std::fs::read_to_string(&args[1])?, None)?;
    let candidate = Audit::run(&std::fs::read_to_string(&args[2])?, Some(manifest))?;
    let mut plain = EGraph::default();
    plain.parse_and_run_program(None, &std::fs::read_to_string(&args[2])?)?;
    let a =
        snapshot::snapshot_filtered(&plain, &BTreeSet::new(), "walk-control", &BTreeSet::new())?;
    let b = snapshot::snapshot_filtered(
        &candidate.eg,
        &BTreeSet::new(),
        "walk-control",
        &BTreeSet::new(),
    )?;
    let control_equal = matches!(
        egg_layout::saturated_rule_composition::compare(&a, &b)?,
        egg_layout::saturated_rule_composition::Comparison::Equivalent { .. }
    );
    let original_terms = original.terms()?;
    let candidate_terms = candidate.terms()?;
    let initial_terms: BTreeMap<_, _> = original
        .roots
        .iter()
        .map(|(n, r)| (n.clone(), r.term.clone()))
        .collect();
    let mut membership = vec![];
    for (name, term) in &candidate_terms {
        let root = original
            .roots
            .get(name)
            .ok_or("candidate introduced a function")?;
        let text = format!("(check (= {term} {}))", root.term);
        let commands = original.eg.parse_program(None, &text)?;
        let Command::Check(_, facts) = &commands[0] else {
            unreachable!()
        };
        let query = original.eg.compile_ground_check(facts)?;
        let result = original.eg.run_ground_check(&query);
        original.eg.free_ground_check(query);
        membership.push(json!({"function":name,"in_original_root_eclass":result.is_ok(),"reason":result.err().map(|e|e.to_string()),"same_as_original_extraction":original_terms.get(name)==Some(term)}));
    }
    if candidate_terms.keys().ne(initial_terms.keys()) {
        return Err("function set changed".into());
    }
    for (name, terms) in [
        ("input", &initial_terms),
        ("baseline", &original_terms),
        ("candidate", &candidate_terms),
    ] {
        std::fs::write(out.join(format!("{name}.term")), program(terms)?)?;
    }
    let bad: Vec<_> = candidate
        .checks
        .iter()
        .filter(|c| c["sound"] != true || c["complete"] != true)
        .collect();
    let report = json!({"scope":"actual optimized execution; independent copied-row marker oracle, plus original-rule eclass membership of native tree extraction; not production DAG-extractor validation",
      "schedule_walk_matches_plain_execution":control_equal,"marker_checkpoints":candidate.checks.len(),"consumer_checkpoints":candidate.checks.iter().filter(|c|c["completeness_required"]==true).count(),"marker_violations":bad,
      "function_membership":membership,"to_hoist":{"baseline":hoist_check(&original.eg)?,"candidate":hoist_check(&candidate.eg)?},
      "programs":{"input":"input.term","baseline":"baseline.term","candidate":"candidate.term"},
      "membership_queries":"read-only compiled ground checks; no terms inserted into the baseline"});
    std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        json!({"marker_violations":bad.len(),"function_membership":report["function_membership"],"to_hoist":report["to_hoist"]})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn oracle_detects_both_missing_and_invented_facts() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
      (sort E) (constructor X () E)
      (sort B) (constructor B0 () B) (constructor B1 () B)
      (sort T) (constructor Base (B) T)
      (sort H) (constructor HasType (E T) H)
      (relation M (E B))
      (HasType (X) (Base (B0)))
    "#,
        )
        .unwrap();
        let m = json!({"outer":"HasType","inner":"Base","known_marker":"M","slot":1});
        assert_eq!(marker_check(&eg, &m, true).unwrap()["missing"], 1);
        eg.parse_and_run_program(None, "(M (X) (B1))").unwrap();
        assert_eq!(marker_check(&eg, &m, false).unwrap()["invalid"], 1);
        eg.parse_and_run_program(None, "(union (B0) (B1))").unwrap();
        let r = marker_check(&eg, &m, true).unwrap();
        assert_eq!(r["invalid"], 0);
        assert_eq!(r["missing"], 0);
    }
}
