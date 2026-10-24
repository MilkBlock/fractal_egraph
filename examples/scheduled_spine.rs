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
    let mut seal = None;
    let mut selected_layouts = None;
    let mut all_schedules = false;
    let mut source_closures = false;
    let mut args = a[2..].iter();
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--joint" => joint = true,
            "--all-schedules" => all_schedules = true,
            "--closures" => source_closures = true,
            "--certificates" => certificate = Some(args.next().ok_or("missing certificate file")?),
            "--select-layouts" => {
                selected_layouts = Some(args.next().ok_or("missing selected layout")?)
            }
            "--rebind" => rebind = Some(args.next().ok_or("missing selected layout")?),
            "--seal" => seal = Some(args.next().ok_or("missing selected layout")?),
            _ => return Err("unknown scheduled-spine option".into()),
        }
    }
    EGraph::set_num_threads(1);
    let mut eg = EGraph::default();
    let text = std::fs::read_to_string(&a[0])?;
    let explicit = cell_assisted_egglog::stable_choice::source::install(&mut eg, &text)?.is_some();
    let cs = eg.parse_program(Some(a[0].clone()), &text)?;
    if source_closures || seal.is_some() {
        cell_assisted_egglog::source_cells::Document::check_commands(&cs)?;
    }
    let end = cell_assisted_egglog::program_io::run_start(&cs);
    if end == cs.len() {
        return Err("no schedule after declarations".into());
    }
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
            let bundle = cell_assisted_egglog::parameterized_cell::artifact::Bundle::load(
                std::path::Path::new(file),
            )?;
            layout_queries = bundle.layout_queries_for(&cs[..end])?;
            let runtime = bundle.attach(eg, &cs[..end])?;
            library = Some(bundle);
            runtime
        } else if explicit {
            eg.run_program(cs[..end].to_vec())?;
            Runtime::attach_joint(eg, &cs[..end], Some(&[]))?
        } else {
            Runtime::new_joint_stable_commands(cs[..end].to_vec())?
        };
        certificates = r.report()["certificates"].clone();
        for p in r.report()["programs"].as_array().unwrap() {
            if let (Some(set), Some(n)) = (p["ruleset"].as_str(), p["rule"].as_str()) {
                let id = (set.to_owned(), n.to_owned());
                if !layout_queries.contains(&id) {
                    reserved.insert(id);
                }
            }
        }
        eg = r.detach_joint()?.0;
    } else {
        eg.run_program(cs[..end].to_vec())?;
    }
    let declared_closures = if source_closures {
        cell_assisted_egglog::parameterized_cell::declared::Closures::discover(eg.clone(),&cs[..end])?
    } else {None};
    if let Some(c)=&declared_closures {reserved.extend(c.protected_queries());}
    if let Some(path) = seal {
        if joint || certificate.is_some() {
            return Err("source sealing cannot install implicit proof services".into());
        }
        if text
            .lines()
            .any(|s| s.starts_with(cell_assisted_egglog::source_cells::HEADER))
        {
            return Err("source already has Cell declarations".into());
        }
        let queries = boundary::load_file(std::path::Path::new(path), &cs[..end])?;
        let mut native_rulesets = BTreeSet::new();
        let mut marker_tables = BTreeSet::new();
        for line in text.lines() {
            if let Some(s) = line.strip_prefix(cell_assisted_egglog::stable_choice::source::HEADER)
            {
                let v: serde_json::Value = serde_json::from_str(s)?;
                for s in v["native_rulesets"].as_array().into_iter().flatten() {
                    native_rulesets.insert(s.as_str().ok_or("invalid key helper")?.to_owned());
                }
            }
            if let Some(s) = line.strip_prefix("; cell-marker-tables/v1 ") {
                let v: serde_json::Value = serde_json::from_str(s)?;
                for t in v["tables"].as_array().into_iter().flatten() {
                    marker_tables.insert(t.as_str().ok_or("invalid marker table")?.to_owned());
                }
            }
        }
        let d = cell_assisted_egglog::source_cells::Document {
            schema: "cell-runtime/v1".into(),
            commands: cs.iter().map(ToString::to_string).collect(),
            queries,
            native_rulesets: native_rulesets.into_iter().collect(),
            marker_tables: marker_tables.into_iter().collect(),
            closures: declared_closures,
        };
        d.attach(eg,&cs)?;
        std::fs::write(
            &a[1],
            format!(
                "{}{}\n{}",
                cell_assisted_egglog::source_cells::HEADER,
                serde_json::to_string(&d)?,
                text
            ),
        )?;
        return Ok(());
    }
    if let Some(path) = selected_layouts {
        let mut bundle = library.ok_or("select-layouts needs a certificate library")?;
        let specs = boundary::load_file(std::path::Path::new(path), &cs[..end])?;
        bundle.select_layouts(&mut eg, &cs[..end], &specs)?;
        std::fs::write(&a[1], bundle.text()?)?;
        return Ok(());
    }
    if let Some(path) = rebind {
        let file = boundary::rebind_file(&mut eg, &cs[..end], std::path::Path::new(path))?;
        std::fs::write(&a[1], serde_json::to_vec_pretty(&file)?)?;
        return Ok(());
    }
    let mut selected = BTreeSet::new();
    if all_schedules {
        for c in &cs {
            if let Command::RunSchedule(s) = c {
                sets(s, &mut selected)
            }
        }
    } else if let Command::RunSchedule(s) = &cs[end] {
        sets(s, &mut selected)
    }
    for line in text.lines().filter_map(|s|s.strip_prefix(cell_assisted_egglog::stable_choice::source::HEADER)) {
        let spec:serde_json::Value=serde_json::from_str(line)?;
        for s in spec["native_rulesets"].as_array().into_iter().flatten() {
            selected.remove(s.as_str().ok_or("invalid native helper ruleset")?);
        }
    }
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
        let id = (
            q["ruleset"].as_str().unwrap().to_owned(),
            q["rule"].as_str().unwrap().to_owned(),
        );
        if layout_queries.contains(&id) {
            q["authority"] = serde_json::json!("analyze_complete_query");
        }
    }
    report["protected_queries"] = serde_json::json!(reserved);
    report["declared_closures"] = serde_json::json!(declared_closures);
    report["parameterized_library"] = serde_json::json!(library.as_ref().map(|b| b.report()));
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
