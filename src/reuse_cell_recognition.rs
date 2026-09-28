//! Cell recognition plans: *how to recognise* one shared RipSatCell, compiled
//! from recorded rule compositions instead of discovered by searching the graph.
//!
//! A plan is a description, not an execution. It lists, per legal source
//! composition, the ordered rule applications with their parent edges, relative
//! bindings, alias and fact/equality obligations, and it carries the compact
//! storage template the cell is installed with. Compiling a plan creates no
//! facts, proves no rewrite, and replays no history.
//!
//! Two adapters produce the same vocabulary:
//!
//! * [`compile_library`] over a validated library body and its origin recipes;
//! * [`compile_bake`] over an adaptive `Bake`, where one macro is one trigger
//!   step plus a complete body template.
//!
//! Every legal source for one shared body is preserved. Identical step prefixes
//! are hash-consed into shared [`RecognitionGraph`] nodes, so two sources that
//! agree on a prefix advance one partial match. Nothing here is truncated,
//! ranked, sampled or given a candidate budget: a plan that cannot be compiled
//! exactly is rejected instead.
use crate::{
    coarse_smooth::{canonical_effect, symbol, Comb, CombKind, Effect, RelativeBinding},
    pipeline::Result,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// Identity of a rule environment. Two rule texts with the same name in
/// different environments are different rules, so rule identity carries this.
///
/// The value is a stable hash of the normalized environment text, not a process
/// hash, so a plan compiled in one run still compares equal in another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EnvironmentId(pub u64);

impl EnvironmentId {
    /// The caller asserts that every recipe in the plan comes from a single,
    /// unnamed environment. Adapters that can see the environment text must
    /// use [`EnvironmentId::from_text`] instead.
    pub const UNKNOWN: EnvironmentId = EnvironmentId(0);

    /// Hash the normalized environment text. Normalization is the same join the
    /// adaptive runtime uses when it compares a Bake against a source program:
    /// one command per line, in order, without surrounding whitespace.
    pub fn from_text(environment: &[String]) -> Self {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut feed = |bytes: &[u8]| {
            for b in bytes {
                hash ^= *b as u64;
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        };
        for (i, command) in environment.iter().enumerate() {
            if i != 0 {
                feed(b"\n");
            }
            feed(command.trim().as_bytes());
        }
        EnvironmentId(hash)
    }
}

/// Identity of one recorded rule application.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct RuleIdentity {
    pub environment: EnvironmentId,
    /// Rule text for library recipes. The adaptive `Bake` does not store rule
    /// text, so its adapter uses the stable `#<rule-id>` form; the environment
    /// keeps distinct rule sets apart.
    pub rule: String,
}

/// One step of a recognition source: the vocabulary of [`Comb`], with rule
/// identity attached. Symbol ids stay recipe-local; this module never assumes
/// they are body value indices.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecognitionStep {
    pub rule: RuleIdentity,
    pub kind: CombKind,
    /// Parent steps inside the same source, strictly earlier and distinct.
    pub parents: Vec<usize>,
    pub binding: Vec<RelativeBinding>,
    /// Alias pattern in one namespace across a source's steps. `None` for a
    /// synthesized step that carries no recorded alias evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<usize>>,
    pub input_roles: Vec<String>,
    pub output_roles: Vec<String>,
    pub input_count: usize,
    pub output_count: usize,
    pub external_count: usize,
    pub requirements: Vec<Effect>,
    pub effects: Vec<Effect>,
    pub external_facts: Vec<Effect>,
}

impl RecognitionStep {
    fn from_comb(rule: RuleIdentity, step: &Comb) -> Self {
        Self {
            rule,
            kind: step.kind.clone(),
            parents: step.parents.clone(),
            binding: step.binding.clone(),
            aliases: Some(step.aliases.clone()),
            input_roles: step.input_roles.clone(),
            output_roles: step.output_roles.clone(),
            input_count: step.input_count,
            output_count: step.output_count,
            external_count: step.external_count,
            requirements: step.requirements.clone(),
            effects: step.effects.clone(),
            external_facts: step.external_facts.clone(),
        }
    }
    /// The same validation the library applies to a stored origin recipe. A plan
    /// may not be looser than the artifact it was compiled from.
    fn validate(&self, index: usize) -> Result<()> {
        if self.rule.rule.trim().is_empty()
            || (self.parents.is_empty() && self.kind != CombKind::CoarseRuleComposition)
            || self.parents.iter().any(|p| *p >= index)
            || self.input_roles.len() != self.input_count
            || self.output_roles.len() != self.output_count
            || self.binding.len() != self.input_count
            || self
                .aliases
                .as_ref()
                .is_some_and(|a| a.len() != self.input_count + self.output_count + self.external_count)
            || self.parents.iter().collect::<BTreeSet<_>>().len() != self.parents.len()
        {
            return Err("invalid recognition step".into());
        }
        Ok(())
    }
}

/// A shared recognition state. Steps are hash-consed on their content *plus*
/// their parent nodes, so equal prefixes are literally the same node.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecognitionNode {
    pub step: RecognitionStep,
    /// Canonical, sorted, distinct node ids of the parents.
    pub parents: Vec<usize>,
}

/// Prefix-sharing store for recognition nodes.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecognitionGraph {
    nodes: Vec<RecognitionNode>,
    index: BTreeMap<RecognitionNode, usize>,
}

impl RecognitionGraph {
    fn intern(&mut self, step: RecognitionStep, mut parents: Vec<usize>) -> usize {
        parents.sort_unstable();
        parents.dedup();
        let node = RecognitionNode { step, parents };
        if let Some(id) = self.index.get(&node) {
            return *id;
        }
        let id = self.nodes.len();
        self.index.insert(node.clone(), id);
        self.nodes.push(node);
        id
    }
    pub fn len(&self) -> usize {
        self.nodes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    pub fn node(&self, id: usize) -> Option<&RecognitionNode> {
        self.nodes.get(id)
    }
    pub fn nodes(&self) -> &[RecognitionNode] {
        &self.nodes
    }
}

/// Where a source composition came from. Provenance is kept so a plan can be
/// traced back to the artifact that justified it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SourceProvenance {
    /// One enclosure of a validated library body.
    LibraryEnclosure(usize),
    /// One macro of an adaptive `Bake`.
    AdaptiveMacro(usize),
}

/// One legal way to reach the shared body. A body with several recorded
/// compositions keeps all of them.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecognitionSource {
    pub provenance: SourceProvenance,
    /// Node ids in topological order. `steps[i]`'s parents are earlier nodes.
    pub steps: Vec<usize>,
}

impl RecognitionSource {
    /// The last step; the body is certified once every step of the source is.
    pub fn root(&self) -> Option<usize> {
        self.steps.last().copied()
    }
}

/// Identity of the shared body a plan recognises. Opaque on purpose: the
/// library uses interned body ids, the adaptive `Bake` uses macro ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum CellId {
    LibraryBody(usize),
    AdaptiveMacro(usize),
}

/// How a compact-storage binding position is obtained.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum SlotOrigin {
    /// Not produced inside the body: must be supplied by the source boundary.
    Boundary,
    /// Produced by the body itself; never a recognition input.
    Derived,
    /// A fixed literal stored with the plan.
    Literal,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PlanSlot {
    pub sort: String,
    pub origin: SlotOrigin,
    /// The stored term, when the source artifact carries one (adaptive macro).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub term: Option<String>,
}

/// One row of the compact storage template: a function and the binding slots of
/// its arguments followed by its result.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct BodyRow {
    pub function: String,
    pub columns: Vec<usize>,
}

/// How a source composition maps onto the compact storage unit.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SlotMapping {
    pub slots: Vec<PlanSlot>,
    /// Storage template: which slots each body row is built from.
    pub template: Vec<BodyRow>,
    /// Slots the recogniser must obtain from events, in slot order.
    pub boundary: Vec<usize>,
}

impl SlotMapping {
    fn validate(&self) -> Result<()> {
        let slots = self.slots.len();
        if self.template.is_empty() {
            return Err("cell has no recognisable body rows".into());
        }
        if self
            .template
            .iter()
            .any(|r| r.function.trim().is_empty() || r.columns.iter().any(|c| *c >= slots))
        {
            return Err("cell template references an unknown slot".into());
        }
        if self.boundary.iter().any(|s| *s >= slots) {
            return Err("cell boundary references an unknown slot".into());
        }
        if self.boundary.iter().collect::<BTreeSet<_>>().len() != self.boundary.len() {
            return Err("cell boundary repeats a slot".into());
        }
        if self
            .boundary
            .iter()
            .any(|s| self.slots[*s].origin != SlotOrigin::Boundary)
        {
            return Err("cell boundary contains a slot that is not a boundary".into());
        }
        Ok(())
    }
}

/// One shared body and every legal composition that reaches it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CellRecognitionPlan {
    pub cell: CellId,
    pub environment: EnvironmentId,
    pub graph: RecognitionGraph,
    pub sources: Vec<RecognitionSource>,
    pub slots: SlotMapping,
}

impl CellRecognitionPlan {
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }
    /// Every node reachable from any source root.
    pub fn reachable(&self) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut pending: Vec<usize> = self.sources.iter().filter_map(|s| s.root()).collect();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            if let Some(node) = self.graph.node(id) {
                pending.extend(node.parents.iter().copied());
            }
        }
        seen
    }
}

/// Re-symbolize a stored recipe so plans compiled from re-exported artifacts
/// agree. This mirrors the library's own registration step and is idempotent
/// on an already-canonical recipe.
pub fn canonical_recipe(recipe: &[Comb]) -> Vec<Comb> {
    let mut names = BTreeMap::new();
    recipe
        .iter()
        .map(|step| {
            let mut step = step.clone();
            step.aliases = step
                .aliases
                .iter()
                .map(|v| symbol(*v, &mut names))
                .collect();
            for effects in [
                &mut step.requirements,
                &mut step.effects,
                &mut step.external_facts,
            ] {
                for e in effects {
                    *e = canonical_effect(e, &mut names);
                }
            }
            step
        })
        .collect()
}

/// Compile one source's steps into shared graph nodes.
fn compile_source(
    graph: &mut RecognitionGraph,
    provenance: SourceProvenance,
    steps: Vec<RecognitionStep>,
) -> Result<RecognitionSource> {
    if steps.is_empty() {
        return Err("recognition source has no steps".into());
    }
    let mut ids = Vec::with_capacity(steps.len());
    for (i, step) in steps.iter().enumerate() {
        step.validate(i)?;
        for binding in &step.binding {
            let valid = match binding {
                RelativeBinding::External { slot } => *slot < step.external_count,
                RelativeBinding::ParentPort { parent, output } => steps
                    .get(*parent)
                    .is_some_and(|p| *output < p.output_count),
            };
            if !valid {
                return Err("recognition binding references an unknown port".into());
            }
        }
        let parents = step.parents.iter().map(|p| ids[*p]).collect();
        ids.push(graph.intern(step.clone(), parents));
    }
    // Two positions of one source may legitimately share a node: the source
    // keeps its own occurrence sequence, the graph only shares identical work.
    Ok(RecognitionSource {
        provenance,
        steps: ids,
    })
}

/// A validated library body plus its legal compositions.
pub struct LibraryBody {
    /// Interned body identity inside the library.
    pub body: usize,
    /// Body value slots: sort name and fixed literal, in canonical body order.
    pub values: Vec<(String, Option<String>)>,
    /// Body rows: function name and body value columns (arguments then result).
    pub rows: Vec<(String, Vec<usize>)>,
    pub sources: Vec<LibraryCellSource>,
}

/// One enclosure of a library body.
pub struct LibraryCellSource {
    pub enclosure: usize,
    /// Interface ports as body value indices: the declared cell boundary.
    pub interface: Vec<usize>,
    /// Stored origin recipe, in any symbol numbering.
    pub recipe: Vec<Comb>,
}

/// Compile library bodies into recognition plans. `environment` is the single
/// rule environment the library was recorded in; the library format does not
/// store rule text per entry, so the caller supplies it.
pub fn compile_library(
    environment: EnvironmentId,
    bodies: &[LibraryBody],
) -> Result<Vec<CellRecognitionPlan>> {
    let mut plans = Vec::with_capacity(bodies.len());
    let mut seen = BTreeSet::new();
    for body in bodies {
        if !seen.insert(body.body) {
            return Err("duplicate body in recognition catalog".into());
        }
        if body.sources.is_empty() {
            return Err("library body has no legal source composition".into());
        }
        let boundary: BTreeSet<usize> = body
            .sources
            .iter()
            .flat_map(|s| s.interface.iter().copied())
            .collect();
        let slots: Vec<PlanSlot> = body
            .values
            .iter()
            .enumerate()
            .map(|(i, (sort, literal))| PlanSlot {
                sort: sort.clone(),
                origin: if literal.is_some() {
                    SlotOrigin::Literal
                } else if boundary.contains(&i) {
                    SlotOrigin::Boundary
                } else {
                    SlotOrigin::Derived
                },
                term: literal.clone(),
            })
            .collect();
        let template: Vec<BodyRow> = body
            .rows
            .iter()
            .map(|(function, columns)| BodyRow {
                function: function.clone(),
                columns: columns.clone(),
            })
            .collect();
        let mapping = SlotMapping {
            boundary: boundary.iter().copied().collect(),
            slots,
            template,
        };
        mapping.validate()?;
        let mut graph = RecognitionGraph::default();
        let mut sources = Vec::with_capacity(body.sources.len());
        for source in &body.sources {
            let recipe = canonical_recipe(&source.recipe);
            let steps = recipe
                .iter()
                .map(|comb| {
                    RecognitionStep::from_comb(
                        RuleIdentity {
                            environment,
                            rule: comb.rule.clone(),
                        },
                        comb,
                    )
                })
                .collect::<Vec<_>>();
            sources.push(compile_source(
                &mut graph,
                SourceProvenance::LibraryEnclosure(source.enclosure),
                steps,
            )?);
        }
        plans.push(CellRecognitionPlan {
            cell: CellId::LibraryBody(body.body),
            environment,
            graph,
            sources,
            slots: mapping,
        });
    }
    Ok(plans)
}

/// One adaptive macro, as the Bake stores it.
pub struct BakeCell {
    pub rule: usize,
    /// Command text of the rule environment this macro was learned in.
    pub environment: Vec<String>,
    /// Macro slots: stored term and sort.
    pub slots: Vec<(String, String)>,
    /// Macro template: function and slot columns (arguments then result).
    pub template: Vec<(String, Vec<usize>)>,
}

/// Classify a baked macro slot term. Internal values are named
/// `adaptive_local_*` by the learning step; a term that parses as a literal is
/// fixed; everything else is a rule variable or the adaptive root and must be
/// supplied by the boundary. Nothing is inferred beyond that naming.
fn slot_origin(term: &str) -> SlotOrigin {
    if term.starts_with("adaptive_local_") {
        return SlotOrigin::Derived;
    }
    match egglog::ast::Parser::default().get_expr_from_string(None, term) {
        Ok(egglog::ast::Expr::Lit(..)) => SlotOrigin::Literal,
        Ok(_) => SlotOrigin::Boundary,
        // A term this adapter cannot classify is treated as a required boundary
        // input: the recogniser waits for it instead of guessing.
        Err(_) => SlotOrigin::Boundary,
    }
}

/// Compile adaptive macros into recognition plans. Each macro contributes one
/// trigger step; the complete body template is kept so completeness can be
/// confirmed per instance without re-searching the graph.
pub fn compile_bake(cells: &[BakeCell]) -> Result<Vec<CellRecognitionPlan>> {
    let mut plans = Vec::with_capacity(cells.len());
    for cell in cells {
        let environment = EnvironmentId::from_text(&cell.environment);
        let slots: Vec<PlanSlot> = cell
            .slots
            .iter()
            .map(|(term, sort)| PlanSlot {
                sort: sort.clone(),
                origin: slot_origin(term),
                term: Some(term.clone()),
            })
            .collect();
        let boundary: Vec<usize> = slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.origin == SlotOrigin::Boundary)
            .map(|(i, _)| i)
            .collect();
        let mapping = SlotMapping {
            boundary,
            slots,
            template: cell
                .template
                .iter()
                .map(|(function, columns)| BodyRow {
                    function: function.clone(),
                    columns: columns.clone(),
                })
                .collect(),
        };
        mapping.validate()?;
        let mut graph = RecognitionGraph::default();
        // The trigger step binds every boundary slot as an external input and
        // claims the body template as its effect. It is the step an observed
        // source application must match to locate an instance. Alias evidence is
        // absent because a macro stores no cross-step alias namespace.
        let trigger = RecognitionStep {
            rule: RuleIdentity {
                environment,
                rule: format!("#{}", cell.rule),
            },
            kind: CombKind::CoarseRuleComposition,
            parents: vec![],
            binding: (0..mapping.boundary.len())
                .map(|slot| RelativeBinding::External { slot })
                .collect(),
            aliases: None,
            input_roles: mapping.boundary.iter().map(|s| format!("slot:{s}")).collect(),
            // A macro stores no rule output roles. Body rows are obligations
            // (effects), not ports, so the trigger declares no outputs.
            output_roles: vec![],
            input_count: mapping.boundary.len(),
            output_count: 0,
            external_count: mapping.boundary.len(),
            requirements: vec![],
            effects: (0..mapping.template.len()).map(Effect::RowFact).collect(),
            external_facts: vec![],
        };
        let source = compile_source(
            &mut graph,
            SourceProvenance::AdaptiveMacro(cell.rule),
            vec![trigger],
        )?;
        plans.push(CellRecognitionPlan {
            cell: CellId::AdaptiveMacro(cell.rule),
            environment,
            graph,
            sources: vec![source],
            slots: mapping,
        });
    }
    Ok(plans)
}

/// Plans may only be combined when they were compiled in the same environment.
pub fn merge_plans(plans: Vec<CellRecognitionPlan>) -> Result<Vec<CellRecognitionPlan>> {
    let mut environment: Option<EnvironmentId> = None;
    for plan in &plans {
        match environment {
            None => environment = Some(plan.environment),
            Some(e) if e == plan.environment => {}
            Some(_) => return Err("cannot combine recognition plans from different environments".into()),
        }
    }
    Ok(plans)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(rule: &str, parents: Vec<usize>, external: usize, outputs: usize) -> Comb {
        Comb {
            kind: if parents.is_empty() {
                CombKind::CoarseRuleComposition
            } else {
                CombKind::SmoothRuleComposition
            },
            rule: rule.into(),
            input_roles: vec!["var:x".into()],
            output_roles: (0..outputs).map(|i| format!("var:y{i}")).collect(),
            parents,
            binding: if external > 0 {
                vec![RelativeBinding::External { slot: 0 }]
            } else {
                vec![RelativeBinding::ParentPort {
                    parent: 0,
                    output: 0,
                }]
            },
            aliases: (0..1 + outputs + external).collect(),
            input_count: 1,
            output_count: outputs,
            external_count: external,
            requirements: vec![],
            effects: vec![Effect::RowFact(0)],
            external_facts: vec![],
        }
    }

    fn library_body(recipe_a: Vec<Comb>, recipe_b: Vec<Comb>) -> LibraryBody {
        LibraryBody {
            body: 7,
            values: vec![
                ("Math".into(), None),
                ("Math".into(), None),
                ("i64".into(), Some("3".into())),
            ],
            rows: vec![
                ("Add".into(), vec![0, 1, 2]),
                ("Mul".into(), vec![1, 2, 0]),
            ],
            sources: vec![
                LibraryCellSource {
                    enclosure: 0,
                    interface: vec![0, 1],
                    recipe: recipe_a,
                },
                LibraryCellSource {
                    enclosure: 1,
                    interface: vec![1],
                    recipe: recipe_b,
                },
            ],
        }
    }

    #[test]
    fn environment_identity_is_stable_and_normalized() {
        let a = EnvironmentId::from_text(&["(datatype Math)".into(), "(rule r)".into()]);
        let b = EnvironmentId::from_text(&["(datatype Math) ".into(), " (rule r)".into()]);
        let c = EnvironmentId::from_text(&["(rule r)".into(), "(datatype Math)".into()]);
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(EnvironmentId::UNKNOWN, a);
    }

    #[test]
    fn every_legal_source_is_preserved_and_shared_prefixes_are_one_node() {
        let env = EnvironmentId::from_text(&["(rule r)".into()]);
        let shared = step("r", vec![], 1, 1);
        let body = library_body(
            vec![shared.clone(), step("s", vec![0], 0, 1)],
            vec![shared.clone(), step("t", vec![0], 0, 1)],
        );
        let plans = compile_library(env, &[body]).unwrap();
        assert_eq!(plans.len(), 1);
        let plan = &plans[0];
        assert_eq!(plan.cell, CellId::LibraryBody(7));
        assert_eq!(plan.source_count(), 2);
        // Identical first steps are the same node; the two tails differ.
        assert_eq!(plan.graph.len(), 3);
        assert_eq!(plan.sources[0].steps[0], plan.sources[1].steps[0]);
        assert_ne!(plan.sources[0].steps[1], plan.sources[1].steps[1]);
        assert_eq!(plan.reachable().len(), 3);
        assert_eq!(plan.sources[0].root(), Some(plan.sources[0].steps[1]));
    }

    #[test]
    fn library_mapping_marks_interface_slots_and_literals() {
        let env = EnvironmentId::from_text(&["(rule r)".into()]);
        let body = library_body(vec![step("r", vec![], 1, 1)], vec![step("r", vec![], 1, 1)]);
        let plan = &compile_library(env, &[body]).unwrap()[0];
        assert_eq!(plan.slots.boundary, vec![0, 1]);
        assert_eq!(plan.slots.slots[0].origin, SlotOrigin::Boundary);
        assert_eq!(plan.slots.slots[1].origin, SlotOrigin::Boundary);
        assert_eq!(plan.slots.slots[2].origin, SlotOrigin::Literal);
        assert_eq!(plan.slots.slots[2].term.as_deref(), Some("3"));
        assert_eq!(
            plan.slots.template,
            vec![
                BodyRow {
                    function: "Add".into(),
                    columns: vec![0, 1, 2]
                },
                BodyRow {
                    function: "Mul".into(),
                    columns: vec![1, 2, 0]
                },
            ]
        );
    }

    #[test]
    fn non_topological_or_short_recipe_is_rejected_not_repaired() {
        let env = EnvironmentId::from_text(&["(rule r)".into()]);
        let bad = library_body(vec![step("r", vec![1], 1, 1)], vec![step("r", vec![], 1, 1)]);
        assert!(compile_library(env, &[bad]).is_err());
        let empty = LibraryBody {
            body: 1,
            values: vec![("Math".into(), None)],
            rows: vec![("Add".into(), vec![0])],
            sources: vec![],
        };
        assert!(compile_library(env, &[empty]).is_err());
        let duplicate = library_body(vec![step("r", vec![], 1, 1)], vec![step("r", vec![], 1, 1)]);
        let same_body_twice = vec![
            duplicate,
            LibraryBody {
                body: 7,
                values: vec![("Math".into(), None)],
                rows: vec![("Add".into(), vec![0])],
                sources: vec![LibraryCellSource {
                    enclosure: 0,
                    interface: vec![0],
                    recipe: vec![step("r", vec![], 1, 1)],
                }],
            },
        ];
        assert!(compile_library(env, &same_body_twice).is_err());
    }

    #[test]
    fn adapter_rejects_a_recipe_whose_boundary_is_too_small() {
        let env = EnvironmentId::from_text(&["(rule r)".into()]);
        let mut broken = step("r", vec![], 0, 1);
        broken.binding = vec![RelativeBinding::External { slot: 3 }];
        let body = libbody_with(broken);
        assert!(compile_library(env, &[body]).is_err());
    }

    fn libbody_with(recipe_step: Comb) -> LibraryBody {
        LibraryBody {
            body: 9,
            values: vec![("Math".into(), None)],
            rows: vec![("Add".into(), vec![0])],
            sources: vec![LibraryCellSource {
                enclosure: 0,
                interface: vec![0],
                recipe: vec![recipe_step],
            }],
        }
    }

    #[test]
    fn bake_plans_separate_boundary_derived_and_literal_slots() {
        let cells = vec![BakeCell {
            rule: 4,
            environment: vec!["(datatype Math)".into()],
            slots: vec![
                ("x".into(), "Math".into()),
                ("adaptive_root".into(), "Math".into()),
                ("adaptive_local_0".into(), "Math".into()),
                ("17".into(), "i64".into()),
            ],
            template: vec![
                ("Add".into(), vec![0, 1, 2]),
                ("Const".into(), vec![3, 0]),
            ],
        }];
        let plans = compile_bake(&cells).unwrap();
        assert_eq!(plans.len(), 1);
        let plan = &plans[0];
        assert_eq!(plan.cell, CellId::AdaptiveMacro(4));
        assert_eq!(plan.slots.boundary, vec![0, 1]);
        assert_eq!(plan.slots.slots[2].origin, SlotOrigin::Derived);
        assert_eq!(plan.slots.slots[3].origin, SlotOrigin::Literal);
        assert_eq!(plan.slots.slots[0].term.as_deref(), Some("x"));
        assert_eq!(plan.graph.len(), 1);
        let node = plan.graph.node(plan.sources[0].steps[0]).unwrap();
        assert_eq!(node.step.input_count, 2);
        assert_eq!(node.step.effects.len(), 2);
        assert!(plan.sources[0].steps.iter().all(|s| *s == plan.sources[0].steps[0]));
    }

    #[test]
    fn bake_environment_separates_equal_rule_ids() {
        let mut cells = vec![BakeCell {
            rule: 4,
            environment: vec!["(rule a)".into()],
            slots: vec![("x".into(), "Math".into())],
            template: vec![("A".into(), vec![0])],
        }];
        cells.push(BakeCell {
            rule: 4,
            environment: vec!["(rule b)".into()],
            slots: vec![("x".into(), "Math".into())],
            template: vec![("A".into(), vec![0])],
        });
        let plans = compile_bake(&cells).unwrap();
        assert_ne!(plans[0].environment, plans[1].environment);
        assert!(merge_plans(plans).is_err());
    }
}
