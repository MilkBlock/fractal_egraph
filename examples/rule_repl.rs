//! Interactive inspection of original schedule boundaries, without rule tracing.
//! Usage: rule_repl SOURCE.egg OUTPUT_DIRECTORY [--control]
//! Repeat/Sequence are unpacked with native early-stop semantics preserved;
//! Saturate and Run remain atomic native commands. --control runs the intact AST.
use egg_layout::pipeline::Result;
use egglog::{
    CommandOutput, EGraph,
    ast::{Command, GenericSchedule, Schedule},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{self, BufRead, Write},
    path::PathBuf,
};

#[path = "support/ruleset_bench.rs"]
mod ruleset_bench;

struct Packet {
    label: String,
    commands: Vec<Command>,
}

struct RepeatEnd {
    start: usize,
    end: usize,
    exit: usize,
}

fn schedule(s: Schedule, path: String, packets: &mut Vec<Packet>, repeats: &mut Vec<RepeatEnd>) {
    match s {
        GenericSchedule::Sequence(_, children) => {
            for (i, child) in children.into_iter().enumerate() {
                schedule(child, format!("{path}/{i}"), packets, repeats);
            }
        }
        GenericSchedule::Repeat(_, n, child) => {
            let mut iterations = Vec::new();
            for i in 0..n {
                let start = packets.len();
                schedule(
                    *child.clone(),
                    format!("{path}/repeat{i}"),
                    packets,
                    repeats,
                );
                iterations.push((start, packets.len()));
            }
            let exit = packets.len();
            repeats.extend(iterations.into_iter().map(|(start, end)| RepeatEnd {
                start,
                end,
                exit,
            }));
        }
        atomic => packets.push(Packet {
            label: format!("{path}: {atomic}"),
            commands: vec![Command::RunSchedule(atomic)],
        }),
    }
}

fn counts(eg: &EGraph) -> BTreeMap<String, (usize, f64)> {
    let report = eg.get_overall_run_report();
    report
        .search_and_apply_time_per_rule
        .iter()
        .map(|(name, time)| {
            (
                name.to_string(),
                (
                    report.num_matches_per_rule.get(name).copied().unwrap_or(0),
                    time.as_secs_f64(),
                ),
            )
        })
        .collect()
}

fn rows(
    values: &BTreeMap<String, (usize, f64)>,
    before: &BTreeMap<String, (usize, f64)>,
) -> Vec<Value> {
    let mut result: Vec<_> = values
        .iter()
        .filter_map(|(rule, (matches, seconds))| {
            let (old_matches, old_seconds) = before.get(rule).copied().unwrap_or_default();
            let count = matches - old_matches;
            let elapsed = seconds - old_seconds;
            (count > 0 || elapsed > 0.0)
                .then(|| json!({"rule":rule,"matches":count,"search_and_apply_seconds":elapsed}))
        })
        .collect();
    result.sort_by(|a, b| {
        b["search_and_apply_seconds"]
            .as_f64()
            .unwrap()
            .total_cmp(&a["search_and_apply_seconds"].as_f64().unwrap())
    });
    result
}

fn table_sizes(eg: &EGraph) -> Value {
    let names = [
        "Expr-size",
        "Get",
        "SubTuple",
        "Concat",
        "TCPair",
        "Smaller",
        "ExtractedExpr",
        "PotentialExtractedExpr",
        "lsr-inv",
        "BodyContainsExpr",
    ];
    json!(
        names
            .into_iter()
            .filter(|name| eg.get_function(name).is_some())
            .map(|name| (name, eg.get_size(name)))
            .collect::<BTreeMap<_, _>>()
    )
}

fn execute(eg: &mut EGraph, packet: &Packet, index: usize, journal: &mut Vec<Value>) -> Result<()> {
    let before = counts(eg);
    let tuple_before = eg.num_tuples();
    let probes_before: BTreeMap<_, _> = egglog::probe_counts().into_iter().collect();
    let messages = eg.run_program(packet.commands.clone())?;
    let can_stop = messages
        .iter()
        .filter_map(|m| {
            if let CommandOutput::RunSchedule(r) = m {
                Some(r.can_stop)
            } else {
                None
            }
        })
        .all(|b| b);
    let per_rule = rows(&counts(eg), &before);
    let matches: usize = per_rule
        .iter()
        .map(|r| r["matches"].as_u64().unwrap() as usize)
        .sum();
    let seconds: f64 = per_rule
        .iter()
        .map(|r| r["search_and_apply_seconds"].as_f64().unwrap())
        .sum();
    let probes: BTreeMap<_, _> = egglog::probe_counts()
        .into_iter()
        .map(|(name, value)| (name, value - probes_before.get(name).copied().unwrap_or(0)))
        .collect();
    journal.push(json!({"packet":index,"label":packet.label,"logical_matches":matches,
        "rule_search_and_apply_seconds":seconds,"table_rows_before":tuple_before,"table_rows_after":eg.num_tuples(),
        "rules":per_rule,"probe_counts":probes,"can_stop":can_stop}));
    Ok(())
}

fn advance(mut cursor: usize, repeats: &[RepeatEnd], journal: &[Value]) -> usize {
    loop {
        let next = repeats
            .iter()
            .filter(|r| r.end == cursor && r.exit > cursor)
            .filter(|r| {
                journal
                    .iter()
                    .filter(|row| {
                        let p = row["packet"].as_u64().unwrap() as usize;
                        p >= r.start && p < r.end
                    })
                    .all(|row| row["can_stop"] == true)
            })
            .map(|r| r.exit)
            .max()
            .unwrap_or(cursor);
        if next == cursor {
            return cursor;
        }
        cursor = next;
    }
}

fn stats(eg: &EGraph, cursor: usize, packets: &[Packet], journal: &[Value]) -> Value {
    let report = eg.get_overall_run_report();
    json!({"cursor":cursor,"packet_count":packets.len(),"next":packets.get(cursor).map(|p|&p.label),
        "logical_matches":report.num_matches_per_rule.values().sum::<usize>(),"table_rows":eg.num_tuples(),
        "rules":rows(&counts(eg),&BTreeMap::new()),"tables":table_sizes(eg),"packets":journal,
        "rulesets":report.search_and_apply_time_per_ruleset.iter().map(|(name,time)|(name.to_string(),time.as_secs_f64())).collect::<BTreeMap<_,_>>(),
        "scope":"Replay of supplied real pass source on local native engine. Logical matches are not committed mutations. Search includes apply; global probe counters are not per-rule attribution. Saturate nodes are not split."})
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if !(args.len() == 3 || args.len() == 4 && args[3] == "--control") {
        return Err("usage: rule_repl SOURCE.egg OUTPUT_DIRECTORY [--control]".into());
    }
    let source = PathBuf::from(&args[1]).canonicalize()?;
    let output = PathBuf::from(&args[2]);
    std::fs::create_dir_all(&output)?;
    let text = std::fs::read_to_string(&source)?;
    EGraph::set_num_threads(1);
    if std::env::var_os("EGGLOG_PROBE_COUNTS_OFF").is_some() {
        egglog::set_probe_counts_enabled(false);
    }
    let mut eg = EGraph::default();
    eg.set_report_level(Default::default());
    let commands = eg.parse_program(Some(source.display().to_string()), &text)?;
    if args.len() == 4 {
        let packets = vec![Packet {
            label: "intact native command stream".into(),
            commands,
        }];
        let mut journal = Vec::new();
        execute(&mut eg, &packets[0], 0, &mut journal)?;
        std::fs::write(
            output.join("final.json"),
            serde_json::to_vec_pretty(&stats(&eg, 1, &packets, &journal))?,
        )?;
        println!(
            "intact control completed: matches={}, rows={}",
            eg.get_overall_run_report()
                .num_matches_per_rule
                .values()
                .sum::<usize>(),
            eg.num_tuples()
        );
        return Ok(());
    }
    let mut packets = Vec::new();
    let mut repeats = Vec::new();
    let mut setup = Vec::new();
    for (i, command) in commands.into_iter().enumerate() {
        if let Command::RunSchedule(s) = command {
            if !setup.is_empty() {
                packets.push(Packet {
                    label: format!("setup-before-command{i}"),
                    commands: std::mem::take(&mut setup),
                });
            }
            schedule(s, format!("command{i}"), &mut packets, &mut repeats);
        } else {
            setup.push(command);
        }
    }
    if !setup.is_empty() {
        packets.push(Packet {
            label: "final-commands".into(),
            commands: setup,
        });
    }
    let plan: Vec<_> = packets
        .iter()
        .enumerate()
        .map(|(i, p)| json!({"packet":i,"label":p.label}))
        .collect();
    std::fs::write(
        output.join("schedule.json"),
        serde_json::to_vec_pretty(&plan)?,
    )?;
    println!(
        "{} schedule packets. :next [n], :until TEXT, :bench NAME, :finish, :stats NAME, :last, :tables, :quit. Native one-line .egg commands are accepted.",
        packets.len()
    );
    let mut cursor = 0;
    let mut journal = Vec::new();
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();
    loop {
        print!("paused[{cursor}]> ");
        io::stdout().flush()?;
        line.clear();
        if input.read_line(&mut line)? == 0 {
            break;
        }
        let command = line.trim();
        if command == ":quit" {
            break;
        }
        if command.starts_with(":next") || command == ":finish" || command.starts_with(":until ") {
            let stop = if let Some(filter) = command.strip_prefix(":until ") {
                let Some(found) =
                    (cursor..packets.len()).find(|i| packets[*i].label.contains(filter))
                else {
                    println!("no later packet matches filter; state unchanged");
                    continue;
                };
                found
            } else if command == ":finish" {
                packets.len()
            } else {
                (cursor
                    + command
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("1")
                        .parse::<usize>()?)
                .min(packets.len())
            };
            while cursor < stop {
                execute(&mut eg, &packets[cursor], cursor, &mut journal)?;
                cursor += 1;
                cursor = advance(cursor, &repeats, &journal);
                if cursor % 20 == 0 || cursor == stop {
                    println!(
                        "completed packet {}, cumulative matches={}, rows={}",
                        cursor - 1,
                        eg.get_overall_run_report()
                            .num_matches_per_rule
                            .values()
                            .sum::<usize>(),
                        eg.num_tuples()
                    );
                }
            }
            println!(
                "next: {}",
                packets
                    .get(cursor)
                    .map(|p| p.label.as_str())
                    .unwrap_or("end")
            );
        } else if let Some(name) = command.strip_prefix(":bench ") {
            let packet = packets.get(cursor).ok_or("no next schedule packet")?;
            let result = ruleset_bench::benchmark(&eg, &packet.commands, &packet.label)?;
            let path = output.join(format!("{name}.json"));
            std::fs::write(&path, serde_json::to_vec_pretty(&result)?)?;
            println!(
                "bench saved {}; checkpoint remains unchanged",
                path.display()
            );
        } else if let Some(name) = command.strip_prefix(":stats ") {
            let path = output.join(format!("{name}.json"));
            std::fs::write(
                &path,
                serde_json::to_vec_pretty(&stats(&eg, cursor, &packets, &journal))?,
            )?;
            println!("saved {}", path.display());
        } else if command == ":last" {
            println!("{}", serde_json::to_string_pretty(&journal.last())?);
        } else if command == ":tables" {
            println!("{}", table_sizes(&eg));
        } else if command.starts_with('(') {
            for message in eg.parse_and_run_program(None, command)? {
                println!("{message}");
            }
        } else {
            println!("unknown command");
        }
    }
    std::fs::write(
        output.join("final.json"),
        serde_json::to_vec_pretty(&stats(&eg, cursor, &packets, &journal))?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_repeats_preserve_native_early_stopping_and_nested_reports() {
        EGraph::set_num_threads(1);
        for tail in [
            "(repeat 9 grow)",
            "(repeat 4 (repeat 8 grow) empty)",
            "(repeat 3 (saturate grow) empty)",
            "(repeat 0 grow)",
        ] {
            let setup = "(datatype E (Num i64)) (Num 0) (ruleset grow) (ruleset empty) (rule ((Num n) (< n 3)) ((Num (+ n 1))) :ruleset grow)";
            let text = format!("(run-schedule {tail})");
            let mut control = EGraph::default();
            control.parse_and_run_program(None, setup).unwrap();
            control.parse_and_run_program(None, &text).unwrap();
            let mut split = EGraph::default();
            split.parse_and_run_program(None, setup).unwrap();
            let mut packets = Vec::new();
            let mut repeats = Vec::new();
            for cmd in split.parse_program(None, &text).unwrap() {
                let Command::RunSchedule(s) = cmd else {
                    panic!("schedule expected")
                };
                schedule(s, "test".into(), &mut packets, &mut repeats);
            }
            let mut cursor = 0;
            let mut journal = Vec::new();
            while cursor < packets.len() {
                execute(&mut split, &packets[cursor], cursor, &mut journal).unwrap();
                cursor = advance(cursor + 1, &repeats, &journal);
            }
            assert_eq!(control.get_size("Num"), split.get_size("Num"), "{tail}");
            assert_eq!(
                control.get_overall_run_report().num_matches_per_rule,
                split.get_overall_run_report().num_matches_per_rule,
                "{tail}"
            );
            assert_eq!(
                control.get_overall_run_report().iterations.len(),
                split.get_overall_run_report().iterations.len(),
                "{tail}"
            );
        }
    }
}
