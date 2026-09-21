use serde_json::Value;
use std::{fs, process::Command};
#[test]
fn ripens_dependencies_without_any_installed_use() {
    let root = std::env::temp_dir().join(format!("dependency-ripen-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let src = root.join("source.egg");
    fs::write(&src,"(datatype E (V) (A E) (B E) (C E))\n(rewrite (A x) (B x))\n(rewrite (B x) (C x))\n(A (V))\n(run 4)").unwrap();
    for baseline in [false, true] {
        let out = root.join(if baseline { "use-only" } else { "dependencies" });
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_egg_layout"));
        cmd.env("EGG_LAYOUT_DEPENDENCY_CONES", "1");
        if baseline {
            cmd.env("EGG_LAYOUT_USE_ONLY", "1");
        }
        let p = cmd
            .args(["analyze", "--recapture-tier0", "--source"])
            .arg(&src)
            .arg("--output")
            .arg(&out)
            .output()
            .unwrap();
        assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
        let q: Value =
            serde_json::from_slice(&fs::read(out.join("saturated-rule-composition/queue.json")).unwrap()).unwrap();
        assert_eq!(q["observed_uses"], 0, "{q}");
        if baseline {
            assert!(q["jobs"].as_array().unwrap().is_empty());
        } else {
            assert!(q["counts"]["Saturated"].as_u64().unwrap_or(0) > 0, "{q}");
            let trigger = &q["catalog"]["catalog"]["triggers"][0];
            assert!(trigger["origin"].get("use_id").is_none());
            assert_eq!(trigger["origin"]["candidate_kind"], "DependencyCone");
            assert_eq!(
                q["catalog"]["states"][0]["scope"]["symbolic_boundary"],
                true
            );
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn common_consumer_anchors_two_independent_producers() {
    let root = std::env::temp_dir().join(format!("cross-cs-ripen-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let src = root.join("source.egg");
    fs::write(&src,"(datatype E (V i64) (A E) (B E) (Pair E E))\n(relation P (E))\n(relation Q (E))\n(rule ((= a (A x))) ((P x)))\n(rule ((= b (B y))) ((Q y)))\n(rule ((P x) (Q y)) ((Pair x y)))\n(A (V 1))\n(B (V 2))\n(run 4)").unwrap();
    let out = root.join("run");
    let p = Command::new(env!("CARGO_BIN_EXE_egg_layout"))
        .env("EGG_LAYOUT_DEPENDENCY_CONES", "1")
        .args(["analyze", "--recapture-tier0", "--source"])
        .arg(src)
        .arg("--output")
        .arg(&out)
        .output()
        .unwrap();
    assert!(p.status.success(), "{}", String::from_utf8_lossy(&p.stderr));
    let q: Value =
        serde_json::from_slice(&fs::read(out.join("saturated-rule-composition/queue.json")).unwrap()).unwrap();
    let job = q["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|j| {
            j["candidate_kind"] == "DependencyCone"
                && j["coarse_layers"].as_array().unwrap().len() > 1
        })
        .expect("cross-layer candidate");
    assert_eq!(job["state"], "Saturated", "{q}");
    let trigger = q["catalog"]["catalog"]["triggers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["origin"]["candidate_id"] == job["candidate_id"])
        .unwrap();
    assert!(
        trigger["binding_origin"]["staged_injections"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    assert!(
        trigger["binding_origin"]["comb_members"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["parents"]
            .as_array()
            .unwrap()
            .len()
            >= 2
    );
    fs::remove_dir_all(root).unwrap();
}
