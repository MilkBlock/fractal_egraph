// Included by the measurement-only production driver and local regression tests.
// Keep Saturate atomic: nested schedules are measured exactly once. Only expose
// Sequence/Repeat, preserving each scheduler's distinct Repeat semantics.
use egglog::{
    CommandOutput, EGraph,
    ast::{Command, Expr, GenericSchedule, Literal, ParseError, Schedule},
};
use serde_json::{Value, json};
use std::time::Instant;

fn schedule(
    eg: &mut EGraph,
    s: &Schedule,
    path: &str,
    packets: &mut Vec<Value>,
) -> Result<bool, egglog::Error> {
    match s {
        GenericSchedule::Sequence(_, children) => {
            let mut stop = true;
            for (i, child) in children.iter().enumerate() {
                stop &= schedule(eg, child, &format!("{path}/{i}"), packets)?;
            }
            Ok(stop)
        }
        GenericSchedule::Repeat(_, limit, body) => {
            let mut stop = true;
            for i in 0..*limit {
                let current = schedule(eg, body, &format!("{path}/repeat{i}"), packets)?;
                stop &= current;
                if current {
                    break;
                }
            }
            Ok(stop)
        }
        _ => {
            let kind = if matches!(s, GenericSchedule::Saturate(..)) {
                "saturate"
            } else {
                "run"
            };
            atomic(
                eg,
                Command::RunSchedule(s.clone()),
                kind,
                s.to_string(),
                path,
                packets,
            )
        }
    }
}

fn atomic(
    eg: &mut EGraph,
    command: Command,
    kind: &str,
    label: String,
    path: &str,
    packets: &mut Vec<Value>,
) -> Result<bool, egglog::Error> {
    let commands = vec![command];
    let start = Instant::now();
    let outputs = eg.run_program(commands)?;
    let seconds = start.elapsed().as_secs_f64();
    let (mut stop, mut search, mut merge, mut rebuild, mut matches) = (true, 0.0, 0.0, 0.0, 0usize);
    for out in outputs {
        if let CommandOutput::RunSchedule(r) = out {
            stop &= r.can_stop;
            search += r
                .search_and_apply_time_per_ruleset
                .values()
                .map(|d| d.as_secs_f64())
                .sum::<f64>();
            merge += r
                .merge_time_per_ruleset
                .values()
                .map(|d| d.as_secs_f64())
                .sum::<f64>();
            rebuild += r
                .rebuild_time_per_ruleset
                .values()
                .map(|d| d.as_secs_f64())
                .sum::<f64>();
            matches += r.num_matches_per_rule.values().sum::<usize>();
        }
    }
    packets.push(json!({"path":path,"kind":kind,"schedule":label,"wall_seconds":seconds,
                "search_and_apply_seconds":search,"insert_merge_seconds":merge,"rebuild_seconds":rebuild,
                "logical_matches":matches,"can_stop":stop}));
    Ok(stop)
}

// Production eggcc registers run-schedule as a UserDefined command. Preserve
// its own executor for every atomic block, particularly nested Saturate. Its
// Repeat deliberately runs all iterations (unlike core Schedule::Repeat).
fn validate_extended(e: &Expr) -> Result<(), egglog::Error> {
    match e {
        Expr::Var(..) => Ok(()),
        Expr::Call(_, head, _) if head == "run" => Ok(()),
        Expr::Call(_, head, children) if head == "seq" || head == "saturate" => {
            for child in children {
                validate_extended(child)?;
            }
            Ok(())
        }
        Expr::Call(_, head, children) if head == "repeat" => match children.as_slice() {
            [Expr::Lit(_, Literal::Int(_)), rest @ ..] => {
                for child in rest {
                    validate_extended(child)?;
                }
                Ok(())
            }
            _ => Err(egglog::Error::ParseError(ParseError(
                e.span(),
                "Invalid profiled repeat".into(),
            ))),
        },
        _ => Err(egglog::Error::ParseError(ParseError(
            e.span(),
            "Profiler cannot split custom scheduler scopes".into(),
        ))),
    }
}

fn extended(
    eg: &mut EGraph,
    e: &Expr,
    path: &str,
    packets: &mut Vec<Value>,
) -> Result<(), egglog::Error> {
    match e {
        Expr::Call(_, head, children) if head == "seq" => {
            for (i, child) in children.iter().enumerate() {
                extended(eg, child, &format!("{path}/{i}"), packets)?;
            }
        }
        Expr::Call(_, head, children) if head == "repeat" => {
            let Expr::Lit(_, Literal::Int(n)) = &children[0] else {
                unreachable!()
            };
            for j in 0..*n {
                for (i, child) in children[1..].iter().enumerate() {
                    extended(eg, child, &format!("{path}/repeat{j}/{i}"), packets)?;
                }
            }
        }
        _ => {
            let kind = if matches!(e, Expr::Call(_, head, _) if head == "saturate") {
                "saturate"
            } else {
                "run"
            };
            atomic(
                eg,
                Command::UserDefined(e.span(), "run-schedule".into(), vec![e.clone()]),
                kind,
                e.to_string(),
                path,
                packets,
            )?;
        }
    }
    Ok(())
}

pub fn execute(eg: &mut EGraph, source: &str) -> Result<Value, egglog::Error> {
    let parse_start = Instant::now();
    let commands = eg.parse_program(None, source)?;
    let parse_seconds = parse_start.elapsed().as_secs_f64();
    let (mut pending, mut packets, mut setup_seconds) = (vec![], vec![], 0.0);
    for (i, command) in commands.into_iter().enumerate() {
        match command {
            Command::RunSchedule(s) => {
                let start = Instant::now();
                eg.run_program(std::mem::take(&mut pending))?;
                setup_seconds += start.elapsed().as_secs_f64();
                schedule(eg, &s, &format!("command{i}"), &mut packets)?;
            }
            Command::UserDefined(_, name, args) if name == "run-schedule" => {
                for arg in &args {
                    validate_extended(arg)?;
                }
                let start = Instant::now();
                eg.run_program(std::mem::take(&mut pending))?;
                setup_seconds += start.elapsed().as_secs_f64();
                for (j, arg) in args.iter().enumerate() {
                    extended(eg, arg, &format!("command{i}/{j}"), &mut packets)?;
                }
            }
            other => pending.push(other),
        }
    }
    let start = Instant::now();
    eg.run_program(pending)?;
    setup_seconds += start.elapsed().as_secs_f64();
    let seconds = |kind: &str| {
        packets
            .iter()
            .filter(|p| p["kind"] == kind)
            .map(|p| p["wall_seconds"].as_f64().unwrap())
            .sum::<f64>()
    };
    Ok(
        json!({"saturate_seconds":seconds("saturate"),"run_seconds":seconds("run"),
        "parse_seconds":parse_seconds,"non_schedule_seconds":setup_seconds,"packets":packets}),
    )
}

pub fn verify_production(factory: fn() -> EGraph) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        for spec in [
            "(repeat 5 r s)",
            "(repeat 5 (seq r (saturate s)))",
            "(repeat 3 (repeat 2 r s))",
            "(saturate r (saturate s))",
            "(repeat 5 (run r :until (Q 2)) s)",
        ] {
            let source = format!(
                "(relation P (i64)) (relation Q (i64)) (relation R (i64)) (ruleset r) (ruleset s) \
                (rule ((P x) (< x 3)) ((P (+ x 1)) (Q x)) :ruleset r) \
                (rule ((Q x)) ((R x)) :ruleset s) (P 0) (run-schedule {spec})"
            );
            let mut native = factory();
            native.parse_and_run_program(None, &source).unwrap();
            let mut measured = factory();
            let profile = execute(&mut measured, &source).unwrap();
            assert_eq!(
                native.get_overall_run_report().num_matches_per_rule,
                measured.get_overall_run_report().num_matches_per_rule
            );
            for table in ["P", "Q", "R"] {
                assert_eq!(native.get_size(table), measured.get_size(table));
                for i in 0..5 {
                    let check = format!("(check ({table} {i}))");
                    assert_eq!(
                        native.clone().parse_and_run_program(None, &check).is_ok(),
                        measured.clone().parse_and_run_program(None, &check).is_ok()
                    );
                }
            }
            let packets = profile["packets"].as_array().unwrap();
            if spec == "(repeat 5 r s)" {
                assert_eq!(packets.len(), 10);
            }
            if spec == "(saturate r (saturate s))" {
                assert_eq!(packets.len(), 1);
                assert_eq!(packets[0]["kind"], "saturate");
            }
        }
        eprintln!("SCHEDULE_PROFILE_CHECK: 5 production extended-scheduler cases passed");
    });
}
