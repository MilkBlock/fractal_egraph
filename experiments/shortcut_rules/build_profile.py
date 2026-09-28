#!/usr/bin/env python3
"""Build the per-iteration profiling probe against the pristine egglog baseline."""
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

    work = ROOT / "out/shortcut-rules/profile"
    (work / "src").mkdir(parents=True, exist_ok=True)
    shutil.copyfile(HERE / "profile_probe.rs", work / "src/main.rs")
    (work / "build.rs").write_text("fn main() {}\n")
    (work / "Cargo.toml").write_text(
        """[package]
name="profile_probe"
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
        % (json.dumps(str(baseline / "egglog")), json.dumps(str(baseline / "egglog" / "numeric-id")))
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
    out = ROOT / "out/shortcut-rules/bin"
    out.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(ROOT / "target/native-compare-baseline/release/profile_probe", out / "profile")
    (out / "profile").chmod(0o755)
    print(f"baseline={expected.decode()} -> {out / 'profile'}")


if __name__ == "__main__":
    main()
