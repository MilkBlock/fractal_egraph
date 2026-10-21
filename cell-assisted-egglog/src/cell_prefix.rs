//! Analyze-backed, finite Cell projections. These are positive witnesses, NOT
//! closed-world assumptions: the GPU must also certify that each fully keyed local
//! read has a current visible witness before replacing that join prefix.
use crate::{Part, Result, Template, json, project_mode};
use egglog::{
    EGraph,
    ast::{Expr, Literal},
    cell_assist::CellActionSink,
};
use serde_json::{Value as Json, json as value};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct Library {
    templates: Vec<Template>,
    spines: RefCell<crate::spine_projection::Index>,
    shared_spines: bool,
    pub evidence: Json,
}

#[cfg(test)]
mod spine_tests {
    use super::*;

    fn setup() -> (EGraph, Library, Vec<CellActionSink>) {
        let mut eg = EGraph::default();
        eg.parse_and_run_program(
            None,
            r#"
          (datatype Expr (Leaf i64) (Single Expr) (Get Expr i64) (Box Expr))
          (relation Boundary (Expr i64)) (relation Hit (Expr Expr i64))
          (rule ((= s (Single x)) (= y (Get s 0)) (Boundary s n))
                ((Hit x y n)) :name "first")
          (rule ((Boundary z k) (= v (Get z 0)) (= z (Single u)) (> k 10))
                ((Hit v u k)) :name "renamed")
          (rule ((= s (Single x)) (= x (Get s 0)) (Boundary s n))
                ((Hit x x n)) :name "alias")
          (rule ((= s (Single x)) (= y (Get s 1)) (Boundary s n))
                ((Hit x y n)) :name "literal")
          (rule ((= s (Single x)) (= y (Get s 0)) (= b (Box y)))
                ((Hit x b 0)) :name "longer")
        "#,
        )
        .unwrap();
        let lib = Library::load(
            &mut eg,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/analyze"),
        )
        .unwrap();
        let qs = ["first", "renamed", "alias", "literal", "longer"]
            .into_iter()
            .map(|name| eg.prepare_gpu_cell_action("", name).unwrap())
            .collect();
        (eg, lib, qs)
    }
    fn parts(q: &CellActionSink, ops: &[&str]) -> Vec<usize> {
        ops.iter()
            .map(|op| q.atoms.iter().position(|a| a.function == *op).unwrap())
            .collect()
    }
    fn checks(lib: &Library) -> usize {
        let work = &lib.spines.borrow().work;
        work.anchor_row_checks + work.keyed_row_checks
    }
    #[test]
    fn alpha_renamed_consumers_share_spine_without_merging_aliases_or_guards() {
        let (_, lib, qs) = setup();
        for q in &qs[..2] {
            let p = parts(q, &["Single", "Get"]);
            let expected = lib.scan_alternatives(q, &p);
            assert!(!expected.is_empty());
            assert_eq!(lib.alternatives(q, &p), expected);
        }
        assert_eq!(lib.spines.borrow().work.markers_built, 2);
        let n = checks(&lib);
        for q in &qs[..2] {
            let p = parts(q, &["Single", "Get"]);
            assert_eq!(lib.alternatives(q, &p), lib.scan_alternatives(q, &p));
        }
        assert_eq!(checks(&lib), n, "no new row inspection for the same spine");
        for q in &qs[2..] {
            let p = parts(q, &["Single", "Get"]);
            assert_eq!(lib.alternatives(q, &p), lib.scan_alternatives(q, &p));
        }
        assert!(
            lib.alternatives(&qs[3], &parts(&qs[3], &["Single", "Get"]))
                .is_empty()
        );
        assert_eq!(
            lib.spines.borrow().work.markers_built,
            4,
            "alias and constant change the marker"
        );
        // Extending an already known spine only explores the added atom.
        let before_extension = checks(&lib);
        let p = parts(&qs[4], &["Single", "Get", "Box"]);
        assert_eq!(
            lib.alternatives(&qs[4], &p),
            lib.scan_alternatives(&qs[4], &p)
        );
        assert_eq!(
            checks(&lib),
            before_extension,
            "no Box rows; never reread Single/Get"
        );
    }

    #[test]
    fn indexed_spine_keeps_every_cell_root_visibility_and_binding_alternative() {
        let (mut eg, mut lib, qs) = setup();
        // Enlarge an analyzed state with many distinct symbolic instances. All
        // lie below other nodes; no distinguished-root assumption is permitted.
        let literal = lib.templates[0].literals.clone();
        let mut values = vec![value!({"sort":"i64","literal":"0"})];
        let mut rows = vec![];
        for _ in 0..64 {
            let x = values.len();
            values.push(value!({"sort":"Expr","literal":null}));
            let s = values.len();
            values.push(value!({"sort":"Expr","literal":null}));
            let b = values.len();
            values.push(value!({"sort":"Expr","literal":null}));
            rows.extend([
                value!({"op":"Single","args":[x],"result":s}),
                value!({"op":"Get","args":[s,0],"result":x}),
                value!({"op":"Box","args":[x],"result":b}),
            ]);
        }
        let mut t = lib.templates[0].clone();
        t.id = 7;
        t.state = value!({"values":values,"rows":rows,"subsumed_rows":[1]});
        t.literals = BTreeMap::from([(0, *literal.values().next().unwrap())]);
        lib.templates.push(t.clone());
        t.id = 8;
        lib.templates.push(t);
        lib.spines = RefCell::new(crate::spine_projection::Index::new(&lib.templates).unwrap());
        let q = &qs[4];
        let mut reference_checks = 0;
        for ops in [
            &["Single", "Get"][..],
            &["Single", "Get", "Box"][..],
            &["Get", "Box"][..],
        ] {
            let p = parts(q, ops);
            for t in &lib.templates {
                reference_checks += crate::project_mode_counted(
                    t,
                    q,
                    &Part {
                        atoms: p.clone(),
                        cell: true,
                    },
                    false,
                )
                .1;
            }
            assert_eq!(lib.alternatives(q, &p), lib.scan_alternatives(q, &p));
        }
        let indexed_checks = checks(&lib);
        assert!(indexed_checks < reference_checks);
        eprintln!(
            "synthetic 64-position closure projection: {reference_checks} -> {indexed_checks} binding/row checks"
        );
        let alt = lib.alternatives(q, &parts(q, &["Single", "Get", "Box"]));
        assert_eq!(
            alt.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>(),
            BTreeSet::from([7, 8])
        );
        // Types are part of the spine identity, even when table names coincide.
        let p = parts(q, &["Single", "Get"]);
        let mut wrong_sort = eg.prepare_gpu_cell_action("", "longer").unwrap();
        wrong_sort.atoms[p[0]].sorts[0] = "i64".into();
        assert!(lib.alternatives(&wrong_sort, &p).is_empty());
        // Test all legal ordered prefixes of native atoms against the old oracle.
        for q in &qs {
            for a in 0..q.atoms.len() {
                for b in 0..q.atoms.len() {
                    if a == b {
                        continue;
                    }
                    let expected = {
                        lib.shared_spines = false;
                        lib.alternatives(q, &[a, b])
                    };
                    lib.shared_spines = true;
                    assert_eq!(lib.alternatives(q, &[a, b]), expected);
                }
            }
        }
        // RipenInput rows are never native facts, even if a query mentions one.
        eg.parse_and_run_program(None,"(constructor RipenInput (i64) Expr) (rule ((= x (RipenInput n)) (= y (Single x))) ((Hit x y n)) :name \"marker\")").unwrap();
        let q = eg.prepare_gpu_cell_action("", "marker").unwrap();
        let p = parts(&q, &["RipenInput", "Single"]);
        assert!(lib.alternatives(&q, &p).is_empty());
    }

    #[test]
    fn native_family_program_is_identical_with_shared_projection_enabled() {
        let (mut eg, mut lib, qs) = setup();
        let names = qs
            .iter()
            .flat_map(|q| q.atoms.iter().map(|a| a.function.clone()))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        let tables = eg.subscribe_cell_commits(&names).unwrap().0.tables;
        lib.shared_spines = false;
        let old = crate::family_program::compile(
            &eg,
            &qs,
            &tables,
            Some(&lib),
            None,
            true,
            true,
            true,
            true,
            false,
        )
        .unwrap();
        lib.shared_spines = true;
        let new = crate::family_program::compile(
            &eg,
            &qs,
            &tables,
            Some(&lib),
            None,
            true,
            true,
            true,
            true,
            false,
        )
        .unwrap();
        for (key, value) in old.as_object().unwrap() {
            if key != "spine_projection" {
                assert_eq!(&new[key], value, "changed native/GPU contract: {key}");
            }
        }
        assert!(
            new["spine_projection"]["work"]["prefix_reuses"]
                .as_u64()
                .unwrap()
                > 0
        );
        eprintln!("shared spine compiler: {}", new["spine_projection"]);
    }
}
impl Library {
    pub(crate) fn receipt_tables(&self)->BTreeSet<String>{
        self.templates.iter().flat_map(|t|t.state["rows"].as_array().into_iter().flatten()
            .filter_map(|r|r["op"].as_str()).filter(|op|!t.markers.contains(*op)).map(str::to_owned)).collect()
    }
    /// Values are interned in the target engine, never copied as foreign IDs.
    /// Only complete states referenced by the analyze catalog are accepted.
    pub fn load(eg: &mut EGraph, dir: &Path) -> Result<Self> {
        let catalog = json(&dir.join("catalog/catalog.json"))?;
        if catalog["schema"] != "saturated-rule-composition-catalog/v1" {
            return Err("unsupported analyzed Cell catalog".into());
        }
        let mut states = BTreeMap::<usize, BTreeSet<String>>::new();
        for trigger in catalog["triggers"]
            .as_array()
            .ok_or("missing Cell triggers")?
        {
            let Some(id) = trigger["saturated_rule_composition"].as_u64() else {
                continue;
            };
            let origin = &trigger["binding_origin"];
            let markers = states.entry(id as usize).or_default();
            markers.extend(
                origin["parameter_markers"]
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.values())
                    .chain([&origin["parameter_marker"]])
                    .filter_map(Json::as_str)
                    .map(str::to_owned),
            );
        }
        let mut templates = vec![];
        let mut rejected = vec![];
        for (id, markers) in states {
            let state = json(&dir.join(format!("catalog/states/state-{id:04}.json")))?;
            let semantics = state["scope"]["semantics"].as_str().unwrap_or("");
            if !matches!(
                semantics,
                "positive-constructor-equality/v1" | "positive-constructor-table-equality/v1"
            ) || state["scope"]["symbolic_size_conditions"] == true
            {
                rejected.push(
                    value!({"state":id,"reason":"not a certified positive structural closure"}),
                );
                continue;
            }
            let mut literals = BTreeMap::new();
            for (i, v) in state["values"]
                .as_array()
                .ok_or("missing Cell values")?
                .iter()
                .enumerate()
            {
                if let Some(lit) = v["literal"].as_str() {
                    let e = egglog::ast::Parser::default().get_expr_from_string(None, lit)?;
                    // Unit can be encoded by the printer as (), but eval_expr on
                    // an arbitrary expression would create facts in the runtime.
                    if !matches!(
                        e,
                        Expr::Lit(
                            _,
                            Literal::Int(_)
                                | Literal::Float(_)
                                | Literal::String(_)
                                | Literal::Bool(_)
                                | Literal::Unit
                        )
                    ) {
                        return Err("Cell literal is not a literal".into());
                    }
                    let (sort, val) = eg.eval_expr(&e)?;
                    if Some(sort.name()) != v["sort"].as_str() {
                        return Err("Cell literal sort mismatch".into());
                    }
                    literals.insert(i, val);
                }
            }
            // Shape matching itself checks native column types. Rows not read by
            // this program (including RipenInput) never become native facts.
            templates.push(Template {
                id,
                state,
                parameters: vec![],
                producer: String::new(),
                markers,
                literals,
            });
        }
        let evidence = value!({"analysis_directory":dir,"loaded_states":templates.iter().map(|t|t.id).collect::<Vec<_>>(),"rejected":rejected,
            "contract":"positive projection plus fully keyed versioned witnesses; no saturation or completeness inferred from a historical match"});
        let spines = RefCell::new(crate::spine_projection::Index::new(&templates)?);
        Ok(Self {
            templates,
            spines,
            shared_spines: std::env::var("CELL_SPINE_MARKERS").as_deref() != Ok("0"),
            evidence,
        })
    }

    /// Compiler work only. Never count these offline markers as runtime Cell hits.
    pub fn projection_work(&self) -> Json {
        value!({"shared_spines":self.shared_spines,"work":self.spines.borrow().work,
            "scope":"offline typed closure projection; live row certification and native query costs excluded"})
    }

    /// All witnessed aliases/literals for this exact prefix; root and non-root
    /// positions are treated alike. New aliases use fallback until re-certified.
    pub(crate) fn alternatives(
        &self,
        q: &CellActionSink,
        atoms: &[usize],
    ) -> Vec<(usize, Vec<[u32; 2]>)> {
        if atoms.is_empty() {
            return self.scan_alternatives(q, atoms);
        }
        let mut bound = BTreeSet::new();
        for (i, part) in atoms.iter().enumerate() {
            // Cache only reads functionally determined by the already bound
            // input key; arbitrary reverse fibers remain ordinary queries.
            let a = &q.atoms[*part];
            if i > 0
                && !a.columns[..a.columns.len() - 1].iter().all(|o| match o {
                    egglog::cell_assist::CellOperand::Constant { .. } => true,
                    egglog::cell_assist::CellOperand::Slot(v) => bound.contains(v),
                })
            {
                return vec![];
            }
            let slots = q.atoms[*part]
                .columns
                .iter()
                .filter_map(|x| match x {
                    egglog::cell_assist::CellOperand::Slot(v) => Some(*v),
                    _ => None,
                })
                .collect::<BTreeSet<_>>();
            if i > 0 && bound.is_disjoint(&slots) {
                return vec![];
            }
            bound.extend(slots);
        }
        if self.shared_spines {
            return self
                .spines
                .borrow_mut()
                .alternatives(&self.templates, q, atoms);
        }
        self.scan_alternatives(q, atoms)
    }

    fn scan_alternatives(
        &self,
        q: &CellActionSink,
        atoms: &[usize],
    ) -> Vec<(usize, Vec<[u32; 2]>)> {
        use egglog_numeric_id::NumericId;
        let mut out = BTreeSet::new();
        for t in &self.templates {
            if atoms
                .iter()
                .any(|i| t.markers.contains(&q.atoms[*i].function))
            {
                continue;
            }
            for lens in project_mode(
                t,
                q,
                &Part {
                    atoms: atoms.to_vec(),
                    cell: true,
                },
                false,
            ) {
                // An alias-dependent *new* specialization was not in this closed
                // state. It remains fully supported by the normal query.
                if !lens.aliases.is_empty() {
                    continue;
                }
                let mut seen = BTreeMap::new();
                let conditions = lens
                    .binding
                    .iter()
                    .enumerate()
                    .map(|(slot, v)| {
                        let Some(v) = v else { return [0, 0] };
                        if let Some(lit) = t.literals.get(v) {
                            return [2, lit.rep()];
                        }
                        [1, *seen.entry(*v).or_insert(slot) as u32]
                    })
                    .collect();
                out.insert((t.id, conditions));
            }
        }
        out.into_iter().collect()
    }
}
