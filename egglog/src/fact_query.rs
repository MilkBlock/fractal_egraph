//! Reusable positive table queries. No user actions or result tables are installed.
use super::*;

/// A compiled table query, tied to the EGraph that compiled it. Do not execute on
/// an unrelated graph or concurrently on clones: the callback buffer is shared.
pub struct CompiledFactQuery {
    rule: egglog_bridge::RuleId,
    callback: ExternalFunctionId,
    rows: Arc<std::sync::Mutex<QueryRows>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Default)]
struct QueryRows {
    rows: Vec<Vec<Value>>,
    excluded: Option<std::collections::HashSet<Vec<Value>>>,
    matched: Option<Vec<Value>>,
    fresh: Option<Vec<Value>>,
}

impl EGraph {
    /// Compile a conjunctive query over visible function/relation rows.
    /// Atoms list inputs and optionally an output. Omitted outputs are existential
    /// variables (including the private constructor output of relations).
    /// Values are typed variables or literals; projection contains value indices.
    pub fn compile_fact_query(
        &mut self,
        values: &[(String, Option<Literal>)],
        atoms: &[(String, Vec<usize>)],
        projection: &[usize],
    ) -> Result<CompiledFactQuery, Error> {
        let error = |s: &str| Error::BackendError(s.to_string());
        let sorts = values
            .iter()
            .map(|(s, _)| {
                self.type_info
                    .get_sort_by_name(s)
                    .ok_or_else(|| error("unknown query sort"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (name, literal) in values {
            if let Some(literal) = literal {
                let expected = match literal {
                    Literal::Int(_) => "i64",
                    Literal::Float(_) => "f64",
                    Literal::String(_) => "String",
                    Literal::Bool(_) => "bool",
                    Literal::Unit => "Unit",
                };
                if name != expected {
                    return Err(error("query literal sort mismatch"));
                }
            }
        }
        let mut funcs = vec![];
        let mut bound = std::collections::BTreeSet::new();
        for (name, indices) in atoms {
            let f = self
                .functions
                .get(name)
                .ok_or_else(|| error("unknown query table"))?;
            let schema: Vec<_> = f
                .schema
                .input
                .iter()
                .chain(std::iter::once(&f.schema.output))
                .collect();
            if indices.len() != schema.len() && indices.len() + 1 != schema.len() {
                return Err(error("query atom arity mismatch"));
            }
            for (&i, sort) in indices.iter().zip(&schema) {
                if i >= sorts.len() || sorts[i].name() != sort.name() {
                    return Err(error(&format!(
                        "query atom {name} sort mismatch at {i}: declared {:?}, expected {}",
                        values.get(i),
                        sort.name()
                    )));
                }
                bound.insert(i);
            }
            funcs.push((
                f.backend_id,
                f.schema.output.column_ty(&self.backend),
                schema.len(),
            ));
        }
        if projection.iter().any(|i| !bound.contains(i)) {
            return Err(error("unbound query projection"));
        }
        let rows = Arc::new(std::sync::Mutex::new(QueryRows::default()));
        let buffer = rows.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let success = stop.clone();
        let callback = self
            .backend
            .register_external_func(Box::new(make_external_func(move |_, args| {
                let mut sink = buffer.lock().unwrap();
                if let Some(excluded) = &sink.excluded {
                    let fresh = sink.fresh.is_none() && !excluded.contains(args);
                    if fresh {
                        sink.fresh = Some(args.to_vec());
                        success.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                    if sink.matched.is_none() {
                        sink.matched = Some(args.to_vec());
                    }
                } else {
                    sink.rows.push(args.to_vec());
                }
                Some(Value::new_const(0))
            })));
        let mut rb = self.backend.new_rule("read-only fact query", false);
        rb.set_plan_strategy(core_relations::PlanStrategy::GjFlat);
        let entries: Vec<_> = values
            .iter()
            .zip(&sorts)
            .enumerate()
            .map(|(i, ((_, literal), sort))| match literal {
                Some(l) => literal_to_entry(rb.egraph(), l),
                None => rb.new_var_named(sort.column_ty(rb.egraph()), &format!("q{i}")),
            })
            .collect();
        for ((_, indices), (f, output_ty, arity)) in atoms.iter().zip(funcs) {
            let mut args: Vec<_> = indices.iter().map(|i| entries[*i].clone()).collect();
            if args.len() < arity {
                args.push(rb.new_var_named(output_ty, "existential_output"));
            }
            rb.query_table(f, &args, Some(false))
                .map_err(|e| error(&e.to_string()))?;
        }
        rb.call_external_func(
            callback,
            &projection
                .iter()
                .map(|i| entries[*i].clone())
                .collect::<Vec<_>>(),
            ColumnTy::Id,
            || "fact query collector cannot fail".into(),
        );
        Ok(CompiledFactQuery {
            rule: rb.build(),
            callback,
            rows,
            stop,
        })
    }

    /// Reuse the compiled join. Advances backend maintenance timestamps but
    /// performs no logical writes, unions or user-rule execution.
    pub fn run_fact_query(&mut self, query: &CompiledFactQuery) -> Result<Vec<Vec<Value>>, Error> {
        *query.rows.lock().unwrap() = QueryRows::default();
        self.backend
            .run_rules(&[query.rule])
            .map_err(|e| Error::BackendError(e.to_string()))?;
        Ok(std::mem::take(&mut query.rows.lock().unwrap().rows))
    }

    /// Exact existence and set-difference witnesses, without storing all results.
    /// Stops once a fresh tuple proves both predicates; negative results exhaust
    /// the join. Excluded tuples must use this graph's
    /// current canonical values and the same projection as the compiled query.
    pub fn run_fact_query_difference(
        &mut self,
        query: &CompiledFactQuery,
        excluded: std::collections::HashSet<Vec<Value>>,
    ) -> Result<(Option<Vec<Value>>, Option<Vec<Value>>), Error> {
        *query.rows.lock().unwrap() = QueryRows {
            excluded: Some(excluded),
            ..Default::default()
        };
        query
            .stop
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let result = self
            .backend
            .run_read_only_rules_until(&[query.rule], &query.stop)
            .map_err(|e| Error::BackendError(e.to_string()));
        let rows = std::mem::take(&mut *query.rows.lock().unwrap());
        result?;
        Ok((rows.matched, rows.fresh))
    }

    pub fn fact_query_unique_subset_bytes(&self, query: &CompiledFactQuery) -> Option<usize> {
        self.backend.rule_unique_subset_bytes(query.rule)
    }

    /// Diagnostic row-ID buffer capacities: [headers, atoms]. Not total heap/RSS.
    pub fn fact_query_subset_bytes(&self, query: &CompiledFactQuery) -> Option<[usize; 2]> {
        self.backend.rule_subset_bytes(query.rule)
    }

    pub fn free_fact_query(&mut self, query: CompiledFactQuery) {
        self.backend.free_rule(query.rule);
        self.backend.free_external_func(query.callback);
    }
}

/// Compiled ground conjunction. Valid on its compiling engine and descendant
/// clones created after compilation. Execute serially: the callback is shared.
pub struct CompiledCheck {
    rule: egglog_bridge::RuleId,
    callback: ExternalFunctionId,
    matched: egglog_bridge::SideChannel<()>,
    facts: Vec<ast::Fact>,
}
impl EGraph {
    pub fn compile_ground_check(&mut self, facts: &[ast::Fact]) -> Result<CompiledCheck, Error> {
        fn ground(e: &Expr) -> bool {
            match e {
                Expr::Var(..) => false,
                Expr::Call(_, _, args) => args.iter().all(ground),
                _ => true,
            }
        }
        if self.are_proofs_enabled()
            || !facts.iter().all(|f| match f {
                Fact::Eq(_, a, b) => ground(a) && ground(b),
                Fact::Fact(e) => ground(e),
            })
        {
            return Err(Error::BackendError(
                "cached checks require ground facts and proof mode disabled".into(),
            ));
        }
        let resolved = self.resolve_command(Command::Check(span!(), facts.to_vec()))?;
        let [ResolvedNCommand::Check(span, body)] = resolved.desugared.as_slice() else {
            return Err(Error::BackendError(
                "unexpected ground-check lowering".into(),
            ));
        };
        let rule = ast::ResolvedRule {
            span: span.clone(),
            head: ResolvedActions::default(),
            body: body.clone(),
            name: self.parser.symbol_gen.fresh("cached_check"),
            ruleset: String::new(),
            naive: false,
        };
        let core =
            rule.to_canonicalized_core_rule(&self.type_info, &mut self.parser.symbol_gen, true)?;
        let matched = egglog_bridge::SideChannel::default();
        let sink = matched.clone();
        let callback = self
            .backend
            .register_external_func(Box::new(make_external_func(move |_, _| {
                *sink.lock().unwrap() = Some(());
                Some(Value::new_const(0))
            })));
        let mut translator = BackendRule::new(
            self.backend.new_rule("cached ground check", false),
            &self.functions,
            &self.type_info,
            false,
        );
        // Match ordinary egglog check semantics, including subsumed rows.
        translator.query(&core.body, true);
        translator
            .rb
            .call_external_func(callback, &[], ColumnTy::Id, || {
                "check collector cannot fail".into()
            });
        Ok(CompiledCheck {
            rule: translator.build(),
            callback,
            matched,
            facts: facts.to_vec(),
        })
    }
    pub fn run_ground_check(&mut self, check: &CompiledCheck) -> Result<(), Error> {
        check.matched.lock().unwrap().take();
        self.backend
            .run_rules(&[check.rule])
            .map_err(|e| Error::BackendError(e.to_string()))?;
        if check.matched.lock().unwrap().take().is_some() {
            Ok(())
        } else {
            Err(Error::CheckError(check.facts.clone(), span!()))
        }
    }
    pub fn free_ground_check(&mut self, check: CompiledCheck) {
        self.backend.free_rule(check.rule);
        self.backend.free_external_func(check.callback);
    }
}
