// Public-API snapshot shared by fork and pristine benchmark harnesses.
use egglog::EGraph;
use serde_json::{Value as Json, json};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
pub fn snapshot(eg: &EGraph, enabled: bool) -> Result<Json> {
    if !enabled {
        return Ok(Json::Null);
    }
    let mut rows = vec![];
    let names = eg.get_function_names();
    for name in names {
        if name.starts_with('@') {
            continue;
        }
        let f = eg.get_function(&name).unwrap();
        if f.is_let_binding() {
            continue;
        }
        eg.function_for_each(&name, |r| {
            let args = r
                .vals
                .iter()
                .zip(&f.schema().input)
                .map(|(v, t)| {
                    let value = if t.is_eq_sort() {
                        format!("{}:{:?}", t.name(), eg.get_canonical_value(*v, t))
                    } else {
                        match t.name() {
                            "i64" => eg.value_to_base::<i64>(*v).to_string(),
                            "bool" => eg.value_to_base::<bool>(*v).to_string(),
                            "String" => eg.value_to_base::<egglog::sort::S>(*v).to_string(),
                            "Unit" => "()".into(),
                            other => panic!("unsupported snapshot sort {other}"),
                        }
                    };
                    json!([t.name(), value])
                })
                .collect::<Vec<_>>();
            let v = *r.vals.last().unwrap();
            let sort = f.schema().output.name();
            let result = if f.schema().output.is_eq_sort() {
                format!(
                    "{}:{:?}",
                    sort,
                    eg.get_canonical_value(v, &f.schema().output)
                )
            } else {
                match sort {
                    "i64" => eg.value_to_base::<i64>(v).to_string(),
                    "bool" => eg.value_to_base::<bool>(v).to_string(),
                    "String" => eg.value_to_base::<egglog::sort::S>(v).to_string(),
                    "Unit" => "()".into(),
                    other => panic!("unsupported snapshot sort {other}"),
                }
            };
            rows.push(
                json!({"op":name,"sort":sort,"args":args,"result":result,"subsumed":r.subsumed}),
            );
        })?;
    }
    Ok(json!(rows))
}
