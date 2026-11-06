//! Select the smallest typed-table prefix covering 80% of observed retirement
//! savings. Recheck the target certificate; a hint only narrows its admission.
use eggfuse_cell_runtime::{checksum::artifact_digest, program::Program};
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
fn read(p: &str) -> Result<J, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let [training, summary, case, target, output] = args.as_slice() else {
        return Err("expected TRAIN_PROGRAM SUMMARY CASE TARGET_PROGRAM NEW_OUTPUT".into());
    };
    if Path::new(output).exists() {
        return Err("output exists".into());
    }
    let train = Program::parse(read(training)?)?;
    let summary = read(summary)?;
    if summary["inputs"][case] != json!(format!("{:x}", Sha256::digest(fs::read(training)?))) {
        return Err("training input does not match measurement provenance".into());
    }
    let stats = &summary["cases"][case]["on"]["deferred_incidence"]["stats"];
    let mut ranked = stats["retired_by_table"]
        .as_object()
        .ok_or("missing measured retirements")?
        .iter()
        .map(|(id, n)| Ok((id.parse::<usize>()?, n.as_u64().ok_or("invalid count")?)))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    if ranked.iter().any(|(i, _)| *i >= train.tables.len()) {
        return Err("training table out of bounds".into());
    }
    ranked.sort_by(|(a, na), (b, nb)| {
        nb.cmp(na)
            .then_with(|| train.tables[*a].name.cmp(&train.tables[*b].name))
    });
    let total: u64 = ranked.iter().map(|(_, n)| n).sum();
    let mut covered = 0;
    let mut tables = vec![];
    for (id, n) in ranked {
        if covered as u128 * 5 >= total as u128 * 4 {
            break;
        }
        let t = &train.tables[id];
        tables.push(json!({"name":t.name,"sorts":t.schema,"retirements":n}));
        covered += n;
    }
    let mut target = read(target)?;
    target["deferred_incidence_admission"] = json!({
        "schema":"eggfuse-deferred-incidence-admission/v1","tables":tables,
        "training_program_sha256":train.artifact["sha256"],"observed_retirements":total,"covered_retirements":covered,
        "objective":"smallest typed-table prefix covering 80 percent of actual retirements before reverse subscription"});
    target["sha256"] = json!(artifact_digest(&{
        let mut raw = target.clone();
        raw.as_object_mut().unwrap().remove("sha256");
        raw
    }));
    let checked = Program::parse(target)?;
    fs::write(output, serde_json::to_vec(&checked.artifact)?)?;
    println!("{}", checked.artifact["deferred_incidence_admission"]);
    Ok(())
}
