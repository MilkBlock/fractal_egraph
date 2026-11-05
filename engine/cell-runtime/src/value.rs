use serde::{Deserialize, Serialize};
use serde_json::{Value as J, json};

#[derive(Clone)]
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Value {
    Eq(u64),
    Int(i64),
    Text(std::sync::Arc<String>),
    Float(i64),
    Set(std::sync::Arc<Vec<Value>>),
    Key(eggfuse_structural_key::Key),
}
pub type Parents = rustc_hash::FxHashMap<u64, u64>;
pub fn root(parents: &Parents, mut x: u64) -> u64 {
    while let Some(&y) = parents.get(&x) {
        if x == y {
            break;
        }
        x = y;
    }
    x
}
pub fn canon(v: &Value, p: &Parents) -> Value {
    match v {
        Value::Eq(x) => Value::Eq(root(p, *x)),
        Value::Set(xs) => {
            let mut xs = xs.iter().map(|x| canon(x, p)).collect::<Vec<_>>();
            xs.sort();
            xs.dedup();
            Value::Set(xs.into())
        }
        _ => v.clone(),
    }
}
pub fn integer(v: &Value) -> Result<i64, String> {
    if let Value::Int(x) = v {
        Ok(*x)
    } else {
        Err("expected integer".into())
    }
}
pub fn primitive(
    name: &str,
    args: &[Value],
    output: &str,
    nodes: &J,
) -> Result<Option<Value>, String> {
    if name.starts_with("cell-key-") {
        use eggfuse_structural_key::Key;
        let key = |i: usize| -> Result<&Key, String> {
            match args.get(i) {
                Some(Value::Key(k)) => Ok(k),
                _ => Err("expected structural key".into()),
            }
        };
        let v = match name {
            "cell-key-decode" => {
                let Value::Text(x) = &args[0] else {
                    return Err("key decode input".into());
                };
                return Ok(Key::decode_legacy(x).map(Value::Key));
            }
            "cell-key-min" => Value::Key(key(0)?.min(key(1)?).clone()),
            "cell-key-anchor" => Value::Key(key(0)?.clone().anchor()),
            "cell-key-cmp" => Value::Int(match key(0)?.cmp(key(1)?) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            }),
            "cell-key-prefer-left" => Value::Int(i64::from(
                integer(&args[0])? > integer(&args[1])?
                    || (integer(&args[0])? == integer(&args[1])? && key(2)? <= key(3)?),
            )),
            "cell-key-select" => {
                if integer(&args[2])? != 0 {
                    args[0].clone()
                } else {
                    args[1].clone()
                }
            }
            "cell-key-base-i64" => Value::Key(Key::base_i64(integer(&args[0])?)),
            "cell-key-base-f64" => {
                let Value::Float(x) = args[0] else {
                    return Err("float key input".into());
                };
                Value::Key(Key::base_f64(x as u64))
            }
            "cell-key-base-String" => {
                let Value::Text(x) = &args[0] else {
                    return Err("string key input".into());
                };
                Value::Key(Key::base_string(x))
            }
            "cell-key-base-bool" => Value::Key(Key::new(
                format!("bool:{}", integer(&args[0])? != 0),
                vec![],
            )),
            "cell-key-base-Unit" => Value::Key(Key::new("Unit:()".into(), vec![])),
            _ => {
                let n = nodes
                    .as_array()
                    .and_then(|ns| ns.iter().find(|n| n["primitive"] == name))
                    .ok_or("undeclared key node primitive")?;
                if n["arity"].as_u64() != Some(args.len() as u64) {
                    return Err("key node arity".into());
                }
                Value::Key(Key::new(
                    n["label"].as_str().ok_or("key node label")?.into(),
                    (0..args.len())
                        .map(key)
                        .map(|x| x.cloned())
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            }
        };
        return Ok(Some(v));
    }
    let name = name.strip_prefix("bool-").unwrap_or(name);
    let value = match name {
        "=" | "!=" => {
            let yes = (args[0] == args[1]) == (name == "=");
            if output == "Unit" {
                return Ok(yes.then_some(Value::Int(0)));
            }
            Value::Int(i64::from(yes))
        }
        "<" | "<=" | ">" | ">=" => {
            let a = integer(&args[0])?;
            let b = integer(&args[1])?;
            let yes = match name {
                "<" => a < b,
                "<=" => a <= b,
                ">" => a > b,
                _ => a >= b,
            };
            if output == "Unit" {
                return Ok(yes.then_some(Value::Int(0)));
            }
            Value::Int(i64::from(yes))
        }
        "+" | "-" | "*" | "/" | "%" | "min" | "max" => {
            let a = integer(&args[0])?;
            let b = integer(&args[1])?;
            let n = match name {
                "+" => a.checked_add(b),
                "-" => a.checked_sub(b),
                "*" => a.checked_mul(b),
                "/" => a.checked_div(b),
                "%" => a.checked_rem(b),
                "min" => Some(a.min(b)),
                _ => Some(a.max(b)),
            };
            let Some(n) = n else { return Ok(None) };
            Value::Int(n)
        }
        "not" => Value::Int(i64::from(integer(&args[0])? == 0)),
        "and" | "or" | "xor" | "implies" => {
            let a = integer(&args[0])? != 0;
            let b = integer(&args[1])? != 0;
            Value::Int(i64::from(match name {
                "and" => a && b,
                "or" => a || b,
                "xor" => a != b,
                _ => !a || b,
            }))
        }
        "set-of" | "set-empty" => {
            let mut xs = args.to_vec();
            xs.sort();
            xs.dedup();
            Value::Set(xs.into())
        }
        "set-insert" | "set-remove" => {
            let Value::Set(xs) = &args[0] else {
                return Err("set expected".into());
            };
            let mut xs = xs.as_ref().clone();
            if name == "set-insert" {
                xs.push(args[1].clone());
                xs.sort();
                xs.dedup()
            } else {
                xs.retain(|x| x != &args[1])
            }
            Value::Set(xs.into())
        }
        "set-union" | "set-intersect" | "set-diff" => {
            let (Value::Set(a), Value::Set(b)) = (&args[0], &args[1]) else {
                return Err("set expected".into());
            };
            let mut xs = if name == "set-union" {
                [a.as_slice(), b.as_slice()].concat()
            } else {
                a.iter()
                    .filter(|x| b.contains(x) == (name == "set-intersect"))
                    .cloned()
                    .collect()
            };
            xs.sort();
            xs.dedup();
            Value::Set(xs.into())
        }
        "set-contains" | "set-not-contains" => {
            let Value::Set(xs) = &args[0] else {
                return Err("set expected".into());
            };
            return Ok((xs.contains(&args[1]) == (name == "set-contains")).then_some(Value::Int(0)));
        }
        "set-length" => {
            let Value::Set(xs) = &args[0] else {
                return Err("set expected".into());
            };
            Value::Int(xs.len() as i64)
        }
        _ => return Err(format!("unsupported primitive {name}")),
    };
    Ok(Some(value))
}
pub fn literal(j: &J) -> Result<Value, String> {
    let kind = j["sort"]["kind"].as_str().ok_or("literal sort")?;
    let n = &j["literal"];
    Ok(match kind {
        "String" => Value::Text(n.as_str().ok_or("string literal")?.to_owned().into()),
        "f64" => Value::Float(n.as_i64().ok_or("float bits")?),
        "i64" | "bool" | "Unit" => Value::Int(n.as_i64().ok_or("integer literal")?),
        _ => return Err(format!("unsupported literal {kind}")),
    })
}
pub fn exported(v: &Value, sort: &J) -> J {
    match v {
        Value::Eq(x) => json!({"sort":sort["name"],"eq":format!("Cell({x})")}),
        Value::Int(x) | Value::Float(x) => json!({"sort":sort["name"],"value":x}),
        Value::Text(x) => json!({"sort":sort["name"],"value":x}),
        Value::Set(xs) => {
            json!({"sort":sort["name"],"elements":xs.iter().map(|v|exported(v,&sort["element"])).collect::<Vec<_>>()})
        }
        Value::Key(k) => json!({"sort":sort["name"],"key":k.to_wire()}),
    }
}
