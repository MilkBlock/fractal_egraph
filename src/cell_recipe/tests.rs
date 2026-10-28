use super::*;
#[test]
fn native_ast_roundtrip_scopes_updates_primitives_and_visibility() -> Result<()> {
    let source = r#"(datatype X (N i64) (A X) (B X))
(function F (X) i64 :merge (min old new))
(relation Seen (X))
(ruleset first) (ruleset second)
(rule ((= x (N n)) (> n 0)) ((A x) (set (F x) (+ n 2))) :ruleset first)
(rule ((= x (A y))) ((B x) (Seen y)) :ruleset second)
(N 3) (run first 1) (run second 1)
(subsume (A (N 3))) (check (= (F (N 3)) 5))
(fail (check (= (F (N 3)) 6)))"#;
    let mut l = Library::default();
    l.register(0, &[], &json!({}), source, source)?;
    let dir = std::env::temp_dir().join(format!("cell-recipe-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("library.json");
    l.save(&path)?;
    let p = Program::load(&path)?;
    let mut expected = EGraph::default();
    expected.parse_and_run_program(None, source)?;
    let actual = p.execute(0, true, false)?;
    assert_eq!(
        expected.device_snapshot().map_err(|s| format!("{s}"))?,
        actual.device_snapshot().map_err(|s| format!("{s}"))?
    );
    let mut bad = p.document.clone();
    bad["expressions"][0] = json!({"op":"call","name":"N","args":[0]});
    assert!(Program { document: bad }.validate().is_err());
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
#[test]
fn endpoint_keeps_cycles_sharing_and_hidden_rows() -> Result<()> {
    let s = json!({"version":2,"scope":{"observation_only":false},"values":[{"sort":"X","literal":null}],"rows":[{"op":"A","args":[0],"result":0}],"subsumed_rows":[0],"ports":{"root":0}});
    let p = completion(&s, None, true)?;
    assert_eq!(p["instructions"][1]["result"], 0);
    assert_eq!(p["instructions"][1]["subsumed"], true);
    let mut s = s;
    s["scope"]["observation_only"] = json!(true);
    assert!(completion(&s, None, true).is_err());
    Ok(())
}

#[test]
fn endpoint_verification_anchors_named_ports() -> Result<()> {
    let source = "(datatype X (N i64)) (let root (N 0)) (N 1)";
    let mut library = Library::default();
    library.register(0, &[], &json!({}), source, source)?;
    let state = json!({"version":2,"scope":{},"values":[
        {"sort":"i64","literal":"0"},{"sort":"i64","literal":"1"},
        {"sort":"X","literal":null},{"sort":"X","literal":null}
    ],"rows":[{"op":"N","args":[0],"result":2},{"op":"N","args":[1],"result":3}],"ports":{"root":2}});
    let cell = library.cells.get_mut(&0).unwrap();
    cell["completion"] = completion(&state, None, false)?;
    cell["ripen_state"] = json!("Saturated");
    let mut p = Program {
        document: json!({"schema":"analyzer-cell-recipes/v1",
        "expressions":library.expressions,"instructions":library.commands,
        "environments":library.environments,"cells":library.cells.values().collect::<Vec<_>>()}),
    };
    assert_eq!(p.verify_endpoint(0)?["named_ports_verified"], 1);
    p.document["cells"][0]["completion"]["ports"]["root"] = json!(3);
    assert!(
        p.verify_endpoint(0).is_err(),
        "wrong root must fail despite identical fact rows"
    );
    Ok(())
}

#[test]
fn mutable_reads_are_not_cse_values_and_custom_merges_remain_structured() -> Result<()> {
    let source = r#"(datatype E (V i64) (Choice E E))
(function Best (E) E :merge (Choice old new))
(function Count (E) i64 :merge (min old new))
(set (Best (V 0)) (V 10))
(set (Best (V 0)) (V 7))
(set (Count (V 0)) 9) (check (= (Count (V 0)) 9))
(set (Count (V 0)) 4) (check (= (Count (V 0)) 4))
(check (= (Best (V 0)) (Choice (V 10) (V 7))))"#;
    let mut l = Library::default();
    l.register(0, &[], &json!({}), source, source)?;
    let dir = std::env::temp_dir().join(format!("cell-recipe-custom-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("library.json");
    l.save(&path)?;
    let p = Program::load(&path)?;
    let actual = p.execute(0, true, false)?;
    let mut expected = EGraph::default();
    expected.parse_and_run_program(None, source)?;
    assert_eq!(
        actual.device_snapshot().unwrap(),
        expected.device_snapshot().unwrap()
    );
    let mut bad = p.document.clone();
    let decl = bad["instructions"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|x| x["op"] == "declaration" && !x["merge_expression"].is_null())
        .unwrap();
    decl["merge_expression"] = J::Null;
    assert!(Program { document: bad }.execute(0, true, false).is_err());
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn input_slots_are_local_to_each_cell_frame() -> Result<()> {
    let a = "(datatype T (InputT i64) (BoxT T)) (BoxT (InputT 4))";
    let b = "(datatype U (InputU i64) (BoxU U)) (BoxU (InputU 9))";
    let mut l = Library::default();
    l.register(0, &[], &json!({"parameter_markers":{"T":"InputT"}}), a, a)?;
    l.register(1, &[], &json!({"parameter_markers":{"U":"InputU"}}), b, b)?;
    let dir = std::env::temp_dir().join(format!("cell-recipe-inputs-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("library.json");
    l.save(&path)?;
    let p = Program::load(&path)?;
    for (id, source) in [(0, a), (1, b)] {
        let got = p.execute(id, true, false)?;
        let mut expected = EGraph::default();
        expected.parse_and_run_program(None, source)?;
        assert_eq!(
            got.device_snapshot().unwrap(),
            expected.device_snapshot().unwrap()
        );
    }
    let mut bad = p.document.clone();
    bad["cells"][0]["inputs"][0]["sort"] = json!("U");
    assert!(Program { document: bad }.execute(0, true, false).is_err());
    std::fs::remove_dir_all(dir)?;
    Ok(())
}
