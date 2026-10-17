//! Compact native events for a resident Cell executor. No diagnostic trace,
//! table-row origin history or CPU Cell registry is constructed on this path.
use super::*;
use egglog_bridge::{FunctionId, RuleId};
use std::sync::Mutex;

#[derive(Clone, Debug)]
pub struct CellTable {
    pub name: String,
    pub keys: usize,
    pub eq_columns: Vec<bool>,
    pub i64_columns: Vec<bool>,
    function: FunctionId,
    subsume: bool,
}
#[derive(Debug)]
pub struct CellRowDelta {
    pub table: usize,
    pub key: Vec<Value>,
    pub row: Option<Vec<Value>>,
    pub visible: bool,
}
#[derive(Default, Debug)]
pub struct CellCommitBatch {
    pub rows: Vec<CellRowDelta>,
    pub clears: Vec<usize>,
    pub unions: Vec<(Value, Value)>,
}
pub struct CellCommitFeed {
    pub tables: Vec<CellTable>,
    union_cursor: usize,
}

#[derive(Clone, Debug)]
pub struct CellProducerEvent {
    pub rule: usize,
    pub values: Vec<Value>,
}
pub struct CellProducerTap {
    rules: Vec<RuleId>,
    events: Arc<Mutex<Vec<CellProducerEvent>>>,
}
impl CellProducerTap {
    pub fn drain(&self) -> Vec<CellProducerEvent> {
        std::mem::take(&mut *self.events.lock().unwrap())
    }
}
impl EGraph {
    pub fn cell_sort_is_equality(&self, sort: &str) -> Result<bool, Error> {
        let sort = self
            .get_sort_by_name(sort)
            .ok_or_else(|| Error::BackendError("unknown Cell sort".into()))?;
        if sort.is_container_sort() {
            return Err(Error::BackendError(
                "resident Cell events do not support container columns".into(),
            ));
        }
        Ok(matches!(sort.column_ty(&self.backend), ColumnTy::Id))
    }
    /// Subscribe before input. Existing rows are sent exactly once at bootstrap.
    /// A feed is scoped to this engine; push/pop or replacing its environment is
    /// unsupported. Journal clones have independent queues.
    pub fn subscribe_cell_commits(
        &mut self,
        names: &[String],
    ) -> Result<(CellCommitFeed, CellCommitBatch), Error> {
        let mut tables = vec![];
        let mut initial = CellCommitBatch::default();
        for name in names {
            let f = self
                .functions
                .get(name)
                .ok_or_else(|| Error::BackendError(format!("unknown Cell table {name}")))?;
            let mut eq_columns = vec![];
            for s in f.schema.input.iter().chain([&f.schema.output]) {
                eq_columns.push(self.cell_sort_is_equality(s.name())?);
            }
            let table = CellTable {
                name: name.clone(),
                keys: f.schema.input.len(),
                eq_columns,
                i64_columns: f
                    .schema
                    .input
                    .iter()
                    .chain([&f.schema.output])
                    .map(|s| s.name() == "i64")
                    .collect(),
                function: f.backend_id,
                subsume: f.can_subsume,
            };
            let i = tables.len();
            if !self.backend.enable_function_journal(table.function) {
                return Err(Error::BackendError(
                    "resident events require explicit native tables".into(),
                ));
            }
            self.backend.for_each(table.function, |r| {
                initial.rows.push(CellRowDelta {
                    table: i,
                    key: r.vals[..table.keys].to_vec(),
                    row: Some(r.vals.to_vec()),
                    visible: !r.subsumed,
                })
            });
            tables.push(table);
        }
        let (cursor, unions) = self
            .backend
            .canonical_changes_since(0)
            .ok_or_else(|| Error::BackendError("unsupported union table".into()))?;
        initial.unions = unions;
        Ok((
            CellCommitFeed {
                tables,
                union_cursor: cursor,
            },
            initial,
        ))
    }
    /// Drain at a completed native commit/rebuild boundary. No table lookup or
    /// graph export: row payloads came from the actual commit sites.
    pub fn drain_cell_commits(&self, feed: &mut CellCommitFeed) -> Result<CellCommitBatch, Error> {
        let mut batch = CellCommitBatch::default();
        for (i, t) in feed.tables.iter().enumerate() {
            let changes = self.backend.drain_function_changes(t.function);
            if changes.cleared {
                batch.clears.push(i);
            }
            let cols = t.keys + 1;
            for (key, row) in changes.rows {
                let visible = row
                    .as_ref()
                    .is_some_and(|r| !t.subsume || r[cols + 1] == Value::new_const(0));
                batch.rows.push(CellRowDelta {
                    table: i,
                    key,
                    row: row.map(|r| r[..cols].to_vec()),
                    visible,
                });
            }
        }
        let (cursor, unions) = self
            .backend
            .canonical_changes_since(feed.union_cursor)
            .ok_or_else(|| {
                Error::BackendError("Cell event feed crossed an engine scope reset".into())
            })?;
        feed.union_cursor = cursor;
        batch.unions = unions;
        Ok(batch)
    }
    /// Instrument the *same* native query/actions with a compact post-action
    /// callback retaining only requested bindings. It creates no extra query or
    /// marker relation. The callback is an observation, not proof of a mutation.
    pub fn prepare_cell_producers(
        &mut self,
        ruleset: &str,
        specs: &[(String, Vec<String>)],
    ) -> Result<CellProducerTap, Error> {
        let events = Arc::new(Mutex::new(vec![]));
        let mut ids = vec![];
        let unit = self.backend.base_values().get(());
        let unit_ty = self.backend.base_values().get_ty::<()>();
        for (index, (name, fields)) in specs.iter().enumerate() {
            let Some(Ruleset::Rules(rules)) = self.rulesets.get(ruleset) else {
                return Err(Error::BackendError(
                    "expected direct producer ruleset".into(),
                ));
            };
            let (rule, id) = rules
                .get(name)
                .ok_or_else(|| Error::BackendError(format!("unknown producer {name}")))?;
            if fields.is_empty() {
                ids.push(*id);
                continue;
            }
            let seminaive = self.backend.rule_is_seminaive(*id);
            let rule = rule.clone();
            let vars = rule.body.get_vars();
            let wanted = fields
                .iter()
                .map(|n| {
                    vars.iter().find(|v| v.name == *n).cloned().ok_or_else(|| {
                        Error::BackendError(format!("missing producer binding {name}.{n}"))
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let output = events.clone();
            let callback = self
                .backend
                .register_external_func(Box::new(make_external_func(move |_, v| {
                    output.lock().unwrap().push(CellProducerEvent {
                        rule: index,
                        values: v.to_vec(),
                    });
                    Some(unit)
                })));
            let mut b = BackendRule::new(
                self.backend
                    .new_rule(&format!("cell-producer:{name}"), seminaive),
                &self.functions,
                &self.type_info,
                seminaive,
            );
            b.query(&rule.body, false);
            b.actions(&rule.head)?;
            let args = wanted
                .iter()
                .map(|v| b.entry(&ResolvedAtomTerm::Var(span!(), v.clone())))
                .collect::<Vec<_>>();
            b.rb.call_external_func(callback, &args, ColumnTy::Base(unit_ty), || {
                "Cell producer".into()
            });
            ids.push(b.build());
        }
        Ok(CellProducerTap { rules: ids, events })
    }
    pub fn step_cell_producers(
        &mut self,
        tap: &CellProducerTap,
        ruleset: &str,
    ) -> Result<RunReport, Error> {
        let report = self
            .backend
            .run_rules(&tap.rules)
            .map_err(|e| Error::BackendError(e.to_string()))?;
        Ok(RunReport::singleton(ruleset, report))
    }
}
