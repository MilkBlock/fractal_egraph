//! The certified Smaller fixed point: derive equality effects without
//! enumerating TCPair aliases or running three equivalent structural queries.
//! Enabled only inside the checked joint saturation and uniform-cost scope.
use super::*;

#[derive(Default, Serialize)]
pub(super) struct Counters {
    saturations: usize,
    seed_evaluations: usize,
    union_proposals: usize,
    native_batches: usize,
    propagated_costs: usize,
    unchanged_saturations: usize,
    identity_noop_seeds: usize,
    cost_lookups: usize,
    planning_seconds: f64,
    effect_seconds: f64,
}

pub(super) struct Scope {
    sink: CellActionSink,
    roles: contract::Selection,
    pub counters: Counters,
}

impl Scope {
    /// Under these schemas, TermAndCost unions can only rebuild Smaller,
    /// PotentialExtractedExpr and ExtractedExpr. In particular no foreign
    /// order-sensitive merge callback may observe a different union order.
    pub fn safe_schema(eg: &EGraph, cost_sort: &str, roles:&contract::Selection) -> bool {
        let marker = eg
            .get_function(&roles.candidate)
            .unwrap()
            .schema()
            .output
            .name()
            .to_string();
        fn contains(sort: &egglog::ArcSort, target: &str) -> bool {
            if sort.name() == target {
                return true;
            }
            if !sort.is_container_sort() {
                return false;
            }
            // These containers have fixed, recursively declared element sorts.
            // Function/custom containers may acquire new captured types later.
            use egglog::sort::{
                MapContainer, MultiSetContainer, PairContainer, SetContainer, VecContainer,
            };
            use std::any::TypeId;
            // ArcSort wraps these sorts in ContainerSortImpl. Inspect the
            // container value type, not the wrapper's concrete Rust type.
            if ![
                TypeId::of::<VecContainer>(),
                TypeId::of::<SetContainer>(),
                TypeId::of::<MapContainer>(),
                TypeId::of::<PairContainer>(),
                TypeId::of::<MultiSetContainer>(),
            ]
            .into_iter()
            .any(|ty| sort.value_type() == Some(ty))
            {
                return true;
            }
            sort.inner_sorts().iter().any(|s| contains(s, target))
        }
        eg.get_function_names().iter().all(|name| {
            let f = eg.get_function(name).unwrap();
            let s = f.schema();
            s.input.iter().all(|t| !contains(t, &marker))
                && (!s.input.iter().any(|t| contains(t, cost_sort))
                    || [&roles.compare,&roles.candidate].contains(&name))
        })
    }
    pub fn compile(eg: &mut EGraph, cost_sort: &str, roles:&contract::Selection) -> Result<Self> {
        let mut commands = eg.parse_program(
            None,
            &format!(
                "(function joint_comparison_gate ({cost_sort} {cost_sort}) i64 :merge old)
             (ruleset joint_comparison_macro)
             (rule ((= 0 (joint_comparison_gate lhs rhs)))
                   ((union lhs rhs)) :ruleset joint_comparison_macro :name \"union\")"
            ),
        )?;
        for c in &mut commands {
            if let Command::Function { hidden, .. } = c {
                *hidden = true;
            }
        }
        eg.run_program(commands)?;
        Ok(Self {
            roles: roles.clone(),
            sink: eg.prepare_cell_action("joint_comparison_macro", "union")?,
            counters: Counters::default(),
        })
    }
    pub fn accepts(&self, body: &Schedule) -> bool {
        match body {
            GenericSchedule::Run(_, r) => r.ruleset == self.roles.sets[2] && r.until.is_none(),
            GenericSchedule::Sequence(_, xs) if xs.len() == 1 => self.accepts(&xs[0]),
            _ => false,
        }
    }
    pub fn run(&mut self, r: &mut Runtime) -> Result<bool> {
        let ids = r.sets[&self.roles.sets[2]].clone();
        let smaller = r.names.iter().position(|n| n == &self.roles.compare).unwrap();
        let tc = r.names.iter().position(|n| n == &self.roles.pair).unwrap();
        let mut updated = false;
        self.counters.saturations += 1;
        loop {
            r.sync()?;
            let planning = std::time::Instant::now();
            let mut queue = BTreeSet::new();
            for id in &ids {
                queue.append(&mut r.kernels[*id].dirty);
            }
            let mut known = FxHashMap::<Value, i64>::default();
            let mut done = BTreeSet::new();
            let mut equations = BTreeSet::new();
            while let Some(key) = queue.pop_first() {
                if done.contains(&key) {
                    continue;
                }
                self.counters.seed_evaluations += 1;
                let Some(row) = r.rows.keys.get(&(smaller, key.clone())).cloned() else {
                    for id in &ids {
                        r.kernels[*id].paths.remove(&key);
                        r.kernels[*id].install_dependencies(key.clone(), None);
                    }
                    continue;
                };
                let (a, b, out) = (row.values[0], row.values[1], row.values[2]);
                let mut reads = smallvec![Read::Output(tc, a), Read::Output(tc, b)];
                reads.sort_unstable();
                reads.dedup();
                // Preserve complete invalidation for a later ordinary Run or
                // uncertified boundary, including reads that are missing now.
                for id in &ids {
                    r.kernels[*id].paths.remove(&key);
                    r.kernels[*id].install_dependencies(key.clone(), Some(reads.clone()));
                }
                for v in [a, b] {
                    if known.contains_key(&v) {
                        continue;
                    }
                    self.counters.cost_lookups += 1;
                    r.stats.keyed_reads += 1;
                    if let Some(keys) = r.rows.outputs.get(&(tc, v)) {
                        let k = keys.first().unwrap();
                        // The scope proof establishes equal costs for ALL
                        // aliases. Picking a cost does not pick a Term result.
                        known.insert(v, r.eg.value_to_base::<i64>(k[1]));
                    }
                }
                let (Some(ca), Some(cb)) = (known.get(&a).copied(), known.get(&b).copied()) else {
                    continue;
                };
                let (target, cost) = if ca <= cb { (a, ca) } else { (b, cb) };
                done.insert(key);
                if out != target {
                    equations.insert((out, target));
                } else {
                    self.counters.identity_noop_seeds += 1;
                }
                if let Some(old) = known.insert(out, cost) {
                    if old != cost {
                        return Err("uniform Smaller closure proof violated".into());
                    }
                } else {
                    self.counters.propagated_costs += 1;
                    // A solved Smaller is now a TCPair witness for its users.
                    // Transport the proof before asking native rebuild to
                    // materialize the aliases; never invent an unresolved cost.
                    if let Some(users) = r.kernels[ids[0]].subscribers.get(&Read::Output(tc, out)) {
                        queue.extend(users.iter().cloned());
                    }
                }
            }
            let elapsed = planning.elapsed().as_secs_f64();
            self.counters.planning_seconds += elapsed;
            r.stats.frontier_seconds += elapsed;
            if equations.is_empty() {
                break;
            }
            let effect = std::time::Instant::now();
            let bindings = equations
                .iter()
                .map(|(a, b)| {
                    self.sink
                        .slots()
                        .iter()
                        .map(|(n, _)| match *n {
                            "lhs" => *a,
                            "rhs" => *b,
                            _ => unreachable!(),
                        })
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>();
            self.counters.union_proposals += bindings.len();
            self.counters.native_batches += 1;
            // Only native union/merge/rebuild publishes the derived effects.
            let report = r.eg.apply_cell_bindings(&self.sink, &bindings)?;
            let elapsed = effect.elapsed().as_secs_f64();
            self.counters.effect_seconds += elapsed;
            r.stats.effect_seconds += elapsed;
            updated |= report.updated;
        }
        r.eg.certify_cell_saturation(&self.roles.sets[2])?;
        self.counters.unchanged_saturations += usize::from(!updated);
        Ok(updated)
    }
}
