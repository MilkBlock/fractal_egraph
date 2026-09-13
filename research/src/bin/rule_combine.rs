#[path = "slotted_baseline/alloc.rs"]
mod alloc;
#[path = "rule_combine/compose.rs"]
mod compose;
#[path = "rule_combine/runtime.rs"]
mod runtime;
use serde_json::json;
use std::sync::atomic::Ordering;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).expect("original|observe|static|dynamic");
    let case = args.get(2).expect("chain|algebra|negative");
    assert!(["chain", "algebra", "negative"].contains(&case.as_str()));
    let n: usize = args.get(3).unwrap_or(&"64".into()).parse().unwrap();
    let waves: usize = args.get(4).unwrap_or(&"4".into()).parse().unwrap();
    assert!(n > 0 && waves > 0 && n * waves < 100000);
    let initial = alloc::LIVE.load(Ordering::Relaxed);
    alloc::PEAK.store(initial, Ordering::Relaxed);
    let start = std::time::Instant::now();
    let r = runtime::run(case, mode, n, waves);
    let total_ns = start.elapsed().as_nanos();
    let retained = alloc::LIVE.load(Ordering::Relaxed) - initial;
    let peak = alloc::PEAK.load(Ordering::Relaxed) - initial;
    let closure = runtime::Snapshot::new(&r.graph).closure(&r.roots);
    println!(
        "{}",
        json!({"mode":mode,"case":case,"roots":r.roots.len(),"waves":waves,"rounds":r.rounds,"observed_logical_matches":r.events,"discovery_probes":r.probes,"enabled":r.enabled,"selected":r.selected,"activation":r.activation,"preprocess_ns":r.preprocess_ns,"inference_ns":r.inference_ns,"discovery_ns":r.discovery_ns,"install_ns":r.install_ns,"total_ns":total_ns,"retained_bytes":retained,"peak_bytes":peak,"closure":closure,"candidates":r.candidates.iter().map(|c|json!({"first":c.first,"second":c.second,"path":c.path,"lhs":c.rule.lhs.to_string(),"middle":c.middle.to_string(),"rhs":c.rule.rhs.to_string()})).collect::<Vec<_>>()})
    );
}
