#!/usr/bin/env python3
"""Four-way comparison: whole-fragment discovery vs committed-event recognition.

Variants, all on the same source and schedule:
  1. pristine        - the recorded baseline binary
  2. native          - this kernel, no library
  3. fragment-pack   - whole-fragment discovery, then pack every match
  4. recognition     - event recognition, then install recognised cells

Variant 4 must report `fragment_queries: 0`; that is the point of the change.
Timing is one screening pass per variant and is not a stable speed claim.
"""
import argparse, json, os, re, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAIN = ROOT.parent  # the checkout that holds the recorded pristine binary


def measure(command, out_prefix, env=None):
    e = dict(os.environ)
    e.update(env or {})
    e["EGG_LAYOUT_GRAPHS"] = "0"
    e["EGG_LAYOUT_EAGER_SVG"] = "0"
    start = time.monotonic()
    proc = subprocess.run(
        ["/usr/bin/time", "-l", *map(str, command)],
        cwd=ROOT,
        env=e,
        capture_output=True,
        text=True,
    )
    seconds = time.monotonic() - start
    rss = None
    match = re.search(r"(\d+)\s+maximum resident set size", proc.stderr)
    if match:
        rss = int(match.group(1))
    out_prefix.parent.mkdir(parents=True, exist_ok=True)
    out_prefix.with_suffix(".stdout").write_text(proc.stdout)
    out_prefix.with_suffix(".stderr").write_text(proc.stderr)
    return {
        "seconds": seconds,
        "peak_rss_bytes": rss,
        "exit_code": proc.returncode,
        "stdout": proc.stdout,
    }


def recognition_report(stdout):
    """The commands log first and print one JSON report last; it may be pretty
    printed (`recognize-cells`) or compact (`adaptive_math`)."""
    lines = stdout.splitlines()
    for i in range(len(lines) - 1, -1, -1):
        if not lines[i].lstrip().startswith("{"):
            continue
        try:
            return json.JSONDecoder().raw_decode("\n".join(lines[i:]))[0]
        except json.JSONDecodeError:
            continue
    return None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", required=True)
    parser.add_argument("--library", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--rounds", type=int, default=1)
    parser.add_argument("--pristine", default=str(MAIN / "out/native-compare/bin/pristine"))
    args = parser.parse_args()

    source = Path(args.source)
    library = Path(args.library)
    out = Path(args.output).resolve()
    out.mkdir(parents=True, exist_ok=True)
    # Honour a shared target directory: this checkout may be built with
    # CARGO_TARGET_DIR pointing at another checkout's target.
    target = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))).resolve()
    bench = target / "release/examples/post_pack_bench"
    kernel = target / "release/egg_layout"

    report = {
        "scope": "same source and schedule; one screening pass per variant; not a speed claim",
        "source": str(source),
        "source_sha256": __import__("hashlib").sha256(source.read_bytes()).hexdigest(),
        "library": str(library),
        "rounds": args.rounds,
        "runs": {},
    }

    for variant, command, env in [
        ("pristine", [args.pristine, source], {}),
        ("native", [bench, source], {}),
        (
            "fragment_pack",
            [bench, source],
            {"MEMORY_FIX_LIBRARY": str(library), "MEMORY_FIX_MODE": "pack"},
        ),
    ]:
        run = measure(command, out / variant, env)
        report["runs"][variant] = {
            "seconds": run["seconds"],
            "peak_rss_bytes": run["peak_rss_bytes"],
            "exit_code": run["exit_code"],
        }
        print(variant, round(run["seconds"], 3), run["peak_rss_bytes"], flush=True)

    # RipSat variants: the adaptive runner executes under a trace when the audit
    # enclosure audit is on (EGG_LAYOUT_AUDIT_ENCLOSURES), which is what attaches a\    # trace to the round execution and makes per-row provenance available. `native` runs
    # the source rules only; `frozen` replays the baked closures instead of the
    # source RHSs, so the two rows differ exactly by which rules produced nodes.
    adaptive = target / "release/examples/adaptive_math"
    bake = ROOT / "experiments/math_mini3/adaptive_runtime/bake_lib_round2.json"
    for variant, mode, extra in [
        ("ripsat_off", "native", []),
        ("ripsat_on", "frozen", [bake]),
    ]:
        run_out = out / variant
        if run_out.exists():
            import shutil

            shutil.rmtree(run_out)
        run = measure(
            [adaptive, mode, source, run_out, *extra],
            out / variant,
            {"EGG_LAYOUT_AUDIT_ENCLOSURES": "1"},
        )
        parsed = recognition_report(run["stdout"]) or {}
        report["runs"][variant] = {
            "seconds": run["seconds"],
            "peak_rss_bytes": run["peak_rss_bytes"],
            "exit_code": run["exit_code"],
            "mode": mode,
            "node_origin": parsed.get("node_origin"),
        }
        print(
            variant,
            round(run["seconds"], 3),
            (parsed.get("node_origin") or {}).get("total"),
            flush=True,
        )

    cell_out = out / "recognition-out"
    if cell_out.exists():
        import shutil

        shutil.rmtree(cell_out)
    run = measure(
        [kernel, "recognize-cells", source, library, cell_out, args.rounds],
        out / "recognition",
    )
    parsed = recognition_report(run["stdout"]) or {}
    report["runs"]["recognition"] = {
        "seconds": run["seconds"],
        "peak_rss_bytes": run["peak_rss_bytes"],
        "exit_code": run["exit_code"],
        "fragment_queries": parsed.get("fragment_queries"),
        "plans": parsed.get("plans"),
        "sources": parsed.get("sources"),
        "slot_boundary_sources": parsed.get("slot_boundary_sources"),
        "recognition": parsed.get("recognition"),
        "failures": parsed.get("failures"),
        "analysis_status": parsed.get("analysis_status"),
    }
    print("recognition", round(run["seconds"], 3), parsed.get("fragment_queries"), flush=True)

    report["default_path_fragment_queries"] = report["runs"]["recognition"]["fragment_queries"]
    (out / "results.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report["runs"], indent=2))
    # The acceptance condition of this change: the default path issues none.
    if report["default_path_fragment_queries"] != 0:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
