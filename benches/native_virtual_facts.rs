//! Same-engine native scan timing and explicitly scoped representation accounting.
use egglog::{EGraph, VirtualFactRow};
use std::{collections::BTreeMap, hint::black_box, time::Instant};
fn median(eg: &mut EGraph, q: &egglog::CompiledFactQuery, count: usize) -> f64 {
    let mut samples = vec![];
    for i in 0..35 {
        let start = Instant::now();
        for _ in 0..8 {
            let rows = eg.run_fact_query(q).unwrap();
            assert_eq!(rows.len(), count);
            black_box(rows);
        }
        if i >= 5 {
            samples.push(start.elapsed().as_secs_f64() / 8.);
        }
    }
    samples.sort_by(f64::total_cmp);
    (samples[14] + samples[15]) / 2.
}
fn main() {
    let mut reports = vec![];
    for arity in [1usize, 8] {
        let n = 1024usize;
        let mut e = EGraph::default();
        let sorts = std::iter::repeat_n("E", arity)
            .collect::<Vec<_>>()
            .join(" ");
        let args = std::iter::repeat_n("x", arity)
            .collect::<Vec<_>>()
            .join(" ");
        let mut src = format!(
            "(datatype E (V i64) (R {sorts})) (relation Seed (E)) (rule ((Seed x)) ((R {args})))"
        );
        for i in 0..n {
            src.push_str(&format!("(Seed (V {i}))"));
        }
        src.push_str("(run 1)");
        e.parse_and_run_program(None, &src).unwrap();
        let mut cols = vec![0; arity];
        cols.push(1);
        let q = e
            .compile_fact_query(
                &[("E".into(), None), ("E".into(), None)],
                &[("R".into(), cols.clone())],
                &[0, 1],
            )
            .unwrap();
        let bindings = e.run_fact_query(&q).unwrap();
        let dense = median(&mut e, &q, n);
        let t = e
            .virtual_fact_template(vec![VirtualFactRow {
                function: "R".into(),
                columns: cols,
            }])
            .unwrap();
        let start = Instant::now();
        e.virtualize_existing_facts(&t, &bindings).unwrap();
        let packing = start.elapsed().as_secs_f64();
        let virtual_time = median(&mut e, &q, n);
        e.free_fact_query(q);
        let mut instances = BTreeMap::new();
        let mut templates = BTreeMap::new();
        let mut payload = 0;
        let mut headers = 0;
        let mut binding_headers = BTreeMap::new();
        let mut template_headers = BTreeMap::new();
        let mut original = 0;
        for (_, s) in e.virtual_fact_stats() {
            original += s.dense_equivalent_bytes;
            payload += s.descriptor_bytes + s.metadata_bytes + s.direct_bytes;
            headers += s.local_header_bytes;
            binding_headers.extend(s.binding_headers);
            template_headers.extend(s.template_headers);
            instances.extend(s.instances);
            templates.extend(s.templates);
        }
        payload += instances.values().sum::<usize>() + templates.values().sum::<usize>();
        headers +=
            binding_headers.values().sum::<usize>() + template_headers.values().sum::<usize>();
        reports.push(serde_json::json!({"arity":arity,"rows":n,"dense_query_seconds":dense,"virtual_query_seconds":virtual_time,"query_ratio":virtual_time/dense,"pack_seconds":packing,"dense_row_payload_bytes":original,"accounted_virtual_payload_bytes":payload,"payload_ratio":payload as f64/original as f64,"known_header_bytes":headers,"accounted_representation_ratio":(payload+headers) as f64/original as f64}));
    }
    println!(
        "{}",
        serde_json::json!({"scope":"actual native same-engine facts at different bindings; query includes callback cost; known representation headers reported separately; excludes native indexes/provenance/tombstone hashes/pools/allocator; not RSS or net compression","cases":reports})
    );
}
