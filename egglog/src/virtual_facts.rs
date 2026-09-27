//! Exact virtualization of existing, committed native facts. This is a storage
//! operation, not a rule: it never invents tuples, equalities, or provenance.
use super::*;
use core_relations::{NativeRowInstance, NativeRowTemplate, NativeVirtualStats, VirtualRowRequest};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug)]
pub struct VirtualFactRow {
    pub function: String,
    pub columns: Vec<usize>,
}
#[derive(Clone)]
pub struct VirtualFactTemplate {
    rows: Vec<VirtualFactRow>,
    sorts: Vec<String>,
    core: Arc<NativeRowTemplate>,
}
#[derive(Debug)]
pub struct VirtualizeReport {
    pub requested_rows: usize,
    pub distinct_native_rows: usize,
    pub tables: Vec<(String, NativeVirtualStats)>,
}
#[derive(Debug)]
pub struct VirtualizeBatchReport {
    pub requested_rows: usize,
    pub distinct_native_rows: usize,
    pub new_virtual_rows: usize,
    pub affected_tables: usize,
    pub shared_rows_reused: usize,
    pub validation_seconds: f64,
    pub encoding_seconds: f64,
}
impl EGraph {
    /// Consume a caller-owned notification relation through native
    /// deletion. This must not be used for semantic input relations: subscribers
    /// would observe removal. Constructor facts and their versions are untouched.
    pub fn drain_notification_relation(&mut self, name: &str) -> Result<Vec<Vec<Value>>, Error> {
        let f = self
            .functions
            .get(name)
            .ok_or_else(|| Error::BackendError("unknown notification relation".into()))?;
        if f.schema.output.name() != "Unit"
            && !(f.schema.output.name().starts_with('@')
                && !self.type_info.is_sort_unionable(&f.schema.output))
        {
            return Err(Error::BackendError(
                "expected a Unit or internal non-unionable relation output".into(),
            ));
        }
        let arity = f.schema.input.len();
        let id = f.backend_id;
        let mut rows = vec![];
        self.backend
            .for_each(id, |r| rows.push(r.vals[..arity].to_vec()));
        if !rows.is_empty() {
            let action = egglog_bridge::TableAction::new(&self.backend, id);
            self.backend.with_execution_state(|state| {
                for row in &rows {
                    action.remove(state, row);
                }
            });
            self.backend.flush_updates();
        }
        Ok(rows)
    }
    /// Drain private notifications in one physical update wave. Public semantic
    /// relations must not use this API. Constructor facts are not removed.
    pub fn drain_notification_relations(&mut self,names:&[String])->Result<BTreeMap<String,Vec<Vec<Value>>>,Error> {
        let mut batches=BTreeMap::new();let mut ids=vec![];
        for name in names {
            let f=self.functions.get(name).ok_or_else(||Error::BackendError("unknown notification relation".into()))?;
            if f.schema.output.name()!="Unit" && !(f.schema.output.name().starts_with('@')&&!self.type_info.is_sort_unionable(&f.schema.output)) {
                return Err(Error::BackendError("expected a private notification relation".into()));
            }
            let mut rows=vec![];let arity=f.schema.input.len();
            self.backend.for_each(f.backend_id,|r|rows.push(r.vals[..arity].to_vec()));
            if !rows.is_empty(){ids.push((name.clone(),f.backend_id));}
            batches.insert(name.clone(),rows);
        }
        if !ids.is_empty(){
            self.backend.with_execution_state(|state|{
                for (name,id) in &ids {
                    let action=egglog_bridge::TableAction::new(&self.backend,*id);
                    for row in &batches[name] {action.remove(state,row);}
                }
            });self.backend.flush_updates();
        }
        Ok(batches)
    }

    /// Every logical column (arguments and result) names a typed binding slot.
    /// Constants can be represented by slots bound to their actual native Value.
    pub fn virtual_fact_template(
        &self,
        rows: Vec<VirtualFactRow>,
    ) -> Result<VirtualFactTemplate, Error> {
        let error = |s: &str| Error::BackendError(s.into());
        let core = NativeRowTemplate::new(rows.iter().map(|r| r.columns.clone()).collect())
            .map_err(|s| error(&s))?;
        let mut sorts = vec![None; core.slots()];
        for r in &rows {
            let f = self
                .functions
                .get(&r.function)
                .ok_or_else(|| error("unknown virtual fact function"))?;
            let types: Vec<_> = f.schema.input.iter().chain([&f.schema.output]).collect();
            if r.columns.len() != types.len() {
                return Err(error("virtual logical row arity mismatch"));
            }
            for (&slot, ty) in r.columns.iter().zip(types) {
                if sorts[slot]
                    .as_ref()
                    .is_some_and(|old: &String| old != ty.name())
                {
                    return Err(error("virtual slot is used at incompatible sorts"));
                }
                sorts[slot] = Some(ty.name().to_string());
            }
        }
        Ok(VirtualFactTemplate {
            rows,
            sorts: sorts.into_iter().map(Option::unwrap).collect(),
            core,
        })
    }
    /// Caller supplies observed instance bindings. No discovery heuristic, beam,
    /// row budget or approximate equality is used. All payloads must match native
    /// rows exactly before any table changes representation. Overlaps share the
    /// existing native key; first storage ownership is retained without changing
    /// the actual fact's producer.
    pub fn virtualize_existing_facts(
        &mut self,
        template: &VirtualFactTemplate,
        bindings: &[Vec<Value>],
    ) -> Result<VirtualizeReport, Error> {
        let error = |s: &str| Error::BackendError(s.into());
        let sorts = template
            .sorts
            .iter()
            .map(|s| {
                self.type_info
                    .get_sort_by_name(s)
                    .ok_or_else(|| error("missing virtual slot sort"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let requested_rows = bindings
            .len()
            .checked_mul(template.rows.len())
            .ok_or_else(|| error("virtual request size overflow"))?;
        let mut unique = BTreeSet::new();
        let mut active = vec![];
        let mut requests = vec![];
        for binding in bindings {
            if binding.len() != sorts.len() {
                return Err(error("virtual instance binding arity mismatch"));
            }
            let binding = binding
                .iter()
                .zip(&sorts)
                .map(|(v, s)| self.get_canonical_value(*v, s))
                .collect::<Vec<_>>();
            let mut owned = vec![];
            for (row, spec) in template.rows.iter().enumerate() {
                let f = self
                    .functions
                    .get(&spec.function)
                    .ok_or_else(|| error("missing virtual function"))?;
                let types: Vec<_> = f.schema.input.iter().chain([&f.schema.output]).collect();
                if types.len() != spec.columns.len()
                    || types
                        .iter()
                        .zip(&spec.columns)
                        .any(|(s, i)| s.name() != template.sorts[*i])
                {
                    return Err(error("virtual template schema changed"));
                }
                let values = spec.columns.iter().map(|i| binding[*i]).collect::<Vec<_>>();
                let key = values[..f.schema.input.len()].to_vec();
                let actual = self
                    .backend
                    .lookup_row(f.backend_id, &key)
                    .ok_or_else(|| error("virtualization requires an existing native fact"))?;
                if actual.len() < values.len() || actual[..values.len()] != values {
                    return Err(error("virtual output does not match the committed fact"));
                }
                if unique.insert((spec.function.clone(), key.clone()))
                    && !self.backend.function_row_is_virtual(f.backend_id, &key)
                {
                    owned.push((spec.function.clone(), key, row));
                }
            }
            if !owned.is_empty() {
                active.push(binding);
                requests.push(owned);
            }
        }
        let instances =
            NativeRowInstance::batch(template.core.clone(), active).map_err(|s| error(&s))?;
        let mut groups: BTreeMap<String, Vec<VirtualRowRequest>> = BTreeMap::new();
        for (instance, owned) in instances.into_iter().zip(requests) {
            for (name, key, template_row) in owned {
                groups.entry(name).or_default().push(VirtualRowRequest {
                    key,
                    instance: instance.clone(),
                    template_row,
                });
            }
        }
        for (name, rows) in &groups {
            self.backend
                .virtualize_function_rows(self.functions[name].backend_id, rows)
                .map_err(|s| error(&s))?;
        }
        Ok(VirtualizeReport {
            requested_rows,
            distinct_native_rows: unique.len(),
            tables: template
                .rows
                .iter()
                .map(|r| r.function.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .filter_map(|name| {
                    self.backend
                        .virtual_function_stats(self.functions[&name].backend_id)
                        .map(|s| (name, s))
                })
                .collect(),
        })
    }
    /// Pack a wave of cell instances together. Validate every proposal before
    /// modifying storage, retain the first owner of shared rows, and encode each
    /// native table at most once. No semantic tuple, ID or version changes.
    pub fn virtualize_existing_facts_batch(&mut self, batches:&[(&VirtualFactTemplate,&[Vec<Value>])])->Result<VirtualizeBatchReport,Error> {
        self.pack_fact_wave(batches,false)
    }
    /// Reuse existing virtual owners and encode only the residual rows. Ignored
    /// shared proposals are NOT certified as matching their supplied payload;
    /// every row that changes representation is still validated exactly.
    pub fn virtualize_unshared_facts_batch(&mut self,batches:&[(&VirtualFactTemplate,&[Vec<Value>])])->Result<VirtualizeBatchReport,Error> {
        self.pack_fact_wave(batches,true)
    }
    fn pack_fact_wave(
        &mut self,
        batches: &[(&VirtualFactTemplate, &[Vec<Value>])],
        reuse_shared: bool,
    ) -> Result<VirtualizeBatchReport, Error> {
        use smallvec::SmallVec;
        let mut shared_rows_reused = 0;
        let started = web_time::Instant::now();
        let error = |s: &str| Error::BackendError(s.into());
        // Identity is native function + canonical key. Store only the checked
        // output: copying the name, key, and complete native row for every fact
        // duplicates most of the graph in the packing scratch space.
        let mut observed = HashMap::<(egglog_bridge::FunctionId, SmallVec<[Value; 2]>), Value>::default();
        let mut groups = BTreeMap::<egglog_bridge::FunctionId, Vec<VirtualRowRequest>>::new();
        let mut requested_rows = 0usize;
        for (template, bindings) in batches {
            let sorts = template.sorts.iter().map(|s| self.type_info.get_sort_by_name(s)
                .ok_or_else(|| error("missing virtual slot sort"))).collect::<Result<Vec<_>, _>>()?;
            let mut schema = Vec::with_capacity(template.rows.len());
            for spec in &template.rows {
                let f = self.functions.get(&spec.function).ok_or_else(|| error("missing virtual function"))?;
                let types = f.schema.input.iter().chain([&f.schema.output]).collect::<Vec<_>>();
                if types.len() != spec.columns.len() || types.iter().zip(&spec.columns).any(|(s, i)| s.name() != template.sorts[*i]) {
                    return Err(error("virtual template schema changed"));
                }
                schema.push((f.backend_id, f.schema.input.len()));
            }
            requested_rows = requested_rows.checked_add(bindings.len().checked_mul(template.rows.len())
                .ok_or_else(|| error("virtual request size overflow"))?).ok_or_else(|| error("virtual request size overflow"))?;
            let mut active = vec![];
            let mut requests = vec![];
            for binding in *bindings {
                if binding.len() != sorts.len() { return Err(error("virtual instance binding arity mismatch")); }
                let binding = binding.iter().zip(&sorts).map(|(v, s)| self.get_canonical_value(*v, s)).collect::<Vec<_>>();
                let mut owned = vec![];
                for (row, (spec, &(function, arity))) in template.rows.iter().zip(&schema).enumerate() {
                    let key = spec.columns[..arity].iter().map(|i| binding[*i]).collect::<SmallVec<[Value; 2]>>();
                    if reuse_shared && self.backend.function_row_is_virtual(function, &key) {
                        shared_rows_reused += 1;
                        continue;
                    }
                    let expected = binding[spec.columns[arity]];
                    match observed.entry((function, key)) {
                        hashbrown::hash_map::Entry::Occupied(v) => {
                            if *v.get() != expected { return Err(error("virtual output does not match the committed fact")); }
                        }
                        hashbrown::hash_map::Entry::Vacant(v) => {
                            let key = &v.key().1;
                            let actual = self.backend.lookup_row(function, key).ok_or_else(|| error("virtualization requires an existing native fact"))?;
                            if actual.get(arity) != Some(&expected) { return Err(error("virtual output does not match the committed fact")); }
                            if !self.backend.function_row_is_virtual(function, key) {
                                owned.push((function, key.to_vec(), row));
                            }
                            v.insert(expected);
                        }
                    }
                }
                if !owned.is_empty() { active.push(binding); requests.push(owned); }
            }
            let instances = NativeRowInstance::batch(template.core.clone(), active).map_err(|s| error(&s))?;
            for (instance, owned) in instances.into_iter().zip(requests) {
                for (function, key, template_row) in owned {
                    groups.entry(function).or_default().push(VirtualRowRequest { key, instance: instance.clone(), template_row });
                }
            }
        }
        let distinct_native_rows = observed.len();
        drop(observed); // Validation is complete before any table is changed.
        let new_virtual_rows = groups.values().map(Vec::len).sum();
        let affected_tables = groups.len();
        let validation_seconds = started.elapsed().as_secs_f64();
        let encode = web_time::Instant::now();
        // Move each table's requests out so its scratch memory is released before
        // the next table is packed; the physical shared instances remain alive.
        for (function, rows) in groups {
            self.backend.virtualize_function_rows(function, &rows).map_err(|s| error(&s))?;
        }
        Ok(VirtualizeBatchReport {
            requested_rows, distinct_native_rows, new_virtual_rows, affected_tables,
            shared_rows_reused, validation_seconds, encoding_seconds: encode.elapsed().as_secs_f64(),
        })
    }

    /// Explicitly return virtual storage to native dense rows. Only physical row
    /// positions/generation change; facts, values, timestamps and origins do not.
    pub fn materialize_virtual_facts(&mut self) -> usize {
        self.functions
            .values()
            .filter(|f| self.backend.materialize_function_rows(f.backend_id))
            .count()
    }
    pub fn virtual_fact_stats(&self) -> Vec<(String, NativeVirtualStats)> {
        self.functions
            .iter()
            .filter_map(|(name, f)| {
                self.backend
                    .virtual_function_stats(f.backend_id)
                    .map(|s| (name.clone(), s))
            })
            .collect()
    }
}
