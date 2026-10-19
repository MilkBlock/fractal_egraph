"""Optional production boundary profiler; no engine/dependency instrumentation."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess


def build(root, repo, out):
    out.mkdir(parents=True, exist_ok=False)
    source = repo / 'dag_in_context/src/lib.rs'
    old = source.read_text()
    original_bin = repo / 'target/release/eggcc'
    backup = out / 'eggcc-original'
    shutil.copy2(original_bin, backup)
    def replace(text, a, b):
        assert text.count(a) == 1, a
        return text.replace(a, b, 1)
    marker = '            let mut egraph = new_single_threaded_egraph();\n            egraph.parse_and_run_program(None, &egglog_prog)?;'
    patched = replace(old, marker, '''            pass_schedule_profile::verify_production(new_single_threaded_egraph);
            let execution_start = Instant::now();
            let mut egraph = new_single_threaded_egraph();
            let schedule_profile = pass_schedule_profile::execute(&mut egraph, &egglog_prog)?;
            let execution_seconds = execution_start.elapsed().as_secs_f64();
            let run_report = egraph.get_overall_run_report();
            let phase_totals = serde_json::json!({
                "search_and_apply_seconds": run_report.search_and_apply_time_per_ruleset.values().map(Duration::as_secs_f64).sum::<f64>(),
                "insert_merge_seconds": run_report.merge_time_per_ruleset.values().map(Duration::as_secs_f64).sum::<f64>(),
                "rebuild_seconds": run_report.rebuild_time_per_ruleset.values().map(Duration::as_secs_f64).sum::<f64>(),
                "matches":run_report.num_matches_per_rule.values().sum::<usize>()});
''')
    patched = replace(patched, '            let (iter_result, region_timings, extract_time) =',
        '            let extraction_wall_start = Instant::now();\n            let (iter_result, region_timings, extract_time) =')
    marker = '            eggcc_extraction_time += extract_time;'
    patched = replace(patched, marker, '''            let extraction_wall = extraction_wall_start.elapsed().as_secs_f64();
            if let Ok(dir) = std::env::var("EGGCC_PASS_PROFILE_DIR") {
                let dir = PathBuf::from(dir); fs::create_dir_all(&dir).unwrap();
                let prefix = format!("pass-{}", i+1);
                fs::write(dir.join(format!("{prefix}.egg")), &egglog_prog).unwrap();
                let row = serde_json::json!({"pass":i+1,"batch":batch,"schedule":schedule.egglog_schedule(),
                    "execution_seconds":execution_seconds,"schedule_profile":schedule_profile,
                    "native_totals":phase_totals,"serialization_seconds":serialization_duration.as_secs_f64(),
                    "extraction_seconds":extract_time.as_secs_f64(),"extraction_wall_seconds":extraction_wall,
                    "nodes":serialized.nodes.len(),"eclasses":serialized.nodes.values().map(|n|n.eclass.clone()).collect::<std::collections::HashSet<_>>().len(),
                    "use_context":eggcc_config.use_context,"extractor":"production Tiger pipeline"});
                fs::write(dir.join(format!("{prefix}.json")),serde_json::to_vec_pretty(&row).unwrap()).unwrap();
            }
''' + marker)
    helper = (root / 'experiments/eggcc_passes/schedule_profile.rs').read_text()
    patched += '\nmod pass_schedule_profile {\n' + helper + '\n}\n'
    import difflib
    (out / 'driver.patch').write_text(''.join(difflib.unified_diff(old.splitlines(True), patched.splitlines(True), fromfile='a/dag_in_context/src/lib.rs', tofile='b/dag_in_context/src/lib.rs')))
    try:
        source.write_text(patched)
        env = dict(os.environ, LIBRARY_PATH='/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/zstd/lib:/opt/homebrew/lib', LLVM_SYS_180_PREFIX='/opt/homebrew/opt/llvm@18')
        subprocess.run(['cargo','build','--offline','--release','--bin','eggcc'], cwd=repo, env=env, check=True)
        target = out / 'eggcc-schedule-profile'
        shutil.copy2(original_bin, target)
        (out/'provenance.json').write_text(json.dumps({
            'production_head':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo,text=True).strip(),
            'original_driver_sha256':hashlib.sha256(old.encode()).hexdigest(),
            'profile_driver_sha256':hashlib.sha256(patched.encode()).hexdigest(),
            'profile_binary_sha256':hashlib.sha256(target.read_bytes()).hexdigest(),
            'scope':'Only production pass driver instrumented; original native engine/dependencies, actual extraction chain. Saturate remains atomic; no per-match counters, no coverage analysis.'
        }, indent=2)+'\n')
        print(target)
    finally:
        if source.read_text() != patched:
            raise RuntimeError('Concurrent edit preserved: restore production driver manually from backup/patch')
        source.write_text(old)
        shutil.copy2(backup, original_bin)
