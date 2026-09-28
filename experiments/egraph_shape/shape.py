#!/usr/bin/env python3
"""E-graph shape as a 2D heatmap (with marginals), or the older violin view.

For every e-class we compute
  * `root distance` -- shortest number of constructor steps from a top e-class
    (roots are classes nobody points at); the x axis of the original request,
  * `leaf height`   -- shortest term height above the leaves (0 = a class that has
    an e-node with no Math arguments); the dual x axis,
  * e-nodes per e-class, which is the second axis (log2 bins).

Default `heatmap` mode draws, for each level (x) and each log2 bin of e-nodes per
class (y), the number of e-classes in that cell as a colour on a log10 scale. The
top marginal is log10(#eclasses) per level -- the y value of the original request
-- and the right marginal is the pooled distribution over e-node counts.
`violin` mode keeps the earlier per-level distribution view.

Input is the constructor-row export used elsewhere in this repository
(`[{"op", "args": [["Math", v], ...], "result"}]`) or a directory tree of
`SaturatedRuleComposition` cell exports.
"""
import argparse
import collections
import glob as globmod
import json
import math
import statistics
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

INK = "#162b3a"
MUTED = "#5b6b7a"
GRID = "#dfe6ec"
BLUE = "#2678a9"
ORANGE = "#d77819"

RAMP = ["#eef5fa", "#cfe3f1", "#9ec9e2", "#f2c14e", "#e08a1e", "#b8530c", "#8c2f06"]


def norm(v):
    return v if isinstance(v, str) else str(v)


def index_rows(rows):
    enodes = collections.defaultdict(list)
    for row in rows:
        enodes[norm(row["result"])].append(row)
    classes = set(enodes)
    children = {c: set() for c in classes}
    parents = collections.defaultdict(set)
    for c, group in enodes.items():
        for row in group:
            for sort, value in row["args"]:
                if sort == "Math" and norm(value) in classes:
                    children[c].add(norm(value))
                    parents[norm(value)].add(c)
    return enodes, children, parents, classes


def load_graph(path):
    return json.loads(Path(path).read_text())


def composition_rows(path):
    """Convert a SaturatedRuleComposition export (values + indexed rows) to rows."""
    data = json.loads(Path(path).read_text())
    sorts = [v["sort"] for v in data["values"]]
    literals = [v["literal"] for v in data["values"]]
    rows = []
    for row in data["rows"]:
        args = []
        for a in row["args"]:
            if sorts[a] == "Math":
                args.append(["Math", f"v{a}"])
            else:
                args.append([sorts[a], literals[a] if literals[a] is not None else str(a)])
        rows.append({"op": row["op"], "args": args, "result": f"v{row['result']}"})
    return rows


def root_distance(classes, children, parents):
    roots = [c for c in classes if not parents[c]]
    if not roots:
        roots = sorted(classes)
    dist = {c: 0 for c in roots}
    queue = collections.deque(roots)
    while queue:
        c = queue.popleft()
        for child in children[c]:
            if child not in dist:
                dist[child] = dist[c] + 1
                queue.append(child)
    return dist, len(roots)


def leaf_height(classes, enodes):
    INF = float("inf")
    height = {c: INF for c in classes}
    for c in classes:
        if any(not [a for a in row["args"] if a[0] == "Math"] for row in enodes[c]):
            height[c] = 0
    changed = True
    while changed:
        changed = False
        for c in classes:
            best = INF
            for row in enodes[c]:
                kids = [norm(v) for s, v in row["args"] if s == "Math" and norm(v) in height]
                if kids and all(height[k] < INF for k in kids):
                    best = min(best, 1 + max(height[k] for k in kids))
            if best < height[c]:
                height[c] = best
                changed = True
    return height


def levels(values, dist):
    """Group e-node counts (kept in log2 space) by level."""
    buckets = collections.defaultdict(list)
    for c, level in dist.items():
        if level is None:
            continue
        buckets[level].append(values[c])
    out = []
    for level in sorted(buckets):
        counts = sorted(buckets[level])
        logs = sorted(math.log2(v) for v in counts)
        out.append(
            {
                "level": level,
                "classes": len(counts),
                "enodes_min": counts[0],
                "enodes_p25": counts[len(counts) // 4],
                "enodes_median": statistics.median(counts),
                "enodes_p75": counts[3 * len(counts) // 4],
                "enodes_max": counts[-1],
                "log2_samples": logs,
                "log2_min": logs[0],
                "log2_max": logs[-1],
            }
        )
    return out


def describe_rows(rows, label, path=""):
    enodes, children, parents, classes = index_rows(rows)
    counts = {c: len(v) for c, v in enodes.items()}
    rd, roots = root_distance(classes, children, parents)
    lh = leaf_height(classes, enodes)
    finite = {c: h for c, h in lh.items() if h < float("inf")}
    return {
        "label": label,
        "path": path,
        "classes": len(classes),
        "enodes": sum(counts.values()),
        "roots": roots,
        "root_distance": levels(counts, rd),
        "leaf_height": levels(counts, finite),
        "cyclic_classes": len(classes) - len(finite),
    }


def describe(path, label):
    return describe_rows(load_graph(path), label, str(path))


def describe_pool(paths, label):
    """Pool the interiors of many exported cells into one shape description."""
    values, root_level, leaf_level = {}, {}, {}
    per_cell = []
    for i, path in enumerate(paths):
        enodes, children, parents, classes = index_rows(composition_rows(path))
        counts = {c: len(v) for c, v in enodes.items()}
        rd, _roots = root_distance(classes, children, parents)
        lh = leaf_height(classes, enodes)
        for c in classes:
            key = (i, c)
            values[key] = counts[c]
            root_level[key] = rd.get(c)
            leaf_level[key] = lh[c] if lh[c] < float("inf") else None
        finite = [h for h in lh.values() if h < float("inf")]
        per_cell.append(
            {
                "path": str(path),
                "enodes": sum(counts.values()),
                "classes": len(classes),
                "root_depth": max(rd.values()) if rd else 0,
                "leaf_height": max(finite) if finite else 0,
            }
        )
    return {
        "label": label,
        "path": f"{len(paths)} cell exports",
        "classes": len(values),
        "enodes": sum(values.values()),
        "roots": None,
        "root_distance": levels(values, root_level),
        "leaf_height": levels(values, leaf_level),
        "cyclic_classes": 0,
        "per_cell": per_cell,
    }


# --------------------------------------------------------------------------- #
# heatmap rendering
# --------------------------------------------------------------------------- #


def histogram(rows):
    """counts per (level, log2 e-node bin); bin b covers [2^b, 2^(b+1))."""
    max_log = max(r["log2_max"] for r in rows)
    nbins = int(math.floor(max_log)) + 1
    grid = [[0] * nbins for _ in rows]
    for i, r in enumerate(rows):
        for s in r["log2_samples"]:
            b = min(nbins - 1, max(0, int(math.floor(s))))
            grid[i][b] += 1
    return nbins, grid


def ramp(t):
    t = max(0.0, min(1.0, t))
    pos = t * (len(RAMP) - 1)
    i = min(len(RAMP) - 2, int(pos))
    f = pos - i
    a, b = RAMP[i].lstrip("#"), RAMP[i + 1].lstrip("#")
    return "#%02x%02x%02x" % tuple(
        int(round(int(a[2 * k : 2 * k + 2], 16) + (int(b[2 * k : 2 * k + 2], 16) - int(a[2 * k : 2 * k + 2], 16)) * f))
        for k in range(3)
    )


def heatmap_panel(rows, x_label, title, note, ox, oy, w, h):
    parts = []
    nbins, grid = histogram(rows)
    n_x = len(rows)
    total = sum(sum(r) for r in grid)
    peak = max(max(r) for r in grid) or 1
    norm_max = math.log10(1 + peak)

    left = ox + 104
    top = oy + 92
    plot_w = w - 104 - 104
    plot_h = h - 92 - 112
    cw, ch = plot_w / n_x, plot_h / nbins

    # cells
    for i in range(n_x):
        for b in range(nbins):
            count = grid[i][b]
            cx = left + i * cw
            cy = top + plot_h - (b + 1) * ch
            colour = ramp(math.log10(1 + count) / norm_max) if count else "#f7f9fb"
            parts.append(f'<rect x="{cx:.1f}" y="{cy:.1f}" width="{cw:.1f}" height="{ch:.1f}" '
                         f'fill="{colour}" stroke="{GRID}" stroke-width="0.7"/>')
            if count and cw > 30 and ch > 15:
                dark = math.log10(1 + count) / norm_max > 0.55
                parts.append(f'<text x="{cx + cw / 2:.1f}" y="{cy + ch / 2 + 4:.1f}" text-anchor="middle" '
                             f'font-size="11" fill="{"#ffffff" if dark else INK}">{count}</text>')

    # y axis: log2 bins
    for b in range(nbins):
        cy = top + plot_h - (b + 0.5) * ch
        parts.append(f'<text x="{left - 10:.1f}" y="{cy + 4:.1f}" text-anchor="end" font-size="11" '
                     f'fill="{INK}">{2 ** b}</text>')
    parts.append(f'<text x="{left - 78:.1f}" y="{top - 10:.1f}" font-size="12" fill="{INK}">'
                 f'e-nodes/class</text>')
    parts.append(f'<text x="{left - 78:.1f}" y="{top + 6:.1f}" font-size="11" fill="{MUTED}">(log2 bin)</text>')

    # x axis: levels
    for i, r in enumerate(rows):
        cx = left + (i + 0.5) * cw
        parts.append(f'<text x="{cx:.1f}" y="{top + plot_h + 20:.1f}" text-anchor="middle" font-size="13" '
                     f'font-weight="600" fill="{INK}">{r["level"]}</text>')
        parts.append(f'<text x="{cx:.1f}" y="{top + plot_h + 35:.1f}" text-anchor="middle" font-size="10" '
                     f'fill="{MUTED}">{r["classes"]}</text>')
    parts.append(f'<text x="{left:.1f}" y="{top + plot_h + 58:.1f}" font-size="14" fill="{INK}">{x_label}'
                 f' (0 = closest to root / to the leaves)</text>')

    # top marginal: log10(#eclasses) per level
    mh = 52
    mmax = max(math.log10(max(1, r["classes"])) for r in rows) or 1.0
    for i, r in enumerate(rows):
        bar = mh * math.log10(max(1, r["classes"])) / mmax
        cx = left + i * cw + cw * 0.15
        parts.append(f'<rect x="{cx:.1f}" y="{top - bar - 6:.1f}" width="{cw * 0.7:.1f}" height="{bar:.1f}" '
                     f'fill="{ORANGE}" fill-opacity="0.55"/>')
    parts.append(f'<line x1="{left:.1f}" y1="{top - 6:.1f}" x2="{left + plot_w:.1f}" y2="{top - 6:.1f}" '
                 f'stroke="{INK}" stroke-width="1"/>')
    parts.append(f'<text x="{left:.1f}" y="{top - mh - 14:.1f}" font-size="11" fill="{ORANGE}">'
                 f'top: log10(#eclasses) per level (max {10 ** mmax:.0f})</text>')

    # right marginal: pooled e-node distribution
    mw = 72
    bin_counts = [sum(grid[i][b] for i in range(n_x)) for b in range(nbins)]
    bmax = max(bin_counts) or 1
    for b in range(nbins):
        width = mw * bin_counts[b] / bmax
        cy = top + plot_h - (b + 1) * ch
        parts.append(f'<rect x="{left + plot_w + 6:.1f}" y="{cy + 1:.1f}" width="{width:.1f}" '
                     f'height="{ch - 2:.1f}" fill="{BLUE}" fill-opacity="0.55"/>')
        if bin_counts[b]:
            parts.append(f'<text x="{left + plot_w + 10 + width:.1f}" y="{cy + ch / 2 + 4:.1f}" font-size="10" '
                         f'fill="{MUTED}">{bin_counts[b]}</text>')
    parts.append(f'<text x="{left + plot_w + 6:.1f}" y="{top - 14:.1f}" font-size="11" fill="{BLUE}">'
                 f'size marginal</text>')

    # colour legend
    lx, ly, lw, lh = left + plot_w - 190, oy + h - 44, 150, 12
    for k in range(60):
        parts.append(f'<rect x="{lx + lw * k / 60:.1f}" y="{ly:.1f}" width="{lw / 60 + 0.6:.1f}" '
                     f'height="{lh}" fill="{ramp(k / 59)}"/>')
    for tick in range(0, int(math.ceil(norm_max)) + 1):
        tx = lx + lw * tick / norm_max
        parts.append(f'<text x="{tx:.1f}" y="{ly + lh + 12:.1f}" text-anchor="middle" font-size="10" '
                     f'fill="{MUTED}">{10 ** tick}</text>')
    parts.append(f'<text x="{lx + lw + 14:.1f}" y="{ly + 10:.1f}" font-size="11" fill="{MUTED}">'
                 f'#eclasses per cell ({n_x}×{nbins} cells, {total} classes)</text>')

    parts.append(f'<text x="{ox + 16:.1f}" y="{oy + 26:.1f}" font-size="17" fill="{INK}" '
                 f'font-weight="600">{title}</text>')
    parts.append(f'<text x="{ox + 16:.1f}" y="{oy + 45:.1f}" font-size="12" fill="{MUTED}">{note}</text>')
    return parts


def note_for(key, rows):
    if not rows:
        return "no classes"
    if key == "root_distance" and len(rows) <= 2:
        return "degenerate at the root: one top e-class, every other class is its direct child"
    if key == "leaf_height":
        top = rows[-1]
        return (f"highest level: {top['classes']} class(es), up to {top['enodes_max']} e-nodes each; "
                f"cell text = #eclasses")
    return "cell text = #eclasses in that (level, size) bin"


def kde(samples, points, bandwidth=0.22):
    if not samples:
        return [0.0] * len(points)
    factor = 1.0 / (len(samples) * bandwidth * math.sqrt(2 * math.pi))
    out = []
    for p in points:
        total = 0.0
        for s in samples:
            d = (p - s) / bandwidth
            total += math.exp(-0.5 * d * d)
        out.append(total * factor)
    return out


def violin_panel(rows, x_label, title, note, ox, oy, w, h):
    """The earlier per-level distribution view, for comparison."""
    parts = []
    n = max(1, len(rows) - 1)
    y_max = max((math.log10(max(1, r["classes"])) for r in rows), default=1.0)
    plot_w = w - 260
    sy = 0.30 * plot_w / (0.52 * max(1.0, y_max))
    sx = (plot_w - 0.30 * plot_w) / n
    sz = (h - 170) / 11.0
    accent = BLUE if x_label == "root distance" else ORANGE

    def point(level, log_classes, log2_enodes):
        return (ox + 150 + level * sx + 0.52 * log_classes * sy,
                oy + h - 80 - 0.30 * log_classes * sy - log2_enodes * sz)

    floor_y = max(point(r["level"], 0, 0)[1] for r in rows)
    for r in rows:
        y = math.log10(max(1, r["classes"]))
        lo, hi = max(0.0, r["log2_min"] - 0.8), r["log2_max"] + 0.8
        pts = [lo + (hi - lo) * k / 48 for k in range(49)]
        dens = kde(r["log2_samples"], pts)
        peak = max(dens) or 1.0
        half = 0.40 * sx
        left, right = [], []
        for p, d in zip(pts, dens):
            x, yy = point(r["level"], y, p)
            left.append((x - half * d / peak, yy))
            right.append((x + half * d / peak, yy))
        polygon = " ".join(f"{x:.1f},{y:.1f}" for x, y in left + list(reversed(right)))
        parts.append(f'<polygon points="{polygon}" fill="{accent}" fill-opacity="0.45" '
                     f'stroke="{INK}" stroke-width="0.9"/>')
        base = point(r["level"], y, 0)
        parts.append(f'<text x="{base[0]:.1f}" y="{base[1] + 16:.1f}" text-anchor="middle" font-size="11" '
                     f'fill="{INK}">{r["classes"]}</text>')
        lvl = point(r["level"], y, -1.3)
        parts.append(f'<text x="{lvl[0]:.1f}" y="{lvl[1] + 14:.1f}" text-anchor="middle" font-size="12" '
                     f'font-weight="600" fill="{INK}">{r["level"]}</text>')
    parts.append(f'<line x1="{point(0, 0, 0)[0]:.1f}" y1="{point(0, 0, 0)[1]:.1f}" '
                 f'x2="{point(n, 0, 0)[0]:.1f}" y2="{point(n, 0, 0)[1]:.1f}" stroke="{INK}" stroke-width="1.4"/>')
    parts.append(f'<text x="{ox + w / 2:.1f}" y="{floor_y + 40:.1f}" text-anchor="middle" font-size="13" '
                 f'fill="{INK}">{x_label} (violin = e-nodes/class distribution)</text>')
    parts.append(f'<text x="{ox + 16:.1f}" y="{oy + 26:.1f}" font-size="17" fill="{INK}" '
                 f'font-weight="600">{title}</text>')
    parts.append(f'<text x="{ox + 16:.1f}" y="{oy + 45:.1f}" font-size="12" fill="{MUTED}">{note}</text>')
    return parts


def render(descriptions, out, mode, width=1760):
    row_h = 560
    height = 70 + row_h * len(descriptions) + 30
    body = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" width="{width}" '
            f'height="{height}" role="img" aria-label="e-graph shape heatmap">',
            f'<rect width="{width}" height="{height}" fill="#ffffff"/>',
            f'<text x="40" y="38" font-size="22" fill="{INK}" font-weight="700">'
            f'e-graph shape: level × log2(e-nodes per e-class) × log10(#eclasses)</text>']
    for i, desc in enumerate(descriptions):
        for j, key in enumerate(("root_distance", "leaf_height")):
            rows = desc[key]
            ox = 40 + j * (width // 2)
            oy = 70 + i * row_h
            x_label = "root distance" if key == "root_distance" else "leaf height"
            title = (f'{desc["label"]} — x = distance from root e-class' if key == "root_distance"
                     else f'{desc["label"]} — x = height above leaves')
            note = note_for(key, rows)
            body.append("<g>")
            if mode == "heatmap":
                body.extend(heatmap_panel(rows, x_label, title, note, ox, oy, width // 2 - 60, row_h - 60))
            else:
                body.extend(violin_panel(rows, x_label, title, note, ox, oy, width // 2 - 60, row_h - 40))
            body.append("</g>")
    body.append("</svg>")
    Path(out).write_text("\n".join(body) + "\n")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--graph", action="append", help="path[:label]")
    p.add_argument("--cells", type=Path, default=None,
                   help="directory tree of */saturated-rule-composition.json cell exports to pool")
    p.add_argument("--mode", choices=("heatmap", "violin"), default="heatmap")
    p.add_argument("--out", type=Path, default=ROOT / "out/egraph-shape/shape.svg")
    p.add_argument("--data", type=Path, default=None, help="also write the level statistics as JSON")
    a = p.parse_args()
    descriptions = []
    if a.cells:
        paths = sorted(globmod.glob(str(a.cells / "*/saturated-rule-composition.json")))
        if not paths:
            raise SystemExit(f"no cell exports under {a.cells}")
        descriptions.append(describe_pool([Path(x) for x in paths],
                                          f"ripened cell interiors ({len(paths)} exports)"))
    for spec in (a.graph or []):
        path, _, label = spec.partition(":")
        full = Path(path) if Path(path).is_absolute() else ROOT / path
        descriptions.append(describe(full, label or Path(path).stem))
    if not descriptions:
        raise SystemExit("nothing to draw: pass --graph and/or --cells")
    a.out.parent.mkdir(parents=True, exist_ok=True)
    render(descriptions, a.out, a.mode)
    if a.data:
        trimmed = json.loads(json.dumps(descriptions))
        for desc in trimmed:
            for key in ("root_distance", "leaf_height"):
                for level in desc[key]:
                    level.pop("log2_samples", None)
        a.data.write_text(json.dumps(trimmed, indent=2) + "\n")
    for d in descriptions:
        print(f'{d["label"]}: classes={d["classes"]} enodes={d["enodes"]} roots={d["roots"]} '
              f'root-levels={len(d["root_distance"])} leaf-levels={len(d["leaf_height"])}')
    print(f"wrote {a.out} ({a.mode})" + (f" and {a.data}" if a.data else ""))


if __name__ == "__main__":
    main()
