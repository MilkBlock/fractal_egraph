//! Diagnostic only: exact query/packing phases, Rust live allocations, macOS
//! allocator statistics and RSS. This is not a timing benchmark.
use egg_layout::reuse_graph::{library::Library, native_storage};
use egglog::{
    EGraph, VirtualFactRow,
    ast::{Expr, Parser},
};
use serde_json::json;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
};
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
struct Tracked;
fn add(n: usize) {
    let value = LIVE.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(value, Relaxed);
}
unsafe impl GlobalAlloc for Tracked {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            add(l.size());
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        LIVE.fetch_sub(l.size(), Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let r = unsafe { System.realloc(p, l, n) };
        if !r.is_null() {
            if n >= l.size() {
                add(n - l.size());
            } else {
                LIVE.fetch_sub(l.size() - n, Relaxed);
            }
        }
        r
    }
}
#[global_allocator]
static ALLOC: Tracked = Tracked;
#[cfg(target_os = "macos")]
#[derive(Default)]
#[repr(C)]
struct Stats {
    blocks: u32,
    live: usize,
    max: usize,
    reserved: usize,
}
#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn malloc_zone_statistics(zone: *mut std::ffi::c_void, stats: *mut Stats);
    fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
}
fn point(stage: &str, pattern: Option<usize>, rows: usize) {
    let live = LIVE.load(Relaxed);
    let peak = PEAK.load(Relaxed);
    #[cfg(target_os = "macos")]
    let allocator = {
        let mut s = Stats::default();
        unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut s) };
        json!({"blocks":s.blocks,"live_bytes":s.live,"reserved_bytes":s.reserved})
    };
    #[cfg(not(target_os = "macos"))]
    let allocator = serde_json::Value::Null;
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    let rss = String::from_utf8(output.stdout)
        .unwrap()
        .trim()
        .parse::<usize>()
        .unwrap()
        * 1024;
    println!(
        "{}",
        json!({"stage":stage,"pattern":pattern,"rows":rows,"rust_live_bytes":live,"rust_peak_bytes":peak,"rss_bytes":rss,"allocator":allocator})
    );
}
fn relief() {
    #[cfg(target_os = "macos")]
    println!(
        "{}",
        json!({"pressure_relief_bytes":unsafe{malloc_zone_pressure_relief(std::ptr::null_mut(),0)}})
    );
}
fn shape(eg: &EGraph) -> (usize, usize) {
    let mut classes = BTreeSet::new();
    let mut rows = 0;
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if f.is_let_binding() || f.schema().output.name() != "Math" {
            continue;
        }
        eg.function_for_each(&name, |r| {
            rows += 1;
            classes.insert(eg.get_canonical_value(*r.vals.last().unwrap(), &f.schema().output));
        })
        .unwrap();
    }
    (rows, classes.len())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() != 3 || !["match", "pack"].contains(&a[2].as_str()) {
        return Err("virtual_memory_audit SOURCE LIBRARY match|pack".into());
    }
    let mut eg = EGraph::default();
    point("empty", None, 0);
    eg.parse_and_run_program(Some(a[0].clone()), &std::fs::read_to_string(&a[0])?)?;
    let before = shape(&eg);
    point("native", None, before.0);
    let lib = Library::load(std::path::Path::new(&a[1]))?;
    point("library_loaded", None, 0);
    for (id, (q, arities)) in lib.fragment_catalog().into_iter().enumerate() {
        if std::env::var("MEMORY_AUDIT_PATTERN")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .is_some_and(|selected| selected != id)
        {
            continue;
        }
        let mut parser = Parser::default();
        let values = q
            .values
            .iter()
            .map(|(s, l)| {
                let lit = l
                    .as_ref()
                    .map(|s| match parser.get_expr_from_string(None, s).unwrap() {
                        Expr::Lit(_, v) => v,
                        _ => panic!("nonliteral"),
                    });
                (s.clone(), lit)
            })
            .collect::<Vec<_>>();
        let query = match eg.compile_fact_query(&values, &q.atoms, &q.projection) {
            Ok(q) => q,
            Err(e) => {
                println!("{}", json!({"pattern":id,"rejected":e.to_string()}));
                continue;
            }
        };
        println!(
            "{}",
            json!({"pattern":id,"cached_subset_bytes":eg.fact_query_subset_bytes(&query)})
        );
        point("compiled", Some(id), 0);
        let found = eg.run_fact_query(&query)?;
        point("query_results", Some(id), found.len());
        eg.free_fact_query(query);
        point("query_freed", Some(id), found.len());
        let found = found.into_iter().collect::<BTreeSet<_>>();
        point("deduplicated", Some(id), found.len());
        println!(
            "{}",
            json!({"pattern":id,"atoms":q.atoms.len(),"slots":q.projection.len(),"matched_instances":found.len()})
        );
        if a[2] == "pack" && !found.is_empty() {
            let slots = q
                .projection
                .iter()
                .enumerate()
                .map(|(i, v)| (*v, i))
                .collect::<BTreeMap<_, _>>();
            let mut next = slots.len();
            let mut missing = vec![];
            let mut rows = vec![];
            for ((op, cols), arity) in q.atoms.iter().zip(&arities) {
                let mut columns = cols.iter().map(|v| slots[v]).collect::<Vec<_>>();
                if cols.len() == *arity {
                    missing.push((op.clone(), columns.clone()));
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
            for mut b in found {
                for (op, cols) in &missing {
                    let key = cols.iter().map(|i| b[*i]).collect::<Vec<_>>();
                    b.push(eg.lookup_function(op, &key).ok_or("missing output")?);
                }
                bindings.push(b);
            }
            point("packing_inputs", Some(id), bindings.len());
            eg.virtualize_existing_facts(&template, &bindings)?;
            point("packed", Some(id), bindings.len());
            drop(bindings);
            drop(template);
        } else {
            drop(found);
        }
        point("pattern_temporaries_dropped", Some(id), 0);
    }
    let after = shape(&eg);
    assert_eq!(before, after);
    point("all_patterns", None, after.0);
    println!(
        "{}",
        json!({"shape":after,"storage":native_storage::storage_report(&eg)})
    );
    drop(lib);
    point("library_dropped", None, 0);
    relief();
    point("engine_alive_after_relief", None, 0);
    drop(eg);
    point("engine_dropped", None, 0);
    relief();
    point("engine_dropped_after_relief", None, 0);
    Ok(())
}
