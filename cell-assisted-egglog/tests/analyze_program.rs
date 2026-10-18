use cell_assisted_egglog::{analyze_program, program::*};
use egglog::{EGraph, ast::Command};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};

fn fixture() -> Json {
    serde_json::from_str(include_str!("data/analyze_ports.json")).unwrap()
}
fn compile(f: &Json) -> cell_assisted_egglog::Result<Program> {
    let states = f["states"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.parse().unwrap(), v.clone()))
        .collect();
    analyze_program::import(&f["ports"], &f["history"], &states)
}
#[test]
fn witnessed_connections_compile_and_do_not_enumerate_all_pairs() {
    let p = compile(&fixture()).unwrap();
    assert_eq!(p.channels.len(), 2);
    assert_eq!(p.stages.len(), 1);
    assert_eq!(p.cells.iter().filter(|c| c.binding.is_some()).count(), 3);
    let r = p.check().unwrap().execute();
    assert_eq!(r.epochs[0][0].decisions, vec![1, 1]);
    assert_eq!(r.epochs[0][0].target_status, vec![1, 1]);
}
#[test]
fn altered_evidence_or_missing_residuals_cannot_be_silently_accepted() {
    for corrupt in 0..8 {
        let mut f = fixture();
        match corrupt {
            0 => {
                f["ports"]["families"][0]["source_body_maps"][0]["checked_visible_rows"] =
                    json!([0]);
            }
            1 => {
                f["ports"]["families"][0]["witnesses"][0]["versioned_facts"][0] = json!(999);
            }
            2 => {
                let code = f["history"]["rules"][4]["code"]
                    .as_str()
                    .unwrap()
                    .replace("(= q (Pair b c))", "(= q (Pair b c)) (!= a c)");
                f["history"]["rules"][4]["code"] = json!(code);
            }
            3 => {
                f["states"]["0"]["subsumed_rows"] = json!([1]);
            }
            4 => {
                f["ports"]["families"][0]["contract"]["target_port_maps"][0] = json!([1, 0]);
            }
            5 => {
                f["ports"]["instances"][0]["ports"][0]["sort"] = json!("OtherSort");
            }
            6 => {
                f["ports"]["instances"][0]["ports"][0]["marker_index"] = json!(900);
            }
            7 => {
                f["ports"]["instances"][0]["ports"][1]["token"] =
                    f["ports"]["instances"][0]["ports"][0]["token"].clone();
            }
            _ => unreachable!(),
        }
        assert!(compile(&f).is_err(), "corrupt case {corrupt}");
    }
}

// The oracle explicitly expands certified source bodies into native egglog.
// Matrix execution never uses this materialization or its query result.
fn native(p: &Program, f: &Json) -> (EGraph, Vec<BTreeMap<usize, String>>) {
    let mut eg = EGraph::default();
    let commands = eg
        .parse_program(None, f["history"]["source_text"].as_str().unwrap())
        .unwrap();
    eg.run_program(
        commands
            .into_iter()
            .filter(|c| {
                !matches!(
                    c,
                    Command::Action(..) | Command::RunSchedule(..) | Command::Check(..)
                )
            })
            .collect(),
    )
    .unwrap();
    eg.parse_and_run_program(None, "(constructor MatrixInput (i64) E)")
        .unwrap();
    let mut result = vec![];
    for (ci, c) in p.cells.iter().enumerate() {
        let mut map = BTreeMap::new();
        if let Some(binding) = &c.binding {
            let state = &p.templates[c.template]["closure"];
            let inst = &f["ports"]["instances"][ci];
            for (port, &id) in inst["ports"].as_array().unwrap().iter().zip(binding) {
                map.insert(
                    port["closure_value"].as_u64().unwrap() as usize,
                    format!("(MatrixInput {id})"),
                );
            }
            for (v, desc) in state["values"].as_array().unwrap().iter().enumerate() {
                if let Some(s) = desc["literal"].as_str() {
                    map.insert(v, s.to_string());
                }
            }
            let mut waiting: BTreeSet<_> = state["rows"]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(_, r)| r["op"] != "RipenInput")
                .map(|(i, _)| i)
                .collect();
            while !waiting.is_empty() {
                let before = waiting.len();
                for i in waiting.clone() {
                    let row = &state["rows"][i];
                    let args = row["args"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|x| map.get(&(x.as_u64().unwrap() as usize)).cloned())
                        .collect::<Option<Vec<_>>>();
                    if let Some(args) = args {
                        let expr = format!("({} {})", row["op"].as_str().unwrap(), args.join(" "));
                        let out = row["result"].as_u64().unwrap() as usize;
                        if let Some(old) = map.get(&out) {
                            eg.parse_and_run_program(None, &format!("(union {old} {expr})"))
                                .unwrap();
                        } else {
                            let var = format!("audit_{ci}_{out}");
                            eg.parse_and_run_program(None, &format!("(let {var} {expr})"))
                                .unwrap();
                            map.insert(out, var);
                        }
                        waiting.remove(&i);
                    }
                }
                assert!(waiting.len() < before, "ungrounded test Cell");
            }
        }
        result.push(map);
    }
    (eg, result)
}
#[test]
fn imported_matrix_decisions_match_native_for_positive_and_broken_connections() {
    let f = fixture();
    let p = compile(&f).unwrap();
    for broken in [false, true] {
        let mut p = p.clone();
        if broken {
            let w = p.conditions[0].left;
            p.cells[w.0].binding.as_mut().unwrap()[w.1] = 7;
            p.bits = 3;
        }
        let report = p.clone().check().unwrap().execute();
        let (mut eg, _) = native(&p, &f);
        let sink = eg.prepare_cell_action("", "overlap").unwrap();
        let matches = eg.audit_cell_matches(&sink).unwrap();
        let names = sink.slots();
        for (ci, ch) in p.channels.iter().enumerate() {
            let left = p.cells[ch.sources[0]].binding.as_ref().unwrap();
            let right = p.cells[ch.sources[1]].binding.as_ref().unwrap();
            let query = [("a", left[0]), ("b", left[1]), ("c", right[1])];
            let vals = query
                .iter()
                .map(|(n, v)| {
                    let ex = egglog::ast::Parser::default()
                        .get_expr_from_string(None, &format!("(MatrixInput {v})"))
                        .unwrap();
                    (*n, eg.eval_expr(&ex).unwrap().1)
                })
                .collect::<BTreeMap<_, _>>();
            let expected = matches.iter().any(|row| {
                names
                    .iter()
                    .enumerate()
                    .all(|(j, (name, _))| vals.get(name).is_none_or(|v| *v == row[j]))
            });
            assert_eq!(report.epochs[0][0].decisions[ci] == 1, expected);
        }
        if broken {
            assert!(report.epochs[0][0].decisions.contains(&2));
        }
    }
}
