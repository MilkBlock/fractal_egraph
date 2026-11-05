//! Rank matching RHS applications in two or more checked, observational replays.
use eggfuse_cell_runtime::program::{Call, Op, Program};
use serde_json::{Value as J, json};
use std::{collections::BTreeMap, env, fs};
type R<T> = Result<T, Box<dyn std::error::Error>>;
fn read(p: &str) -> R<J> {
    Ok(serde_json::from_reader(std::io::BufReader::new(
        fs::File::open(p)?,
    ))?)
}
const FIELDS: [&str; 10] = [
    "calls",
    "frames",
    "lookups",
    "facts",
    "function_updates",
    "touches",
    "union_requests",
    "nonreflexive",
    "local_completions",
    "block_local_intermediates",
];
fn counts(j: &J) -> R<[u64; 10]> {
    let mut a = [0; 10];
    for (i, k) in FIELDS.iter().enumerate() {
        a[i] = j[*k].as_u64().ok_or("invalid RHS work report")?;
    }
    Ok(a)
}
fn row(members: &[usize], before: [u64; 10], after: [u64; 10], shape: &str) -> J {
    let saved: BTreeMap<_, _> = FIELDS
        .iter()
        .enumerate()
        .take(8)
        .skip(1)
        .map(|(i, k)| (*k, i128::from(before[i]) - i128::from(after[i])))
        .collect();
    let kept = before[3..6] == [0, 0, 0] && after[3..6] == [0, 0, 0] && before[7] == after[7];
    // Queued non-reflexive effects are separately matched, in order, per binding
    // by the replay, and all Run barriers and input tuples match.
    json!({"shape":shape,"rules":members,"calls":before[0],"before":before,"after":after,"saved":saved,
        "saved_published_index_rows":kept.then_some(0),"saved_consumer_wakes":kept.then_some(0)})
}
fn main() -> R<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();
    let [pg, base, other, rest @ ..] = args.as_slice() else {
        return Err(
            "usage: rank_rhs_work PROGRAM ORDINARY/RHS-WORK.JSON TRIAL/RHS-WORK.JSON".into(),
        );
    };
    if rest.len() > 1 {
        return Err("at most one output profile path is supported".into());
    }
    let p = Program::parse(read(pg)?)?;
    let a = read(base)?;
    let b = read(other)?;
    for k in ["program", "cohort_sha256", "applications", "barriers"] {
        if a[k].is_null() || a[k] != b[k] {
            return Err(format!("unmatched {k}").into());
        }
    }
    if a["program"] != p.artifact["sha256"] {
        return Err("program differs".into());
    }
    let mut groups: BTreeMap<String, (Vec<usize>, [u64; 10], [u64; 10])> = BTreeMap::new();
    let mut rules = vec![];
    let mut priority = BTreeMap::<Vec<[String; 2]>, i64>::new();
    if a["rules"].as_object().map(|x| x.len()) != b["rules"].as_object().map(|x| x.len()) {
        return Err("different rule cohorts".into());
    }
    for (rid, j) in a["rules"].as_object().ok_or("missing rules")? {
        let r: usize = rid.parse()?;
        let rule = p.rules.get(r).ok_or("invalid rule")?;
        let shape = p.templates[rule.template]
            .ops
            .iter()
            .enumerate()
            .filter(|(i, _)| !p.known_results.static_skip[r][*i])
            .filter_map(|(_, op)| match op {
                Op::Copy(..) => None,
                Op::Let(_, Call::Table(_), _) => Some("Let"),
                Op::Set(..) => Some("Set"),
                Op::Union(..) => Some("Union"),
                _ => Some("other"),
            })
            .collect::<Vec<_>>()
            .join(" ");
        let before = counts(j)?;
        let after = counts(&b["rules"][rid])?;
        if before[0] != after[0] || before[7] != after[7] {
            return Err("mismatched applications or unions".into());
        }
        if let Some(key) = p.rhs_operator_shape(r) {
            *priority.entry(key).or_default() +=
                i64::try_from(before[1])? - i64::try_from(after[1])?;
        }
        let g = groups.entry(shape.clone()).or_default();
        g.0.push(r);
        for i in 0..10 {
            g.1[i] += before[i];
            g.2[i] += after[i];
        }
        let mut rr = row(&[r], before, after, &shape);
        rr["source"] = p.artifact["recipe"]["rules"][r]["metadata"]["name"].clone();
        rules.push(rr);
    }
    let mut shapes = groups
        .into_iter()
        .map(|(s, (mut rs, a, b))| {
            rs.sort_unstable();
            row(&rs, a, b, &s)
        })
        .collect::<Vec<_>>();
    for items in [&mut shapes, &mut rules] {
        items.sort_by_key(|r| {
            (
                std::cmp::Reverse(r["saved"]["frames"].as_i64().unwrap()),
                std::cmp::Reverse(r["saved"]["lookups"].as_i64().unwrap()),
                r["rules"][0].as_u64(),
            )
        });
    }
    let mut order: Vec<_> = priority.into_iter().filter(|(_, n)| *n > 0).collect();
    order.sort_by(|(ka, na), (kb, nb)| nb.cmp(na).then_with(|| ka.cmp(kb)));
    let total: i64 = order.iter().map(|x| x.1).sum();
    let mut covered = 0;
    let mut selected = vec![];
    for (key, n) in &order {
        if covered * 5 >= total * 4 {
            break;
        }
        selected.push(key);
        covered += n;
    }
    let profile = json!({"schema":"eggfuse/rhs-work-admission/v1","shapes":selected,"training_program":a["program"],"training_saved_frames":total,"selected_frames":covered});
    if let Some(path) = rest.first() {
        let f = fs::File::create_new(path)?;
        serde_json::to_writer_pretty(f, &profile)?;
    }
    serde_json::to_writer_pretty(
        std::io::stdout().lock(),
        &json!({"profile":profile,"operator_priority":order,"fields":FIELDS,"program":p.artifact["sha256"],"cohort_sha256":a["cohort_sha256"],"shapes":shapes,"rules":rules}),
    )?;
    Ok(())
}
