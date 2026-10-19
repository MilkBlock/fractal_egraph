//! Exact lockstep diagnostic; never used for performance measurements.
use cell_assisted_egglog::table_live::Runtime;
use egglog::{
    EGraph,
    ast::{Command, GenericSchedule, Schedule},
};
use serde_json::{Value, json};
use std::path::Path;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

struct Compare<'a> {
    base: &'a EGraph,
    cpu: Runtime,
    gpu: Runtime,
    open: String,
    inspect: String,
    rows: Vec<Value>,
    out: &'a Path,
}
impl Compare<'_> {
    fn step(&mut self, s: &Schedule, path: &str) -> Result<bool> {
        match s {
            GenericSchedule::Sequence(_, xs) => {
                let mut changed = false;
                for (i, x) in xs.iter().enumerate() {
                    changed |= self.step(x, &format!("{path}/{i}"))?;
                }
                Ok(changed)
            }
            GenericSchedule::Repeat(_, n, child) => {
                let mut changed = false;
                for i in 0..*n {
                    let c = self.step(child, &format!("{path}/r{i}"))?;
                    changed |= c;
                    if !c {
                        break;
                    }
                }
                Ok(changed)
            }
            GenericSchedule::Saturate(_, child)
                if !self.open.is_empty()
                    && (path.starts_with(&self.open) || self.open.starts_with(path)) =>
            {
                let mut changed = false;
                for i in 0.. {
                    let c = self.step(child, &format!("{path}/s{i}"))?;
                    changed |= c;
                    if !c {
                        break;
                    }
                }
                Ok(changed)
            }
            _ => {
                let before = (path == self.inspect)
                    .then(|| (self.cpu.engine().clone(), self.gpu.engine().clone()));
                let a = self.cpu.run_schedule(s)?;
                let b = self.gpu.run_schedule(s)?;
                let equal = (self.inspect.is_empty() || path == self.inspect)
                    .then(|| super::bench::equal(self.base, self.cpu.engine(), self.gpu.engine()));
                let row = json!({"path":path,"schedule":s.to_string(),"cpu_updated":a,"gpu_updated":b,"equal":equal.as_ref().map(|r|r.is_ok()),"error":equal.as_ref().and_then(|r|r.as_ref().err()).map(ToString::to_string),"cpu_rows":self.cpu.engine().num_tuples(),"gpu_rows":self.gpu.engine().num_tuples()});
                eprintln!(
                    "[diff] {path}: equal={:?} updated={a}/{b}",
                    equal.as_ref().map(|r| r.is_ok())
                );
                self.rows.push(row);
                if let Some((cpu, gpu)) = before {
                    self.inspect_boundary(cpu, gpu, s)?;
                }
                if equal.as_ref().is_some_and(|r| r.is_err()) || a != b {
                    self.dump()?;
                    return Err(format!("first differing boundary: {path}").into());
                }
                Ok(a)
            }
        }
    }
    fn inspect_boundary(&self, mut cpu: EGraph, mut gpu: EGraph, s: &Schedule) -> Result<()> {
        let GenericSchedule::Run(_, r) = s else {
            return Err("inspect a single Run".into());
        };
        fn table(eg: &EGraph, name: &str) -> Result<Vec<Vec<String>>> {
            let f = eg.get_function(name).ok_or("missing diagnostic table")?;
            let mut rows = vec![];
            eg.function_for_each(name, |row| {
                if !row.subsumed {
                    rows.push(
                        row.vals
                            .iter()
                            .zip(f.schema().input.iter().chain([&f.schema().output]))
                            .map(|(v, s)| format!("{:?}", eg.get_canonical_value(*v, s)))
                            .collect(),
                    );
                }
            })?;
            rows.sort();
            Ok(rows)
        }
        fn traced(eg: &mut EGraph, set: &str) -> Result<Value> {
            let trace = egglog::TraceSession::new();
            eg.step_rules_with_trace(set, &trace)?;
            let events=trace.drain_matches().into_iter().map(|e|json!({"rule":e.rule.to_string(),"bindings":e.bindings.into_iter().map(|b|(b.name.map(|n|n.to_string()).unwrap_or_else(||format!("{:?}",b.variable)),format!("{:?}",b.value))).collect::<std::collections::BTreeMap<_,_>>()})).collect::<Vec<_>>();
            Ok(json!({"events":events,"to_hoist":table(eg,"to-hoist")?}))
        }
        let before_equal = super::bench::equal(self.base, &cpu, &gpu).is_ok();
        let cpu_after = table(self.cpu.engine(), "to-hoist")?;
        let gpu_after = table(self.gpu.engine(), "to-hoist")?;
        let mut cpu_untraced = cpu.clone();
        let mut gpu_untraced = gpu.clone();
        cpu_untraced.step_rules(&r.ruleset)?;
        gpu_untraced.step_rules(&r.ruleset)?;
        let cpu_clone_reproduces = table(&cpu_untraced, "to-hoist")? == cpu_after;
        let gpu_clone_reproduces = table(&gpu_untraced, "to-hoist")? == gpu_after;
        let cpu_trace = traced(&mut cpu, &r.ruleset)?;
        let gpu_trace = traced(&mut gpu, &r.ruleset)?;
        let mut sizes = std::collections::BTreeMap::new();
        cpu.function_for_each("Expr-size", |row| {
            sizes.insert(
                format!("{:?}", row.vals[0]),
                cpu.value_to_base::<i64>(row.vals[1]),
            );
        })?;
        let projection = super::bench::equal_except(
            self.base,
            self.cpu.engine(),
            self.gpu.engine(),
            &["to-hoist"],
        );
        std::fs::write(
            self.out.join("boundary.json"),
            serde_json::to_vec_pretty(
                &json!({"schedule":s.to_string(),"before_equal":before_equal,"except_to_hoist":projection.as_ref().ok(),"except_error":projection.as_ref().err().map(ToString::to_string),"cpu_clone_reproduces":cpu_clone_reproduces,"gpu_clone_reproduces":gpu_clone_reproduces,"cpu_traced_reproduces":table(&cpu,"to-hoist")?==cpu_after,"gpu_traced_reproduces":table(&gpu,"to-hoist")?==gpu_after,"cpu_after":cpu_after,"gpu_after":gpu_after,"cpu_trace":cpu_trace,"gpu_trace":gpu_trace,"expr_sizes":sizes}),
            )?,
        )?;
        Ok(())
    }
    fn dump(&self) -> Result<()> {
        std::fs::write(
            self.out.join("diff.json"),
            serde_json::to_vec_pretty(
                &json!({"boundaries":self.rows,"cpu":self.cpu.report(),"gpu":self.gpu.report(),"open":self.open}),
            )?,
        )?;
        Ok(())
    }
}
pub fn run(
    base: EGraph,
    commands: &[Command],
    selected: &[String],
    worker: &Path,
    out: &Path,
    sequential: bool,
) -> Result<()> {
    let mut c = Compare {
        cpu: Runtime::new_hybrid(base.clone(), commands, None, false, selected)?,
        gpu: Runtime::new_hybrid(base.clone(), commands, Some(worker), false, selected)?,
        base: &base,
        open: std::env::var("PASS3_DIFF_OPEN").unwrap_or_default(),
        inspect: std::env::var("PASS3_DIFF_INSPECT").unwrap_or_default(),
        rows: vec![],
        out,
    };
    if sequential {
        c.cpu.run(commands)?;
        c.gpu.run(commands)?;
    } else {
        for (i, cmd) in commands.iter().enumerate() {
            let Command::RunSchedule(s) = cmd else {
                return Err("expected schedule".into());
            };
            c.step(s, &format!("/{i}"))?;
        }
    }
    let final_equal = super::bench::equal(&base, c.cpu.engine(), c.gpu.engine());
    let projection =
        super::bench::equal_except(&base, c.cpu.engine(), c.gpu.engine(), &["to-hoist"]);
    let secondary = super::bench::equal_except(
        &base,
        c.cpu.engine(),
        c.gpu.engine(),
        &["to-hoist", "Smaller"],
    );
    c.rows.push(json!({"path":"final","sequential":sequential,"equal":final_equal.is_ok(),"error":final_equal.as_ref().err().map(ToString::to_string),"except_to_hoist":projection.as_ref().ok(),"except_error":projection.as_ref().err().map(ToString::to_string),"except_to_hoist_and_smaller":secondary.as_ref().ok(),"except_two_error":secondary.as_ref().err().map(ToString::to_string)}));
    c.dump()?;
    final_equal.map(|_| ())
}
