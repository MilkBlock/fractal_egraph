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
impl EGraph {
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
