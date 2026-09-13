//! Finite template-unit DAG adapter. Does not unfold recursive grammar or union views.
use crate::dag_embedding::{Dag, Edge, Interface, Node, Outcome, embed};
use serde_json::{Value as Json, json};
fn node(def: &Json) -> Node {
    let constraints = match &def["refinement"] {
        Json::String(s) => serde_json::from_str(s).unwrap_or_else(|_| json!(s)),
        v => v.clone(),
    };
    let ports = def["inputs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Node {
        label: json!({"rule":def["rule"],"inputs":def["inputs"],"literal":def["literal"]}),
        constraints,
        ports,
    }
}
pub(crate) fn graph(pattern: &Json, steps: &[Json]) -> Result<Dag, String> {
    if let Some(value) = pattern.get("dependency_dag") {
        let graph: Dag = serde_json::from_value(value.clone()).map_err(|e| e.to_string())?;
        graph.validate()?;
        return Ok(graph);
    }
    let root = &steps[pattern["entry_template"]
        .as_u64()
        .ok_or("missing entry template")? as usize]["definition"];
    let mut nodes = vec![node(root)];
    let mut edges = vec![];
    for port in pattern["ports"]
        .as_array()
        .ok_or("missing template ports")?
    {
        let def = &steps[port["step_template"]
            .as_u64()
            .ok_or("missing step template")? as usize]["definition"];
        let index = nodes.len();
        nodes.push(node(def));
        // Bundle input position + source role + type. Parent index is represented by
        // the edge endpoint, not retained as a graph-global numeric identity.
        let routes:Vec<_>=def["routes"].as_array().map(|r|r.iter().enumerate().map(|(i,r)|json!({"input":i,"source":r.as_str().unwrap_or("").strip_prefix("parent:0:").unwrap_or(r.as_str().unwrap_or(""))})).collect()).unwrap_or_default();
        edges.push(Edge {
            from: 0,
            to: index,
            label: json!({"binding":routes,"return":port["return_transfer"]}),
        });
    }
    let interface = nodes[0]
        .ports
        .iter()
        .map(|p| Interface {
            name: p.clone(),
            node: 0,
            port: p.clone(),
        })
        .collect();
    let graph = Dag {
        root: 0,
        nodes,
        edges,
        interface,
    };
    graph.validate()?;
    Ok(graph)
}
pub(crate) fn compare(graphs: &[Dag], ids: &[u64], budget: usize) -> Result<Json, String> {
    let mut results = vec![vec![None; graphs.len()]; graphs.len()];
    for a in 0..graphs.len() {
        for b in 0..graphs.len() {
            if a != b {
                results[a][b] = Some(embed(&graphs[a], &graphs[b], budget)?);
            }
        }
    }
    let mut matches = vec![];
    let mut unknown = vec![];
    for a in 0..graphs.len() {
        for b in 0..graphs.len() {
            if a == b {
                continue;
            }
            match results[a][b].as_ref().unwrap(){
   Outcome::Found{node_map,root_image,states}=>{
    let strict=matches!(results[b][a],Some(Outcome::Absent{..}));
    matches.push(json!({"dominant":ids[b],"contained":ids[a],"strict":strict,"node_map":node_map,"small_root":graphs[a].root,"large_root":graphs[b].root,"root_image":root_image,"interface_projection":graphs[a].interface.iter().map(|p|json!({"name":p.name,"node":node_map[p.node],"port":p.port})).collect::<Vec<_>>(),"states":states,"scope":"injective non-induced embedding of the supplied finite labelled DAG; not executable dominance"}));
   },
   Outcome::UnknownBudget{states}=>unknown.push(json!({"small":ids[a],"large":ids[b],"states":states,"status":"unknown_budget"})),
   Outcome::Absent{..}=>{},
  }
        }
    }
    Ok(
        json!({"embeddings":matches,"unknown":unknown,"ordered_pairs_checked":graphs.len()*graphs.len().saturating_sub(1),"state_budget_per_pair":budget,"scope":"All node images searched within the supplied finite DAGs and exact labels/declared constraints. Recursive unfolding and additional egraph equivalences are not inferred."}),
    )
}
