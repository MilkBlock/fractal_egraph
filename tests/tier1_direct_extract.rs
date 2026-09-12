#[test]
fn direct_extraction_recreates_exact_constructor_counts_without_instances(){
    let report:serde_json::Value=serde_json::from_str(include_str!("../experiments/tier1_extract/extraction.json")).unwrap();
    let mut eg=egglog::EGraph::default();
    eg.parse_and_run_program(None,include_str!("../experiments/tier1_extract/existing_combs.egg")).unwrap();
    let mut total=0;
    for name in ["Empty","SmoothComb","CoarseComb"] {
        let mut count=0;eg.function_for_each(name,|r|if !r.subsumed{count+=1}).unwrap();
        assert_eq!(report["kind_counts"][name].as_u64().unwrap(),count);total+=count;
    }
    assert_eq!(report["comb_eclasses"].as_u64().unwrap(),total);
    let mut instances=0;eg.function_for_each("Occurrence",|_|instances+=1).unwrap();assert_eq!(instances,0);
}

#[test]
fn tier0_source_dictionary_is_native_egglog_and_all_combs_have_a_mapping(){
    let mut eg=egglog::EGraph::default();
    eg.parse_and_run_program(None,include_str!("../experiments/tier1_extract/tier0_rules.egg")).unwrap();
    let report:serde_json::Value=serde_json::from_str(include_str!("../experiments/tier1_extract/extraction.json")).unwrap();
    let mut mapped=0;
    for d in report["definitions"].as_array().unwrap(){
        if d["kind"]=="Empty"{continue;}
        assert!(d["tier0_rule_id"].as_str().unwrap().starts_with('R'));
        assert!(d["tier0_definition"].as_str().unwrap().contains("(rule"));mapped+=1;
    }
    assert_eq!(mapped,1388);
}
