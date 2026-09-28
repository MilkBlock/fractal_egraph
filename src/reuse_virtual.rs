//! Opt-in exact native storage packing from baked row patterns. This validates
//! existing tuples; it does not execute a baked rewrite or assume trigger truth.
use crate::{pipeline::Result, reuse_graph::library::Library};
use egglog::{
    EGraph, VirtualFactRow,
    ast::{Expr, Parser},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

pub fn pack_library(eg: &mut EGraph, library: &Library) -> Result<Value> {
    visit_library(eg, library, true, false)
}
/// Benchmark/control path: identical complete queries, without packing their rows.
pub fn match_library(eg: &mut EGraph, library: &Library) -> Result<Value> {
    visit_library(eg, library, false, false)
}
pub fn compact_library(eg: &mut EGraph, library: &Library) -> Result<Value> {
    visit_library(eg, library, true, true)
}
fn visit_library(eg: &mut EGraph, library: &Library, pack: bool, compact: bool) -> Result<Value> {
    let mut admissions = vec![];
    for (pattern_id, (q, arities)) in library.fragment_catalog().into_iter().enumerate() {
        let attempt = (|| -> Result<Value> {
            let mut parser = Parser::default();
            let values = q
                .values
                .iter()
                .map(|(sort, lit)| -> Result<_> {
                    let lit = lit
                        .as_ref()
                        .map(|s| -> Result<_> {
                            match parser.get_expr_from_string(None, s)? {
                                Expr::Lit(_, v) => Ok(v),
                                _ => Err("pattern constant is not a literal".into()),
                            }
                        })
                        .transpose()?;
                    Ok((sort.clone(), lit))
                })
                .collect::<Result<Vec<_>>>()?;
            let query = eg.compile_fact_query(&values, &q.atoms, &q.projection)?;
            let found = eg.run_fact_query(&query);
            eg.free_fact_query(query);
            let found = found?.into_iter().collect::<BTreeSet<_>>();
            if !pack {
                return Ok(
                    json!({"pattern":pattern_id,"matched_instances":found.len(),"packing":false}),
                );
            }
            if found.is_empty() {
                return Ok(json!({"pattern":pattern_id,"matched_instances":0}));
            }
            let slots: BTreeMap<_, _> = q
                .projection
                .iter()
                .enumerate()
                .map(|(slot, v)| (*v, slot))
                .collect();
            let mut next = slots.len();
            let mut missing_outputs = vec![];
            let mut rows = vec![];
            for ((op, cols), arity) in q.atoms.iter().zip(&arities) {
                let mut columns = cols
                    .iter()
                    .map(|v| slots.get(v).copied().ok_or("unprojected virtual column"))
                    .collect::<std::result::Result<Vec<_>, _>>()?;
                if cols.len() == *arity {
                    missing_outputs.push((op.clone(), columns.clone()));
                    columns.push(next);
                    next += 1;
                }
                rows.push(VirtualFactRow {
                    function: op.clone(),
                    columns,
                });
            }
            let template = eg.virtual_fact_template(rows)?;
            let mut bindings = vec![];
            for mut binding in found {
                for (op, cols) in &missing_outputs {
                    let key = cols.iter().map(|i| binding[*i]).collect::<Vec<_>>();
                    binding.push(
                        eg.lookup_function(op, &key)
                            .ok_or("matched virtual row disappeared")?,
                    );
                }
                bindings.push(binding);
            }
            if compact {
                let r=eg.compact_cell_facts(&[(&template,&bindings)])?;
                return Ok(json!({"pattern":pattern_id,"matched_instances":bindings.len(),"cell_instances":r.cell_instances,"cell_batches":r.cell_batches,"owned_rows":r.owned_rows,"existing_cell_rows":r.existing_cell_rows}));
            }
            let report = eg.virtualize_existing_facts(&template, &bindings)?;
            Ok(
                json!({"pattern":pattern_id,"matched_instances":bindings.len(),"requested_rows":report.requested_rows,"distinct_native_rows":report.distinct_native_rows}),
            )
        })();
        admissions.push(attempt.unwrap_or_else(
            |e| json!({"pattern":pattern_id,"status":"not_packed","reason":e.to_string()}),
        ));
    }
    Ok(
        json!({"compact_cells":if compact {compact_report(eg)}else{Value::Null},"admissions":admissions,"storage":storage_report(eg),"scope":if pack {"exact packing of already committed native facts; no inferred facts, approximate match or candidate budget"} else {"complete native pattern queries only; no row packing"}}),
    )
}
pub fn storage_report(eg: &EGraph) -> Value {
    let mut instances = BTreeMap::new();
    let mut templates = BTreeMap::new();
    let mut tables = vec![];
    let mut shared_indexes = BTreeMap::new();
    let mut binding_headers = BTreeMap::new();
    let mut template_headers = BTreeMap::new();
    for (name, s) in eg.virtual_fact_stats() {
        shared_indexes.extend(s.shared_index_bytes);
        instances.extend(s.instances);
        templates.extend(s.templates);
        binding_headers.extend(s.binding_headers);
        template_headers.extend(s.template_headers);
        tables.push(json!({"table":name,"virtual_rows":s.virtual_rows,"direct_base_rows":s.direct_base_rows,"overlay_rows":s.overlay_rows,"tombstones":s.tombstones,"rebuild_rows_examined":s.rebuild_rows_examined,"rebuild_rows_changed":s.rebuild_rows_changed,"dense_equivalent_bytes":s.dense_equivalent_bytes,"descriptor_bytes":s.descriptor_bytes,"direct_bytes":s.direct_bytes,"metadata_bytes":s.metadata_bytes,"local_header_bytes":s.local_header_bytes}));
    }
    json!({"shared_binding_index_bytes":shared_indexes.values().sum::<usize>(),"tables":tables,"unique_binding_matrices":instances.len(),"unique_templates":templates.len(),"binding_payload_bytes":instances.values().sum::<usize>(),"template_payload_bytes":templates.values().sum::<usize>(),"binding_header_bytes":binding_headers.values().sum::<usize>(),"template_header_bytes":template_headers.values().sum::<usize>(),"accounting":"row descriptors include capacity; matrix/template payload and known headers are deduplicated; excludes native hash/provenance indexes, tombstone hash allocation, pool/spare dense capacity and allocator overhead; not RSS or net compression"})
}

pub fn compact_report(eg:&EGraph)->Value {
    let mut bindings=BTreeMap::new();let mut indexes=BTreeMap::new();let mut bodies=BTreeMap::new();let mut codes=BTreeMap::new();let mut tables=vec![];
    for (name,s) in eg.compact_cell_storage(){bindings.extend(s.bindings);indexes.extend(s.binding_indexes);bodies.extend(s.bodies);codes.extend(s.codes);tables.push(json!({"table":name,"cell_instances_referenced":s.cells,"cell_rows":s.rows,"native_hash_rows":s.native_hash_rows,"materialized_fallbacks":s.materialized_fallbacks,"projections":s.projections,"projection_bytes":s.projection_bytes}));}
    json!({"tables":tables,"unique_cell_batches":bindings.len(),"bindings_bytes":bindings.values().sum::<usize>(),"binding_index_bytes":indexes.values().sum::<usize>(),"cell_headers_bytes":bodies.values().sum::<usize>(),"cell_code_bytes":codes.values().sum::<usize>(),"scope":"whole-cell bodies shared across operator tables; derived row positions, no native hash entry per compact fact; existing eclass identities retained in bindings"})
}
