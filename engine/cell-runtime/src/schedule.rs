use serde::{Deserialize, Serialize};
use serde_json::Value as J;
use std::rc::Rc;

/// Decode each control occurrence once at setup. Child JSON is moved out of
/// its parent. Only checkpoint serialization reconstructs the source tree.
#[derive(Debug, Deserialize)]
#[serde(from = "J")]
pub struct Node {
    source: J,
    instruction: Instruction,
    pub(crate) static_path: Option<Rc<[usize]>>,
}
impl Node {
    fn shared(source: &J, path: Rc<[usize]>) -> Rc<Self> {
        Rc::new(Self::decode(source.clone(), Some(path)))
    }

    pub fn instruction(&self) -> &Instruction {
        &self.instruction
    }
}
impl From<J> for Node {
    fn from(source: J) -> Self {
        Self::decode(source, None)
    }
}
impl Node {
    fn decode(mut source: J, path: Option<Rc<[usize]>>) -> Self {
        let instruction = if let Some(set) = source["run"].as_str() {
            Instruction::Run {
                set: set.into(),
                until_run: source["until_run"].as_str().map(Into::into),
                check: source["assert"].as_bool() == Some(true),
                unlowered: !source["until"].is_null() || !source["assert_table"].is_null(),
            }
        } else if source["sequence"].is_array() {
            let nodes = std::mem::take(source["sequence"].as_array_mut().unwrap());
            Instruction::Sequence(
                nodes
                    .into_iter()
                    .enumerate()
                    .map(|(i, n)| {
                        let child = path
                            .as_ref()
                            .map(|p| p.iter().copied().chain(std::iter::once(i)).collect());
                        Rc::new(Self::decode(n, child))
                    })
                    .collect(),
            )
        } else if let Some(n) = source["repeat"].as_u64() {
            let body = source.get_mut("body").map(J::take).unwrap_or_default();
            Instruction::Repeat(n, Rc::new(Self::decode(body, path.clone())))
        } else if !source["saturate"].is_null() {
            Instruction::Saturate(Rc::new(Self::decode(
                source["saturate"].take(),
                path.clone(),
            )))
        } else {
            Instruction::Unknown
        };
        Self {
            source,
            instruction,
            static_path: path,
        }
    }
}
impl Serialize for Node {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut raw = self.source.clone();
        match &self.instruction {
            Instruction::Sequence(xs) => raw["sequence"] = serde_json::json!(xs),
            Instruction::Saturate(x) => raw["saturate"] = serde_json::json!(x),
            Instruction::Repeat(_, x) if raw.get("body").is_some() => {
                raw["body"] = serde_json::json!(x)
            }
            _ => (),
        }
        raw.serialize(ser)
    }
}

#[derive(Debug)]
pub enum Instruction {
    Run {
        set: Box<str>,
        until_run: Option<Box<str>>,
        check: bool,
        unlowered: bool,
    },
    Sequence(Vec<Rc<Node>>),
    Repeat(u64, Rc<Node>),
    Saturate(Rc<Node>),
    Unknown,
}

/// Serializable source-control continuations. Only Run publication boundaries
/// can be checkpointed; no speculative union or partial RHS crosses a resume.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Task {
    Eval {
        node: Rc<Node>,
        path: Rc<[usize]>,
        iterations: Rc<[u64]>,
    },
    Repeat {
        body: Rc<Node>,
        count: u64,
        next: u64,
        start: usize,
        path: Rc<[usize]>,
        iterations: Rc<[u64]>,
    },
    Saturate {
        body: Rc<Node>,
        iteration: u64,
        start: usize,
        path: Rc<[usize]>,
        iterations: Rc<[u64]>,
    },
}
pub fn initial(control: &[J]) -> Vec<Task> {
    control
        .iter()
        .enumerate()
        .rev()
        .map(|(i, node)| {
            let path: Rc<[usize]> = Rc::from([i]);
            Task::Eval {
                node: Node::shared(node, path.clone()),
                path,
                iterations: Rc::from([]),
            }
        })
        .collect()
}

/// Record the source occurrence once. JSON is only materialized for callers
/// that inspect an individual report field; the scheduler reads the typed flag.
#[derive(Serialize)]
pub struct RunStep {
    pub ordinal: usize,
    pub ruleset: String,
    pub path: Rc<[usize]>,
    pub iterations: Rc<[u64]>,
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

#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    #[test]
    fn nested_control_preserves_checkpoint_shape_and_metadata() {
        let source = serde_json::json!({"sequence":[{"repeat":0,"note":"no body"},
            {"repeat":3,"body":{"saturate":{"run":"r","until_run":"p"}}}],"meta":[1,2]});
        let task = initial(std::slice::from_ref(&source)).pop().unwrap();
        let wire = serde_json::to_value(task).unwrap();
        assert_eq!(wire["Eval"]["node"], source);
        let parsed: Task = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), wire);
    }
}
