"""Verify the accepted checkpoint and latest allocation trials, then render tables."""
from pathlib import Path
import hashlib
import json
import math
import statistics
import sys

root = Path(__file__).resolve().parents[1]
accepted = json.loads((root / "evidence/performance-acceptance.json").read_text())
assert accepted["implementation"].startswith("502e29cd")
out = {"implementation": accepted["implementation"], "headline": [], "execution": [],
       "counts": [], "limits": []}
out["evidence_sha256"] = {n: hashlib.sha256((root / "evidence" / n).read_bytes()).hexdigest()
                           for n in ["performance-acceptance.json"]}
for case, info in accepted["cases"].items():
    candidate_arm = "port-pgo-" + ("raytrace" if case == "sum-bits" else "sum-bits")
    groups = {}
    for arm, summary_key in [("pinned-original", "original_median"), (candidate_arm, "candidate_median")]:
        points = [r for r in accepted["samples"] if r["case"] == case and r["arm"] == arm and r["i"] > 0]
        assert len(points) == info["measured_pairs"]
        assert sorted(p["i"] for p in points) == list(range(1, info["measured_pairs"] + 1))
        for key in ["ms", "cli", "rss", "visits"]:
            assert math.isclose(statistics.median(p[key] for p in points), info[summary_key][key], abs_tol=0.0011)
        groups[arm] = points
    control, candidate = info["original_median"], info["candidate_median"]
    fixed = info["fixed_reference_ms"] / candidate["ms"]
    paired = control["ms"] / candidate["ms"]
    assert math.isclose(fixed, info["fixed_speedup"])
    assert math.isclose(paired, info["paired_speedup"])
    assert sum(p["ms"] < info["fixed_cutoff_ms"] for p in groups[candidate_arm]) == info["individual_candidates_below_fixed_cutoff"]
    out["headline"].append([case, f'{control["ms"]:,.3f}', f'{candidate["ms"]:,.3f}',
                            f"{fixed:.3f}×", f"{paired:.3f}×"])
    out["execution"].append([case, f'{control["cli"]:,.3f}', f'{candidate["cli"]:,.3f}',
                            f'{control["rss"] / 2**20:,.2f}', f'{candidate["rss"] / 2**20:,.2f}'])
    out["counts"].append([case, f'{control["visits"]:,}', f'{candidate["visits"]:,}',
                         str(info["measured_pairs"]), str(info["audit"]["runs"]),
                         f'{info["audit"]["resumes"]:,} / {info["audit"]["longest"]}'])
    out["limits"].append([case, f'{info["fixed_reference_ms"]:,.4f}',
                          f'{info["fixed_cutoff_ms"]:,.4f}',
                          f'{info["individual_candidates_below_fixed_cutoff"]}/{info["measured_pairs"]}'])
assert accepted["fixed_targets_met"] is True
assert accepted["paired_targets_met"] is False
payload = json.dumps(out, indent=2) + "\n"
path = root / "performance-update.json"
if "--check" in sys.argv:
    assert path.read_text() == payload
else:
    path.write_text(payload)
print("Verified fixed-target checkpoint, all measured pairs, both speedup denominators.")
