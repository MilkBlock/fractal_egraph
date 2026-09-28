//! Native RSS and logical storage on the fully grown Math run, before and after
//! library packing. The graph is not rebuilt between samples, so the two RSS
//! readings refer to the same equivalence classes (same e-node/e-class counts).
//!
//! mode `pack`  : adopt the library's rows as virtual facts
//! mode `match` : run the same fragment queries without packing (control)
use egg_layout::reuse_graph::{library::Library, native_storage};
use egglog::EGraph;
use serde_json::json;
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

fn rss_bytes() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse::<u64>()
        .unwrap()
        * 1024
}

/// Math constructor rows and canonical Math result classes, as in math_growth.
fn shape(eg: &EGraph) -> (usize, usize) {
    let mut classes = BTreeSet::new();
    let mut enodes = 0usize;
    for name in eg.get_function_names() {
        let f = eg.get_function(&name).unwrap();
        if f.is_let_binding() || f.schema().output.name() != "Math" {
            continue;
        }
        let sort = f.schema().output.clone();
        eg.function_for_each(&name, |r| {
            enodes += 1;
            classes.insert(eg.value_to_class_id(&sort, *r.vals.last().unwrap()).to_string());
        })
        .unwrap();
    }
    (enodes, classes.len())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 2 || args.len() > 3 {
        return Err("math_pack_rss SOURCE.egg LIBRARY.json [pack|match]".into());
    }
    let mode = args.get(2).map(String::as_str).unwrap_or("pack");
    if mode != "pack" && mode != "match" {
        return Err("mode must be pack or match".into());
    }

    let mut eg = EGraph::default();
    let t = Instant::now();
    eg.parse_and_run_program(Some(args[0].clone()), &std::fs::read_to_string(&args[0])?)?;
    let run_seconds = t.elapsed().as_secs_f64();
    let (enodes_before, eclasses_before) = shape(&eg);
    let rss_before = rss_bytes();

    let library = Library::load(Path::new(&args[1]))?;
    let library_bytes = std::fs::metadata(&args[1])?.len();
    let rss_loaded = rss_bytes();

    let t = Instant::now();
    let applied = if mode == "pack" {
        native_storage::pack_library(&mut eg, &library)?
    } else {
        native_storage::match_library(&mut eg, &library)?
    };
    let apply_seconds = t.elapsed().as_secs_f64();
    let (enodes_after, eclasses_after) = shape(&eg);
    let rss_after = rss_bytes();
    let virtual_rows: usize = eg
        .virtual_fact_stats()
        .iter()
        .map(|(_, s)| s.virtual_rows)
        .sum();
    let report = native_storage::storage_report(&eg);

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "mode": mode,
            "source": args[0],
            "library": args[1],
            "library_file_bytes": library_bytes,
            "native_run_seconds": run_seconds,
            "apply_seconds": apply_seconds,
            "enodes_before": enodes_before,
            "eclasses_before": eclasses_before,
            "enodes_after": enodes_after,
            "eclasses_after": eclasses_after,
            "rss_before_bytes": rss_before,
            "rss_loaded_bytes": rss_loaded,
            "rss_after_bytes": rss_after,
            "rss_ratio_before_over_after": rss_before as f64 / rss_after as f64,
            "virtual_rows": virtual_rows,
            "applied": applied,
            "storage_report": report,
            "accounting": "RSS read with ps at the same graph; peak RSS and allocator retention are not separated. storage_report is logical accounting, not RSS or net compression.",
        }))?
    );
    Ok(())
}
