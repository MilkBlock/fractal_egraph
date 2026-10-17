//! Cell-assisted execution: expose typed query wiring and reuse the original RHS.
//!
//! This is deliberately separate from `Scheduler`: a Scheduler has already paid
//! for the complete Join. Here a trusted Cell provider supplies proven bindings.
//! No original LHS query is compiled or executed by the action sink.
use super::*;
use egglog_bridge::{DefaultVal, FunctionConfig, FunctionId, MergeFn, RuleId, TableAction};

#[derive(Clone, Debug)]
pub enum CellOperand {
    Slot(usize),
    Constant { value: Value, text: String },
}

#[derive(Clone, Debug)]
pub struct CellQueryAtom {
    pub function: String,
    /// Arguments followed by the result. Equal slot numbers are equality tests.
    pub columns: Vec<CellOperand>,
    pub sorts: Vec<String>,
}

/// Compiled once, and owned by the same EGraph for its entire lifetime.
pub struct CellActionSink {
    pub name: String,
    pub atoms: Vec<CellQueryAtom>,
    vars: Vec<ResolvedVar>,
    decided: FunctionId,
    action: RuleId,
    body: crate::core::Query<ResolvedCall, ResolvedVar>,
}
impl CellActionSink {
    pub fn slots(&self) -> Vec<(&str, &str)> {
        self.vars.iter().map(|v| (v.name.as_str(), v.sort.name())).collect()
    }
    pub fn canonicalize(&self, eg: &EGraph, binding: &[Value]) -> Result<Vec<Value>, Error> {
        if binding.len() != self.vars.len() {
            return Err(Error::BackendError("Cell binding arity mismatch".into()));
        }
        Ok(binding.iter().zip(&self.vars)
            .map(|(v,s)|eg.get_canonical_value(*v,&s.sort)).collect())
    }
}

impl EGraph {
    /// Opt-in external action receipts for the Cell boundary index. Ordinary
    /// `run_program_with_trace` retains its existing scope and event counts.
    pub fn run_cell_input_with_trace(&mut self, commands:Vec<Command>, trace:&TraceSession) -> Result<Vec<CommandOutput>,Error> {
        let old = std::mem::replace(&mut self.cell_trace_actions,true);
        let result = self.run_program_with_trace(commands,trace);
        self.cell_trace_actions=old;
        result
    }
    /// Prepare the action half of an existing positive constructor/relation rule.
    /// Unsupported primitive predicates, globals, custom functions and nonpositive
    /// actions fail explicitly. This is not an optimizer for arbitrary eggcc rules.
    pub fn prepare_cell_action(&mut self, ruleset: &str, name: &str) -> Result<CellActionSink, Error> {
        let fail = |s: &str| Error::BackendError(s.into());
        if self.are_proofs_enabled() || self.proof_state.original_typechecking.is_some() {
            return Err(fail("Cell assistance does not support proof/term encoding"));
        }
        let Some(Ruleset::Rules(rules)) = self.rulesets.get(ruleset) else {
            return Err(fail("Cell assistance requires a direct ruleset"));
        };
        let (rule, _) = rules.get(name).ok_or_else(||fail("unknown assisted rule"))?;
        let rule = rule.clone();
        let vars: Vec<_> = rule.body.get_vars().into_iter().collect();
        let mut atoms = vec![];
        for atom in &rule.body.atoms {
            let ResolvedCall::Func(f) = &atom.head else {
                return Err(fail("primitive LHS requires an explicit residual implementation"));
            };
            let function = &self.functions[&f.name];
            if function.decl.subtype == FunctionSubtype::Custom || function.decl.internal_let {
                return Err(fail("custom function/global evidence is not supported"));
            }
            let columns = atom.args.iter().map(|a| match a {
                ResolvedAtomTerm::Var(_,v) => Ok(CellOperand::Slot(vars.iter().position(|x|x==v).unwrap())),
                ResolvedAtomTerm::Literal(_,l) => Ok(CellOperand::Constant {
                    value: literal_to_value(&self.backend,l), text:l.to_string(),
                }),
                _ => Err(fail("global query values are not supported")),
            }).collect::<Result<Vec<_>,_>>()?;
            atoms.push(CellQueryAtom {function:f.name.clone(), columns,
                sorts:atom.args.iter().map(|a|a.output().name().to_string()).collect()});
        }
        for action in &rule.head.0 {
            use crate::core::GenericCoreAction as A;
            match action {
                A::Union(..) | A::LetAtomTerm(..) => (),
                A::Let(_,_,ResolvedCall::Func(f),_) | A::Set(_,ResolvedCall::Func(f),_,_)
                    if self.functions[&f.name].decl.subtype != FunctionSubtype::Custom => (),
                _ => return Err(fail("only positive constructor/relation RHS and union are supported")),
            }
        }
        if !rule.head.get_free_vars().iter().all(|v|vars.contains(v)) {
            return Err(fail("RHS has a value absent from the query interface"));
        }
        let unit = self.backend.base_values().get(());
        let decided = self.backend.add_table(FunctionConfig {
            schema:vars.iter().map(|v|v.sort.column_ty(&self.backend))
                .chain([ColumnTy::Base(self.backend.base_values().get_ty::<()>())]).collect(),
            default:DefaultVal::Const(unit), merge:MergeFn::AssertEq,
            name:format!("cell-bindings:{name}"), can_subsume:false,
        });
        let unit_type = self.backend.base_values().get_ty::<()>();
        let mut builder = BackendRule::new(self.backend.new_rule(&format!("cell-action:{name}"),false),
            &self.functions,&self.type_info,false);
        let entries: Vec<_> = vars.iter().map(|v|builder.entry(&ResolvedAtomTerm::Var(span!(),v.clone()))).collect();
        let mut query = entries.clone();
        query.push(QueryEntry::Const {val:unit,ty:ColumnTy::Base(unit_type)});
        builder.rb.query_table(decided,&query,None).unwrap();
        builder.actions(&rule.head)?;
        builder.rb.remove(decided,&entries);
        let action = builder.build();
        Ok(CellActionSink {name:name.into(),atoms,vars,decided,action,body:rule.body})
    }

    /// Diagnostic oracle ONLY: runs the full native query, without its actions.
    /// Never use this to produce assisted candidates or include it in timing runs.
    pub fn audit_cell_matches(&mut self, sink:&CellActionSink) -> Result<Vec<Vec<Value>>,Error> {
        let rows=Arc::new(std::sync::Mutex::new(Vec::new()));
        let output=rows.clone();
        let unit=self.backend.base_values().get(());
        let unit_type=self.backend.base_values().get_ty::<()>();
        let external=self.backend.register_external_func(Box::new(make_external_func(move |_,values| {
            output.lock().unwrap().push(values.to_vec());Some(unit)
        })));
        let mut builder=BackendRule::new(self.backend.new_rule("cell-audit",false),&self.functions,&self.type_info,false);
        builder.query(&sink.body,true);
        let entries=sink.vars.iter().map(|v|builder.entry(&ResolvedAtomTerm::Var(span!(),v.clone()))).collect::<Vec<_>>();
        builder.rb.call_external_func(external,&entries,ColumnTy::Base(unit_type),||"cell audit".into());
        let id=builder.build();
        let result=self.backend.run_rules(&[id]);
        self.backend.free_rule(id);self.backend.free_external_func(external);
        result.map_err(|e|Error::BackendError(e.to_string()))?;
        let values=std::mem::take(&mut *rows.lock().unwrap());
        values.iter().map(|b|sink.canonicalize(self,b)).collect()
    }

    /// Trusted optimizer boundary, like `step_rules_with_completed_cells`.
    /// Caller must prove *all* LHS atoms for every complete typed substitution,
    /// with current visibility and canonical values, and exclusively own execution
    /// between checking and calling this method. Never pass raw GPU guesses here.
    /// The original actions, merge policy and rebuild machinery are reused.
    pub fn apply_cell_bindings(&mut self, sink: &CellActionSink, bindings: &[Vec<Value>]) -> Result<RunReport, Error> {
        self.apply_cell_bindings_with_trace(sink,bindings,None)
    }
    pub fn apply_cell_bindings_with_trace(&mut self, sink: &CellActionSink, bindings: &[Vec<Value>], trace:Option<&TraceSession>) -> Result<RunReport, Error> {
        let rows = bindings.iter().map(|b|sink.canonicalize(self,b)).collect::<Result<Vec<_>,_>>()?;
        let table = TableAction::new(&self.backend,sink.decided);
        self.backend.with_execution_state(|state| {
            let unit = state.base_values().get(());
            for b in &rows { table.insert(state,b.iter().copied().chain([unit])); }
        });
        self.backend.flush_updates();
        let report = match trace {
            Some(t)=>self.backend.run_rules_with_trace(&[sink.action],t),
            None=>self.backend.run_rules(&[sink.action]),
        }.map_err(|e|Error::BackendError(e.to_string()))?;
        Ok(RunReport::singleton("cell-assisted",report))
    }

    /// Exact keyed read used to admit/revalidate Cell evidence. Includes native
    /// subsumption visibility; does not construct missing terms or scan a table.
    pub fn cell_visible_row(&self, name: &str, key: &[Value]) -> Option<Vec<Value>> {
        let f = self.functions.get(name)?;
        if key.len()!=f.schema.input.len() { return None; }
        let key:Vec<_>=key.iter().zip(&f.schema.input).map(|(v,s)|self.get_canonical_value(*v,s)).collect();
        let row = self.backend.lookup_row(f.backend_id,&key)?;
        let n=f.schema.input.len()+1;
        if f.can_subsume && row[n+1] != Value::new_const(0) { return None; }
        Some(row[..n].iter().zip(f.schema.input.iter().chain([&f.schema.output]))
            .map(|(v,s)|self.get_canonical_value(*v,s)).collect())
    }

    pub fn cell_table_name(&self, table: core_relations::TableId) -> Option<&str> {
        self.functions.values().find(|f|self.backend.function_table_id(f.backend_id)==table).map(|f|f.name())
    }

    /// Native rules not assigned to the assisted lane. Explicit names keep the
    /// two lanes disjoint; this method does not advance omitted rules' timestamps.
    pub fn step_cell_native_rules(&mut self, ruleset:&str, names:&[String], trace:Option<&TraceSession>) -> Result<RunReport,Error> {
        let Some(Ruleset::Rules(rules))=self.rulesets.get(ruleset) else {
            return Err(Error::BackendError("expected a direct ruleset".into()));
        };
        let ids=names.iter().map(|n|rules.get(n).map(|(_,id)|*id)
            .ok_or_else(||Error::BackendError(format!("unknown native rule {n}"))))
            .collect::<Result<Vec<_>,_>>()?;
        let report=match trace {
            Some(t)=>self.backend.run_rules_with_trace(&ids,t),
            None=>self.backend.run_rules(&ids),
        }.map_err(|e|Error::BackendError(e.to_string()))?;
        Ok(RunReport::singleton(ruleset,report))
    }
}
