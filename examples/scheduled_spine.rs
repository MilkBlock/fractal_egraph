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
    let mut rebind = None;
    let mut selected_layouts = None;
    let mut args = a[2..].iter();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--joint" => joint = true,
            "--certificates" => certificate = Some(args.next().ok_or("missing certificate file")?),
            "--select-layouts" => selected_layouts = Some(args.next().ok_or("missing selected layout")?),
            "--rebind" => rebind = Some(args.next().ok_or("missing selected layout")?),
            _ => return Err("unknown scheduled-spine option".into()),
        }
    }
    EGraph::set_num_threads(1);
    let mut eg = EGraph::default();
    let text = std::fs::read_to_string(&a[0])?;
    let explicit = cell_assisted_egglog::stable_choice::source::install(&mut eg, &text)?.is_some();
    let cs = eg.parse_program(Some(a[0].clone()), &text)?;
    let end = cs
        .iter()
        .rposition(|c| matches!(c, Command::RunSchedule(_)))
        .ok_or("no main schedule")?;
    let mut reserved = BTreeSet::new();
    let mut certificates = serde_json::Value::Null;
    let mut library = None;
    let mut layout_queries = BTreeSet::new();
    if joint || certificate.is_some() {
        let r = if let Some(file) = certificate {
            if joint && !explicit {
                cell_assisted_egglog::stable_choice::setup(&mut eg, &cs[..end])?;
            } else {
                eg.run_program(cs[..end].to_vec())?;
            }
            let bundle=cell_assisted_egglog::parameterized_cell::artifact::Bundle::load(std::path::Path::new(file))?;
            layout_queries=bundle.layout_queries_for(&cs[..end])?;
            let runtime=bundle.attach(eg,&cs[..end])?;
            library=Some(bundle);
            runtime
        } else if explicit {
            eg.run_program(cs[..end].to_vec())?;
            Runtime::attach_joint(eg, &cs[..end], Some(&[]))?
        } else {
            Runtime::new_joint_stable_commands(cs[..end].to_vec())?
        };
        certificates = r.report()["certificates"].clone();
        for p in r.report()["programs"].as_array().unwrap() {
            if let (Some(set),Some(n))=(p["ruleset"].as_str(),p["rule"].as_str()) {
                let id=(set.to_owned(),n.to_owned());
                if !layout_queries.contains(&id) {reserved.insert(id);}
            }
        }
        eg = r.detach_joint()?.0;
    } else {
        eg.run_program(cs[..end].to_vec())?;
    }
    if let Some(path)=selected_layouts {
        let mut bundle=library.ok_or("select-layouts needs a certificate library")?;
        let specs=boundary::load_file(std::path::Path::new(path),&cs[..end])?;
        bundle.select_layouts(&mut eg,&cs[..end],&specs)?;
        std::fs::write(&a[1],bundle.text()?)?;
        return Ok(());
    }
    if let Some(path) = rebind {
        let file = boundary::rebind_file(&mut eg, &cs[..end], std::path::Path::new(path))?;
        std::fs::write(&a[1], serde_json::to_vec_pretty(&file)?)?;
        return Ok(());
    }
    let mut selected = BTreeSet::new();
    if let Command::RunSchedule(s) = &cs[end] {
        sets(s, &mut selected)
    }
    selected.remove("cell-stable-keys");
    selected.remove("cell-stable-freeze");
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
    for q in report["queries"].as_array_mut().unwrap() {
        let id=(q["ruleset"].as_str().unwrap().to_owned(),q["rule"].as_str().unwrap().to_owned());
        if layout_queries.contains(&id) {
            q["authority"]=serde_json::json!("analyze_complete_query");
        }
    }
    report["parameterized_library"]=serde_json::json!(library.as_ref().map(|b|b.report()));
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
