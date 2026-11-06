#!/bin/sh
# Reproduce the two-workload memory arms with existing frozen artifacts.
set -eu
if [ "$#" -ne 4 ]; then
  echo "usage: memory-replay.sh CODE_ROOT SUM_PROGRAM RAY_PROGRAM NEW_OUTPUT" >&2
  exit 2
fi
code=$(cd "$1" && pwd)
sum_program=$2
ray_program=$3
output=$4
case "$sum_program:$ray_program:$output" in /*:/*:/*) ;; *) echo "use absolute input and output paths" >&2; exit 2;; esac
test ! -e "$output"
mkdir -p "$output"
git -C "$code" rev-parse HEAD >"$output/source-commit.txt"
build="$output/cargo-target"
cd "$code"
CARGO_TARGET_DIR="$build" cargo build --locked --release -p eggfuse-cell-runtime --features sparse-cells,view-ports,storage-metrics
cp "$build/release/eggfuse-cell-runtime" "$output/control"
CARGO_TARGET_DIR="$build" cargo build --locked --release -p eggfuse-cell-runtime --features entry-retirement,origin-demand,view-ports,storage-metrics
cp "$build/release/eggfuse-cell-runtime" "$output/reclaimed"
for workload in sum-bits raytrace; do
  if [ "$workload" = sum-bits ]; then program=$sum_program; else program=$ray_program; fi
  "$output/control" --program "$program" --output "$output/$workload-control"
  "$output/reclaimed" --program "$program" --entry-retire-interval 32 --release-source-artifact --output "$output/$workload-reclaimed"
done
