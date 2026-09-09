//! A producer -> committed constructor row -> consumer -> combined-prefix seed.
#[allow(dead_code)]
#[path = "rule_combine/compose.rs"]
mod compose;
use egglog::{EGraph, RuleActionOutcome, TraceSession, WriteOutcome};
use serde_json::json;
use std::collections::HashSet;
fn main() {
    let mut output = Vec::new();
    for scenario in ["fresh", "preexisting", "competing", "no_source_match"] {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
   (datatype E (Var i64) (A E) (B E) (C E))
   (ruleset source) (ruleset consumer)
   (rewrite (A x) (B x) :ruleset source :name "A-to-B")
   (rewrite (B x) (C x) :ruleset consumer :name "B-to-C")
  "#,
        )
        .unwrap();
        if scenario != "no_source_match" {
            eg.parse_and_run_program(None, "(A (Var 7))").unwrap();
        }
        if scenario == "preexisting" || scenario == "no_source_match" {
            eg.parse_and_run_program(None, "(B (Var 7))").unwrap();
        }
        if scenario == "competing" {
            eg.parse_and_run_program(
                None,
                r#"(rewrite (A x) (B x) :ruleset source :name "A2-to-B")"#,
            )
            .unwrap();
        }
        let mut plain = eg.clone();
        let trace = TraceSession::with_dependencies();
        for ruleset in ["source", "consumer"] {
            assert_eq!(
                eg.step_rules_with_trace(ruleset, &trace).unwrap().updated,
                plain.step_rules(ruleset).unwrap().updated
            );
        }
        for g in [&mut eg, &mut plain] {
            g.parse_and_run_program(None, "(check (= (B (Var 7)) (C (Var 7))))")
                .unwrap();
        }
        let matches = trace.matches();
        let writes = trace.write_events();
        let reads = trace.row_reads();
        let survived: HashSet<_> = trace
            .action_outcomes()
            .into_iter()
            .filter(|o| o.outcome == RuleActionOutcome::Survived)
            .map(|o| o.match_event_id)
            .collect();
        let mut seeds = Vec::new();
        for read in &reads {
            let Some(producer_id) = read.producer_match_event_id else {
                continue;
            };
            let producer = matches.iter().find(|m| m.event_id == producer_id).unwrap();
            let consumer = matches
                .iter()
                .find(|m| m.event_id == read.match_event_id)
                .unwrap();
            if !["A-to-B", "A2-to-B"].contains(&producer.rule.as_ref())
                || consumer.rule.as_ref() != "B-to-C"
            {
                continue;
            }
            assert!(producer.physical_witness_complete && consumer.physical_witness_complete);
            assert!(survived.contains(&producer.event_id) && survived.contains(&consumer.event_id));
            let commit = writes
                .iter()
                .find(|w| Some(w.event_id) == read.producer_write_event_id)
                .unwrap();
            assert_eq!(commit.outcome, WriteOutcome::Inserted);
            assert_eq!(commit.actual, read.row);
            let definitions = [
                compose::Rule::new(&producer.rule, "(A x)", "(B x)"),
                compose::Rule::new(&consumer.rule, "(B x)", "(C x)"),
            ];
            let px = producer
                .bindings
                .iter()
                .find(|b| b.name.as_deref() == Some("x"))
                .unwrap()
                .value;
            let cx = consumer
                .bindings
                .iter()
                .find(|b| b.name.as_deref() == Some("x"))
                .unwrap()
                .value;
            assert_eq!(px, cx);
            let c = compose::candidates(&definitions);
            assert_eq!(c.len(), 1);
            seeds.push(json!({"producer":producer.rule.as_ref(),"consumer":consumer.rule.as_ref(),"producer_match":producer.event_id,"committed_write":commit.event_id,"consumed_read":read.event_id,"consumer_match":consumer.event_id,"fact_table":read.table_name.as_deref(),"fact_key":format!("{:?}",read.key),"fact_row":format!("{:?}",read.row),"combined_lhs":c[0].rule.lhs.to_string(),"middle":c[0].middle.to_string(),"combined_rhs":c[0].rule.rhs.to_string(),"boundary_bindings":{"v0":format!("{px:?}")},"installed":false}));
        }
        assert_eq!(
            seeds.len(),
            if scenario == "fresh" || scenario == "competing" {
                1
            } else {
                0
            },
            "{scenario}"
        );
        output.push(json!({"scenario":scenario,"matches":matches.iter().map(|m|json!({"event":m.event_id,"rule":m.rule.as_ref(),"keyed_atom_witnesses_complete":m.physical_witness_complete})).collect::<Vec<_>>(),"writes":writes.iter().map(|w|json!({"event":w.event_id,"match":w.match_event_id,"table":format!("{:?}",w.table),"outcome":format!("{:?}",w.outcome),"proposed":format!("{:?}",w.proposed),"actual":format!("{:?}",w.actual)})).collect::<Vec<_>>(),"combined_prefix_seeds":seeds,"traced_untraced_checks":true}));
    }
    println!("{}",serde_json::to_string_pretty(&json!({"mode":"diagnostic_inserted_row_dependencies","union_lineage_complete":false,"cases":output})).unwrap());
}
