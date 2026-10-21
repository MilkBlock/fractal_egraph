//! Complete rooted boundary queries, maintained from native row commits.
//! This is source-derived context reuse, not a new saturation/absence theorem.
use super::*;

#[derive(Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spec {
    pub ruleset: String,
    pub rule: String,
    pub root: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: String,
    rules: Vec<Spec>,
}
pub(super) fn load() -> Result<Vec<Spec>> {
    let Some(path) = std::env::var_os("CELL_BOUNDARY_PROGRAMS") else {
        return Ok(vec![]);
    };
    let f: File = serde_json::from_slice(&std::fs::read(path)?)?;
    if f.schema != "cell-boundary-programs/v1" {
        return Err("unknown boundary program format".into());
    }
    Ok(f.rules)
}

pub(super) fn compile(
    eg: &EGraph,
    sink: CellActionSink,
    tables: Vec<usize>,
    root: &str,
) -> Result<Kernel> {
    if sink.atoms.iter().any(|a| a.columns.iter().zip(&a.sorts).any(|(o,s)|
        matches!(o,CellOperand::Constant{..}) && !["i64","f64","bool","String","Unit"].contains(&s.as_str()))) {
        return Err("boundary equality/container constants require recanonicalization support".into());
    }
    if !eg.get_function(root).is_some_and(|f| f.is_constructor()) {
        return Err("boundary root must be a constructor".into());
    }
    let roots = sink
        .atoms
        .iter()
        .enumerate()
        .filter(|(_, a)| a.function == root)
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    if roots.len() != 1 || sink.atoms.iter().any(|a| a.columns.len() > 64) {
        return Err("boundary requires one unambiguous root and at most 64 columns per atom".into());
    }
    let seed = roots[0];
    let mut bound = vec![false; sink.slots().len()];
    for c in &sink.atoms[seed].columns {
        if let CellOperand::Slot(i) = c {
            bound[*i] = true;
        }
    }
    let root_slots = bound.clone();
    let mut pending = (0..sink.atoms.len())
        .filter(|i| *i != seed)
        .collect::<BTreeSet<_>>();
    let mut steps = vec![];
    while !pending.is_empty() {
        let next = pending
            .iter()
            .filter_map(|i| {
                let a = &sink.atoms[*i];
                let n = a.columns.len() - 1;
                let full = a.columns[..n].iter().all(|o| known(o, &bound));
                let output =
                    eg.cell_sort_is_equality(&a.sorts[n]).ok()? && known(&a.columns[n], &bound);
                let mask = a
                    .columns
                    .iter()
                    .enumerate()
                    .filter(|(j, o)| a.sorts[*j] != "Unit" && known(o, &bound))
                    .fold(0u64, |m, (j, _)| m | (1 << j));
                if !full && !output && mask == 0 {
                    return None;
                }
                let rooted = a
                    .columns
                    .iter()
                    .filter(|o| matches!(o,CellOperand::Slot(s) if root_slots[*s]))
                    .count();
                let missing = a.columns.iter().filter(|o| !known(o, &bound)).count();
                Some((
                    *i,
                    if full {
                        Access::Key
                    } else if output {
                        Access::Output
                    } else {
                        Access::Columns(mask)
                    },
                    (
                        full,
                        output,
                        rooted > 0,
                        mask.count_ones(),
                        std::cmp::Reverse(missing),
                        std::cmp::Reverse(*i),
                    ),
                ))
            })
            .max_by_key(|x| x.2);
        let Some((i, access, _)) = next else {
            return Err("unanchored boundary component; retain ordinary query".into());
        };
        for o in &sink.atoms[i].columns {
            if let CellOperand::Slot(s) = o {
                bound[*s] = true
            }
        }
        pending.remove(&i);
        steps.push((i, access));
    }
    if bound.iter().any(|b| !*b) {
        return Err("unbound boundary slot".into());
    }
    Ok(Kernel::from_wiring(sink, tables, seed, steps))
}

impl Runtime {
    pub(super) fn query_boundary(
        &mut self,
        set: &str,
        sinks: &[&CellActionSink],
    ) -> Result<Vec<(usize, Vec<Vec<Value>>)>> {
        self.sync()?;
        let mut selected = vec![];
        for id in self.boundary.get(set).into_iter().flatten() {
            let source = &self.kernels[*id].sink;
            let Some(i) = sinks.iter().position(|s| s.name == source.name) else {
                continue;
            };
            if source.slots() != sinks[i].slots() {
                return Err("boundary slot interface changed".into());
            }
            selected.push((*id, i));
        }
        let mut out = vec![];
        for (id, i) in selected {
            let rows =
                self.kernels[id].matches(&self.eg, &self.rows, &mut self.stats.keyed_reads)?;
            out.push((i, rows));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_frontier_matches_after_missing_ports_union_visibility_and_reinsert() {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
          (datatype E (N i64) (Loop E E) (Const E E E))
          (relation Ctx (E E)) (relation Ty (E E)) (relation Hit (E E E E))
          (ruleset s)
          (rule ((= root (Loop x y)) (Ctx x a) (Ctx y b) (= k (Const c t b)) (Ty x u))
                ((Hit root k a u)) :ruleset s :name "q")
          (Loop (N 1) (N 2)) (Ctx (N 1) (N 10))
          (Ty (N 1) (N 20)) (Const (N 3) (N 20) (N 11))
        "#,
        )
        .unwrap();
        let spec = Spec {
            ruleset: "s".into(),
            rule: "q".into(),
            root: "Loop".into(),
        };
        let mut r =
            Runtime::new_with_boundaries(eg, &[], &BTreeMap::new(), vec![], None, &[spec]).unwrap();
        for input in [
            "",
            "(Ctx (N 2) (N 11))",
            "(Ctx (N 2) (N 12)) (Const (N 4) (N 20) (N 12))",
            "(union (N 11) (N 12))",
            "(delete (Ctx (N 2) (N 11)))",
            "(Ctx (N 2) (N 11))",
            "(subsume (Loop (N 1) (N 2)))",
            "(Loop (N 1) (N 2))",
        ] {
            if !input.is_empty() {
                r.input(input).unwrap();
            }
            let sink = r.eg.prepare_cell_action("s", "q").unwrap();
            let query = r.eg.prepare_cell_query(&sink).unwrap();
            let mut expected = r.eg.run_cell_queries(&[&query]).unwrap().pop().unwrap();
            expected.sort();
            expected.dedup();
            let answer = r.query_boundary("s", &[&sink]).unwrap();
            let got = &answer[0].1;
            assert_eq!(got, &expected, "after {input}");
            r.eg.apply_completed_cell_ruleset(&[(&sink, got.as_slice())])
                .unwrap();
        }
        assert!(!r.rows.projected.is_empty());
        assert!(r.kernels[0].rows_examined.get() > 0);
    }
}
