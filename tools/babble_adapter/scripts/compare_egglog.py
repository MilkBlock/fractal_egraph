"""Differential test of native egglog AU/matching against pinned upstream babble."""
import os
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
def run(command, **kwargs):
    subprocess.run(command, cwd=ROOT, check=True, **kwargs)

run([sys.executable, str(Path(__file__).with_name("bootstrap.py"))])
env = dict(os.environ, CARGO_INCREMENTAL="0")
run(["cargo", "build", "--release", "--locked", "--manifest-path", "tools/babble_adapter/Cargo.toml"])
run(["cargo", "run", "--bin", "babble_corpus", "--",
     "experiments/binding_program/math.json", "experiments/annotated_export/math_microbenchmark/profile.json",
     "experiments/babble/corpus.json"], env=env)
output = ROOT / "experiments/egglog_babble"
output.mkdir(exist_ok=True)
run(["tools/babble_adapter/target/release/combine-babble", "experiments/babble/corpus.json",
     "experiments/egglog_babble/math.fixture.json", "au-reference"], timeout=180)
run(["cargo", "build", "--release", "--bin", "egglog_babble"], env=env)
run(["target/release/egglog_babble", "experiments/egglog_babble/math.fixture.json",
     "experiments/egglog_babble/results.json"], timeout=180)
binary = "tools/babble_adapter/target/release/combine-babble"
run([binary, "experiments/babble/corpus.json", "experiments/egglog_babble/upstream.json"], timeout=180)
run([binary, "experiments/babble/corpus.json", "experiments/egglog_babble/hybrid.json",
     "candidates=experiments/egglog_babble/results.json.candidates.json"], timeout=180)
baseline = json.loads((output / "upstream.json").read_text())
hybrid = json.loads((output / "hybrid.json").read_text())
definitions = lambda r: {str(x["id"]): x["definition"] for x in r["libraries"]}
comparison = {
    "scope": "same syntax corpus and upstream selector; native co-occurrence, AU and matching; not independent workloads",
    "train_cost_equal": baseline["train"] == hybrid["train"],
    "test_cost_equal": baseline["test"] == hybrid["test"],
    "library_count_equal": baseline["selected_train_libraries"] == hybrid["selected_train_libraries"],
    "selected_definitions_equal": definitions(baseline) == definitions(hybrid),
    "both_expansions_verified": all(baseline["lossless_expansion"].values()) and all(hybrid["lossless_expansion"].values()),
    "train": hybrid["train"], "test": hybrid["test"],
}
(output / "comparison.json").write_text(json.dumps(comparison, indent=2) + "\n")
assert comparison["train_cost_equal"] and comparison["test_cost_equal"]
assert comparison["library_count_equal"] and comparison["selected_definitions_equal"]
assert comparison["both_expansions_verified"]
