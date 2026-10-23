//! Portable, checked port programs. Loading validates existing wiring; it does
//! not discover a plan, run Analyze, saturate a training graph or load history.
use super::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Operand {
    Slot(usize),
    Literal(String),
}
impl From<&CellOperand> for Operand {
    fn from(o: &CellOperand) -> Self {
        match o {
            CellOperand::Slot(i) => Self::Slot(*i),
            CellOperand::Constant { text, .. } => Self::Literal(text.clone()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Atom {
    table: String,
    sorts: Vec<String>,
    columns: Vec<Operand>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Shape {
    pub(super) slots: Vec<(String, String)>,
    atoms: Vec<Atom>,
    guards: Vec<(u32, [Operand; 2])>,
    native_residuals: bool,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    primitives: Vec<(u32,[Operand;2],Option<Operand>)>,
}
impl Shape {
    pub(super) fn of(s: &CellActionSink) -> Self {
        Self {
            slots: s
                .slots()
                .into_iter()
                .map(|(n, t)| (n.into(), t.into()))
                .collect(),
            atoms: s
                .atoms
                .iter()
                .map(|a| Atom {
                    table: a.function.clone(),
                    sorts: a.sorts.clone(),
                    columns: a.columns.iter().map(Operand::from).collect(),
                })
                .collect(),
            guards: s
                .guards
                .iter()
                .map(|g| (g.opcode, g.operands.each_ref().map(Operand::from)))
                .collect(),
            native_residuals: s.native_residuals,
            primitives:s.primitives.iter().map(|p|(p.opcode,p.args.each_ref().map(Operand::from),p.result.as_ref().map(Operand::from))).collect(),
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Program {
    pub ruleset: String,
    pub rule: String,
    shape: Shape,
    seed: usize,
    steps: Vec<(usize, Access)>,
}
impl Program {
    pub(super) fn root(&self) -> Option<&str> { self.shape.atoms.get(self.seed).map(|a|a.table.as_str()) }
    pub(super) fn instantiate(
        &self,
        eg: &EGraph,
        sink: CellActionSink,
        tables: Vec<usize>,
    ) -> Result<Kernel> {
        if self.rule != sink.name || self.shape != Shape::of(&sink) {
            return Err(format!("transfer shape/guard mismatch: {}", self.rule).into());
        }
        let seed = sink.atoms.get(self.seed).ok_or("invalid transfer seed")?;
        let mut bound = vec![false; sink.slots().len()];
        let bind = |columns: &[CellOperand], bound: &mut Vec<bool>| {
            for v in columns {
                if let CellOperand::Slot(i) = v {
                    bound[*i] = true;
                }
            }
        };
        bind(&seed.columns, &mut bound);
        let mut seen = BTreeSet::from([self.seed]);
        for &(i, access) in &self.steps {
            let a = sink.atoms.get(i).ok_or("invalid transfer atom")?;
            if !seen.insert(i) {
                return Err("duplicate transfer atom".into());
            }
            let n = a.columns.len() - 1;
            let valid = match access {
                Access::Key => a.columns[..n].iter().all(|o| known(o, &bound)),
                Access::Output => {
                    eg.cell_sort_is_equality(&a.sorts[n])? && known(&a.columns[n], &bound)
                }
                Access::Columns(mask) => mask != 0 && a.columns.len() <= 64
                    && (a.columns.len() == 64 || mask >> a.columns.len() == 0)
                    && a.columns.iter().enumerate().all(|(j,o)|
                        mask & (1u64 << j) == 0 || (a.sorts[j] != "Unit" && known(o, &bound))),
            };
            if !valid {
                return Err(format!("unbound transfer read in {}", self.rule).into());
            }
            bind(&a.columns, &mut bound);
        }
        if seen.len() != sink.atoms.len() || bound.iter().any(|b| !b) {
            return Err("transfer omits an atom or a binding".into());
        }
        Ok(Kernel::from_wiring(
            sink,
            tables,
            self.seed,
            self.steps.clone(),
        ))
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    rulesets: Vec<String>,
    schedules: Vec<String>,
    #[serde(default,skip_serializing_if="Option::is_none")]
    selection: Option<contract::Selection>,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    scalars:Vec<scalar::Descriptor>,
    #[serde(default,skip_serializing_if="Vec::is_empty")]
    queries:Vec<generic::Query>,
}
pub struct Bundle {
    header: Header,
    programs: Vec<Program>,
    environment: String,
    pub evidence: Json,
}
fn declarations_only(c: &Command) -> bool {
    matches!(
        c,
        Command::Datatype { .. }
            | Command::Datatypes { .. }
            | Command::Sort { .. }
            | Command::Constructor { .. }
            | Command::Function { .. }
            | Command::Relation { .. }
            | Command::AddRuleset(..)
            | Command::Rule { .. }
    )
}
impl Bundle {
    /// Offline compiler for the currently checked terms family. Evidence is
    /// supplied by Analyze, never synthesized by the loader.
    pub fn bake_terms(environment: &str, schedules: Vec<String>, evidence: Json) -> Result<Self> {
        let mut parser = EGraph::default();
        let commands = parser.parse_program(None, environment)?;
        if !commands.iter().all(declarations_only) {
            return Err("transfer environment must not contain training facts or schedules".into());
        }
        let runtime = Runtime::new_joint_commands(commands)?;
        if !runtime.scalar.is_empty() {
            return Err("scalar transfer export is not supported by v1".into());
        }
        let mut programs = vec![];
        for (set, ids) in &runtime.sets {
            for id in ids {
                let k = &runtime.kernels[*id];
                programs.push(Program {
                    ruleset: set.clone(),
                    rule: k.sink.name.clone(),
                    shape: Shape::of(&k.sink),
                    seed: k.seed,
                    steps: k.steps.clone(),
                });
            }
        }
        Ok(Self {
            header: Header {
                version: 1,
                rulesets: runtime.sets.keys().cloned().collect(),
                schedules,
                selection: None,
                scalars: vec![],
                queries: vec![],
            },
            programs,
            environment: environment.into(),
            evidence,
        })
    }
    /// Typed parameterized export. Selection/scalar proofs retain their own
    /// checkers; observed general queries receive complete-query authority only.
    pub fn bake_certificates(environment:&str,schedules:Vec<String>,mut evidence:Json)->Result<Self> {
        let mut eg=EGraph::default();let commands=eg.parse_program(None,environment)?;
        if !commands.iter().all(declarations_only){return Err("certificate environment contains executable input".into())}
        eg.run_program(commands.clone())?;
        let selection=contract::Selection::discover(&eg,&commands).ok();
        let excluded=selection.as_ref().map(|s|s.sets.to_vec()).unwrap_or_default();
        let scalar=scalar::discover(&eg,&commands,&excluded);
        let scalars=scalar.iter().map(scalar::Spec::descriptor).collect::<Vec<_>>();
        let reserved=commands.iter().filter_map(|c|match c {
            Command::Rule{rule} if excluded.contains(&rule.ruleset)=>Some((rule.ruleset.clone(),rule.name.clone())),_=>None
        }).chain(scalars.iter().map(|s|(s.set.clone(),s.name.clone()))).collect();
        let (queries,rejected)=generic::discover(&mut eg,&evidence,&reserved);
        if selection.is_none() && scalar.is_empty() && queries.is_empty(){return Err("no checked parameterized closure scheme or observed port program; retain native execution".into())}
        if evidence.is_object(){evidence["query_export_rejections"]=json!(rejected);}
        let r=Runtime::certified_engine_queries(eg,&commands,selection.clone(),scalar,None,&queries)?;
        let mut programs=vec![];
        for (i,k) in r.kernels.iter().enumerate(){
            let set=r.sets.iter().find_map(|(s,ids)|ids.contains(&i).then(||s.clone()))
                .or_else(||r.scalar.iter().find_map(|(s,g)|g.plans.iter().any(|(id,_)|*id==i).then(||s.clone())))
                .or_else(||r.boundary.iter().find_map(|(s,ids)|ids.contains(&i).then(||s.clone())))
                .ok_or("unowned certificate query")?;
            programs.push(Program{ruleset:set,rule:k.sink.name.clone(),shape:Shape::of(&k.sink),seed:k.seed,steps:k.steps.clone()});
        }
        Ok(Self{header:Header{version:if queries.is_empty(){2}else{3},rulesets:r.sets.keys().cloned().collect(),schedules,selection,scalars,queries},
            programs,environment:environment.into(),evidence})
    }
    pub fn text(&self) -> Result<String> {
        let mut s = format!(
            "; @ripsat-certificates {{\"version\":1,\"kind\":\"parameterized\"}}\n; Parameterized Cell transfers. Native declarations/actions below; no training facts.\n; @cell-transfer {}\n",
            serde_json::to_string(&self.header)?
        );
        for p in &self.programs {
            s.push_str(&format!("; @cell-program {}\n", serde_json::to_string(p)?));
        }
        s.push_str(&format!(
            "; @cell-evidence {}\n\n{}\n",
            serde_json::to_string(&self.evidence)?,
            self.environment
        ));
        Ok(s)
    }
    pub fn parse(text: &str) -> Result<Self> {
        let mut header = None;
        let mut evidence = None;
        let mut programs = vec![];
        for line in text.lines().map(str::trim) {
            if let Some(s) = line.strip_prefix("; @cell-transfer ") {
                if header.replace(serde_json::from_str::<Header>(s)?).is_some() {
                    return Err("duplicate transfer header".into());
                }
            } else if let Some(s) = line.strip_prefix("; @cell-program ") {
                programs.push(serde_json::from_str(s)?);
            } else if let Some(s) = line.strip_prefix("; @cell-evidence ") {
                if evidence.replace(serde_json::from_str(s)?).is_some() {
                    return Err("duplicate evidence".into());
                }
            }
        }
        let header = header.ok_or("missing transfer header")?;
        if ![1,2,3].contains(&header.version) {
            return Err("unsupported transfer version".into());
        }
        let environment = text
            .lines()
            .filter(|line| !line.trim().starts_with("; @cell-"))
            .collect::<Vec<_>>()
            .join("\n");
        Ok(Self {
            header,
            programs,
            environment,
            evidence: evidence.ok_or("missing Analyze evidence")?,
        })
    }
    pub fn load(path: &Path) -> Result<Self> {
        Self::parse(&std::fs::read_to_string(path)?)
    }
    fn validate_header(&self) -> Result<()> {
        if self.programs.is_empty()
            || (self.header.rulesets.is_empty() && self.header.scalars.is_empty() && self.header.queries.is_empty())
            || self
                .programs
                .iter()
                .map(|p| (&p.ruleset,&p.rule))
                .collect::<BTreeSet<_>>()
                .len()
                != self.programs.len()
            || self.header.rulesets.iter().collect::<BTreeSet<_>>().len()
                != self.header.rulesets.len()
        {
            return Err("missing or duplicate transfer programs/rulesets".into());
        }
        if self.header.version < 3 && !self.header.queries.is_empty(){return Err("generic queries require transfer version 3".into())}
        let query_ids=self.header.queries.iter().map(|q|(&q.ruleset,&q.rule)).collect::<BTreeSet<_>>();
        if query_ids.len()!=self.header.queries.len(){return Err("duplicate query scheme".into())}
        for q in &self.header.queries {
            if self.header.rulesets.contains(&q.ruleset) || self.header.scalars.iter().any(|s|s.set==q.ruleset && s.name==q.rule)
                || !self.programs.iter().any(|p|p.ruleset==q.ruleset && p.rule==q.rule && p.root()==Some(q.root.as_str())) {
                return Err("missing, overlapping or mismatched query scheme".into())
            }
        }
        if self.header.version >= 2 {
            let sets = self.header.selection.as_ref().map(|s|s.sets.iter().cloned().collect::<BTreeSet<_>>()).unwrap_or_default();
            if self.header.rulesets != sets.into_iter().collect::<Vec<_>>() {
                return Err("certificate ruleset coverage differs".into());
            }
        }
        Ok(())
    }
    pub fn instantiate(&self) -> Result<Runtime> {
        self.validate_header()?;
        let mut eg = EGraph::default();
        let cmds = eg.parse_program(None, &self.environment)?;
        if !cmds.iter().all(declarations_only) {
            return Err("transfer file contains executable setup/training input".into());
        }
        eg.run_program(cmds.clone())?;
        if self.header.version>=2 {
            let excluded=self.header.selection.as_ref().map(|s|s.sets.to_vec()).unwrap_or_default();
            if self.header.rulesets!=excluded.iter().cloned().collect::<BTreeSet<_>>().into_iter().collect::<Vec<_>>() {
                return Err("certificate ruleset coverage differs".into());
            }
            let specs=scalar::restore(&eg,&cmds,&excluded,&self.header.scalars)?;
            return Runtime::certified_engine_queries(eg,&cmds,self.header.selection.clone(),specs,Some(&self.programs),&self.header.queries);
        }
        if self.header.selection.is_some() || !self.header.scalars.is_empty(){return Err("closure authority requires certificate version 2".into())}
        let sets = self
            .header
            .rulesets
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        Runtime::new_with_plans(eg, &sets, &BTreeMap::new(), vec![], Some(&self.programs))
    }
    /// Attach checked schemes to an already initialized native engine. Native
    /// query shapes and the complete CURRENT closure universe are revalidated.
    pub fn attach(&self,eg:EGraph,commands:&[Command])->Result<Runtime>{
        self.validate_header()?;
        if self.header.version<2{return Err("runtime attachment needs a version-2/3 certificate library".into())}
        let excluded=self.header.selection.as_ref().map(|s|s.sets.to_vec()).unwrap_or_default();
        let specs=scalar::restore(&eg,commands,&excluded,&self.header.scalars)?;
        Runtime::certified_engine_queries(eg,commands,self.header.selection.clone(),specs,Some(&self.programs),&self.header.queries)
    }
    /// Keep an existing finite positive closure proof in preference to a mere
    /// query plan. Numeric inputs require the symbolic slot program instead of
    /// reusing a finite graph with training indices frozen in its rows.
    pub fn prefers_finite(&self)->bool {
        self.header.selection.is_none() && self.header.scalars.is_empty()
            && self.programs.iter().all(|p|p.shape.slots.iter().all(|(_,s)|s!="i64" && s!="f64"))
    }
    pub fn report(&self) -> Json {
        let queries=self.header.queries.iter().map(|q|{
            let p=self.programs.iter().find(|p|p.ruleset==q.ruleset && p.rule==q.rule).unwrap();
            json!({"ruleset":q.ruleset,"rule":q.rule,"root":q.root,"strength":"complete_query",
                "numeric_slots":p.shape.slots.iter().enumerate().filter(|(_,(_,s))|s=="i64" || s=="f64").map(|(i,(n,s))|json!({"slot":i,"name":n,"sort":s})).collect::<Vec<_>>(),
                "guards":p.shape.guards,"atoms":p.shape.atoms,"steps":p.steps,
                "effects":"original native RHS, including checked arithmetic and intermediate successor facts"})
        }).collect::<Vec<_>>();
        json!({"selection":self.header.selection,"scalar_schemes":self.header.scalars,"query_schemes":queries,"version":self.header.version,"programs":self.programs.len(),"rulesets":self.header.rulesets,
            "schedules":self.header.schedules,"evidence":self.evidence,
            "scope":"Loaded fixed port wiring; original native guards/actions/merge. No history, concrete closure cache, or online wiring discovery. Native installation still compiles schema and action sinks."})
    }
}
impl Runtime {
    pub fn from_transfer(path: &Path) -> Result<Self> {
        Bundle::load(path)?.instantiate()
    }
}
