#!/bin/sh
# Three matched standalone CPU diagnostics on one frozen Pass 3 program.
set -eu
if [ "$#" -ne 2 ]; then echo "usage: sh reproduce.sh PROGRAM.json NEW_DIRECTORY" >&2; exit 2; fi
program=$1
dest=$2
mkdir "$dest"
export CARGO_TARGET_DIR="$dest/target"
pkg=eggfuse-cell-runtime
collect() {
    label=$1; flags=$2; round=$3
    dir="$dest/$label-$round"
    cargo run --release --locked -p "$pkg" --features "$flags" -- --program "$program" --output "$dir" >"$dir.log"
    digest=$(shasum -a 256 "$dir/state.json" | cut -d ' ' -f 1)
    if [ -f "$dest/expected-state" ]; then
        test "$(cat "$dest/expected-state")" = "$digest"
    else
        printf '%s\n' "$digest" >"$dest/expected-state"
    fi
    rm "$dir/state.json"
}
for r in 1 2 3; do
    collect clock family-survey,view-ports "$r"
    collect replay family-replay,view-ports "$r"
    collect reads family-overlap,view-ports "$r"
    collect entry family-frontiers,view-ports "$r"
    collect scan family-scan,view-ports "$r"
done
files() { printf '%s' "$dest/$1-1/family-survey.json,$dest/$1-2/family-survey.json,$dest/$1-3/family-survey.json"; }
cargo run --release --locked -p "$pkg" --features family-survey --example rank_families -- \
    "$program" "$(files clock)" "$(files replay)" "$(files reads)" "$(files entry)" "$(files scan)" "$dest/ranked.json"
