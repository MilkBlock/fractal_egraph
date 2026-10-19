//! Closure-backed port progressions, derived exclusively from analyze evidence.
//! No new native query, saturation, source scan, or speculative instance pairs.
use super::super::*;

#[derive(Clone)]
struct Port {
    token: usize,
    sort: String,
    value: usize,
    marker: String,
    marker_index: i64,
}
#[derive(Clone)]
struct Instance {
    class: usize,
    ports: Vec<Port>,
    units: BTreeSet<usize>,
    markers: BTreeSet<String>,
}
struct Read {
    instance: usize,
    record: usize,
    read: usize,
    columns: Vec<Option<usize>>,
    variables: BTreeMap<usize, usize>,
}

struct CertificateIndex<'a> {
    rows: BTreeMap<(&'a str, Vec<usize>), usize>,
    parameters: BTreeMap<(&'a str, &'a str, &'a str), Vec<usize>>,
    literals: BTreeMap<(&'a str, &'a str), usize>,
}
impl<'a> CertificateIndex<'a> {
    fn new(state: &'a Json) -> Self {
        let mut out = Self {
            rows: BTreeMap::new(),
            parameters: BTreeMap::new(),
            literals: BTreeMap::new(),
        };
        for (i, v) in state["values"].as_array().into_iter().flatten().enumerate() {
            if let (Some(sort), Some(lit)) = (v["sort"].as_str(), v["literal"].as_str()) {
                out.literals.insert((sort, lit), i);
            }
        }
        let hidden: BTreeSet<_> = state["subsumed_rows"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Json::as_u64)
            .collect();
        for (row_index, row) in state["rows"].as_array().into_iter().flatten().enumerate() {
            if hidden.contains(&(row_index as u64)) {
                continue;
            }
            let op = row["op"].as_str().unwrap();
            let args: Vec<_> = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as usize)
                .collect();
            if args.len() == 1 && state["values"][args[0]]["sort"] == "i64" {
                if let Some(lit) = state["values"][args[0]]["literal"].as_str() {
                    let result = row["result"].as_u64().unwrap() as usize;
                    let sort = state["values"][result]["sort"].as_str().unwrap();
                    out.parameters
                        .entry((op, lit, sort))
                        .or_default()
                        .push(result);
                }
            }
            out.rows
                .insert((op, args), row["result"].as_u64().unwrap() as usize);
        }
        out
    }
}

fn ports(trigger: &Json, index: &CertificateIndex<'_>) -> Option<Vec<Port>> {
    let o = &trigger["binding_origin"];
    let mut markers: BTreeSet<_> = o["parameter_markers"]
        .as_object()
        .into_iter()
        .flat_map(|m| m.values())
        .filter_map(Json::as_str)
        .collect();
    if let Some(m) = o["parameter_marker"].as_str() {
        markers.insert(m);
    }
    o["reuse_interface_terms"]["lower"]
        .as_array()?
        .iter()
        .map(|term| {
            let text = term.as_str()?;
            let known: Vec<_> = o["symbolic_values"]
                .as_array()?
                .iter()
                .filter(|v| v["term"] == *term)
                .collect();
            if known.len() != 1 {
                return None;
            }
            let v = known[0];
            let token = v["token"].as_u64()? as usize;
            let sort = v["sort"].as_str()?.to_owned();
            // Interpret an already exported interface term; never parse source rules.
            let expr = egglog::ast::Parser::default()
                .get_expr_from_string(None, text)
                .ok()?;
            let Expr::Call(_, op, args) = expr else {
                return None;
            };
            if !markers.contains(op.as_str()) || args.len() != 1 {
                return None;
            }
            let Expr::Lit(_, Literal::Int(n)) = &args[0] else {
                return None;
            };
            let literal = n.to_string();
            let values = index
                .parameters
                .get(&(op.as_str(), literal.as_str(), sort.as_str()))?;
            if values.len() != 1 {
                return None;
            }
            let value = values[0];
            Some(Port {
                token,
                sort,
                value,
                marker: op,
                marker_index: *n,
            })
        })
        .collect()
}

// Deterministic constructor transport seeded by the observed parameter map.
// Every mapped argument tuple has one hashcons result; no subgraph search.
fn retain_body(
    source: &Json,
    target: &Json,
    src: &Instance,
    dst: &Instance,
    ports: &[usize],
    index: &CertificateIndex<'_>,
) -> Option<Json> {
    if source["scope"] != target["scope"] || ports.len() != src.ports.len() {
        return None;
    }
    let mut map = BTreeMap::new();
    for (p, &to) in src.ports.iter().zip(ports) {
        let q = dst.ports.get(to)?;
        if p.sort != q.sort {
            return None;
        }
        if map
            .insert(p.value, q.value)
            .is_some_and(|old| old != q.value)
        {
            return None;
        }
    }
    for (i, v) in source["values"].as_array()?.iter().enumerate() {
        if let (Some(sort), Some(lit)) = (v["sort"].as_str(), v["literal"].as_str()) {
            if let Some(&to) = index.literals.get(&(sort, lit)) {
                map.insert(i, to);
            }
        }
    }
    let hidden: BTreeSet<_> = source["subsumed_rows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Json::as_u64)
        .collect();
    let mut rows = vec![];
    for (i, r) in source["rows"].as_array()?.iter().enumerate() {
        if hidden.contains(&(i as u64)) {
            continue;
        }
        let op = r["op"].as_str()?;
        if src.markers.contains(op) {
            let args = r["args"].as_array()?;
            if args.len() != 1 {
                return None;
            }
            let literal = &source["values"][args[0].as_u64()? as usize]["literal"];
            if !src.ports.iter().any(|p| {
                p.marker == op && *literal == p.marker_index.to_string() && r["result"] == p.value
            }) {
                return None;
            }
        } else {
            rows.push((i, r));
        }
    }
    let mut waiting = BTreeMap::<usize, Vec<usize>>::new();
    let mut missing = vec![0; rows.len()];
    let mut queue = std::collections::VecDeque::new();
    for (i, (_, r)) in rows.iter().enumerate() {
        let unknown: BTreeSet<_> = r["args"]
            .as_array()?
            .iter()
            .map(|v| v.as_u64().map(|x| x as usize))
            .collect::<Option<BTreeSet<_>>>()?
            .into_iter()
            .filter(|v| !map.contains_key(v))
            .collect();
        missing[i] = unknown.len();
        if unknown.is_empty() {
            queue.push_back(i)
        }
        for v in unknown {
            waiting.entry(v).or_default().push(i);
        }
    }
    let mut checked = vec![];
    while let Some(i) = queue.pop_front() {
        let (original, r) = rows[i];
        let args: Vec<_> = r["args"]
            .as_array()?
            .iter()
            .map(|v| map.get(&(v.as_u64()? as usize)).copied())
            .collect::<Option<_>>()?;
        let &to = index.rows.get(&(r["op"].as_str()?, args))?;
        let value = r["result"].as_u64()? as usize;
        if let Some(old) = map.get(&value) {
            if *old != to {
                return None;
            }
        } else {
            map.insert(value, to);
            for consumer in waiting.remove(&value).unwrap_or_default() {
                missing[consumer] -= 1;
                if missing[consumer] == 0 {
                    queue.push_back(consumer);
                }
            }
        }
        checked.push(original);
    }
    if checked.len() != rows.len() {
        return None;
    }
    Some(
        json!({"value_map":map.into_iter().collect::<Vec<_>>(),"checked_visible_rows":checked,"port_map":ports,
        "scope":"all visible non-parameter source rows retained; parameter markers renamed by explicit interface mapping"}),
    )
}

fn epoch(c: &Captured, record: usize) -> usize {
    let b = c.boundaries.partition_point(|b| b.end <= record) + 1;
    c.changes
        .iter()
        .filter(|x| x.reset && x.boundary <= b)
        .count()
}

fn read(
    c: &Captured,
    consumer: usize,
    slot: usize,
    instance: usize,
    instances: &[Instance],
    index: &CertificateIndex<'_>,
) -> Option<Read> {
    let r = &c.records[consumer];
    let Input::Read(span) = &r.inputs[slot] else {
        return None;
    };
    let PortRef { producer, output } = parent(c, consumer, slot)?;
    let p = &c.records[producer];
    let Output::Row(source_span) = &p.outputs[output] else {
        return None;
    };
    if !p.produced.contains(&r.wanted[slot])
        || p.values[output] != r.wanted[slot]
        || epoch(c, producer) != epoch(c, consumer)
    {
        return None;
    }
    let (_, Expr::Call(_, op, args)) = c.rules[r.rule].calls.get(span)? else {
        return None;
    };
    let (_, Expr::Call(_, source_op, _)) = c.rules[p.rule].calls.get(source_span)? else {
        return None;
    };
    if op != source_op {
        return None;
    }
    let cell = &instances[instance];
    let mut columns = vec![];
    let mut variables = BTreeMap::new();
    for (col, arg) in args.iter().enumerate() {
        let vals: BTreeSet<_> = p
            .outputs
            .iter()
            .enumerate()
            .filter_map(|(i, o)| match o {
                Output::Column(s, j) if s == source_span && *j == col => Some(p.values[i]),
                _ => None,
            })
            .collect();
        if vals.len() != 1 {
            return None;
        }
        let value = *vals.first()?;
        let ps: Vec<_> = cell
            .ports
            .iter()
            .enumerate()
            .filter(|(_, p)| p.token == value)
            .map(|(i, _)| i)
            .collect();
        if ps.len() != 1 {
            return None;
        } // ambiguous or internal, non-boundary argument
        let index = ps[0];
        columns.push(Some(index));
        let Expr::Var(_, name) = arg else { return None };
        let target = r
            .inputs
            .iter()
            .position(|i| matches!(i,Input::Var(n) if n==name))?;
        if r.wanted[target] != value {
            return None;
        }
        variables.insert(target, index);
    }
    // The concrete read must be present in the certified closure under this frame.
    let expected: Vec<_> = columns
        .iter()
        .map(|p| cell.ports[p.unwrap()].value)
        .collect();
    if !index.rows.contains_key(&(op.as_str(), expected)) {
        return None;
    }
    Some(Read {
        instance,
        record: producer,
        read: slot,
        columns,
        variables,
    })
}
struct PortRef {
    producer: usize,
    output: usize,
}
fn parent(c: &Captured, record: usize, slot: usize) -> Option<PortRef> {
    match c.records[record].ports.get(slot)? {
        super::super::Port::Parent(p, o) => Some(PortRef {
            producer: *c.records[record].parents.get(*p)?,
            output: *o,
        }),
        _ => None,
    }
}

/// Recognize a complete overlap translation, not an approximate numeric fit.
fn displacement(width: usize, pairs: &[(usize, usize)]) -> Option<usize> {
    let &(a, b) = pairs.first()?;
    let d = a.checked_sub(b)?;
    if d == 0 || d >= width {
        return None;
    }
    let actual: BTreeSet<_> = pairs.iter().copied().collect();
    let expected: BTreeSet<_> = (0..width - d).map(|i| (i + d, i)).collect();
    (actual == expected).then_some(d)
}

fn components(edges: &[Json], instances: &[Instance], width: usize, shift: usize) -> Vec<Json> {
    let mut undirected = BTreeMap::<usize, BTreeSet<usize>>::new();
    let mut next = BTreeMap::<usize, BTreeSet<usize>>::new();
    let mut prev = BTreeMap::<usize, BTreeSet<usize>>::new();
    for e in edges {
        let a = e["from"].as_u64().unwrap() as usize;
        let b = e["to"].as_u64().unwrap() as usize;
        undirected.entry(a).or_default().insert(b);
        undirected.entry(b).or_default().insert(a);
        next.entry(a).or_default().insert(b);
        prev.entry(b).or_default().insert(a);
    }
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    for &seed in undirected.keys() {
        if seen.contains(&seed) {
            continue;
        }
        let mut todo = vec![seed];
        let mut nodes = BTreeSet::new();
        while let Some(n) = todo.pop() {
            if !nodes.insert(n) {
                continue;
            }
            seen.insert(n);
            todo.extend(&undirected[&n]);
        }
        let branching = nodes.iter().any(|n| {
            next.get(n).map_or(0, BTreeSet::len) > 1 || prev.get(n).map_or(0, BTreeSet::len) > 1
        });
        let starts: Vec<_> = nodes
            .iter()
            .filter(|n| prev.get(n).is_none_or(BTreeSet::is_empty))
            .copied()
            .collect();
        if branching || starts.len() != 1 {
            out.push(json!({"status":if branching{"branching"}else{"cyclic"},"instances":nodes,"coordinates":null}));
            continue;
        }
        let mut order = vec![];
        let mut n = starts[0];
        loop {
            order.push(n);
            match next.get(&n).and_then(BTreeSet::first) {
                Some(&q) => n = q,
                None => break,
            }
        }
        if order.len() != nodes.len() {
            out.push(json!({"status":"cyclic","instances":nodes,"coordinates":null}));
            continue;
        }
        let mut elements = BTreeMap::<usize, (String, usize)>::new();
        let mut valid = true;
        for (i, &n) in order.iter().enumerate() {
            for (p, port) in instances[n].ports.iter().enumerate() {
                let value = (port.sort.clone(), port.token);
                if elements
                    .insert(i * shift + p, value.clone())
                    .is_some_and(|old| old != value)
                {
                    valid = false;
                }
            }
        }
        if !valid {
            out.push(json!({"status":"inconsistent-overlap","instances":nodes,"coordinates":null}));
            continue;
        }
        let positions: BTreeMap<_, _> = order.iter().enumerate().map(|(i, n)| (*n, i)).collect();
        out.push(json!({"status":"observed-indexed-chain","instances":order,
            "window_count":order.len(),"element_count":elements.len(),
            "elements":elements.into_iter().map(|(index,(sort,token))|json!({"index":index,"sort":sort,"token":token})).collect::<Vec<_>>(),
            "coordinates":order.iter().enumerate().map(|(i,n)|json!({"instance":n,"i":i,"base":i*shift})).collect::<Vec<_>>(),
            "interactions":edges.iter().filter(|e|nodes.contains(&(e["from"].as_u64().unwrap() as usize))).map(|e|json!({"event":e["event"],"record":e["record"],"i":positions[&(e["from"].as_u64().unwrap() as usize)]})).collect::<Vec<_>>(),
            "affine_accesses":(0..width).flat_map(|j|[json!({"source":0,"port":j,"coefficient":shift,"offset":j}),json!({"source":1,"port":j,"coefficient":shift,"offset":shift+j})]).collect::<Vec<_>>(),
            "domain":{"window_i":[0,order.len()-1],"interaction_i":[0,order.len()-2]},
            "origin_instance":order[0],
            "scope":"snapshot-local coordinates for this observed finite chain; origin may move with new evidence; no arbitrary-length recurrence or memory layout proof"}));
    }
    out
}

// This contract describes a recognizer over captured evidence, not a complete
// matcher over tier0. One outcome per record; the first failed gate is retained.
fn discovery_contract() -> Json {
    let outcomes = [
        (
            "no committed effect",
            0,
            "outside-scope",
            "没有已提交的状态变化",
            "这条记录不能证明状态转移；查看产生新事实、union 或更新的实际 apply。",
        ),
        (
            "visibility effects lack port lineage",
            1,
            "outside-scope",
            "可见性变化的来源证据尚不支持",
            "该 apply 含 subsume/delete；增加 fact 或 rounds 不能补齐这项实现能力。",
        ),
        (
            "consumer does not have two reads",
            2,
            "outside-scope",
            "不是两次读取的交互",
            "当前只识别双来源窗口；单次读取不是窗口拼接，多来源需要扩展识别器。",
        ),
        (
            "no completed exact closure catalog",
            3,
            "missing-evidence",
            "尚无认证闭包目录",
            "先检查本轮 ripen 是否完成、是否仅有观察结果；不能用后续快照补本轮证据。",
        ),
        (
            "two certified producer interfaces unavailable",
            4,
            "missing-evidence",
            "来源读取或闭包接口证据不完整",
            "查看下方每个 read 的来源：缺 producer、未登记、未认证或端口无法对应，处理方法不同。",
        ),
        (
            "source interface has fewer than two ports",
            5,
            "outside-scope",
            "来源接口不足两个端口",
            "当前窗口需要至少两个可区分的位置；hashcons 已合并的重复位置不会被重新猜出。",
        ),
        (
            "source closures not certified equivalent",
            5,
            "incompatible",
            "两个来源没有认证为同一闭包",
            "查看两份来源闭包的差异；当前要求整个主体相同，仅外形或触点相似不够。",
        ),
        (
            "overlap is not a complete port translation",
            6,
            "incompatible",
            "端口没有形成完整平移重叠",
            "对照实际共享端口；若要测试窗口推进，让相邻实例按相同位置重叠并真实触发 consumer。",
        ),
        (
            "target closure not certified",
            7,
            "missing-evidence",
            "目标闭包或接口尚未认证",
            "检查目标 unit 的 ripen 状态及端口映射；已有 match 本身不能代替闭包认证。",
        ),
        (
            "target has unresolved or extra external parameters",
            7,
            "outside-scope",
            "目标有额外或无法唯一定位的参数",
            "保留这些外部输入和别名义务；当前双来源模型不能隐藏它们，需要扩展接口表示。",
        ),
        (
            "target closure does not retain source bodies",
            8,
            "incompatible",
            "不能验证目标保留两份来源结构",
            "查看内部事实、等价关系及可见性；端口对齐不等于闭包内部也得到保留。",
        ),
        (
            "branching",
            9,
            "outside-scope",
            "交互形成分叉",
            "接线证据已保留；当前只生成线性坐标，增加轮次不保证消除分叉。",
        ),
        (
            "cyclic",
            9,
            "outside-scope",
            "交互形成环或没有唯一链首",
            "当前不把环当作有限线性窗口；需要环形或递归表示。",
        ),
        (
            "inconsistent-overlap",
            9,
            "incompatible",
            "重叠位置的元素身份冲突",
            "检查对应位置的版本化 token 与别名，不按数字标签或 match 时间强行排列。",
        ),
        (
            "indexed",
            10,
            "recognized",
            "已识别有限窗口交互",
            "可查看坐标与原始 binding；继续提供按同一接线真实交互的实例，可检验更长的有限链。",
        ),
    ];
    json!({"schema":"port-discovery-contract/v1",
        "gates":["存在已提交 effect","无未支持的可见性变化","consumer 恰有两次读取","存在认证闭包目录","两次读取均有完整来源与接口证据","来源为同一闭包且至少两个端口","共享端口是完整平移","目标接口唯一且无额外参数","目标保留两份来源内部结构","交互组成无分叉、无环且一致的链"],
        "outcomes":outcomes.into_iter().map(|(reason,gate,category,label,next)|
            (reason.to_owned(),json!({"gate":gate,"category":category,"label":label,"next_experiment":next}))).collect::<BTreeMap<_,_>>(),
        "scope":"每条 Captured 记录都有一个结果；首个失败条件之后的检查尚未执行。未采集的匹配不在此保证内。",
        "guarantee":"在本轮已有证据及所列条件内，逐条检查并归类；有限坐标不是任意长度递归、终止性或完整模式发现的证明。",
        "environment":"保证相对于认证目录使用的规则环境；不额外保证原 Multi ruleset 调度等价。",
        "example":{"positive":"L(a,b), R(b,c), L(c,d)，且实际执行 Pair(a,b)+Pair(b,c) 与 Pair(b,c)+Pair(c,d) 两次交互。L/R 的闭包必须认证相同，目标必须保留来源。",
            "negative":"L(a,b), L(c,d), L(e,f) 可复用同一主体，但没有相邻交互证据，不能据此生成窗口顺序。"}})
}

pub(super) fn report(c: &Captured, store: &cs::Store, catalog: Option<&Json>) -> Json {
    let mut reasons = BTreeMap::<String, usize>::new();
    let mut reject = |s: &str| {
        *reasons.entry(s.to_owned()).or_default() += 1;
    };
    let empty = json!({"states":[],"catalog":{"triggers":[]}});
    let cat = catalog.unwrap_or(&empty);
    let states = cat["states"].as_array().unwrap();
    let mut indexes = BTreeMap::new();
    let triggers: BTreeMap<_, _> = cat["catalog"]["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| Some((t["binding_origin"]["candidate_id"].as_u64()? as usize, t)))
        .collect();
    let entries: BTreeMap<_, _> = store
        .windows
        .iter()
        .filter(|w| w.kind == "apply_entry")
        .map(|w| (w.consumer, w.unit))
        .collect();
    let mut instances = vec![];
    let mut instance_keys = BTreeMap::new();
    let mut by_record = BTreeMap::new();
    let mut interface_errors = BTreeMap::new();
    for (&record, &unit) in &entries {
        let Some(t) = triggers.get(&unit) else {
            interface_errors.insert(record, "closure not certified");
            continue;
        };
        let class = t["saturated_rule_composition"].as_u64().unwrap() as usize;
        let index = indexes
            .entry(class)
            .or_insert_with(|| CertificateIndex::new(&states[class]));
        let Some(ps) = ports(t, index) else {
            reject("unresolved certified port frame");
            interface_errors.insert(record, "unresolved certified port frame");
            continue;
        };
        let key = json!([
            class,
            epoch(c, record),
            ps.iter()
                .map(|p| json!([p.sort, p.token, p.value, p.marker, p.marker_index]))
                .collect::<Vec<_>>()
        ])
        .to_string();
        let i = if let Some(&i) = instance_keys.get(&key) {
            i
        } else {
            let i = instances.len();
            instance_keys.insert(key, i);
            instances.push(Instance {
                class,
                ports: ps,
                units: BTreeSet::new(),
                markers: t["binding_origin"]["parameter_markers"]
                    .as_object()
                    .into_iter()
                    .flat_map(|m| m.values())
                    .filter_map(Json::as_str)
                    .chain(t["binding_origin"]["parameter_marker"].as_str())
                    .map(str::to_owned)
                    .collect(),
            });
            i
        };
        instances[i].units.insert(unit);
        by_record.insert(record, i);
    }
    let mut groups = BTreeMap::<String, Json>::new();
    let mut outcomes = Vec::with_capacity(c.records.len());
    for (r, record) in c.records.iter().enumerate() {
        let read_slots: Vec<_> = record
            .inputs
            .iter()
            .enumerate()
            .filter_map(|(i, x)| matches!(x, Input::Read(_)).then_some(i))
            .collect();
        outcomes.push(
            json!({"record":r,"event":record.id,"rule":c.rules[record.rule].rule.name,
            "target_unit":entries.get(&r),"read_count":read_slots.len()}),
        );
        macro_rules! stop {
            ($reason:expr, $detail:expr) => {{
                reject($reason);
                outcomes[r]["reason"] = json!($reason);
                outcomes[r]["detail"] = $detail;
                continue;
            }};
        }
        if record.produced.is_empty() && record.unions.is_empty() && !record.updated_rows {
            stop!("no committed effect", Json::Null);
        }
        if c.rules[record.rule]
            .rule
            .head
            .0
            .iter()
            .any(|a| matches!(a, Action::Change(..)))
        {
            stop!("visibility effects lack port lineage", Json::Null);
        }
        if read_slots.len() != 2 {
            stop!(
                "consumer does not have two reads",
                json!({"read_slots":read_slots})
            );
        }
        if states.is_empty() {
            stop!("no completed exact closure catalog", Json::Null);
        }
        let mut reads = vec![];
        let mut read_evidence = vec![];
        for &slot in &read_slots {
            let p = parent(c, r, slot);
            let producer = p.as_ref().map(|p| p.producer);
            let mut evidence = json!({"slot":slot,"fact_token":record.wanted[slot],
                "producer_record":producer,"source_unit":producer.and_then(|p|entries.get(&p))});
            let v = p
                .and_then(|p| by_record.get(&p.producer).copied())
                .and_then(|i| read(c, r, slot, i, &instances, &indexes[&instances[i].class]));
            evidence["status"] = json!(if v.is_some() {
                "verified"
            } else if let Some(p) = producer {
                interface_errors
                    .get(&p)
                    .copied()
                    .unwrap_or(if entries.contains_key(&p) {
                        "read row or port mapping not verified"
                    } else {
                        "producer has no registered apply entry"
                    })
            } else {
                "missing recorded producer"
            });
            read_evidence.push(evidence);
            if let Some(v) = v {
                reads.push(v)
            }
        }
        outcomes[r]["reads"] = json!(read_evidence);
        if reads.len() != 2 {
            stop!("two certified producer interfaces unavailable", Json::Null);
        }
        let a = &reads[0];
        let b = &reads[1];
        let width = instances[a.instance].ports.len();
        if width < 2 {
            stop!(
                "source interface has fewer than two ports",
                json!({"width":width})
            );
        }
        if instances[a.instance].class != instances[b.instance].class {
            stop!(
                "source closures not certified equivalent",
                json!({"width":width,
                "source_closures":[instances[a.instance].class,instances[b.instance].class]})
            );
        }
        let pairs: Vec<_> = a
            .variables
            .iter()
            .filter_map(|(v, p)| b.variables.get(v).map(|q| (*p, *q)))
            .collect();
        let reverse: Vec<_> = pairs.iter().map(|(a, b)| (*b, *a)).collect();
        let (left, right, shift) = if let Some(d) = displacement(width, &pairs) {
            (a, b, d)
        } else if let Some(d) = displacement(width, &reverse) {
            (b, a, d)
        } else {
            stop!(
                "overlap is not a complete port translation",
                json!({"width":width,"overlap":pairs})
            );
        };
        let Some(&target) = by_record.get(&r) else {
            stop!(
                "target closure not certified",
                json!({"interface":interface_errors.get(&r).copied().unwrap_or("no registered apply entry")})
            );
        };
        let dst = &instances[target];
        let mut target_map = vec![];
        let mut good = true;
        for src in [left, right] {
            let mut map = vec![];
            for p in &instances[src.instance].ports {
                let candidates: Vec<_> = dst
                    .ports
                    .iter()
                    .enumerate()
                    .filter(|(_, q)| p.sort == q.sort && p.token == q.token)
                    .map(|(i, _)| i)
                    .collect();
                if candidates.len() != 1 {
                    good = false;
                    break;
                }
                map.push(candidates[0]);
            }
            target_map.push(map);
        }
        let covered: BTreeSet<_> = target_map.iter().flatten().copied().collect();
        if !good || covered.len() != dst.ports.len() {
            stop!(
                "target has unresolved or extra external parameters",
                json!({"target_port_count":dst.ports.len(),"covered_ports":covered,"unique":good})
            );
        }
        let descriptor = json!({"source_closure":instances[left.instance].class,"target_closure":dst.class,
            "width":width,"shift":shift,"read_columns":[left.columns,right.columns],"target_port_maps":target_map,
            "source_port_values":[instances[left.instance].ports.iter().map(|p|p.value).collect::<Vec<_>>(),instances[right.instance].ports.iter().map(|p|p.value).collect::<Vec<_>>()],
            "target_port_values":dst.ports.iter().map(|p|p.value).collect::<Vec<_>>(),
            "source_parameter_markers":[instances[left.instance].markers,instances[right.instance].markers]});
        let key = descriptor.to_string();
        if !groups.contains_key(&key) {
            let body_maps: Option<Vec<_>> = [left, right]
                .iter()
                .enumerate()
                .map(|(i, src)| {
                    let source = &instances[src.instance];
                    retain_body(
                        &states[source.class],
                        &states[dst.class],
                        source,
                        dst,
                        &target_map[i],
                        &indexes[&dst.class],
                    )
                })
                .collect();
            let Some(body_maps) = body_maps else {
                stop!(
                    "target closure does not retain source bodies",
                    json!({"source_closure":instances[left.instance].class,"target_closure":dst.class})
                );
            };
            let id = groups.len();
            groups.insert(
                key.clone(),
                json!({"id":id,"contract":descriptor,"source_body_maps":body_maps,"witnesses":[]}),
            );
        }
        let g = groups.get_mut(&key).unwrap();
        outcomes[r]["family"] = g["id"].clone();
        g["witnesses"].as_array_mut().unwrap().push(json!({"record":r,"event":record.id,"rule":c.rules[record.rule].rule.name,
            "from":left.instance,"to":right.instance,"target":target,
            "producer_records":[left.record,right.record],"read_slots":[left.read,right.read],
            "versioned_facts":[record.wanted[left.read],record.wanted[right.read]],
            "source_units":[entries[&left.record],entries[&right.record]],"target_unit":entries[&r],
            "producer_rules":[c.rules[c.records[left.record].rule].rule.name,c.rules[c.records[right.record].rule].rule.name]}));
    }
    let mut families: Vec<_> = groups.into_values().collect();
    families.sort_by_key(|f| f["id"].as_u64());
    for f in &mut families {
        let witnesses = f["witnesses"].as_array().unwrap();
        let members: BTreeSet<_> = witnesses
            .iter()
            .flat_map(|w| {
                [
                    w["from"].as_u64().unwrap() as usize,
                    w["to"].as_u64().unwrap() as usize,
                ]
            })
            .collect();
        let source_units: BTreeSet<_> = members
            .iter()
            .flat_map(|i| instances[*i].units.iter().copied())
            .collect();
        let targets: BTreeSet<_> = witnesses
            .iter()
            .filter_map(|w| w["target_unit"].as_u64())
            .map(|u| u as usize)
            .collect();
        let split: BTreeSet<_> = witnesses
            .iter()
            .map(|w| w["producer_rules"].to_string())
            .collect();
        let shift = f["contract"]["shift"].as_u64().unwrap() as usize;
        let mut chains = components(
            witnesses,
            &instances,
            f["contract"]["width"].as_u64().unwrap() as usize,
            shift,
        );
        let component_status: BTreeMap<_, _> = chains
            .iter()
            .flat_map(|chain| {
                let status = if chain["status"] == "observed-indexed-chain" {
                    "indexed"
                } else {
                    chain["status"].as_str().unwrap()
                };
                chain["instances"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(move |n| (n.as_u64().unwrap(), status))
            })
            .collect();
        for w in witnesses {
            let record = w["record"].as_u64().unwrap() as usize;
            outcomes[record]["reason"] = json!(component_status[&w["from"].as_u64().unwrap()]);
        }
        let mut target_accesses = BTreeMap::<usize, BTreeSet<usize>>::new();
        for (side, map) in f["contract"]["target_port_maps"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
        {
            for (port, target) in map.as_array().unwrap().iter().enumerate() {
                target_accesses
                    .entry(target.as_u64().unwrap() as usize)
                    .or_default()
                    .insert(side * shift + port);
            }
        }
        for chain in &mut chains {
            if chain["status"] == "observed-indexed-chain" {
                chain["target_affine_accesses"]=json!(target_accesses.iter().map(|(port,offsets)|json!({"target_port":port,"coefficient":shift,"offsets":offsets})).collect::<Vec<_>>());
            }
        }
        f["components"] = json!(chains);
        f["source_units"] = json!(source_units);
        f["target_units"] = json!(targets);
        f["units"] = json!(source_units.union(&targets).copied().collect::<Vec<_>>());
        f["historical_producer_spellings"] = json!(split.len());
        f["guarantee"] = json!(
            "source and target classes require exact completed RipSat certificates; target retains both source bodies under explicit port transport; adjacency requires actual versioned producer reads; coordinates cover only observed interactions"
        );
    }
    let contract = discovery_contract();
    let mut counts = BTreeMap::<String, usize>::new();
    let mut by_reason = BTreeMap::<String, usize>::new();
    for outcome in &outcomes {
        let reason = outcome["reason"]
            .as_str()
            .expect("every captured record has an outcome");
        let category = contract["outcomes"][reason]["category"]
            .as_str()
            .expect("known discovery outcome");
        *counts.entry(category.to_owned()).or_default() += 1;
        *by_reason.entry(reason.to_owned()).or_default() += 1;
    }
    assert_eq!(outcomes.len(), c.records.len());
    json!({"schema":"closure-port-progressions/v1","families":families,"unsupported":reasons,
        "discovery":{"contract":contract,"total_records":c.records.len(),"accounted_records":outcomes.len(),
            "counts":counts,"by_reason":by_reason,"outcomes":outcomes},
        "instances":instances.iter().enumerate().map(|(i,x)|json!({"id":i,"catalog_state":x.class,"units":x.units,
            "ports":x.ports.iter().enumerate().map(|(j,p)|json!({"port":j,"token":p.token,"sort":p.sort,"closure_value":p.value,"marker":p.marker,"marker_index":p.marker_index})).collect::<Vec<_>>()})).collect::<Vec<_>>(),
        "source":"analyze history plus its completed exact RipSat catalog; no source-file matching",
        "scope":"two-read complete-overlap progressions; no all-pairs search, arbitrary-length proof, or predefined array DSL"})
}

#[cfg(test)]
mod tests {
    use super::*;
    fn instance(tokens: &[usize]) -> Instance {
        Instance {
            class: 0,
            ports: tokens
                .iter()
                .enumerate()
                .map(|(value, token)| Port {
                    token: *token,
                    sort: "E".into(),
                    value,
                    marker: "Input".into(),
                    marker_index: value as i64,
                })
                .collect(),
            units: BTreeSet::new(),
            markers: BTreeSet::from(["Input".into()]),
        }
    }
    fn edge(a: usize, b: usize, event: usize) -> Json {
        json!({"from":a,"to":b,"record":event,"event":event})
    }
    #[test]
    fn complete_overlap_is_required() {
        assert_eq!(displacement(2, &[(1, 0)]), Some(1));
        assert_eq!(displacement(3, &[(1, 0), (2, 1)]), Some(1));
        assert_eq!(displacement(3, &[(1, 0)]), None);
        assert_eq!(displacement(2, &[(0, 1)]), None);
        assert_eq!(displacement(2, &[(0, 0)]), None);
    }
    #[test]
    fn complete_body_transport_preserves_internal_equalities_and_visibility() {
        let source = json!({"scope":"same","values":[{"sort":"E"},{"sort":"E"},{"sort":"i64","literal":"0"}],
            "rows":[{"op":"Input","args":[2],"result":0},{"op":"A","args":[0],"result":1},{"op":"B","args":[1],"result":1}]});
        let cell = instance(&[7]);
        assert!(
            retain_body(
                &source,
                &source,
                &cell,
                &cell,
                &[0],
                &CertificateIndex::new(&source)
            )
            .is_some()
        );
        let mut missing = source.clone();
        missing["rows"].as_array_mut().unwrap().pop();
        assert!(
            retain_body(
                &source,
                &missing,
                &cell,
                &cell,
                &[0],
                &CertificateIndex::new(&missing)
            )
            .is_none()
        );
        let mut hidden = source.clone();
        hidden["subsumed_rows"] = json!([2]);
        assert!(
            retain_body(
                &source,
                &hidden,
                &cell,
                &cell,
                &[0],
                &CertificateIndex::new(&hidden)
            )
            .is_none()
        );
        let mut wrong = source.clone();
        wrong["rows"][2]["result"] = json!(0);
        assert!(
            retain_body(
                &source,
                &wrong,
                &cell,
                &cell,
                &[0],
                &CertificateIndex::new(&wrong)
            )
            .is_none()
        );
        let mut quotient = source.clone();
        quotient["rows"][1]["result"] = json!(0);
        quotient["rows"][2]["args"] = json!([0]);
        quotient["rows"][2]["result"] = json!(0);
        assert!(
            retain_body(
                &source,
                &quotient,
                &cell,
                &cell,
                &[0],
                &CertificateIndex::new(&quotient)
            )
            .is_some(),
            "a target union may merge source classes while retaining all facts"
        );
    }
    #[test]
    fn coordinates_follow_wiring_not_chronology_or_value_order() {
        let xs = vec![instance(&[2, 91]), instance(&[83, 17]), instance(&[17, 2])];
        let c = components(&[edge(2, 0, 0), edge(1, 2, 1)], &xs, 2, 1);
        assert_eq!(c[0]["instances"], json!([1, 2, 0]));
        assert_eq!(
            c[0]["elements"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p["token"].as_u64().unwrap())
                .collect::<Vec<_>>(),
            vec![83, 17, 2, 91]
        );
        assert_eq!(c[0]["interactions"][0]["i"], 1);
        assert_eq!(c[0]["interactions"][1]["i"], 0);
    }
    #[test]
    fn equal_values_do_not_collapse_positions() {
        let xs = vec![instance(&[4, 7]), instance(&[7, 4]), instance(&[4, 9])];
        let c = components(&[edge(0, 1, 1), edge(1, 2, 2)], &xs, 2, 1);
        assert_eq!(c[0]["element_count"], 4);
        assert_eq!(c[0]["elements"][0]["token"], c[0]["elements"][2]["token"]);
        assert_ne!(c[0]["elements"][0]["index"], c[0]["elements"][2]["index"]);
    }
    #[test]
    fn branching_cycles_and_inconsistent_evidence_do_not_become_arrays() {
        let xs = vec![instance(&[1, 2]), instance(&[2, 3]), instance(&[2, 4])];
        assert_eq!(
            components(&[edge(0, 1, 0), edge(0, 2, 1)], &xs, 2, 1)[0]["status"],
            "branching"
        );
        assert_eq!(
            components(&[edge(0, 1, 0), edge(1, 0, 1)], &xs, 2, 1)[0]["status"],
            "cyclic"
        );
        assert_eq!(
            components(&[edge(1, 2, 0)], &xs, 2, 1)[0]["status"],
            "inconsistent-overlap"
        );
    }
}
