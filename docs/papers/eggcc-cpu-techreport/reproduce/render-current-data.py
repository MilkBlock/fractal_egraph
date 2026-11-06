"""Build the native-versus-Cell tables and named workload plots from repeated runs."""
from pathlib import Path
import collections
import csv
import io
import json
import math
import statistics as st
import sys
from xml.sax.saxutils import escape

root = Path(__file__).resolve().parents[1]
assert sys.argv[1:] in ([], ["--check"]), "usage: render-current-data.py [--check]"
check = "--check" in sys.argv

def write(path, value):
    destination = root / path
    if check:
        assert destination.read_text() == value, f"Stale derived artifact: {path}"
    else:
        destination.write_text(value)

raw = [json.loads(line) for line in (root / "suite-current.jsonl").read_text().splitlines()]
manifest = json.loads((root / "evidence/manifest.json").read_text())["cases"]
historical = {(r["case"], r["pass"]): r for r in map(json.loads, (root / "suite.jsonl").read_text().splitlines())}
by = {(r["case"], r["pass_number"]): r for r in raw}
assert len(by) == len(raw) == 288
names = sorted({name for name, _ in by})
assert len(names) == len(manifest) == 96
for r in raw:
    assert r["status"] == "complete", (r["case"], r["pass_number"], r.get("error"))
    assert len(r["samples"]) == 3
    assert [s["i"] for s in r["samples"]] == [1, 2, 3]
    assert not any(k.endswith("_error") for s in r["samples"] for k in s)
    assert r["units"] == (64 if r["pass_number"] == 3 else 59)
    assert r["matches_historical_cell_endpoint"]
    assert r["state_sha256"] == historical[r["case"], r["pass_number"]]["state_sha256"]
    assert math.isclose(r["speedup"], r["native_ms"] / r["cell_ms"])
    for key in ("native_ms", "cell_ms", "native_cli_ms", "cell_cli_ms", "native_rss_bytes", "cell_rss_bytes"):
        assert math.isclose(r[key], st.median(s[key] for s in r["samples"]), abs_tol=1e-6)
    source = next(c for c in manifest if c["source"].split("/benchmarks/passing/")[1] == r["case"])
    assert r["flat_sha256"] == source["passes"][r["pass_number"] - 1]["input_sha256"]
    assert r["source_sha256"] == source["source_sha256"]
    index = int(source["directory"].split("-")[0])
    for s in r["samples"]:
        expected = ["native", "cell"] if (index + r["pass_number"] + s["i"] - 1) % 2 == 0 else ["cell", "native"]
        assert s["order"] == expected
totals = {
    n: {key: sum(by[n, p][key] for p in (1, 2, 3)) for key in ("native_ms", "cell_ms")}
    for n in names
}
def ratio(v):
    return v["native_ms"] / v["cell_ms"]
def summary(label, rows):
    ratios = [ratio(v) for v in rows]
    return [label, str(len(rows)), str(sum(x > 1 for x in ratios)),
            f"{st.geometric_mean(ratios):.3f}×",
            f'{sum(v["native_ms"] for v in rows) / sum(v["cell_ms"] for v in rows):.3f}×']
out = {"summary": [summary("Pass " + str(p), [by[n, p] for n in names]) for p in (1, 2, 3)]}
out["summary"].append(summary("Three-pass sum", list(totals.values())))
out["n"] = 96
out["native_s"] = round(sum(v["native_ms"] for v in totals.values()) / 1000, 3)
out["cell_s"] = round(sum(v["cell_ms"] for v in totals.values()) / 1000, 3)
out["native_cli_s"] = round(sum(r["native_cli_ms"] for r in raw) / 1000, 3)
out["cell_cli_s"] = round(sum(r["cell_cli_ms"] for r in raw) / 1000, 3)
out["cli_ratio"] = f'{sum(r["native_cli_ms"] for r in raw) / sum(r["cell_cli_ms"] for r in raw):.3f}'
out["saved_s"] = round(out["native_s"] - out["cell_s"], 3)
out["prep_s"] = round(sum(r["prepare_s"] for r in raw), 2)
out["break_even"] = round(out["prep_s"] / out["saved_s"], 1) if out["saved_s"] > 0 else None
out["unchanged_endpoints"] = sum(bool(r["matches_historical_cell_endpoint"]) for r in raw)
out["wins"] = sum(ratio(v) > 1 for v in totals.values())
out["losses"] = 96 - out["wins"]
out["geomean"] = f"{st.geometric_mean(ratio(v) for v in totals.values()):.3f}"
ray = next(n for n in names if n.endswith("/raytrace.rs"))
others = [v for n, v in totals.items() if n != ray]
out["without_raytrace_ratio"] = f'{sum(v["native_ms"] for v in others) / sum(v["cell_ms"] for v in others):.3f}'
out["raytrace_saved_share"] = f'{100 * (totals[ray]["native_ms"] - totals[ray]["cell_ms"]) / (1000 * out["saved_s"]):.1f}'
ordered = sorted(names, key=lambda n: ratio(totals[n]))
out["tails"] = [[n, f'{totals[n]["native_ms"]:,.2f}', f'{totals[n]["cell_ms"]:,.2f}', f'{ratio(totals[n]):.3f}×']
                for n in ordered[:4] + ordered[-4:][::-1]]
out["pairs"] = []
for suffix in ("/sum-bits.bril", "/fenwick_tree.rs", "/raytrace.rs"):
    n = next(n for n in names if n.endswith(suffix))
    r = by[n, 3]
    out["pairs"].append([Path(n).stem, "3", f'{r["native_ms"]:,.3f}', f'{r["cell_ms"]:,.3f}',
                         f'{ratio(r):.3f}×', str(sum(s["native_ms"] > s["cell_ms"] for s in r["samples"]))])
out["tasks"] = []
id_map = {}
for c in manifest:
    n = c["source"].split("/benchmarks/passing/")[1]
    ident = int(c["directory"].split("-")[0])
    id_map[n] = ident
    out["tasks"].append([str(ident), n.replace("linear-algebra", "LA"),
                         *[f'{by[n, p][k]:,.2f}' for p in (1, 2, 3) for k in ("native_ms", "cell_ms")],
                         f'{ratio(totals[n]):.3f}×'])
out["tasks"].sort(key=lambda row: int(row[0]))
out["memory_rows"] = [[Path(n).stem, f'{by[n, 3]["native_rss_bytes"] / 2**20:,.2f}',
                       f'{by[n, 3]["cell_rss_bytes"] / 2**20:,.2f}',
                       f'{by[n, 3]["native_cli_ms"]:,.2f}', f'{by[n, 3]["cell_cli_ms"]:,.2f}']
                      for n in names if n.endswith(("/sum-bits.bril", "/fenwick_tree.rs", "/raytrace.rs"))]
columns = ["case", "pass_number", "native_ms", "cell_ms", "speedup", "native_cli_ms", "cell_cli_ms",
           "native_rss_bytes", "cell_rss_bytes", "prepare_s", "units", "runs", "bindings", "resumed",
           "matches_historical_cell_endpoint", "flat_sha256", "program_sha256", "state_sha256"]
with io.StringIO() as f:
    writer = csv.DictWriter(f, fieldnames=columns, extrasaction="ignore", lineterminator="\n")
    writer.writeheader()
    writer.writerows(sorted(raw, key=lambda r: (r["case"], r["pass_number"])))
    write("benchmark-current.csv", f.getvalue())

# Equal-width log2 ratio scale on every named chart. The exact totals accompany
# every workload, so small and large programs remain distinguishable.
w, h = 800, 748
low = min(-1.0, math.floor(math.log2(min(ratio(v) for v in totals.values()))))
high = max(2.0, math.ceil(math.log2(max(ratio(v) for v in totals.values()))))
x0, x1 = 297, 563
def x(r):
    return x0 + (math.log2(r) - low) / (high - low) * (x1 - x0)
def text(px, py, value, size=12, anchor="start", color="#17394b", bold=False):
    return (f'<text x="{px:.2f}" y="{py:.2f}" text-anchor="{anchor}" fill="{color}" '
            f'font-family="Helvetica,Arial,sans-serif" font-size="{size}" '
            f'font-weight="{"700" if bold else "400"}">{escape(str(value))}</text>')
charts = []
for page in range(4):
    selected = ordered[page * 24:(page + 1) * 24]
    b = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">',
         '<rect width="100%" height="100%" fill="white"/>']
    b += [text(4, 20, "Workload (ID links to per-pass table)", 12, bold=True),
          text(430, 20, "Native / Cell", 12, "middle", bold=True),
          text(643, 20, "Native ms", 12, "end", bold=True),
          text(729, 20, "Cell ms", 12, "end", bold=True),
          text(795, 20, "Ratio", 12, "end", bold=True)]
    for power in range(int(low), int(high) + 1):
        px = x(2 ** power)
        b += [f'<line x1="{px:.2f}" x2="{px:.2f}" y1="43" y2="709" stroke="{"#17394b" if power == 0 else "#d7e0e3"}" stroke-width="{1.5 if power == 0 else 0.6}"/>',
              text(px, 38, f"{2 ** power:g}×", 11, "middle")]
    for i, n in enumerate(selected):
        py = 57 + i * 27
        if i % 2 == 0:
            b.append(f'<rect x="0" y="{py-13}" width="800" height="25" fill="#edf4f5" fill-opacity=".7"/>')
        val = totals[n]
        r = ratio(val)
        color = "#126f65" if r >= 1 else "#b34f37"
        label = f"{id_map[n]:03d}  {n.replace('linear-algebra', 'LA')}"
        # Length is bounded by the original case paths. Two-line labels keep the
        # complete identity visible without reducing type for a few long paths.
        if len(label) > 43:
            split = label.rfind("/", 0, 43) + 1
            if split < 5:
                split = 43
            b += [text(4, py-3, label[:split], 10.7), text(18, py+8, label[split:], 10.7)]
        else:
            b.append(text(4, py+3, label, 11.5))
        b += [f'<line x1="{min(x(1), x(r)):.2f}" x2="{max(x(1), x(r)):.2f}" y1="{py}" y2="{py}" stroke="{color}" stroke-width="3"/>',
              f'<circle cx="{x(r):.2f}" cy="{py}" r="3.3" fill="{color}"/>',
              text(643, py+3, f'{val["native_ms"]:,.2f}', 11.5, "end"),
              text(729, py+3, f'{val["cell_ms"]:,.2f}', 11.5, "end"),
              text(795, py+3, f"{r:.3f}×", 11.5, "end", color, True)]
    b += [text(4, 731, "Three-pass totals sum the median of 3 fresh processes per pass. Right of 1× favors Cell.", 11),
          "</svg>"]
    path = f"figures/workloads-current-{page+1}.svg"
    write(path, "\n".join(b))
    charts.append(path)
out["workload_charts"] = charts
# The overview retains one mark per workload and an explicit equality line.
b = ['<svg xmlns="http://www.w3.org/2000/svg" width="800" height="230"><rect width="100%" height="100%" fill="#f4f8f9"/>']
def y(r):
    return 183 - (math.log2(r) - low) / (high - low) * 155
for power in range(int(low), int(high) + 1):
    py = y(2 ** power)
    b += [f'<line x1="48" x2="790" y1="{py:.2f}" y2="{py:.2f}" stroke="{"#17394b" if power == 0 else "#ccdadd"}"/>',
          text(4, py+4, f"{2**power:g}×", 12)]
for i, n in enumerate(ordered):
    xx = 54 + 730 * (i + .5) / 96
    r = ratio(totals[n])
    b.append(f'<line x1="{xx:.2f}" x2="{xx:.2f}" y1="{y(1):.2f}" y2="{y(r):.2f}" stroke="{"#126f65" if r>=1 else "#b34f37"}" stroke-width="5"/>')
b += [text(50, 216, "96 workloads sorted by three-pass native / Cell ratio. Full names and times follow.", 13), "</svg>"]
write("figures/suite-current.svg", "\n".join(b))
write("current-report-data.json", json.dumps(out, indent=2) + "\n")
print(f"Verified 96 workloads × 3 passes × 3 measured pairs. Cell faster on {out['wins']}/96 three-pass totals.")
