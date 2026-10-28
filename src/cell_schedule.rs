//! Original schedule control, shared by Cell evidence and recipe consumers.
//! Repeat's stop flag is distinct from Saturate's fixed-point test.
use crate::pipeline::Result;
use egglog::{
    CommandOutput, EGraph, TraceSession,
    ast::{Command, Schedule},
};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub updated: bool,
    pub can_stop: bool,
}
impl Default for Progress {
    fn default() -> Self {
        Self::empty()
    }
}
impl Progress {
    fn empty() -> Self {
        Self {
            updated: false,
            can_stop: true,
        }
    }
    fn include(&mut self, other: Self) {
        self.updated |= other.updated;
        self.can_stop &= other.can_stop;
    }
}

pub fn encode(s: &Schedule) -> Value {
    match s {
        Schedule::Run(_, r) => json!({"op":"run","ruleset":r.ruleset,
            "until":r.until.as_ref().map(|fs|fs.iter().map(ToString::to_string).collect::<Vec<_>>())}),
        Schedule::Repeat(_, n, b) => json!({"op":"repeat","count":n,"body":encode(b)}),
        Schedule::Saturate(_, b) => json!({"op":"saturate","body":encode(b)}),
        Schedule::Sequence(_, bs) => {
            json!({"op":"sequence","body":bs.iter().map(encode).collect::<Vec<_>>()})
        }
    }
}
pub fn source_program(source: &str) -> Result<Value> {
    let mut eg = EGraph::default();
    let commands = eg.parse_program(None, source)?;
    let fingerprint = source.bytes().fold(0xcbf29ce484222325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
    });
    Ok(
        json!({"schema":"cell-schedule/v1","source_fingerprint":{"bytes":source.len(),"fnv1a64":format!("{fingerprint:016x}")},"commands":commands.iter().enumerate().filter_map(|(i,c)|
        if let Command::RunSchedule(s)=c {Some(json!({"command":i,"program":encode(s)}))}else{None}).collect::<Vec<_>>(),
        "repeat_stop":"can_stop","saturate_stop":"not updated"}),
    )
}

/// Leaf execution stays native, including until, seminaive and RHS/rebuild.
/// Begin permits interface maintenance; end is after native commit/rebuild.
pub fn execute(
    eg: &mut EGraph,
    trace: &TraceSession,
    s: &Schedule,
    command: usize,
    boundary: &mut impl FnMut(&mut EGraph, Value) -> Result<()>,
) -> Result<Progress> {
    fn walk(
        eg: &mut EGraph,
        trace: &TraceSession,
        s: &Schedule,
        path: &mut Vec<usize>,
        iterations: &mut Vec<usize>,
        boundary: &mut impl FnMut(&mut EGraph, Value) -> Result<()>,
    ) -> Result<Progress> {
        match s {
            Schedule::Run(_, r) => {
                boundary(
                    eg,
                    json!({"phase":"begin","path":path,"iterations":iterations,"ruleset":r.ruleset}),
                )?;
                let before = eg.num_tuples();
                let output =
                    eg.run_program_with_trace(vec![Command::RunSchedule(s.clone())], trace)?;
                let report = output
                    .into_iter()
                    .find_map(|o| {
                        if let CommandOutput::RunSchedule(r) = o {
                            Some(r)
                        } else {
                            None
                        }
                    })
                    .ok_or("native Run returned no progress report")?;
                let p = Progress {
                    updated: report.updated,
                    can_stop: report.can_stop,
                };
                boundary(
                    eg,
                    json!({"phase":"end","path":path,"iterations":iterations,"ruleset":r.ruleset,
                    "updated":p.updated,"can_stop":p.can_stop,"executed":!report.iterations.is_empty(),"logical_matches":report.num_matches_per_rule.values().sum::<usize>(),"before_rows":before,"after_rows":eg.num_tuples()}),
                )?;
                Ok(p)
            }
            Schedule::Sequence(_, bs) => {
                let mut p = Progress::empty();
                for (i, b) in bs.iter().enumerate() {
                    path.push(i);
                    p.include(walk(eg, trace, b, path, iterations, boundary)?);
                    path.pop();
                }
                Ok(p)
            }
            Schedule::Repeat(_, n, b) => {
                let mut p = Progress::empty();
                path.push(0);
                for i in 0..*n {
                    iterations.push(i);
                    let q = walk(eg, trace, b, path, iterations, boundary)?;
                    iterations.pop();
                    p.include(q);
                    if q.can_stop {
                        break;
                    }
                }
                path.pop();
                Ok(p)
            }
            Schedule::Saturate(_, b) => {
                let mut p = Progress::empty();
                let mut i = 0;
                path.push(0);
                loop {
                    iterations.push(i);
                    let q = walk(eg, trace, b, path, iterations, boundary)?;
                    iterations.pop();
                    p.include(q);
                    if !q.updated {
                        break;
                    }
                    i += 1;
                }
                path.pop();
                Ok(p)
            }
        }
    }
    walk(eg, trace, s, &mut vec![command], &mut vec![], boundary)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_finite_and_saturated_control_matches_native() -> Result<()> {
        let source = r#"(datatype X (N i64)) (ruleset a) (ruleset b)
        (rule ((N n) (< n 9)) ((N (+ n 1))) :ruleset a)
        (relation P (X)) (rule ((= x (N n))) ((P x)) :ruleset b)
        (N 0) (run-schedule (repeat 5 (run a) (saturate (run b))))
        (check (N 5)) (fail (check (N 6)))"#;
        let mut oracle = EGraph::default();
        oracle.parse_and_run_program(None, source)?;
        let mut actual = EGraph::default();
        let commands = actual.parse_program(None, source)?;
        let trace = TraceSession::with_effects();
        let mut events = vec![];
        for (i, c) in commands.into_iter().enumerate() {
            if let Command::RunSchedule(s) = c {
                execute(&mut actual, &trace, &s, i, &mut |_, e| {
                    if e["phase"] == "end" {
                        events.push(e);
                    }
                    Ok(())
                })?;
            } else {
                actual.run_program(vec![c])?;
            }
        }
        assert_eq!(
            actual.device_snapshot().unwrap(),
            oracle.device_snapshot().unwrap()
        );
        assert_eq!(events.iter().filter(|e| e["ruleset"] == "a").count(), 5);
        assert!(
            events
                .iter()
                .any(|e| e["iterations"].as_array().unwrap().len() == 2)
        );
        Ok(())
    }
    #[test]
    fn until_and_zero_repeat_keep_native_stop_semantics() -> Result<()> {
        let source = "(datatype X (N i64)) (ruleset a) (rule ((N n)) ((N (+ n 1))) :ruleset a) (N 0) (run-schedule (repeat 0 (run a)) (repeat 5 (run a :until (N 2))))";
        let mut eg = EGraph::default();
        let cs = eg.parse_program(None, source)?;
        let trace = TraceSession::default();
        let mut events = vec![];
        for (i, c) in cs.into_iter().enumerate() {
            if let Command::RunSchedule(s) = c {
                execute(&mut eg, &trace, &s, i, &mut |_, e| {
                    if e["phase"] == "end" {
                        events.push(e);
                    }
                    Ok(())
                })?;
            } else {
                eg.run_program(vec![c])?;
            }
        }
        assert_eq!(events.len(), 3);
        assert_eq!(events[2]["can_stop"], true);
        eg.parse_and_run_program(None, "(check (N 2)) (fail (check (N 3)))")?;
        Ok(())
    }
}
