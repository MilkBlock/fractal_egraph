//! Effect-preserving query rejection derived from the original RHS.
//!
//! This privileged read is NOT a general seminaive LHS primitive. It may only
//! discard a substitution whose original actions are already no-ops in the
//! frozen native snapshot. Original positive atoms still own the delta frontier.
//! Deleting a head fact therefore does not silently introduce a new trigger.
use super::*;
use crate::core::GenericCoreAction as A;
use smallvec::SmallVec;

#[derive(Clone)]
enum Operand {
    Slot(usize),
    Constant(Value),
}
impl Operand {
    fn get(&self, values: &[Value]) -> Value {
        match self {
            Self::Slot(i) => values[*i],
            Self::Constant(v) => *v,
        }
    }
}
#[derive(Clone)]
enum Op {
    Lookup(TableAction, Vec<Operand>),
    Union(Operand, Operand),
}
#[derive(Clone, PartialEq, Eq, Hash)]
enum Origin {
    Input(ResolvedVar),
    Literal(Value),
    Fresh(usize),
}
fn potential_effects(eg: &EGraph, rule: &core::ResolvedCoreRule) -> Option<usize> {
    let mut vars = rule
        .body
        .get_vars()
        .into_iter()
        .map(|v| (v.clone(), Origin::Input(v)))
        .collect::<HashMap<_, _>>();
    fn value(
        eg: &EGraph,
        a: &ResolvedAtomTerm,
        vars: &HashMap<ResolvedVar, Origin>,
    ) -> Option<Origin> {
        match a {
            ResolvedAtomTerm::Var(_, v) => vars.get(v).cloned(),
            ResolvedAtomTerm::Literal(_, l) => {
                Some(Origin::Literal(literal_to_value(&eg.backend, l)))
            }
            _ => None,
        }
    }
    let mut known = HashMap::default();
    for atom in &rule.body.atoms {
        let ResolvedCall::Func(f) = &atom.head else {
            return None;
        };
        let n = atom.args.len() - 1;
        let args = atom.args[..n]
            .iter()
            .map(|a| value(eg, a, &vars))
            .collect::<Option<Vec<_>>>()?;
        known.insert((f.name.clone(), args), value(eg, &atom.args[n], &vars)?);
    }
    let mut effects = 0;
    for action in &rule.head.0 {
        match action {
            A::Let(_, v, ResolvedCall::Func(f), args) => {
                let args = args
                    .iter()
                    .map(|a| value(eg, a, &vars))
                    .collect::<Option<Vec<_>>>()?;
                let key = (f.name.clone(), args);
                let out = if let Some(out) = known.get(&key) {
                    out.clone()
                } else {
                    let out = Origin::Fresh(effects);
                    effects += 1;
                    known.insert(key, out.clone());
                    out
                };
                vars.insert(v.clone(), out);
            }
            A::LetAtomTerm(_, v, a) => {
                vars.insert(v.clone(), value(eg, a, &vars)?);
            }
            A::Union(_, a, b) => {
                if value(eg, a, &vars)? != value(eg, b, &vars)? {
                    effects += 1
                }
            }
            _ => return None,
        }
    }
    Some(effects)
}

struct Program {
    inputs: Vec<ResolvedVar>,
    ops: Vec<Op>,
    prefix_safe: bool,
}
impl Program {
    fn compile(eg: &EGraph, rule: &core::ResolvedCoreRule) -> Option<Self> {
        // Do not suppress an opaque/fallible LHS primitive, or its errors.
        if rule
            .body
            .atoms
            .iter()
            .any(|a| !matches!(&a.head, ResolvedCall::Func(_)))
        {
            return None;
        }
        let mut inputs = rule.head.get_free_vars().into_iter().collect::<Vec<_>>();
        inputs.sort_by(|a, b| a.name.cmp(&b.name));
        let body = rule.body.get_vars();
        if inputs.is_empty() || inputs.iter().any(|v| !body.contains(v)) {
            return None;
        }
        let mut vars = inputs
            .iter()
            .enumerate()
            .map(|(i, v)| (v.clone(), Operand::Slot(i)))
            .collect::<HashMap<_, _>>();
        let operand =
            |a: &ResolvedAtomTerm, vars: &HashMap<ResolvedVar, Operand>| -> Option<Operand> {
                match a {
                    ResolvedAtomTerm::Var(_, v) => vars.get(v).cloned(),
                    ResolvedAtomTerm::Literal(_, l) => {
                        Some(Operand::Constant(literal_to_value(&eg.backend, l)))
                    }
                    _ => None,
                }
            };
        let mut ops = vec![];
        let mut registers = inputs.len();
        for action in &rule.head.0 {
            match action {
                A::Let(_, v, ResolvedCall::Func(f), args) => {
                    if !eg.functions.get(&f.name)?.is_constructor() {
                        return None;
                    }
                    let args = args
                        .iter()
                        .map(|a| operand(a, &vars))
                        .collect::<Option<Vec<_>>>()?;
                    let table =
                        TableAction::new(&eg.backend, eg.functions.get(&f.name)?.backend_id);
                    ops.push(Op::Lookup(table, args));
                    vars.insert(v.clone(), Operand::Slot(registers));
                    registers += 1;
                }
                A::LetAtomTerm(_, v, a) => {
                    let a = operand(a, &vars)?;
                    vars.insert(v.clone(), a);
                }
                A::Union(_, a, b) => {
                    if !matches!(a.output().column_ty(&eg.backend), ColumnTy::Id) {
                        return None;
                    }
                    ops.push(Op::Union(operand(a, &vars)?, operand(b, &vars)?));
                }
                // Set/merge, deletion, primitive actions and panic need other
                // effect contracts; an apparently equal value is insufficient.
                _ => return None,
            }
        }
        // Alias-only lets do not make a variable an action input. Preserve
        // the original existential projections instead of widening the Join.
        let old_len = inputs.len();
        let mut used = std::collections::BTreeSet::new();
        for op in &ops {
            match op {
                Op::Lookup(_, args) => {
                    for a in args {
                        if let Operand::Slot(i) = a {
                            if *i < old_len {
                                used.insert(*i);
                            }
                        }
                    }
                }
                Op::Union(a, b) => {
                    for a in [a, b] {
                        if let Operand::Slot(i) = a {
                            if *i < old_len {
                                used.insert(*i);
                            }
                        }
                    }
                }
            }
        }
        let used = used.into_iter().collect::<Vec<_>>();
        let remap = |a: &mut Operand| {
            if let Operand::Slot(i) = a {
                *i = if *i < old_len {
                    used.binary_search(i).unwrap()
                } else {
                    *i - old_len + used.len()
                }
            }
        };
        for op in &mut ops {
            match op {
                Op::Lookup(_, args) => {
                    for a in args {
                        remap(a)
                    }
                }
                Op::Union(a, b) => {
                    remap(a);
                    remap(b)
                }
            }
        }
        inputs = used.iter().map(|i| inputs[*i].clone()).collect();
        (!ops.is_empty() && !inputs.is_empty()).then_some(Self {
            inputs,
            ops,
            prefix_safe: potential_effects(eg, rule).is_some_and(|n| n <= 1),
        })
    }
}

/// Engine-owned compiled alternatives. Source rule identities/cursors stay the
/// authority, including when returning to ordinary native execution.
#[derive(Clone)]
pub struct CellEffectRuleset {
    pub(super) environment: u64,
    pub(super) name: String,
    pub(super) original: Vec<RuleId>,
    pub(super) actual: Vec<RuleId>,
    guarded: Vec<String>,
    prefix_rules: Vec<String>,
    pub(super) observations: crate::core_relations::CounterId,
    pub(super) prefix: bool,
    pub(super) early_observations: crate::core_relations::CounterId,
    existence: Vec<(RuleId, RuleId)>,
}
impl CellEffectRuleset {
    pub fn prefix_rules(&self) -> &[String] {
        &self.prefix_rules
    }
    pub fn prefix_enabled(&self) -> bool {
        self.prefix
    }
    pub fn guarded_rules(&self) -> &[String] {
        &self.guarded
    }
}
impl EGraph {
    pub fn prepare_cell_effect_ruleset(
        &mut self,
        set: &str,
    ) -> Result<Option<CellEffectRuleset>, Error> {
        self.prepare_cell_effect_ruleset_with_prefix(set, true)
    }
    /// Keep prefix=false when another optimizer needs exact full-LHS activity
    /// for admission of its own closure. Early rejection only proves no effect.
    pub fn prepare_cell_effect_ruleset_with_prefix(
        &mut self,
        set: &str,
        prefix: bool,
    ) -> Result<Option<CellEffectRuleset>, Error> {
        if self.are_proofs_enabled() || self.proof_state.original_typechecking.is_some() {
            return Ok(None);
        }
        let Some(Ruleset::Rules(rules)) = self.rulesets.get(set) else {
            return Ok(None);
        };
        let source = rules
            .iter()
            .map(|(n, (r, id))| (n.clone(), r.clone(), *id))
            .collect::<Vec<_>>();
        let observations = self.backend.new_cell_observation_counter();
        let early_observations = self.backend.new_cell_observation_counter();
        let mut existence = vec![];
        let mut original = vec![];
        let mut actual = vec![];
        let mut guarded = vec![];
        let mut prefix_rules = vec![];
        for (name, rule, id) in source {
            original.push(id);
            let program = if self.backend.rule_is_seminaive(id) {
                Program::compile(self, &rule)
            } else {
                None
            };
            let Some(program) = program else {
                actual.push(id);
                continue;
            };
            let witness = self
                .backend
                .register_external_func(Box::new(make_external_func(move |state, _| {
                    state.inc_counter(observations);
                    state.trigger_early_stop();
                    Some(Value::new_const(0))
                })));
            let unit_ty = ColumnTy::Base(self.backend.base_values().get_ty::<()>());
            let mut exists = BackendRule::new(
                self.backend.new_rule("cell effect LHS witness", true),
                &self.functions,
                &self.type_info,
                true,
            );
            exists.query(&rule.body, false);
            exists.rb.call_external_func(witness, &[], unit_ty, || {
                "Cell existence callback is total".into()
            });
            existence.push((id, exists.build()));
            let unit = self.backend.base_values().get(());
            let ty = ColumnTy::Base(self.backend.base_values().get_ty::<()>());
            let ops = program.ops;
            let check: Arc<crate::core_relations::CellEffectCheck> =
                Arc::new(move |state, values| {
                    let mut registers = SmallVec::<[Value; 16]>::from_slice(values);
                    let mut key = SmallVec::<[Value; 4]>::new();
                    for op in &ops {
                        match op {
                            Op::Lookup(table, args) => {
                                key.clear();
                                key.extend(args.iter().map(|a| a.get(&registers)));
                                let Some(v) = table.lookup(state, &key) else {
                                    return false;
                                };
                                registers.push(v);
                            }
                            Op::Union(a, b) if a.get(&registers) != b.get(&registers) => {
                                return false;
                            }
                            Op::Union(..) => (),
                        }
                    }
                    state.inc_counter(early_observations);
                    true
                });
            let late = check.clone();
            let predicate = self
                .backend
                .register_external_func(Box::new(make_external_func(move |state, values| {
                    state.inc_counter(observations);
                    (!late(state, values)).then_some(unit)
                })));
            let mut builder = BackendRule::new(
                self.backend.new_rule(&name, true),
                &self.functions,
                &self.type_info,
                true,
            );
            builder.query(&rule.body, false);
            let mut args = program
                .inputs
                .iter()
                .map(|v| builder.entry(&ResolvedAtomTerm::Var(span!(), v.clone())))
                .collect::<Vec<_>>();
            if prefix && program.prefix_safe {
                builder.rb.cell_effect_prefix(check, &args);
                prefix_rules.push(name.clone());
            }
            args.push(QueryEntry::Const { val: unit, ty });
            builder
                .rb
                .query_prim(predicate, &args, ty)
                .map_err(|e| Error::BackendError(e.to_string()))?;
            builder.actions(&rule.head)?;
            actual.push(builder.build());
            guarded.push(name);
        }
        Ok((!guarded.is_empty()).then_some(CellEffectRuleset {
            environment: self.cell_environment_stamp,
            name: set.into(),
            original,
            actual,
            guarded,
            prefix_rules,
            observations,
            prefix,
            early_observations,
            existence,
        }))
    }
    pub(super) fn sync_effect_inputs(&mut self, plan: &CellEffectRuleset) {
        for (original, query) in &plan.existence {
            let stamp = self.backend.rule_last_run_value(*original);
            self.backend.complete_external_queries(&[*query], stamp);
        }
        for (old, new) in plan.original.iter().zip(&plan.actual) {
            if old != new {
                let stamp = self.backend.rule_last_run_value(*old);
                self.backend.complete_external_queries(&[*new], stamp);
            }
        }
    }
    pub(super) fn finish_effect_inputs(&mut self, plan: &CellEffectRuleset) {
        for (old, new) in plan.original.iter().zip(&plan.actual) {
            if old != new {
                let stamp = self.backend.rule_last_run_value(*new);
                self.backend.complete_external_queries(&[*old], stamp);
            }
        }
    }
    /// Only after all non-guarded peer rules had no match and the intervening
    /// writes cannot affect these bodies. Reuses the PRE-Run source cursors.
    /// Stops on the first complete LHS; it never executes the original actions.
    pub fn cell_effect_had_lhs_match(&mut self, plan: &CellEffectRuleset) -> Result<bool, Error> {
        if self.cell_environment_stamp != plan.environment {
            return Err(Error::BackendError(
                "Cell effect environment changed".into(),
            ));
        }
        let before = self
            .backend
            .read_cell_observation_counter(plan.observations);
        let ids = plan.existence.iter().map(|(_, q)| *q).collect::<Vec<_>>();
        self.backend
            .run_rules(&ids)
            .map_err(|e| Error::BackendError(e.to_string()))?;
        Ok(self
            .backend
            .read_cell_observation_counter(plan.observations)
            != before)
    }
    pub fn step_rules_with_cell_effects(
        &mut self,
        plan: &CellEffectRuleset,
    ) -> Result<RunReport, Error> {
        if self.cell_environment_stamp != plan.environment {
            return Err(Error::BackendError(
                "Cell effect environment changed".into(),
            ));
        }
        if self.program_trace.is_some() {
            return self.step_rules(&plan.name);
        }
        for (old, new) in plan.original.iter().zip(&plan.actual) {
            if old != new {
                let stamp = self.backend.rule_last_run_value(*old);
                self.backend.complete_external_queries(&[*new], stamp);
            }
        }
        let report = self.backend.run_rules(&plan.actual);
        for (old, new) in plan.original.iter().zip(&plan.actual) {
            if old != new {
                let stamp = self.backend.rule_last_run_value(*new);
                self.backend.complete_external_queries(&[*old], stamp);
            }
        }
        let report = report.map_err(|e| Error::BackendError(e.to_string()))?;
        Ok(RunReport::singleton(&plan.name, report))
    }
}
