use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::{cell::OnceCell, rc::Rc};

/// The checkpoint wire format remains the source JSON. A repeated occurrence
/// shares the decoded child handles with all later visits to the same body.
#[derive(Debug, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Node {
    source: J,
    #[serde(skip)]
    children: OnceCell<Vec<Rc<Node>>>,
}
impl Node {
    fn shared(source: &J) -> Rc<Self> {
        Rc::new(Self {
            source: source.clone(),
            children: OnceCell::new(),
        })
    }

    pub fn source(&self) -> &J {
        &self.source
    }

    pub fn sequence(&self, nodes: &[J]) -> &[Rc<Node>] {
        self.children
            .get_or_init(|| nodes.iter().map(Self::shared).collect())
    }

    pub fn body(&self, field: &str) -> Rc<Node> {
        Rc::clone(
            &self
                .children
                .get_or_init(|| vec![Self::shared(&self.source[field])])[0],
        )
    }
}

/// Serializable source-control continuations. Only Run publication boundaries
/// can be checkpointed; no speculative union or partial RHS crosses a resume.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Task {
    Eval {
        node: Rc<Node>,
        path: Vec<usize>,
        iterations: Vec<u64>,
    },
    Repeat {
        body: Rc<Node>,
        count: u64,
        next: u64,
        start: usize,
        path: Vec<usize>,
        iterations: Vec<u64>,
    },
    Saturate {
        body: Rc<Node>,
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
            node: Node::shared(node),
            path: vec![i],
            iterations: vec![],
        })
        .collect()
}

/// Record the source occurrence once. JSON is only materialized for callers
/// that inspect an individual report field; the scheduler reads the typed flag.
#[derive(Serialize)]
pub struct RunStep {
    pub ordinal: usize,
    pub ruleset: String,
    pub path: Vec<usize>,
    pub iterations: Vec<u64>,
    pub epoch: u64,
    pub logical_matches: usize,
    pub local_bindings: usize,
    pub executed_bindings: usize,
    pub changed: bool,
    pub live_cells: usize,
    pub elapsed_seconds: f64,
    pub phases: Phases,
    #[serde(skip)]
    pub(crate) json: std::cell::OnceCell<J>,
}
#[derive(Serialize)]
pub struct Phases {
    pub search: f64,
    pub rhs: f64,
    pub repair: f64,
    pub delivery_gc: f64,
    pub query_update: f64,
}
impl std::ops::Index<&str> for RunStep {
    type Output = J;
    fn index(&self, key: &str) -> &J {
        &self.json.get_or_init(|| serde_json::json!(self))[key]
    }
}
