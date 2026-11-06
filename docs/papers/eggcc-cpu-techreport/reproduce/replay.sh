#!/bin/sh
# Usage: replay.sh /absolute/path/to/eggfuse NEW_OUTPUT_DIRECTORY [one|suite|anchors]
set -eu
repo=$(cd "$1" && pwd)
out_arg=$2
mode=${3:-one}
here=$(CDPATH= cd "$(dirname "$0")" && pwd)
test "$(git -C "$repo" rev-parse HEAD)" = 70b291077252a8a6cfd9a00ebdd98f9599cd0c62 || { echo "Checkout eggfuse commit 70b2910 first" >&2; exit 1; }
mkdir "$out_arg"
out=$(cd "$out_arg" && pwd)
mkdir -p "$out/native" "$out/link/src" "$out/capture" "$out/tmp"
export CARGO_TARGET_DIR="$out/cargo-target"
cargo build --release --locked --manifest-path "$repo/tools/cell-compiler/Cargo.toml"
cargo build --release --locked --manifest-path "$repo/engine/cell-runtime/Cargo.toml" --features iterative-bake
git -C "$repo" archive egglog-baseline egglog | tar -x -C "$out/native"
bench="$repo/tools/cell-compiler/benchmarks/free-join"
(cd "$out/native" && git apply --unidiff-zero "$bench/count.patch")
mkdir -p "$out/native/egglog/examples"
cp "$bench/count_fj.rs" "$out/native/egglog/examples/"
unset EGGLOG_COUNT_FREE_JOIN
cargo +stable build --release --locked --no-default-features --manifest-path "$out/native/egglog/Cargo.toml" --example count_fj
cp "$here/transfer.rs" "$out/link/src/main.rs"
cp "$here/transfer.lock" "$out/link/Cargo.lock"
python3 - "$repo" "$out/link/Cargo.toml" <<'PY'
import sys,json,pathlib
path=pathlib.Path(sys.argv[1])/'engine/cell-runtime'
pathlib.Path(sys.argv[2]).write_text('[package]\nname="report-transfer"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nserde_json="1"\neggfuse-cell-runtime={path='+json.dumps(str(path))+', features=["iterative-bake"]}\n')
PY
cargo build --release --locked --manifest-path "$out/link/Cargo.toml"
tar -xzf "$here/captures.tar.gz" -C "$out/capture"
gzip -dc "$here/donor.json.gz" > "$out/donor.json"
cat "$repo/experiments/profile_suite/inputs/common-prefix.egg" "$repo/experiments/profile_suite/inputs/bake.eggpart" > "$out/bake.egg"
bin="$CARGO_TARGET_DIR/release"
export RAYON_NUM_THREADS=1 CELL_CPU_STAGE_SPINE=1 CELL_CPU_STAGE_SPINE_RECEIPTS=1 CELL_CPU_SHARED_NODE_SCHEDULE=1
export CELL_CPU_FREE_JOIN=planned CELL_CPU_JOIN_INTERSECTION=1 CELL_CPU_SIEVE=1 CELL_CPU_KNOWN_RESULTS=static
export CELL_CPU_RANGE_GUARDS=1 CELL_CPU_FD_KEYS=0 CELL_CPU_SCALAR_MERGE=0 CELL_CPU_SLOT_TRANSFER=off
case "$mode" in
 one)
  src="$out/capture/032-sum-bits/pass-3-flat.egg"
  "$bin/examples/count_fj" "$src"
  "$bin/eggfuse-cell-compiler" compile --source "$src" --bake-source "$out/bake.egg" --emit-free-join-entry --output "$out/compiled"
  "$bin/report-transfer" "$out/donor.json" "$out/compiled/program.json" "$out/ready.json"
  "$bin/eggfuse-cell-runtime" --program "$out/ready.json" --output "$out/measured";;
 suite|anchors)
  script=sweep.py; results=suite.jsonl
  if [ "$mode" = anchors ]; then script=paired.py; results=paired.json; fi
  python3 "$here/$script" "$out/capture" "$bin/eggfuse-cell-compiler" "$out/bake.egg" "$out/donor.json" "$bin/report-transfer" "$bin/examples/count_fj" "$bin/eggfuse-cell-runtime" "$out/tmp" "$out/$results";;
 *) echo "Choose one, suite or anchors" >&2; exit 1;;
esac
