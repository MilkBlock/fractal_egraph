"""Validate and render the November 6 memory update from its bundled evidence."""
from pathlib import Path
import hashlib
import json
import math
import statistics
import sys

root = Path(__file__).resolve().parents[1]
source = root / "evidence/memory-lifetimes.json"
raw = json.loads(source.read_text())
assert raw["base"] == "27f098a2"
assert raw["implementation"] == "4d9ea942"
mib = 1024 ** 2
data = {"evidence_sha256": hashlib.sha256(source.read_bytes()).hexdigest()}
categories = raw["cases"]["raytrace"]["allocation_census"]["baseline"]["inference"]["categories"]
assert sum(c["live_bytes"] for c in categories) == raw["cases"]["raytrace"]["allocation_census"]["baseline"]["inference"]["requested_live_bytes"]
data["allocation"] = [
    [c["category"].replace("_", " "), f'{c["live_bytes"] / mib:,.1f}']
    for c in sorted(categories, key=lambda c: -c["live_bytes"])
]
data["allocation_total"] = f'{sum(c["live_bytes"] for c in categories) / mib:,.2f}'
native = raw["native"]["samples"]
assert len(native) == 2 and all(n["threads"] == 1 for n in native)
native_rss = statistics.median(n["peak_rss_bytes"] for n in native)
data["native_rss"] = f"{native_rss / mib:,.2f}"
data["native_samples"] = [f'{n["peak_rss_bytes"] / mib:.3f}' for n in native]
data["outcomes"] = []
data["physical"] = []
data["factorial"] = []
data["paired"] = []
for case, repeat in (("sum-bits", 8), ("raytrace", 4)):
    evidence = raw["cases"][case]
    matrices = evidence["matrices"]
    for matrix in matrices.values():
        for arm in matrix.values():
            for metric in ("inference_ms", "cli_ms", "rss"):
                values = arm[metric]["raw"]
                assert len(values) == repeat
                assert math.isclose(statistics.median(values), arm[metric]["median"], abs_tol=0.0011)
    arms = matrices["origins"]
    baseline, optimized = arms["base"], arms["all"]
    data["outcomes"].append([
        case, f'{baseline["inference_ms"]["median"]:,.3f}',
        f'{optimized["inference_ms"]["median"]:,.3f}',
        f'{baseline["rss"]["median"] / mib:,.2f}',
        f'{optimized["rss"]["median"] / mib:,.2f}',
    ])
    data["paired"].append([
        case, f'{optimized["median_paired_delta_ms"]:+.3f} ms',
        f'{optimized["faster_pairs"]}/{repeat}',
        f'{baseline["cli_ms"]["median"]:,.3f}',
        f'{optimized["cli_ms"]["median"]:,.3f}',
    ])
    old, new = baseline["storage"], optimized["storage"]
    a = old["full_cells"] + old["embedded_writers"]["live"]
    b = new["full_cells"] + new["embedded_writers"]["live"]
    data["physical"].append([
        case, f'{old["full_cells"]:,} → {new["full_cells"]:,}',
        f'{old["embedded_writers"]["live"]:,} → {new["embedded_writers"]["live"]:,}',
        f"{a:,} → {b:,}",
    ])
    for label, arm in (("Control", "base"), ("Consumed entries", "retire"),
                       ("Validation IR", "source"), ("Both", "both")):
        if case == "raytrace":
            v = matrices["factorial"][arm]
            data["factorial"].append([
                label, f'{v["inference_ms"]["median"] / 1000:.3f}',
                f'{v["rss"]["median"] / mib:,.2f}',
            ])
    if case == "raytrace":
        data["reduction_percent"] = f'{100 * (1 - optimized["rss"]["median"] / baseline["rss"]["median"]):.1f}'
        data["old_native_ratio"] = f'{baseline["rss"]["median"] / native_rss:.2f}'
        data["new_native_ratio"] = f'{optimized["rss"]["median"] / native_rss:.2f}'
        data["entry_packets"] = [
            f'{matrices["factorial"][a]["entries"]["live_packets"]:,}' for a in ("base", "retire")
        ]
tables = raw["raytrace_table_counts"]
data["rows"] = sum(t["rows"] for t in tables)
data["views"] = sum(t["rows"] for t in tables if t["table"].startswith("CellSpineView"))
data["receipts"] = sum(t["rows"] for t in tables if t["table"].startswith("CellSpineApplied"))
assert (data["rows"], data["views"], data["receipts"]) == (1034514, 213334, 121502)
data["generated_percent"] = f'{100 * (data["views"] + data["receipts"]) / data["rows"]:.1f}'
payload = json.dumps(data, indent=2) + "\n"
if "--check" in sys.argv:
    assert (root / "memory-update.json").read_text() == payload
else:
    (root / "memory-update.json").write_text(payload)
print("Verified all three memory matrices, allocation sums, native samples and row counts.")
