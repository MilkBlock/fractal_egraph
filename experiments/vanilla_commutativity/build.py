#!/usr/bin/env python3
"""Build the size-printing probe against the immutable pristine egglog baseline.

The generated crate is an independent consumer of ``egglog-baseline``; the
baseline checkout is only read, never modified.
"""
import argparse
import json
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--baseline", type=Path, required=True, help="Clean egglog-baseline checkout")
    a = p.parse_args()
    baseline = a.baseline.resolve()
    expected = subprocess.check_output(["git", "rev-parse", "egglog-baseline^{}"], cwd=ROOT).strip()
    actual = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=baseline).strip()
    assert actual == expected, f"baseline must be at egglog-baseline ({expected!r}), got {actual!r}"
    assert not subprocess.check_output(["git", "status", "--porcelain", "--", "egglog"], cwd=baseline), (
        "baseline source must be clean"
    )

    work = ROOT / "out/vanilla-commutativity/probe"
    (work / "src").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(HERE / "probe.rs", work / "src/main.rs")
    (work / "build.rs").write_text("fn main() {}\n")
    (work / "Cargo.toml").write_text(
        """[package]
name="vanilla_probe"
version="0.1.0"
edition="2024"
[workspace]
[dependencies]
egglog={path=%s,default-features=false}
egglog-numeric-id={path=%s}
[profile.release]
lto="thin"
codegen-units=1
[build-dependencies]
syn={version="2",features=["full"]}
"""
        % (
            json.dumps(str(baseline / "egglog")),
            json.dumps(str(baseline / "egglog/numeric-id")),
        )
    )
    shutil.copyfile(ROOT / "experiments/native_compare/Cargo.lock", work / "Cargo.lock")
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--offline",
            "--manifest-path",
            str(work / "Cargo.toml"),
            "--target-dir",
            str(ROOT / "target/native-compare-baseline"),
        ],
        check=True,
    )
    out = ROOT / "out/vanilla-commutativity/bin"
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "target/native-compare-baseline/release/vanilla_probe", out / "probe")
    (out / "probe").chmod(0o755)
    print(f"baseline={expected.decode()} -> {out / 'probe'}")


if __name__ == "__main__":
    main()
