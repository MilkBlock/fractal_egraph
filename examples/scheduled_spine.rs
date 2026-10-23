//! Export typed candidates for a complete scheduled program. Main inference is
//! never flattened, saturated or archived by this tool.
use cell_assisted_egglog::{
    Result,
    parameterized_cell::{Runtime, boundary},
};
use egglog::{
    EGraph,
    ast::{Command, GenericSchedule, Schedule},
};
use std::collections::BTreeSet;
fn sets(s: &Schedule, out: &mut BTreeSet<String>) {
    match s {
        GenericSchedule::Run(_, r) => {
            out.insert(r.ruleset.clone());
        }
        GenericSchedule::Sequence(_, xs) => {
            for x in xs {
                sets(x, out)
            }
        }
        GenericSchedule::Repeat(_, _, s) | GenericSchedule::Saturate(_, s) => sets(s, out),
    }
}
fn main() -> Result<()> {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    if a.len() < 2 {
        return Err("SOURCE OUTPUT [--joint] [--certificates FILE]".into());
    }
    let mut certificate = None;
    let mut joint = false;
    let mut args = a[2..].iter();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--joint" => joint = true,
            "--certificates" => certificate = Some(args.next().ok_or("missing certificate file")?),
            _ => return Err("unknown scheduled-spine option".into()),
        }
    }
    EGraph::set_num_threads(1);
    let mut eg = EGraph::default();
    let cs = eg.parse_program(Some(a[0].clone()), &std::fs::read_to_string(&a[0])?)?;
    let end = cs
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("no main schedule")?;
    let mut reserved = BTreeSet::new();
    let mut certificates = serde_json::Value::Null;
    if joint || certificate.is_some() {
        let r = if let Some(file) = certificate {
            if joint {
                cell_assisted_egglog::stable_choice::setup(&mut eg, &cs[..end])?;
            } else {
                eg.run_program(cs[..end].to_vec())?;
            }
            cell_assisted_egglog::parameterized_cell::artifact::Bundle::load(std::path::Path::new(
                file,
            ))?
            .attach(eg, &cs[..end])?
        } else {
            Runtime::new_joint_stable_commands(cs[..end].to_vec())?
        };
        certificates = r.report()["certificates"].clone();
        for p in r.report()["programs"].as_array().unwrap() {
            if let Some(n) = p["rule"].as_str() {
                reserved.insert(n.to_owned());
            }
        }
        eg = r.detach_joint()?.0;
    } else {
        eg.run_program(cs[..end].to_vec())?;
    }
    let mut selected = BTreeSet::new();
    if let Command::RunSchedule(s) = &cs[end] {
        sets(s, &mut selected)
    }
    selected.remove("cell-stable-keys");
    loop {
        let before = selected.clone();
        for c in &cs {
            if let Command::UnstableCombinedRuleset(_, name, members) = c {
                if selected.contains(name) {
                    selected.extend(members.iter().cloned());
                }
            }
        }
        if before == selected {
            break;
        }
    }
    for c in &cs {
        if let Command::UnstableCombinedRuleset(_, name, _) = c {
            selected.remove(name);
        }
    }

    let mut report = boundary::inspect(
        &mut eg,
        &selected.into_iter().collect::<Vec<_>>(),
        &reserved,
    )?;
    report["table_rows"] = serde_json::json!(
        eg.get_function_names()
            .into_iter()
            .map(|n| {
                let size = eg.get_size(&n);
                (n, size)
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    );
    report["schedule"] = serde_json::json!(cs[end].to_string());
    report["setup_commands"] = serde_json::json!(end);
    report["preserved_commands"] =
        serde_json::json!(cs.iter().map(ToString::to_string).collect::<Vec<_>>());
    report["joint"] = serde_json::json!(joint);
    report["certificates"] = certificates;
    std::fs::write(&a[1], serde_json::to_vec_pretty(&report)?)?;
    Ok(())
}
