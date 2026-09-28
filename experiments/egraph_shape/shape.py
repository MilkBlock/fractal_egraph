#!/usr/bin/env python3
"""Draw the e-class shape of an e-graph as a 3D statistical figure.

For every e-class we compute
  * `root distance` -- shortest number of constructor steps from the top e-class
    (roots are classes nobody points at); this is the x axis the user asked for,
  * `leaf height`   -- shortest term height above the leaves (0 = a class that has
    an e-node with no Math arguments); this is the dual x axis,
  * e-nodes per e-class (the z axis, plotted as log2),
  * e-classes per level (the y axis, plotted as log10).

Each violin is drawn in the (x, z) plane and placed at depth y = log10(#classes),
so a single panel shows a distribution (z) per level (x) at its class count (y).

Input is the constructor-row export used elsewhere in this repository:
a list of {"op", "args": [[sort, value], ...], "result"} rows.
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
GRID = "#dbe3ea"
BLUE = "#2678a9"
ORANGE = "#d77819"
TEAL = "#2a9d8f"


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
    return index_rows(json.loads(Path(path).read_text()))


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
    """Group e-node counts (in log2 space) by level."""
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


def describe(path, label):
    return describe_rows(load_graph(path), label, str(path))


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


def describe_pool(paths, label):
    """Pool the interiors of many exported cells into one shape description."""
    values, root_level, leaf_level = {}, {}, {}
    per_cell = []
    for i, path in enumerate(paths):
        rows = composition_rows(path)
        enodes, children, parents, classes = index_rows(rows)
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
        "cyclic_classes": sum(1 for v in values if leaf_level.get((0, 0)) is None) * 0,
        "per_cell": per_cell,
    }


def kde(samples, points, bandwidth=0.22):
    if not samples:
        return [0.0] * len(points)
    norm_factor = 1.0 / (len(samples) * bandwidth * math.sqrt(2 * math.pi))
    out = []
    for p in points:
        total = 0.0
        for s in samples:
            d = (p - s) / bandwidth
            total += math.exp(-0.5 * d * d)
        out.append(total * norm_factor)
    return out


class Panel:
    """Isometric projection: x = level, y = depth (log10 classes), z = log2 e-nodes."""

    def __init__(self, ox, oy, width, height, x_steps, y_max, z_max):
        self.ox, self.oy, self.w, self.h = ox, oy, width, height
        self.x_steps = max(1, x_steps)
        self.y_max = max(1.0, y_max)
        self.z_max = max(4.0, z_max)
        plot_w = width - 190
        depth_budget = 0.30 * plot_w
        self.sy = depth_budget / (0.52 * self.y_max)
        self.sx = (plot_w - depth_budget) / (self.x_steps + 0.9)
        self.sz = (height - 150) / self.z_max

    def project(self, level, log_classes, log2_enodes):
        return (
            self.ox + 110 + level * self.sx + 0.52 * log_classes * self.sy,
            self.oy + self.h - 66 - 0.30 * log_classes * self.sy - log2_enodes * self.sz,
        )


def fmt(value):
    if value >= 1000:
        return f"{value / 1000:.1f}k"
    return str(value)


def draw_panel(rows, x_key, title, note, panel, accent):
    parts = []
    ox, oy = panel.ox, panel.oy
    max_level = max(r["level"] for r in rows) if rows else 0
    y_max = max(math.log10(max(1, r["classes"])) for r in rows) if rows else 1.0
    z_max = max(r["log2_max"] for r in rows) + 0.8 if rows else 4.0

    for r in rows:
        x0, y0 = panel.project(r["level"], 0, 0)
        x1, y1 = panel.project(r["level"], 0, z_max)
        parts.append(f'<line x1="{x0:.1f}" y1="{y0:.1f}" x2="{x1:.1f}" y2="{y1:.1f}" stroke="{GRID}" stroke-width="1"/>')
    spine = " ".join(
        "%.1f,%.1f" % panel.project(r["level"], math.log10(max(1, r["classes"])), 0) for r in rows
    )
    parts.append(f'<polyline points="{spine}" fill="none" stroke="{accent}" stroke-width="2.4"/>')

    spacing = panel.sx
    for r in rows:
        y = math.log10(max(1, r["classes"]))
        lo = max(0.0, r["log2_min"] - 0.8)
        hi = r["log2_max"] + 0.8
        points = [lo + (hi - lo) * i / 48 for i in range(49)]
        dens = kde(r["log2_samples"], points)
        peak = max(dens) or 1.0
        half = 0.40 * spacing
        left, right = [], []
        for p, d in zip(points, dens):
            x, yy = panel.project(r["level"], y, p)
            w = half * (d / peak)
            left.append((x - w, yy))
            right.append((x + w, yy))
        polygon = left + list(reversed(right))
        pts = " ".join(f"{x:.1f},{y:.1f}" for x, y in polygon)
        shade = 0.30 + 0.42 * min(1.0, y / 3.0)
        parts.append(
            f'<polygon points="{pts}" fill="{accent}" fill-opacity="{shade:.2f}" '
            f'stroke="{INK}" stroke-width="0.9"/>'
        )
        for value, weight in ((r["log2_min"], 1.0), (r["log2_max"], 1.0), (math.log2(r["enodes_median"]), 2.2)):
            x, yy = panel.project(r["level"], y, value)
            parts.append(f'<line x1="{x - half * 0.5:.1f}" y1="{yy:.1f}" x2="{x + half * 0.5:.1f}" y2="{yy:.1f}" '
                         f'stroke="{INK}" stroke-width="{weight}"/>')
        bx, by = panel.project(r["level"], y, -0.30)
        parts.append(
            f'<text x="{bx:.1f}" y="{by + 14:.1f}" text-anchor="middle" font-size="12" fill="{INK}">'
            f'{r["classes"]}</text>'
        )
        lx, ly = panel.project(r["level"], y, -1.35)
        parts.append(
            f'<text x="{lx:.1f}" y="{ly + 14:.1f}" text-anchor="middle" font-size="13" fill="{INK}" '
            f'font-weight="600">{r["level"]}</text>'
        )

    # x axis
    x0, y0 = panel.project(0, 0, 0)
    x1, y1 = panel.project(max_level, 0, 0)
    parts.append(f'<line x1="{x0:.1f}" y1="{y0:.1f}" x2="{x1:.1f}" y2="{y1:.1f}" stroke="{INK}" stroke-width="1.6"/>')
    parts.append(f'<text x="{(x0 + x1) / 2:.1f}" y="{max(y0, y1) + 46:.1f}" text-anchor="middle" '
                 f'font-size="14" fill="{INK}">{x_key}</text>')
    # z axis
    zx0, zy0 = panel.project(0, 0, 0)
    zx1, zy1 = panel.project(0, 0, z_max)
    parts.append(f'<line x1="{zx0:.1f}" y1="{zy0:.1f}" x2="{zx1:.1f}" y2="{zy1:.1f}" stroke="{INK}" stroke-width="1.6"/>')
    for tick in range(0, int(z_max) + 1):
        x, y = panel.project(0, 0, tick)
        parts.append(f'<line x1="{x:.1f}" y1="{y:.1f}" x2="{x - 6:.1f}" y2="{y:.1f}" stroke="{INK}" stroke-width="1"/>')
        parts.append(f'<text x="{x - 10:.1f}" y="{y + 4:.1f}" text-anchor="end" font-size="11" fill="{INK}">'
                     f'{fmt(2 ** tick)}</text>')
    parts.append(f'<text x="{zx0 - 84:.1f}" y="{zy1 - 12:.1f}" font-size="13" fill="{INK}">e-nodes/class (log2)</text>')
    # depth axis (log10 class count)
    dx0, dy0 = panel.project(max_level, 0, 0)
    dx1, dy1 = panel.project(max_level, y_max, 0)
    parts.append(f'<line x1="{dx0:.1f}" y1="{dy0:.1f}" x2="{dx1:.1f}" y2="{dy1:.1f}" stroke="{INK}" stroke-width="1.6"/>')
    for lc in range(0, int(y_max) + 1):
        x, y = panel.project(max_level, lc, 0)
        parts.append(f'<line x1="{x:.1f}" y1="{y:.1f}" x2="{x + 5:.1f}" y2="{y - 3:.1f}" stroke="{INK}" stroke-width="1"/>')
        parts.append(f'<text x="{x + 9:.1f}" y="{y + 5:.1f}" font-size="11" fill="{INK}">{fmt(10 ** lc)}</text>')
    lx, ly = panel.project(max_level, y_max, 0)
    parts.append(f'<text x="{lx + 8:.1f}" y="{ly - 20:.1f}" font-size="13" fill="{INK}">#eclasses (log10)</text>')
    parts.append(f'<text x="{ox + 16:.1f}" y="{oy + 26:.1f}" font-size="17" fill="{INK}" font-weight="600">{title}</text>')
    parts.append(f'<text x="{ox + 16:.1f}" y="{oy + 45:.1f}" font-size="12" fill="#5b6b7a">{note}</text>')
    return parts


def note_for(key, rows, desc):
    if not rows:
        return "no classes"
    if key == "root_distance" and len(rows) <= 2:
        return "degenerate at the root: one top e-class, every other class is its direct child"
    if key == "leaf_height":
        top = rows[-1]
        return (f"highest level holds {top['classes']} class(es) with up to "
                f"{fmt(top['enodes_max'])} e-nodes each")
    return "violin width = density of e-nodes/class at that level; label at the base = #eclasses"


def render(descriptions, out, width=1760):
    panels = []
    row_h = 580
    for i, desc in enumerate(descriptions):
        for j, key in enumerate(("root_distance", "leaf_height")):
            rows = desc[key]
            ox = 40 + j * (width // 2)
            oy = 70 + i * row_h
            y_max = max((math.log10(max(1, r["classes"])) for r in rows), default=1.0)
            z_max = max((r["log2_max"] + 0.8 for r in rows), default=4.0)
            panel = Panel(ox, oy, width // 2 - 60, row_h - 90, len(rows) - 1, y_max, z_max)
            title = (
                f'{desc["label"]} — x = distance from root e-class'
                if key == "root_distance"
                else f'{desc["label"]} — x = height above leaves'
            )
            accent = BLUE if key == "root_distance" else ORANGE
            panels.append(
                (
                    title,
                    rows,
                    "root distance" if key == "root_distance" else "leaf height",
                    note_for(key, rows, desc),
                    panel,
                    accent,
                )
            )

    height = 70 + row_h * len(descriptions) + 40
    body = [f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {width} {height}" '
            f'width="{width}" height="{height}" role="img" '
            f'aria-label="e-class shape: level vs class count vs e-nodes per class">',
            f'<rect width="{width}" height="{height}" fill="#ffffff"/>',
            f'<text x="40" y="38" font-size="22" fill="{INK}" font-weight="700">'
            f'e-graph shape: level × log10(#eclasses) × distribution of e-nodes per e-class</text>']
    for title, rows, x_key, note, panel, accent in panels:
        body.append('<g>')
        body.extend(draw_panel(rows, x_key, title, note, panel, accent))
        body.append("</g>")
    body.append("</svg>")
    Path(out).write_text("\n".join(body) + "\n")


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--graph", action="append", help="path[:label]")
    p.add_argument("--out", type=Path, default=ROOT / "out/egraph-shape/shape.svg")
    p.add_argument("--data", type=Path, default=None, help="also write the level statistics as JSON")
    p.add_argument("--cells", type=Path, default=None,
                   help="directory tree of */saturated-rule-composition.json cell exports to pool")
    a = p.parse_args()
    descriptions = []
    if a.cells:
        paths = sorted(globmod.glob(str(a.cells / "*/saturated-rule-composition.json")))
        if not paths:
            raise SystemExit(f"no cell exports under {a.cells}")
        descriptions.append(describe_pool([Path(x) for x in paths], f"ripened cell interiors ({len(paths)} exports)"))
    for spec in (a.graph or []):
        path, _, label = spec.partition(":")
        descriptions.append(describe(ROOT / path if not Path(path).is_absolute() else Path(path), label or Path(path).stem))
    a.out.parent.mkdir(parents=True, exist_ok=True)
    render(descriptions, a.out)
    if a.data:
        # the raw per-class log2 samples are only needed for drawing; keep the summary
        trimmed = json.loads(json.dumps(descriptions))
        for desc in trimmed:
            for key in ("root_distance", "leaf_height"):
                for level in desc[key]:
                    level.pop("log2_samples", None)
        a.data.write_text(json.dumps(trimmed, indent=2) + "\n")
    for d in descriptions:
        print(f'{d["label"]}: classes={d["classes"]} enodes={d["enodes"]} roots={d["roots"]} '
              f'root-levels={len(d["root_distance"])} leaf-levels={len(d["leaf_height"])}')
    print(f"wrote {a.out}" + (f" and {a.data}" if a.data else ""))


if __name__ == "__main__":
    main()
