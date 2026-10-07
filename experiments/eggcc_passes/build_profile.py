"""Build an optional per-pass timing probe; restore all input source files afterwards.
Uses the already cached production dependency, never substitutes egg_layout's kernel.
"""
import pathlib, subprocess, shutil, difflib, json
root=pathlib.Path(__file__).resolve().parents[2]
repo=root.parent/'check_statewalk'
core=pathlib.Path.home()/'.cargo/git/checkouts/egg-smol-65fdca4a5529f39b/ebba7bb'
files={}
written={}
backup=root/'out/eggcc-pass-audit/eggcc-original'
backup.parent.mkdir(parents=True,exist_ok=True)
shutil.copy2(repo/'target/release/eggcc',backup)
def change(p,fn):
    old=p.read_text();files[p]=old;written[p]=fn(old);p.write_text(written[p])
def replace(s,a,b):
    assert a in s,a
    return s.replace(a,b,1)
try:
    change(core/'core-relations/src/lib.rs',lambda s:s+'''
// Measurement-only instrumentation, restored after the profiler build.
pub mod pass_profile {
    use std::sync::atomic::{AtomicBool,AtomicU64,Ordering};
    static ACTIVE:AtomicBool=AtomicBool::new(false);
    static RHS:AtomicU64=AtomicU64::new(0);
    pub struct Scope(bool);
    pub fn scope(active:bool)->Scope {Scope(ACTIVE.swap(active,Ordering::Relaxed))}
    impl Drop for Scope {fn drop(&mut self){ACTIVE.store(self.0,Ordering::Relaxed);}}
    pub fn record(start:std::time::Instant){if ACTIVE.load(Ordering::Relaxed){RHS.fetch_add(start.elapsed().as_nanos() as u64,Ordering::Relaxed);}}
    #[unsafe(no_mangle)] pub extern "C" fn eggcc_profile_rhs_ns()->u64 {RHS.load(Ordering::Relaxed)}
}
''')
    def execution(s):
        import re
        s,n=re.subn(r'(\s*)(let succeeded = [^\n]*\.run_instrs\([^\n]*;)',r'\1let rhs_tick = std::time::Instant::now();\1\2\1crate::pass_profile::record(rhs_tick);',s)
        assert n==3,n
        marker='let merge_timer = Instant::now();';assert marker in s
        s=s.replace(marker,'let _merge_profile = crate::pass_profile::scope(false);\n        '+marker,1)
        return s
    change(core/'core-relations/src/free_join/execute.rs',execution)
    def bridge(s):
        marker='let rule_set_report =\n            run_rules_impl(&mut self.db, &mut self.rules, rules, ts, self.report_level)?;'
        return replace(s,marker,'let _profile = core_relations::pass_profile::scope(true);\n        '+marker+'\n        drop(_profile);')
    change(core/'egglog-bridge/src/lib.rs',bridge)
    def driver(s):
        s=replace(s,'            let mut egraph = new_single_threaded_egraph();\n            egraph.parse_and_run_program(None, &egglog_prog)?;', '''            unsafe extern "C" { fn eggcc_profile_rhs_ns() -> u64; }
            let rhs_before = unsafe { eggcc_profile_rhs_ns() };
            let execution_start = Instant::now();
            let mut egraph = new_single_threaded_egraph();
            egraph.parse_and_run_program(None, &egglog_prog)?;
            let execution_seconds = execution_start.elapsed().as_secs_f64();
            let rhs_seconds = (unsafe { eggcc_profile_rhs_ns() } - rhs_before) as f64 / 1e9;
            let report = egraph.get_overall_run_report();
            let sum = |values: Vec<Duration>| values.iter().map(Duration::as_secs_f64).sum::<f64>();
            let search_apply = sum(report.search_and_apply_time_per_ruleset.values().copied().collect());
            let merge = sum(report.merge_time_per_ruleset.values().copied().collect());
            let rebuild = sum(report.rebuild_time_per_ruleset.values().copied().collect());
            let matches: usize = report.num_matches_per_rule.values().sum();
''')
        s=replace(s,'            let (iter_result, region_timings, extract_time) =','            let extraction_wall_start = Instant::now();\n            let (iter_result, region_timings, extract_time) =')
        marker='            eggcc_extraction_time += extract_time;'
        s=replace(s,marker,'''            let extraction_wall = extraction_wall_start.elapsed().as_secs_f64();
            if let Ok(dir) = std::env::var("EGGCC_PASS_PROFILE_DIR") {
                let dir = PathBuf::from(dir); fs::create_dir_all(&dir).unwrap();
                let prefix = format!("pass-{}", i+1);
                fs::write(dir.join(format!("{prefix}.egg")), &egglog_prog).unwrap();
                let row=serde_json::json!({"pass":i+1,"batch":batch,"schedule":schedule.egglog_schedule(),
                    "execution_seconds":execution_seconds,"search_and_apply_seconds":search_apply,
                    "rhs_seconds":rhs_seconds,"search_and_binding_seconds":search_apply-rhs_seconds,
                    "insert_merge_seconds":merge,"rebuild_seconds":rebuild,"matches":matches,
                    "serialization_seconds":serialization_duration.as_secs_f64(),
                    "extraction_seconds":extract_time.as_secs_f64(),"extraction_wall_seconds":extraction_wall,
                    "nodes":serialized.nodes.len(),"eclasses":serialized.nodes.values().map(|n|n.eclass.clone()).collect::<std::collections::HashSet<_>>().len(),
                    "use_context":eggcc_config.use_context,"extractor":"production Tiger pipeline"});
                fs::write(dir.join(format!("{prefix}.json")),serde_json::to_vec_pretty(&row).unwrap()).unwrap();
            }
'''+marker)
        return s
    change(repo/'dag_in_context/src/lib.rs',driver)
    patches=[]
    for p,old in files.items():
        name=('eggcc/'+str(p.relative_to(repo))) if p.is_relative_to(repo) else ('engine/'+str(p.relative_to(core)))
        patches.extend(difflib.unified_diff(old.splitlines(True),p.read_text().splitlines(True),fromfile='a/'+name,tofile='b/'+name))
    (root/'experiments/eggcc_passes/instrumentation.patch').write_text(''.join(patches))
    import os
    env=dict(os.environ,LIBRARY_PATH='/opt/homebrew/opt/cbc/lib:/opt/homebrew/opt/zstd/lib:/opt/homebrew/lib',LLVM_SYS_180_PREFIX='/opt/homebrew/opt/llvm@18')
    subprocess.run(['cargo','clean','--release','-p','egglog-core-relations','-p','egglog-bridge'],cwd=repo,check=True)
    subprocess.run(['cargo','build','--offline','--release','--bin','eggcc'],cwd=repo,env=env,check=True)
    target=root/'out/eggcc-pass-audit/eggcc-profile';shutil.copy2(repo/'target/release/eggcc',target)
    print(target)
finally:
    for p,old in files.items():
        if p.read_text()!=written[p]:
            print(f'Concurrent edit preserved; restore manually: {p}')
        else:p.write_text(old)
    shutil.copy2(backup,repo/'target/release/eggcc')
    subprocess.run(['cargo','clean','--release','-p','egglog-core-relations','-p','egglog-bridge'],cwd=repo,check=True)
