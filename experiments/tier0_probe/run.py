"""Run native probes with a timeout and auditable incidence output."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "experiments/tier0_probe/results.json"
subprocess.run(["cargo", "build", "--release", "--bin", "tier0_probe"], cwd=ROOT, check=True)
try:
    subprocess.run(["target/release/tier0_probe",
                    "experiments/annotated_export/math_microbenchmark/combined.egg",
                    "R15,R23,combined_017", str(OUT)], cwd=ROOT, check=True, timeout=180)
except (subprocess.TimeoutExpired, subprocess.CalledProcessError) as error:
    result = json.loads(OUT.read_text()) if OUT.exists() else {}
    result["status"] = "timeout" if isinstance(error, subprocess.TimeoutExpired) else "failed"
    result["error"] = str(error)
    OUT.write_text(json.dumps(result, indent=2)+"\n")
    raise

# Check the serialized incidence data independently of the in-memory aggregates.
report = json.loads(OUT.read_text())
incidence = {}
for line in Path(str(OUT)+".coverage.jsonl").open():
    row = json.loads(line)
    key = row["rule"], row["side"]
    item = incidence.setdefault(key, {"count":0,"gross":0,"unique":set()})
    assert len(row["nodes"]) == len(set(row["nodes"]))
    assert set(row["interior"]) <= set(row["nodes"])
    item["count"] += 1
    item["gross"] += len(row["nodes"])
    item["unique"].update(row["nodes"])
covered = set()
for probe in report["probes"]:
    if probe["status"] == "no_explicit_constructor_nodes":
        assert probe["unique_covered_nodes"] == 0
        continue
    item = incidence[(probe["rule"], probe["side"])]
    assert item["count"] == probe["instances"]
    assert item["gross"] == probe["gross_distinct_per_instance"]
    assert len(item["unique"]) == probe["unique_covered_nodes"]
    covered.update(item["unique"])
node_ids = {json.loads(line)["node"] for line in Path(str(OUT)+".nodes.jsonl").open()}
assert node_ids == covered and len(covered) == report["lhs_rhs_joint_unique_coverage"]
report["serialized_incidence_verified"] = True
OUT.write_text(json.dumps(report, indent=2)+"\n")
