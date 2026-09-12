//! Native egglog tier-1 effect reasoning for supplied, positive ground contracts.
//! Certificates are local to a use; they never union or delete context nodes.
use crate::{binding_program::Term, effect_program::Fact, trigger_bridge::BindingRelation};
use egglog::EGraph;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Debug)]
pub enum Effect {
    Fact(Fact),
    Equal(Term, Term),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Context(pub usize);
#[derive(Clone)]
struct Application {
    rule: String,
    binding: String,
    requirements: Vec<Effect>,
    produced: Vec<Effect>,
    grounded: bool,
}
struct Record {
    given: Vec<Effect>,
    parents: Vec<Context>,
    application: Option<Application>,
}
pub struct Tier1 {
    eg: EGraph,
    records: Vec<Record>,
    intern: BTreeMap<String, Context>,
    proposals: Vec<(Context, Context, Context)>,
}
fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap()
}
fn ground(t: &Term) -> bool {
    !t.op.starts_with("in:") && t.args.iter().all(ground)
}
fn subst(t: &Term, b: &BTreeMap<usize, Term>) -> Term {
    if let Some(i) =
        t.op.strip_prefix("in:")
            .and_then(|i| i.parse::<usize>().ok())
    {
        return b.get(&i).cloned().unwrap_or_else(|| t.clone());
    }
    Term::new(&t.sort, &t.op, t.args.iter().map(|a| subst(a, b)).collect())
}
fn fact_subst(f: &Fact, b: &BTreeMap<usize, Term>) -> Fact {
    match f {
        Fact::Node(t) => Fact::Node(subst(t, b)),
        Fact::Relation(n, a) => Fact::Relation(n.clone(), a.iter().map(|t| subst(t, b)).collect()),
    }
}
fn effect_json(e: &Effect) -> Value {
    match e {
        Effect::Fact(Fact::Relation(n, a)) => {
            json!({"kind":"fact","name":n,"args":a.iter().map(Term::json).collect::<Vec<_>>()})
        }
        Effect::Fact(Fact::Node(t)) => json!({"kind":"node","term":t.json()}),
        Effect::Equal(a, b) => json!({"kind":"equal","args":[a.json(),b.json()]}),
    }
}
impl Tier1 {
    pub fn new() -> Result<Self> {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(None, include_str!("../experiments/tier1_effects/ir.egg"))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            eg,
            records: vec![],
            intern: BTreeMap::new(),
            proposals: vec![],
        })
    }
    fn run(&mut self, s: &str) -> Result<()> {
        self.eg
            .parse_and_run_program(None, s)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn name(&self, c: Context) -> Result<String> {
        self.records.get(c.0).ok_or("invalid context")?;
        Ok(format!("$ctx{}", c.0))
    }
    fn val(&mut self, t: &Term) -> Result<String> {
        let v = format!("(V {} {})", quote(&t.sort), quote(&t.json().to_string()));
        self.run(&format!("(ValueKnown {v})"))?;
        Ok(v)
    }
    fn args(&mut self, ts: &[Term]) -> Result<String> {
        let mut s = "(ANil)".to_string();
        for t in ts.iter().rev() {
            s = format!("(ACons {} {s})", self.val(t)?);
        }
        Ok(s)
    }
    fn effect(&mut self, e: &Effect) -> Result<String> {
        Ok(match e {
            Effect::Fact(Fact::Relation(n, args)) => format!(
                "(HasFact {} {})",
                quote(&format!("relation:{n}")),
                self.args(args)?
            ),
            Effect::Fact(Fact::Node(t)) => format!(
                "(HasFact {} {})",
                quote(&format!("node:{}:{}", t.sort, t.op)),
                self.args(&t.args)?
            ),
            Effect::Equal(a, b) => {
                if a.sort != b.sort {
                    return Err("equality sort mismatch".into());
                }
                format!("(Equal {} {})", self.val(a)?, self.val(b)?)
            }
        })
    }
    fn effects(&mut self, es: &[Effect]) -> Result<String> {
        let mut s = "(ENil)".to_string();
        for e in es.iter().rev() {
            s = format!("(ECons {} {s})", self.effect(e)?);
        }
        Ok(s)
    }
    fn insert(
        &mut self,
        expression: String,
        parents: Vec<Context>,
        application: Option<Application>,
    ) -> Result<Context> {
        if let Some(c) = self.intern.get(&expression) {
            return Ok(*c);
        }
        let c = Context(self.records.len());
        self.run(&format!("(let $ctx{} {expression})", c.0))?;
        self.records.push(Record {
            given: vec![],
            parents,
            application,
        });
        self.intern.insert(expression, c);
        Ok(c)
    }
    /// Assumed external effects, not inferred tier-0 committed mutations.
    pub fn entry(&mut self, effects: &[Effect]) -> Result<Context> {
        for e in effects {
            Self::validate_effect(e, true)?;
        }
        let c = self.insert(format!("(Entry {})", self.records.len()), vec![], None)?;
        self.records[c.0].given.extend_from_slice(effects);
        for e in effects {
            let e = self.effect(e)?;
            self.run(&format!("(Given {} {e})", self.name(c)?))?;
        }
        Ok(c)
    }
    /// Add externally witnessed monotone facts to an Entry in this analysis epoch.
    /// Retractions/rollback require a new Tier1 instance; derived certificates are monotone.
    pub fn add_entry_effects(&mut self, c: Context, effects: &[Effect]) -> Result<()> {
        let r = self.records.get(c.0).ok_or("invalid entry")?;
        if !r.parents.is_empty() || r.application.is_some() {
            return Err("only Entry accepts external effects".into());
        }
        for e in effects {
            Self::validate_effect(e, true)?;
        }
        self.records[c.0].given.extend_from_slice(effects);
        for e in effects {
            let encoded = self.effect(e)?;
            self.run(&format!("(Given {} {encoded})", self.name(c)?))?;
        }
        Ok(())
    }
    pub fn dominates_for_use(&mut self, c: Context, app: Context) -> Result<bool> {
        self.check(&format!(
            "(DominatesForUse {} {})",
            self.name(c)?,
            self.name(app)?
        ))
    }
    fn validate_effect(e: &Effect, must_ground: bool) -> Result<()> {
        let terms: Vec<_> = match e {
            Effect::Fact(Fact::Node(t)) => vec![t],
            Effect::Fact(Fact::Relation(_, a)) => a.iter().collect(),
            Effect::Equal(a, b) => {
                if a.sort != b.sort {
                    return Err("equality sort mismatch".into());
                }
                vec![a, b]
            }
        };
        if must_ground && terms.iter().any(|t| !ground(t)) {
            return Err("effect must be ground".into());
        }
        Ok(())
    }
    pub fn join(&mut self, a: Context, b: Context) -> Result<Context> {
        self.insert(
            format!("(Join {} {})", self.name(a)?, self.name(b)?),
            vec![a, b],
            None,
        )
    }
    /// Partial assignments remain immutable pending applications. Complete them
    /// by constructing another candidate, not by pretending missing inputs are effects.
    pub fn apply(
        &mut self,
        parent: Context,
        rule: &str,
        relation: &BindingRelation,
        binding: &BTreeMap<usize, Term>,
        produced: &[Effect],
    ) -> Result<Context> {
        let gap = relation.gap(
            &crate::effect_program::Summary::identity(BTreeMap::new()),
            binding,
        )?;
        let mut requirements: Vec<_> = relation
            .facts
            .iter()
            .map(|f| Effect::Fact(fact_subst(f, binding)))
            .collect();
        requirements.extend(
            relation
                .equalities
                .iter()
                .map(|(a, b)| Effect::Equal(subst(a, binding), subst(b, binding))),
        );
        let outputs: Vec<_> = produced
            .iter()
            .map(|e| match e {
                Effect::Fact(f) => Effect::Fact(fact_subst(f, binding)),
                Effect::Equal(a, b) => Effect::Equal(subst(a, binding), subst(b, binding)),
            })
            .collect();
        // Validate undeclared/mistyped ports in output effects as well.
        let mut output_contract = relation.clone();
        for e in produced {
            match e {
                Effect::Fact(f) => output_contract.facts.push(f.clone()),
                Effect::Equal(a, b) => output_contract.equalities.push((a.clone(), b.clone())),
            }
        }
        let output_gap = output_contract.gap(
            &crate::effect_program::Summary::identity(BTreeMap::new()),
            binding,
        )?;
        let grounded = gap.unbound.is_empty() && output_gap.unbound.is_empty();
        for e in &outputs {
            Self::validate_effect(e, grounded)?;
        }
        let key=json!({"relation":relation.canonical_key()?,"binding":binding.iter().map(|(i,t)|json!([i,t.json()])).collect::<Vec<_>>()}).to_string();
        self.application(
            parent,
            Application {
                rule: rule.into(),
                binding: key,
                requirements,
                produced: outputs,
                grounded,
            },
        )
    }
    fn application(&mut self, parent: Context, a: Application) -> Result<Context> {
        let p = self.name(parent)?;
        let req = self.effects(&a.requirements)?;
        let prod = self.effects(&a.produced)?;
        let grounded = a.grounded;
        let c = self.insert(
            format!(
                "(Apply {p} {} {} {req} {prod})",
                quote(&a.rule),
                quote(&a.binding)
            ),
            vec![parent],
            Some(a),
        )?;
        if grounded {
            self.run(&format!("(Grounded {})", self.name(c)?))?;
        }
        Ok(c)
    }
    fn depends_on(&self, c: Context, target: Context) -> bool {
        let mut todo = vec![c];
        let mut seen = BTreeSet::new();
        while let Some(c) = todo.pop() {
            if c == target {
                return true;
            }
            if seen.insert(c) {
                todo.extend(&self.records[c.0].parents);
            }
        }
        false
    }
    /// Remove one direct Join parent for this application only. No context union.
    pub fn without_join_parent(&mut self, app: Context, removed: Context) -> Result<Context> {
        let rec = self.records.get(app.0).ok_or("invalid application")?;
        let data = rec.application.clone().ok_or("not an application")?;
        let parent = rec.parents[0];
        let ps = &self.records[parent.0].parents;
        if self.records[parent.0].application.is_some() || ps.len() != 2 {
            return Err("application parent must be a binary Join".into());
        }
        let remaining = if ps[0] == removed {
            ps[1]
        } else if ps[1] == removed {
            ps[0]
        } else {
            return Err("not a direct join parent".into());
        };
        let candidate = self.application(remaining, data)?;
        self.run(&format!(
            "(ProposedRemoval {} {} {})",
            self.name(app)?,
            self.name(candidate)?,
            self.name(removed)?
        ))?;
        if !self.depends_on(remaining, removed) {
            self.run(&format!(
                "(Independent {} {})",
                self.name(remaining)?,
                self.name(removed)?
            ))?;
        }
        self.proposals.push((app, candidate, removed));
        Ok(candidate)
    }
    pub fn saturate(&mut self) -> Result<()> {
        self.run("(run-schedule (saturate (run tier1)))")
    }
    fn check(&mut self, q: &str) -> Result<bool> {
        match self.eg.parse_and_run_program(None, &format!("(check {q})")) {
            Ok(_) => Ok(true),
            Err(egglog::Error::CheckError(..)) => Ok(false),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn ready(&mut self, c: Context) -> Result<bool> {
        self.check(&format!("(Ready {})", self.name(c)?))
    }
    pub fn satisfies(&mut self, c: Context, e: &Effect) -> Result<bool> {
        Self::validate_effect(e, true)?;
        let e = self.effect(e)?;
        self.saturate()?;
        self.check(&format!("(Satisfies {} {e})", self.name(c)?))
    }
    pub fn redundant(&mut self, old: Context, new: Context, removed: Context) -> Result<bool> {
        self.check(&format!(
            "(RedundantForUse {} {} {})",
            self.name(old)?,
            self.name(new)?,
            self.name(removed)?
        ))
    }
    pub fn report(&mut self) -> Result<Value> {
        self.saturate()?;
        let mut rows = vec![];
        for i in 0..self.records.len() {
            let c = Context(i);
            let ready = self.ready(c)?;
            rows.push(json!({"context":i,"ready":ready,"given_effects":format!("{:?}",self.records[i].given),"given_data":self.records[i].given.iter().map(effect_json).collect::<Vec<_>>(),"application":self.records[i].application.as_ref().map(|a|json!({"rule":a.rule,"grounded":a.grounded,"binding":a.binding,"requirements":format!("{:?}",a.requirements),"produced":format!("{:?}",a.produced),"requirements_data":a.requirements.iter().map(effect_json).collect::<Vec<_>>(),"produced_data":a.produced.iter().map(effect_json).collect::<Vec<_>>()})),"parents":self.records[i].parents.iter().map(|c|c.0).collect::<Vec<_>>() }));
        }
        let mut certificates = vec![];
        for (old, new, removed) in self.proposals.clone() {
            certificates.push(json!({"old":old.0,"candidate":new.0,"removed":removed.0,"certified_redundant_for_use":self.redundant(old,new,removed)?}));
        }
        let serialized = self.eg.serialize(egglog::SerializeConfig {
            max_functions: None,
            max_calls_per_function: None,
            include_temporary_functions: true,
            root_eclasses: vec![],
        });
        assert!(serialized.is_complete());
        Ok(
            json!({"native_egraph":{"nodes":serialized.egraph.nodes.iter().map(|(id,n)|(id.to_string(),json!({"op":n.op,"children":n.children.iter().map(ToString::to_string).collect::<Vec<_>>(),"eclass":n.eclass.to_string(),"cost":n.cost.into_inner(),"subsumed":n.subsumed}))).collect::<BTreeMap<_,_>>(),"root_eclasses":serialized.egraph.root_eclasses.iter().map(ToString::to_string).collect::<Vec<_>>(),"class_data":serialized.egraph.class_data.iter().map(|(id,c)|{let mut data:BTreeMap<String,Value>=c.extra.iter().map(|(k,v)|(k.clone(),json!(v))).collect();data.insert("type".into(),json!(c.typ));(id.to_string(),data)}).collect::<BTreeMap<_,_>>()},"serialization_complete":true,"contexts":rows,"proposals":certificates,"scope":"actual native tier-1 egglog rules over supplied positive contracts; not tier-0 provenance extraction; no context union or global dependency deletion"}),
        )
    }
}
