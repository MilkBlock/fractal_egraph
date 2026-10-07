use std::{collections::BTreeMap, fs};
fn main() {
    let a: Vec<_> = std::env::args().collect();
    assert_eq!(a.len(), 3);
    let text = fs::read_to_string(&a[1]).unwrap();
    assert!(!text.contains("flat_internal_"));
    let mut eg = egglog::new_experimental_egraph();
    let commands = eg.resolve_program(None, &text).unwrap();
    let printed = commands
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    let chars: Vec<_> = printed.chars().collect();
    let mut out = String::new();
    let mut names = BTreeMap::new();
    let (mut i, mut quoted, mut escaped) = (0, false, false);
    while i < chars.len() {
        let c = chars[i];
        if quoted {
            out.push(c);
            if escaped {
                escaped = false
            } else if c == '\\' {
                escaped = true
            } else if c == '"' {
                quoted = false
            }
            i += 1;
            continue;
        }
        if c == '"' {
            quoted = true;
            out.push(c);
            i += 1;
            continue;
        }
        if c == '@' {
            let start = i;
            i += 1;
            while i < chars.len()
                && !chars[i].is_whitespace()
                && !['(', ')', '"'].contains(&chars[i])
            {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            let next = names.len();
            names.entry(name.clone()).or_insert(next);
            out.push_str("___");
            out.push_str(&name[1..]);
        } else {
            out.push(c);
            i += 1;
        }
    }
    if std::env::var_os("FLATTEN_NAMES").is_some() {
        eprintln!("{names:?}");
    }
    fs::write(&a[2], out).unwrap();
}
