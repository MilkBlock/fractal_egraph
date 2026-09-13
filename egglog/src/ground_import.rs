//! Restricted ground-data loader. Uses the same TableAction API as `(input)`.
//! No rules, primitives, globals, union or custom merge functions are executed.
use super::*;
use egglog_bridge::TableAction;

enum Ground {
    Literal(Value),
    Call(TableAction, Vec<Ground>),
}
impl Ground {
    fn eval(&self, state: &mut ExecutionState<'_>) -> Result<Value, String> {
        match self {
            Self::Literal(v) => Ok(*v),
            Self::Call(table, args) => {
                let keys = args
                    .iter()
                    .map(|a| a.eval(state))
                    .collect::<Result<Vec<_>, _>>()?;
                table
                    .lookup_or_insert(state, &keys)
                    .ok_or_else(|| "missing ground function key".into())
            }
        }
    }
}
enum Import {
    Expr(Ground),
    Set(TableAction, Vec<Ground>, Ground),
}
impl EGraph {
    /// Load ground constructor expressions and sets on no-merge functions.
    /// Checks all schemas before writing; each top-level action commits before
    /// the next, so lookups see preceding sets. This is not a rule/trace API.
    /// Like ordinary action execution, a runtime error is not transactional.
    pub fn run_ground_import(&mut self, commands: &[Command]) -> Result<(), Error> {
        self.ground_import(commands).map_err(Error::BackendError)
    }
    fn prepare_ground(&self, e: &Expr) -> Result<(ArcSort, Ground), String> {
        match e {
            Expr::Lit(_, lit) => {
                let sort = match lit {
                    Literal::Int(_) => "i64",
                    Literal::String(_) => "String",
                    _ => return Err("unsupported ground literal".into()),
                };
                Ok((
                    self.type_info
                        .sorts
                        .get(sort)
                        .ok_or("missing base sort")?
                        .clone(),
                    Ground::Literal(literal_to_value(&self.backend, lit)),
                ))
            }
            Expr::Call(_, name, args) => {
                let f = self
                    .functions
                    .get(name)
                    .ok_or_else(|| format!("unknown ground function {name}"))?;
                if f.decl.merge.is_some()
                    || f.decl.internal_let
                    || f.decl.term_constructor.is_some()
                {
                    return Err("unsupported ground function semantics".into());
                }
                let args = self.prepare_ground_args(args, &f.schema.input)?;
                Ok((
                    f.schema.output.clone(),
                    Ground::Call(TableAction::new(&self.backend, f.backend_id), args),
                ))
            }
            _ => Err("ground import does not accept variables".into()),
        }
    }
    fn prepare_ground_args(&self, args: &[Expr], sorts: &[ArcSort]) -> Result<Vec<Ground>, String> {
        if args.len() != sorts.len() {
            return Err("ground arity mismatch".into());
        }
        args.iter()
            .zip(sorts)
            .map(|(arg, expected)| {
                let (sort, arg) = self.prepare_ground(arg)?;
                if sort.name() != expected.name() {
                    return Err("ground sort mismatch".into());
                }
                Ok(arg)
            })
            .collect()
    }
    fn ground_import(&mut self, commands: &[Command]) -> Result<(), String> {
        if self.program_trace.is_some()
            || self.proof_state.original_typechecking.is_some()
            || self.are_proofs_enabled()
        {
            return Err("ground import is unavailable in trace/proof/encoding mode".into());
        }
        let plan = commands
            .iter()
            .map(|command| match command {
                Command::Action(Action::Expr(_, expr)) => {
                    Ok(Import::Expr(self.prepare_ground(expr)?.1))
                }
                Command::Action(Action::Set(_, name, args, value)) => {
                    let f = self
                        .functions
                        .get(name)
                        .ok_or("unknown ground set function")?;
                    if f.decl.subtype != FunctionSubtype::Custom
                        || f.decl.merge.is_some()
                        || f.decl.internal_let
                        || f.decl.term_constructor.is_some()
                    {
                        return Err("ground set requires a no-merge function".into());
                    }
                    let args = self.prepare_ground_args(args, &f.schema.input)?;
                    let (sort, value) = self.prepare_ground(value)?;
                    if sort.name() != f.schema.output.name() {
                        return Err("ground output sort mismatch".into());
                    }
                    Ok(Import::Set(
                        TableAction::new(&self.backend, f.backend_id),
                        args,
                        value,
                    ))
                }
                _ => Err("unsupported ground import command".into()),
            })
            .collect::<Result<Vec<_>, String>>()?;
        for action in plan {
            let result = self.backend.with_execution_state(|state| match action {
                Import::Expr(expr) => expr.eval(state).map(|_| ()),
                Import::Set(table, args, value) => {
                    let mut row = args
                        .iter()
                        .map(|a| a.eval(state))
                        .collect::<Result<Vec<_>, _>>()?;
                    let value = value.eval(state)?;
                    if table.lookup(state, &row).is_some_and(|old| old != value) {
                        return Err("conflicting no-merge ground set".into());
                    }
                    row.push(value);
                    table.insert(state, row.into_iter());
                    Ok(())
                }
            });
            self.backend.flush_updates();
            result?;
        }
        Ok(())
    }
}
