//! Learn a bounded two-rule Cell class from a full native Pass 3 run, then
//! relocate its checked instructions without training on the destination.
use eggfuse_cell_runtime::{checksum::artifact_digest, compiler, program::Program};
use serde_json::{Value as J, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

type Result<T> = egg_layout::pipeline::Result<T>;
fn rows(j: &J) -> Result<&Vec<J>> {
    j.as_array().ok_or_else(|| "expected array".into())
}
fn id(j: &J) -> Result<usize> {
    Ok(usize::try_from(j.as_u64().ok_or("expected id")?)?)
}
fn load(p: &Path) -> Result<J> {
    Ok(serde_json::from_slice(&fs::read(p)?)?)
}
fn save(p: &Path, j: &J) -> Result<()> {
    let parent = p
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut file, j)?;
    file.persist_noclobber(p)?;
    Ok(())
}
fn sha(s: &[u8]) -> String {
    format!("{:x}", Sha256::digest(s))
}
fn digest(j: &mut J) {
    j.as_object_mut().unwrap().remove("sha256");
    j["sha256"] = json!(artifact_digest(j))
}
fn verify(j: &J) -> Result<()> {
    let mut payload = j.clone();
    let hash = payload
        .as_object_mut()
        .ok_or("expected object")?
        .remove("sha256");
    if hash != Some(json!(artifact_digest(&payload))) {
        return Err("digest mismatch".into());
    }
    Ok(())
}
fn identity(rule: &J) -> String {
    json!([rule["metadata"]["source_set"], rule["metadata"]["name"]]).to_string()
}
fn site(h: &J) -> (String, String, String) {
    (
        h["producer"].to_string(),
        h["consumer"].to_string(),
        h["table"].to_string(),
    )
}
fn select(p: &Program, deployment: &J, history: &[J]) -> Result<Vec<usize>> {
    let ir = &p.artifact["recipe"];
    let rules = rows(&ir["rules"])?;
    let eligible = rows(&deployment["recipe"]["rules"])?
        .iter()
        .map(artifact_digest)
        .collect::<BTreeSet<_>>();
    let ids = rules
        .iter()
        .enumerate()
        .filter(|(_, r)| {
            eligible.contains(&artifact_digest(r))
                && r["metadata"]["source_set"] != "initialization"
        })
        .map(|(i, r)| (identity(r), i))
        .collect::<BTreeMap<_, _>>();
    let mut observed = BTreeMap::<_, Vec<usize>>::new();
    let mut pairs = BTreeSet::new();
    for (i, h) in history.iter().enumerate() {
        let k = site(h);
        if let (Some(&l), Some(&r)) = (ids.get(&k.0), ids.get(&k.1)) {
            pairs.insert([l, r]);
            observed.entry(k).or_default().push(i);
        }
    }
    let plan =
        compiler::composition_program(p, &json!({"recipe_sha256":ir["sha256"],"pairs":pairs}))?;
    let edges = rows(&plan["edges"])?;
    let mut counts = BTreeMap::new();
    for e in edges {
        *counts
            .entry((id(&e["producer"])?, id(&e["consumer"])?))
            .or_insert(0) += 1;
    }
    let mut choices = vec![];
    for e in edges {
        let (l, r) = (id(&e["producer"])?, id(&e["consumer"])?);
        if counts[&(l, r)] != 1
            || rows(&e["instructions"])?.is_empty() && rows(&e["reuse"])?.is_empty()
        {
            continue;
        }
        let t = &rules[r]["query"][id(&e["anchor"])?]["call"]["table"];
        if let Some(events) =
            observed.get(&(identity(&rules[l]), identity(&rules[r]), t.to_string()))
        {
            choices.push((std::cmp::Reverse(events.len()), l, r, events));
        }
    }
    choices.sort();
    let mut selected = choices
        .into_iter()
        .take(8)
        .flat_map(|(_, _, _, v)| v.iter().copied().take(8))
        .collect::<Vec<_>>();
    selected.sort_unstable();
    Ok(selected)
}
fn learn(src: &Path, deployment: &Path, out: &Path) -> Result<()> {
    let text = fs::read_to_string(src)?;
    let deploy = load(deployment)?;
    verify(&deploy)?;
    let dir = deployment
        .parent()
        .ok_or("missing training artifact directory")?;
    if fs::read(dir.join("source.egg"))? != text.as_bytes() {
        return Err("training program must belong to the training source".into());
    }
    let core = egg_layout::device_frontend::core("training.egg", &text)?;
    let p = Program::parse(compiler::compile(core, &BTreeSet::new(), None)?)?;
    let ir = &p.artifact["recipe"];
    let rules = rows(&ir["rules"])?;
    fs::create_dir(out)?;
    let baked =
        egg_layout::native_analyze::bake_transfer_sample("training.egg", &text, out, |h| {
            select(&p, &deploy, h)
        })?;
    save(&out.join("selection.json"), &baked)?;
    let graph = &baked["sample"]["graph"];
    let classes = rows(&graph["units"])?
        .iter()
        .map(|m| Ok((id(&m["unit"])?, id(&m["eclass"])?)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let recipe_file = load(&out.join("cell-recipes.json"))?;
    let recipes = rows(&recipe_file["cells"])?;
    let mut groups = BTreeMap::<usize, Vec<&J>>::new();
    for s in rows(&baked["sample"]["samples"])? {
        let unit = id(&s["unit"])?;
        if recipes
            .iter()
            .any(|r| r["unit"] == unit && r["program_status"] == "ready")
        {
            groups.entry(classes[&unit]).or_default().push(s)
        }
    }
    // Rank classes by distinct observed applications, breaking ties by registry order.
    let (class, members) = groups
        .into_iter()
        .min_by_key(|(c, m)| (std::cmp::Reverse(m.len()), *c))
        .ok_or("no prepared transferable class")?;
    let first = members[0];
    let example = id(&first["unit"])?;
    let h = &first["handoff"];
    if members.iter().any(|m| {
        site(&m["handoff"]) != site(h)
            || m["handoff"]["anchor_literals"] != h["anchor_literals"]
            || m["handoff"]["producer_literals"] != h["producer_literals"]
    }) {
        return Err("class members disagree on handoff ABI".into());
    }
    let cell = recipes
        .iter()
        .find(|c| c["unit"] == example)
        .ok_or("missing prepared cell")?;
    let wiring = rows(&cell["relative_binding"])?;
    if wiring.len() != 2
        || !rows(&wiring[1]["binding"])?
            .iter()
            .any(|v| v.get("ParentPort").is_some())
    {
        return Err("prepared slice lacks relative producer binding".into());
    }
    // Admit exactly one committed row link in the prepared pair. The relative
    // recipe must transfer a source row to a consumer read, in addition to any
    // scalar or eclass coordinates of that row.
    let output_roles = rows(&wiring[0]["output_roles"])?;
    let input_roles = rows(&wiring[1]["input_roles"])?;
    let links = rows(&wiring[1]["binding"])?
        .iter()
        .zip(input_roles)
        .filter(|(b, role)| {
            role.as_str().is_some_and(|r| r.starts_with("read:"))
                && b["ParentPort"]["parent"] == 0
                && b["ParentPort"]["output"]
                    .as_u64()
                    .and_then(|i| output_roles.get(i as usize))
                    .and_then(J::as_str)
                    .is_some_and(|r| r.starts_with("row:"))
        })
        .count();
    if links != 1 {
        return Err("expected exactly one transferred producer row".into());
    }
    let rule_id = |v: &J| {
        rules
            .iter()
            .position(|r| identity(r) == v.to_string())
            .ok_or("unresolved native rule")
    };
    let (l, r) = (rule_id(&h["producer"])?, rule_id(&h["consumer"])?);
    let writes = rows(&rules[l]["effects"])?
        .iter()
        .filter(|op| {
            op["call"]["table"] == h["table"] && (op["kind"] == "construct" || op["kind"] == "set")
        })
        .count();
    let reads = rows(&rules[r]["query"])?
        .iter()
        .filter(|a| a["call"]["table"] == h["table"])
        .count();
    if writes != 1 || reads != 1 {
        return Err("ambiguous producer instruction or consumer anchor".into());
    }
    let mut plan =
        compiler::composition_program(&p, &json!({"recipe_sha256":ir["sha256"],"pairs":[[l,r]]}))?;
    if rows(&plan["edges"])?.len() != 1 {
        return Err("ambiguous training edge".into());
    }
    let e = &mut plan["edges"][0];
    if rules[r]["query"][id(&e["anchor"])?]["call"]["table"] != h["table"] {
        return Err("training edge table differs".into());
    }
    let table = rows(&ir["tables"])?
        .iter()
        .find(|t| t["name"] == h["table"])
        .ok_or("missing anchor table")?;
    let mut literals = vec![];
    for a in rows(&h["anchor_literals"])? {
        let col = id(&a["column"])?;
        let sort = rows(&table["schema"])?
            .get(col)
            .ok_or("anchor column out of range")?;
        if sort["name"] != a["sort"] || sort["kind"] != "i64" {
            return Err("unsupported sampled literal sort".into());
        }
        let value = a["text"]
            .as_str()
            .ok_or("missing integer")?
            .parse::<i64>()?;
        literals.push(json!({"column":col,"operand":{"sort":sort,"literal":value}}));
    }
    let mut input_guards = vec![];
    for a in rows(&h["producer_literals"])? {
        if a["sort"] != "i64" {
            return Err("unsupported producer literal sort".into());
        }
        let slot = if let Some(name) = a["role"]["variable"].as_str() {
            rows(&rules[l]["slots"])?
                .iter()
                .position(|s| s["name"] == name && s["definition"] == "query")
                .ok_or("unresolved producer variable")?
        } else {
            let mut atoms = rows(&rules[l]["query"])?
                .iter()
                .filter(|atom| atom["call"]["table"] == a["role"]["read"]);
            let atom = atoms.next().ok_or("missing producer read")?;
            if atoms.next().is_some() {
                return Err("ambiguous producer read site".into());
            }
            id(&rows(&atom["args"])?.last().ok_or("empty read")?["slot"])?
        };
        let sort = &rules[l]["slots"][slot]["sort"];
        if sort["name"] != a["sort"] {
            return Err("producer guard sort differs".into());
        }
        let value = a["text"]
            .as_str()
            .ok_or("invalid producer literal")?
            .parse::<i64>()?;
        input_guards.push(json!({"slot":slot,"operand":{"sort":sort,"literal":value}}));
    }
    e["producer_literals"] = json!(input_guards);
    e["anchor_literals"] = json!(literals);
    plan["schema"] = json!("eggfuse-baked-composition/v1");
    digest(&mut plan);
    // The relative binding supplies the observed row, while the typed plan also
    // checks every independent consumer port and executes the full original RHS.
    eggfuse_cell_runtime::compositions::Compositions::parse(&p, &plan)?;
    let entry = out.join(format!("unit-{example}/source.egg"));
    let ripen = egg_layout::native_analyze::ripen::run(&entry, &out.join("representative-ripen"))?;
    if ripen["ripen"]["state"] != "Saturated" || ripen["checks"] != "passed" {
        return Err("representative did not reach a checked isolated endpoint".into());
    }
    let mut names = BTreeSet::new();
    for n in [l, r] {
        for k in ["read_tables", "write_tables"] {
            names.extend(rows(&rules[n]["footprint"][k])?.iter().map(J::to_string));
        }
    }
    let decls = rows(&ir["tables"])?
        .iter()
        .filter(|t| names.contains(&t["name"].to_string()) || h["table"] == t["name"])
        .collect::<Vec<_>>();
    let mut unit = json!({"schema":"eggfuse-transferred-unit/v1","training_sha256":sha(text.as_bytes()),"cell_egraph_sha256":artifact_digest(graph),"class":class,"members":members,"relative_binding":wiring,"representative":example,"prepared_source_sha256":sha(&fs::read(entry)?),"ripen_state":ripen["ripen"]["state"],"producer":rules[l],"consumer":rules[r],"declarations":decls,"plan":plan});
    digest(&mut unit);
    save(&out.join("unit.json"), &unit)?;
    println!(
        "{}",
        json!({"class":class,"members":members.len(),"sha256":unit["sha256"],"runs":baked["boundaries"],"cross_stage_witnesses":baked["cross_ruleset_write_read_handoffs"]})
    );
    Ok(())
}
fn link(unit_file: &Path, program: &Path, out: &Path) -> Result<()> {
    let u = load(unit_file)?;
    verify(&u)?;
    if u["schema"] != "eggfuse-transferred-unit/v1" {
        return Err("unknown unit schema".into());
    }
    let mut j = load(program)?;
    verify(&j)?;
    if j.get("compositions").is_some() {
        return Err("input already contains a composition plan".into());
    }
    let ir = &j["recipe"];
    let rules = rows(&ir["rules"])?;
    let tables = rows(&ir["tables"])?;
    for t in rows(&u["declarations"])? {
        if !tables.contains(t) {
            return Err(format!("incompatible table {}", t["name"]).into());
        }
    }
    let find = |rule: &J| -> Result<usize> {
        let mut hits = rules
            .iter()
            .enumerate()
            .filter(|(_, r)| *r == rule)
            .map(|(id, _)| id);
        let id = hits.next().ok_or("incompatible typed rule")?;
        if hits.next().is_some() {
            return Err("ambiguous typed rule".into());
        }
        Ok(id)
    };
    let (l, r) = (find(&u["producer"])?, find(&u["consumer"])?);
    let mut plan = u["plan"].clone();
    verify(&plan)?;
    let old_l = id(&plan["edges"][0]["producer"])?;
    let old_r = id(&plan["edges"][0]["consumer"])?;
    let relocate = |v: &J| -> Result<usize> {
        match id(v)? {
            n if n == old_l => Ok(l),
            n if n == old_r => Ok(r),
            _ => Err("unknown unit step".into()),
        }
    };
    for step in plan["programs"][0]["steps"]
        .as_array_mut()
        .ok_or("missing steps")?
    {
        step["rule"] = json!(relocate(&step["rule"])?);
    }
    for entry in plan["programs"][0]["marker_entries"]
        .as_array_mut()
        .ok_or("missing marker entries")?
    {
        entry["rule"] = json!(relocate(&entry["rule"])?);
    }
    plan["edges"][0]["producer"] = json!(l);
    plan["edges"][0]["consumer"] = json!(r);
    plan["recipe_sha256"] = ir["sha256"].clone();
    plan["evidence"] =
        json!({"recipe_sha256":ir["sha256"],"pairs":[[l,r]],"unit_sha256":u["sha256"]});
    plan["control"] = ir["control"].clone();
    plan["marker_tables"] = j["reference_views"]["marker_tables"].clone();
    digest(&mut plan);
    j["compositions"] = plan;
    digest(&mut j);
    Program::parse(j.clone())?;
    save(out, &j)?;
    Ok(())
}
fn main() -> Result<()> {
    egglog::EGraph::set_num_threads(1);
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.as_slice(){
 [cmd,src,program,out] if cmd=="learn"=>learn(Path::new(src),Path::new(program),Path::new(out)),
 [cmd,unit,program,out] if cmd=="link"=>link(Path::new(unit),Path::new(program),Path::new(out)),
 _=>Err("usage: baked_unit learn TRAIN.egg TRAIN_ARTIFACT/program.json OUT_DIR | link UNIT.json TARGET_PROGRAM.json OUT.json".into())}
}

#[cfg(test)]
mod tests {
    use super::*;
    use eggfuse_cell_runtime::{executor::Executor, graph::Graph};

    #[test]
    fn frozen_unit_requires_compatible_rules_and_live_guards() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let frozen = root.join("experiments/pass3_cell_transfer/unit.json");
        let dir = tempfile::tempdir().unwrap();
        let case = root.join("experiments/profile_suite/inputs");
        let source = ["common-prefix.egg", "sum-bits.eggpart", "common-suffix.egg"]
            .map(|part| fs::read_to_string(case.join(part)).unwrap())
            .concat();
        let core = egg_layout::device_frontend::core("training.egg", &source).unwrap();
        let raw = compiler::compile(core, &BTreeSet::new(), None).unwrap();
        let base = dir.path().join("raw.json");
        save(&base, &raw).unwrap();
        let linked = dir.path().join("linked.json");
        link(&frozen, &base, &linked).unwrap();
        let artifact = load(&linked).unwrap();
        let run = |enabled| {
            let graph = Graph::new(Program::parse(artifact.clone()).unwrap()).unwrap();
            let mut exec = Executor::new(graph, false);
            exec.compositions_enabled = enabled;
            exec.execute_until(None).unwrap();
            (
                exec.graph.export().unwrap(),
                exec.report()["compositions"]["stats"].clone(),
            )
        };
        let (before, _) = run(false);
        let (after, stats) = run(true);
        assert_eq!(before, after);
        assert!(stats["cross_run_applications"].as_u64().unwrap() > 0);
        assert!(stats["baked_guard_misses"].as_u64().unwrap() > 0);

        let mut invalid = artifact;
        invalid["compositions"]["edges"][0]["producer_literals"][0]["slot"] = json!(usize::MAX);
        digest(&mut invalid["compositions"]);
        assert!(Program::parse(invalid).is_err());

        let mut wrong = load(&frozen).unwrap();
        wrong["producer"]["metadata"]["name"] = json!("unseen-rule");
        digest(&mut wrong);
        let path = dir.path().join("mismatch.json");
        save(&path, &wrong).unwrap();
        assert!(link(&path, &base, &dir.path().join("unusable.json")).is_err());
    }
}
