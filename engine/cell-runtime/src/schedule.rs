use serde::{Deserialize, Serialize};
use serde_json::Value as J;

/// Serializable source-control continuations. Only Run publication boundaries
/// can be checkpointed; no speculative union or partial RHS crosses a resume.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Task {
    Eval {
        node: J,
        path: Vec<usize>,
        iterations: Vec<u64>,
    },
    Repeat {
        body: J,
        count: u64,
        next: u64,
        start: usize,
        path: Vec<usize>,
        iterations: Vec<u64>,
    },
    Saturate {
        body: J,
        iteration: u64,
        start: usize,
        path: Vec<usize>,
        iterations: Vec<u64>,
    },
}
pub fn initial(control: &[J]) -> Vec<Task> {
    control
        .iter()
        .enumerate()
        .rev()
        .map(|(i, node)| Task::Eval {
            node: node.clone(),
            path: vec![i],
            iterations: vec![],
        })
        .collect()
}
