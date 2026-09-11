"""Reproduce both encodings with a bounded upstream learner process."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
def run(command, **kwargs):
    subprocess.run(command, cwd=ROOT, check=True, **kwargs)

run([sys.executable, str(Path(__file__).with_name("bootstrap.py"))])
(ROOT / "experiments/babble").mkdir(exist_ok=True)
env = dict(os.environ, CARGO_INCREMENTAL="0")
run(["cargo", "run", "--bin", "babble_corpus", "--",
     "experiments/binding_program/math.json",
     "experiments/annotated_export/math_microbenchmark/profile.json",
     "experiments/babble/corpus.json"], env=env)
run(["cargo", "build", "--release", "--locked", "--manifest-path", "tools/babble_adapter/Cargo.toml"])
binary = "tools/babble_adapter/target/release/combine-babble"
run([binary, "experiments/babble/corpus.json", "experiments/babble/json-control.json", "json-control"], timeout=180)
run([binary, "experiments/babble/corpus.json", "experiments/babble/results.json"], timeout=180)
