#!/bin/sh
# Run the current, matched native/Cell full-suite study in fresh output storage.
set -eu
if [ "$#" -ne 2 ]; then
  echo "usage: refresh-suite.sh CODE_ROOT NEW_OUTPUT_DIRECTORY" >&2
  exit 2
fi
repo=$(cd "$1" && pwd)
here=$(CDPATH= cd "$(dirname "$0")" && pwd)
report_python=${PYTHON:-python3}
"$report_python" -c 'import sys; assert sys.version_info >= (3, 11), "Python 3.11+ is required (set PYTHON to select it)"'
test "$(git -C "$repo" rev-parse HEAD)" = 27f098a251b87449d25cc833aee7c32a7411d340 || {
  echo "Checkout the measured revision 27f098a2 first" >&2; exit 1;
}
test "$(git -C "$repo" rev-parse egglog-baseline)" = 62ae6baa5539af4468a8f1a94d6e108de4dcd2df || {
  echo "The native egglog-baseline tag does not match the measured snapshot" >&2; exit 1;
}
git -C "$repo" diff --quiet HEAD -- engine/cell-runtime tools/cell-compiler workloads/pass3/inputs || {
  echo "Measured compiler, runtime and training sources must be clean" >&2; exit 1;
}
test ! -e "$2"
mkdir -p "$2"
out=$(cd "$2" && pwd)
mkdir -p "$out/native-source" "$out/link/src" "$out/capture" "$out/scratch"
unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS CARGO_PROFILE_RELEASE_LTO CARGO_PROFILE_RELEASE_CODEGEN_UNITS EGGLOG_COUNT_FREE_JOIN
export CARGO_TARGET_DIR="$out/cargo-target"
cargo build --release --locked --manifest-path "$repo/tools/cell-compiler/Cargo.toml"
cp "$CARGO_TARGET_DIR/release/eggfuse-cell-compiler" "$out/compiler"
cargo build --release --locked --manifest-path "$repo/engine/cell-runtime/Cargo.toml" --features sparse-cells,view-ports,storage-metrics
cp "$CARGO_TARGET_DIR/release/eggfuse-cell-runtime" "$out/cell"
git -C "$repo" archive egglog-baseline egglog | tar -x -C "$out/native-source"
(cd "$out/native-source" && git apply --unidiff-zero "$here/native-count.patch")
mkdir -p "$out/native-source/egglog/examples"
cp "$here/native-inference.rs" "$out/native-source/egglog/examples/count_fj.rs"
cargo build --release --locked --no-default-features --manifest-path "$out/native-source/egglog/Cargo.toml" --example count_fj
cp "$CARGO_TARGET_DIR/release/examples/count_fj" "$out/native"
cp "$here/transfer.rs" "$out/link/src/main.rs"
cp "$here/transfer.lock" "$out/link/Cargo.lock"
"$report_python" - "$repo" "$out/link/Cargo.toml" <<'PY'
from pathlib import Path
import json,sys
path=Path(sys.argv[1])/'engine/cell-runtime'
Path(sys.argv[2]).write_text('[package]\nname="report-transfer"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nserde_json="1"\neggfuse-cell-runtime={path='+json.dumps(str(path))+',features=["iterative-bake"]}\n')
PY
cargo build --release --locked --manifest-path "$out/link/Cargo.toml"
cp "$CARGO_TARGET_DIR/release/report-transfer" "$out/transfer"
tar -xzf "$here/captures.tar.gz" -C "$out/capture"
gzip -dc "$here/donor.json.gz" >"$out/donor.json"
cat "$repo/workloads/pass3/inputs/common-prefix.egg" "$repo/workloads/pass3/inputs/bake.eggpart" >"$out/bake.egg"
"$report_python" "$here/refresh-suite.py" "$out" suite
