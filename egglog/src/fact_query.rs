//! Reusable positive table queries. No user actions or result tables are installed.
use super::*;

/// A compiled table query, tied to the EGraph that compiled it. Do not execute on
/// an unrelated graph or concurrently on clones: the callback buffer is shared.
pub struct CompiledFactQuery {
    rule: egglog_bridge::RuleId,
    callback: ExternalFunctionId,
    rows: Arc<std::sync::Mutex<Vec<Vec<Value>>>>,
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
        let rows = Arc::new(std::sync::Mutex::new(Vec::new()));
        let buffer = rows.clone();
        let callback = self
            .backend
            .register_external_func(Box::new(make_external_func(move |_, args| {
                buffer.lock().unwrap().push(args.to_vec());
                Some(Value::new_const(0))
            })));
        let mut rb = self.backend.new_rule("read-only fact query", false);
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
        })
    }

    /// Reuse the compiled join. Advances backend maintenance timestamps but
    /// performs no logical writes, unions or user-rule execution.
    pub fn run_fact_query(&mut self, query: &CompiledFactQuery) -> Result<Vec<Vec<Value>>, Error> {
        query.rows.lock().unwrap().clear();
        self.backend
            .run_rules(&[query.rule])
            .map_err(|e| Error::BackendError(e.to_string()))?;
        Ok(std::mem::take(&mut *query.rows.lock().unwrap()))
    }

    pub fn free_fact_query(&mut self, query: CompiledFactQuery) {
        self.backend.free_rule(query.rule);
        self.backend.free_external_func(query.callback);
    }
}
