//! Optional native oracle for externally computed i64 batches. Nothing in this
//! module is executed on the unchecked path. The oracle uses the resolved
//! native LHS and RHS primitives, then native function merge on isolated clones.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub struct CellI64Check {
    rule: RuleId,
    callback: ExternalFunctionId,
    rows: Arc<std::sync::Mutex<Vec<Vec<Value>>>>,
    function: String,
    consumer: String,
}

pub struct CellI64Proposal {
    pub binding: Vec<Value>,
    pub value: i64,
}

/// One immutable canonical-key snapshot, including unchanged/missing results.
pub struct CellI64Group {
    pub key: Vec<Value>,
    pub initial: Option<i64>,
    pub proposals: Vec<CellI64Proposal>,
    pub result: Option<i64>,
}

#[derive(Debug, Default)]
pub struct CellI64CheckReport {
    pub native_bindings: usize,
    pub accepted_bindings: usize,
    pub proposals: usize,
    pub groups: usize,
}

fn mismatch(message: impl Into<String>) -> Error {
    Error::BackendError(format!("GPU check: {}", message.into()))
}

impl EGraph {
    /// Compile once. This supports a pure i64 primitive DAG ending in one set;
    /// it does not run the RHS on the live graph or trust GPU-lowered bytecode.
    pub fn prepare_cell_i64_check(
        &mut self,
        ruleset: &str,
        sink: &CellActionSink,
    ) -> Result<CellI64Check, Error> {
        let Some(Ruleset::Rules(rules)) = self.rulesets.get(ruleset) else {
            return Err(mismatch("expected direct ruleset"));
        };
        let rule = &rules
            .get(&sink.name)
            .ok_or_else(|| mismatch("unknown rule"))?
            .0;
        use crate::core::GenericCoreAction as A;
        let Some((A::Set(_, ResolvedCall::Func(function), keys, value), prefix)) =
            rule.head.0.split_last()
        else {
            return Err(mismatch("expected one terminal function set"));
        };
        if function.output.name() != "i64"
            || prefix.iter().any(|a| {
                !matches!(a,
                    A::Let(_, _, ResolvedCall::Primitive(p), _) if p.output().name() == "i64"
                ) && !matches!(a, A::LetAtomTerm(..))
            })
        {
            return Err(mismatch("checker requires pure i64 RHS primitives"));
        }
        let function = function.name.clone();
        let keys = keys.clone();
        let value = value.clone();
        let prefix = core::GenericCoreActions(prefix.to_vec());
        let rows = Arc::new(std::sync::Mutex::new(Vec::new()));
        let output = rows.clone();
        let unit = self.backend.base_values().get(());
        let unit_type = self.backend.base_values().get_ty::<()>();
        let callback = self
            .backend
            .register_external_func(Box::new(make_external_func(move |_, values| {
                output.lock().unwrap().push(values.to_vec());
                Some(unit)
            })));
        let mut builder = BackendRule::new(
            self.backend.new_rule("cell-gpu-check", false),
            &self.functions,
            &self.type_info,
            false,
        );
        builder.query(&sink.body, false);
        if let Err(e) = builder.actions(&prefix) {
            self.backend.free_external_func(callback);
            return Err(e);
        }
        let entries = sink
            .vars
            .iter()
            .map(|v| ResolvedAtomTerm::Var(span!(), v.clone()))
            .chain(keys)
            .chain([value])
            .map(|term| builder.entry(&term))
            .collect::<Vec<_>>();
        builder
            .rb
            .call_external_func(callback, &entries, ColumnTy::Base(unit_type), || {
                "Cell GPU verification".into()
            });
        Ok(CellI64Check {
            rule: builder.build(),
            callback,
            rows,
            function,
            consumer: sink.name.clone(),
        })
    }

    pub fn free_cell_i64_check(&mut self, check: CellI64Check) {
        self.backend.free_rule(check.rule);
        self.backend.free_external_func(check.callback);
    }

    /// Strict diagnostic mode: accepted current bindings must equal the complete
    /// native query, and every pending proposal/result must agree with native RHS
    /// and merge. All checks finish before the caller may submit any write.
    /// Clones are per key snapshot, since pending snapshots can share a key.
    pub fn check_cell_i64_batch(
        &mut self,
        sink: &CellActionSink,
        check: &CellI64Check,
        accepted: &[Vec<Value>],
        groups: &[CellI64Group],
    ) -> Result<CellI64CheckReport, Error> {
        if check.consumer != sink.name {
            return Err(mismatch("checker belongs to a different consumer"));
        }
        check.rows.lock().unwrap().clear();
        self.backend
            .run_rules(&[check.rule])
            .map_err(|e| mismatch(format!("native oracle failed: {e}")))?;
        let rows = std::mem::take(&mut *check.rows.lock().unwrap());
        let schema = self.functions[&check.function].schema.clone();
        let canonical_key = |eg: &EGraph, key: &[Value]| -> Result<Vec<Value>, Error> {
            if key.len() != schema.input.len() {
                return Err(mismatch("key arity"));
            }
            Ok(key
                .iter()
                .zip(&schema.input)
                .map(|(v, s)| eg.get_canonical_value(*v, s))
                .collect())
        };
        let mut expected = BTreeMap::new();
        for row in rows {
            let n = sink.vars.len();
            let binding = sink.canonicalize(self, &row[..n])?;
            let key = canonical_key(self, &row[n..row.len() - 1])?;
            let value = self.value_to_base::<i64>(*row.last().unwrap());
            expected.insert(binding, (key, value));
        }
        let actual = accepted
            .iter()
            .map(|b| sink.canonicalize(self, b))
            .collect::<Result<BTreeSet<_>, _>>()?;
        let native = expected.keys().cloned().collect::<BTreeSet<_>>();
        if actual != native {
            return Err(mismatch(format!(
                "{} binding mismatch: {} unexpected, {} missing",
                sink.name,
                actual.difference(&native).count(),
                native.difference(&actual).count()
            )));
        }
        let mut report = CellI64CheckReport {
            native_bindings: native.len(),
            accepted_bindings: actual.len(),
            ..Default::default()
        };
        for (g, group) in groups.iter().enumerate() {
            let key = canonical_key(self, &group.key)?;
            let physical = self
                .lookup_function_row(&check.function, &key)
                .map(|r| self.value_to_base::<i64>(r[key.len()]));
            if physical != group.initial {
                return Err(mismatch(format!("group {g} stale native snapshot")));
            }
            let mut bindings = vec![];
            for proposal in &group.proposals {
                let binding = sink.canonicalize(self, &proposal.binding)?;
                let Some((expected_key, value)) = expected.get(&binding) else {
                    return Err(mismatch(format!(
                        "group {g} proposal has no current native match"
                    )));
                };
                if expected_key != &key || *value != proposal.value {
                    return Err(mismatch(format!(
                        "group {g} RHS/key mismatch: native {expected_key:?} => {value}, GPU {key:?} => {}",
                        proposal.value
                    )));
                }
                bindings.push(binding);
            }
            let result = if bindings.is_empty() {
                physical
            } else {
                let mut shadow = self.clone();
                shadow.apply_cell_bindings(sink, &bindings)?;
                shadow
                    .lookup_function_row(&check.function, &key)
                    .map(|r| shadow.value_to_base::<i64>(r[key.len()]))
            };
            if result != group.result {
                return Err(mismatch(format!(
                    "group {g} merge mismatch: native {result:?}, GPU {:?}",
                    group.result
                )));
            }
            report.proposals += bindings.len();
            report.groups += 1;
        }
        Ok(report)
    }
}
