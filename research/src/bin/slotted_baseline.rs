//! Direct, pinned slotted-egraphs vs local egglog baseline; no snapshot replay.
#[path = "slotted_baseline/alloc.rs"]
mod alloc;
#[path = "slotted_baseline/engine.rs"]
mod engine;
use serde_json::json;
use std::{sync::atomic::Ordering, time::Instant};
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).expect("slotted|egglog");
    let case = args.get(2).expect("renamed|constants|ac");
    assert!(["renamed", "constants", "ac"].contains(&case.as_str()));
    let count: u32 = args.get(3).expect("root count").parse().unwrap();
    assert!(count > 0 && count < 1000000);
    let start_bytes = alloc::LIVE.load(Ordering::Relaxed);
    alloc::PEAK.store(start_bytes, Ordering::Relaxed);
    let start = Instant::now();
    let mut e = engine::Engine::new(mode);
    for i in 0..count {
        e.insert(&engine::input(case, i));
    }
    let build_ns = start.elapsed().as_nanos();
    let built_bytes = alloc::LIVE
        .load(Ordering::Relaxed)
        .saturating_sub(start_bytes);
    let start = Instant::now();
    let mut rounds = 0;
    let mut saturated = case != "ac";
    if case == "ac" {
        for _ in 0..32 {
            rounds += 1;
            if !e.step() {
                saturated = true;
                break;
            }
        }
    }
    let rewrite_ns = start.elapsed().as_nanos();
    assert!(saturated, "iteration cap reached");
    let retained = alloc::LIVE
        .load(Ordering::Relaxed)
        .saturating_sub(start_bytes);
    let peak = alloc::PEAK
        .load(Ordering::Relaxed)
        .saturating_sub(start_bytes);
    let (nodes, classes, root_classes) = e.counts();
    let mut checks = 0;
    for i in [0, count / 2, count - 1]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>()
    {
        if case == "ac" {
            for t in engine::ac_terms(i) {
                assert!(e.root_matches(i as usize, &t));
                checks += 1;
            }
        } else {
            assert!(e.root_matches(i as usize, &engine::input(case, i)));
            checks += 1;
        }
    }
    if count > 1 {
        assert!(!e.root_matches(0, &engine::input(case, count - 1)));
        checks += 1;
    }
    println!(
        "{}",
        json!({"engine":mode,"case":case,"roots":count,"slotted_version":"0.0.36","slotted_checks":cfg!(feature="slotted-checks"),"built_requested_bytes":built_bytes,"retained_requested_bytes":retained,"peak_requested_bytes":peak,"build_ns":build_ns,"rewrite_ns":rewrite_ns,"rounds":rounds,"saturated":saturated,"constructor_nodes":nodes,"live_classes":classes,"distinct_root_classes":root_classes,"external_slot_map_entries":e.root_maps(),"verified_terms":checks})
    );
}
