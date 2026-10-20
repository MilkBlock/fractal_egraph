//! One actual native-runtime correctness/lookup-work ablation, not a GPU speedup.
use cell_assisted_egglog::{parameterized_cell::Runtime, table_live};
use egglog::ast::Command;
use serde_json::{Value as Json, json};

pub const SOURCE: &str = include_str!("../../experiments/parameterized_cell/terms.egg");
pub const UPDATES: &str =
    include_str!("../../experiments/parameterized_cell/competing_updates.egg");

pub fn run() -> Result<Json, Box<dyn std::error::Error>> {
    let mut reuse = Runtime::new_joint(SOURCE)?;
    let base = reuse.engine().clone();
    let mut parser = base.clone();
    let commands = parser.parse_program(None, UPDATES)?;
    let schedules = commands
        .iter()
        .filter(|c| matches!(c, Command::RunSchedule(_)))
        .cloned()
        .collect::<Vec<_>>();
    let mut reference = table_live::Runtime::new_cpu(base.clone(), &schedules)?;
    let mut cold = Runtime::new_joint(SOURCE)?;
    cold.set_port_reuse(false);
    let mut input = String::new();
    let mut phases = vec![];
    let names = [
        "initial two producers",
        "x: 7 to 2, p: 10 to 7, q: 15 to 12",
        "worse candidate",
        "second improvement",
        "input union",
        "one-to-many TCPair alias",
        "quiescent",
    ];
    for command in commands {
        if !matches!(command, Command::RunSchedule(_)) {
            input.push_str(&format!("{command}\n"));
            continue;
        }
        reference.input(&input)?;
        cold.input(&input)?;
        reuse.input(&input)?;
        input.clear();
        let before_cold = cold.stats.keyed_reads;
        let before_reuse = reuse.stats.keyed_reads;
        let before_saved = reuse.report()["port_reuse"]["reused_reads"]
            .as_u64()
            .unwrap();
        reference.run(std::slice::from_ref(&command))?;
        cold.run(std::slice::from_ref(&command))?;
        reuse.run(std::slice::from_ref(&command))?;
        reuse.audit_mirror()?;
        cold.audit_mirror()?;
        let native = super::compare::equal(&base, reference.engine(), reuse.engine())?;
        let ablation = super::compare::equal(&base, cold.engine(), reuse.engine())?;
        let counts = |r: &Runtime| {
            r.report()["programs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p["rule"].clone(), p["emitted_bindings"].clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            counts(&cold),
            counts(&reuse),
            "port reuse changed effect submissions"
        );
        phases.push(json!({"label":names[phases.len()],
            "same_submit":native,"reuse_off_on":ablation,
            "index_reads_without_reuse":cold.stats.keyed_reads-before_cold,
            "index_reads_with_reuse":reuse.stats.keyed_reads-before_reuse,
            "reused_reads":reuse.report()["port_reuse"]["reused_reads"].as_u64().unwrap()-before_saved}));
    }
    assert!(input.trim().is_empty(), "trailing checks were not executed");
    assert!(reuse.stats.keyed_reads < cold.stats.keyed_reads);
    assert_eq!(reuse.stats.program_compilations, 1);
    assert_eq!(cold.report()["port_reuse"]["reused_reads"], 0);
    assert_eq!(phases.last().unwrap()["index_reads_with_reuse"], 0);
    Ok(
        json!({"passed":true,"port_transition_verified":true,"scope":"Actual native effects and full committed graph checks at each saturation boundary. Same-submit CPU reference. Index reads, not time or GPU speedup. No cost-specific closure catalog.",
        "phases":phases,"with_reuse":reuse.report(),"without_reuse":cold.report()}),
    )
}
