"""Differential test of native egglog AU/matching against pinned upstream babble."""
import os
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
