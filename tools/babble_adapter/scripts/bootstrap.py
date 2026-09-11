"""Fetch pinned upstream sources into ignored tools/.deps; apply recorded compatibility edits."""
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile

ROOT = Path(__file__).resolve().parents[3]
DEPS = ROOT / "tools/.deps"
SOURCES = {
    "babble": ("115f920db52f124fd7247d1f80e811f3784e6896", "a6fe3790b66dddfe4c1fa2c502079d99c3c8392a776c77ff662a56a9ba269e81"),
    "egg": ("caf623cad25cacba83193464dcf0d3dd4161ba7a", "923cc3815e23b6b01bc068f34a0c41e508df2faa246c5c356191d7494aca1f10"),
}

def main():
    DEPS.mkdir(parents=True, exist_ok=True)
    for name, (rev, digest) in SOURCES.items():
        destination = DEPS / name
        stamp = destination / ".adapter-source.json"
        expected = {"revision": rev, "archive_sha256": digest, "patch_version": 1}
        if stamp.exists() and json.loads(stamp.read_text()) == expected:
            continue
        if destination.exists():
            raise RuntimeError(f"Unstamped or different dependency at {destination}; move it aside before bootstrapping")
        with tempfile.TemporaryDirectory() as temp:
            archive = Path(temp) / "source.tar.gz"
            subprocess.run(["curl", "--fail", "--location", "--max-time", "60", "--output", str(archive),
                            f"https://codeload.github.com/dcao/{name}/tar.gz/{rev}"], check=True)
            assert hashlib.sha256(archive.read_bytes()).hexdigest() == digest, "archive checksum mismatch"
            with tarfile.open(archive) as tar:
                tar.extractall(temp, filter="data")
            source = Path(temp) / f"{name}-{rev}"
            if name == "babble":
                manifest = source / "Cargo.toml"
                old = 'egg = { git = "https://github.com/dcao/egg", features = ["serde-1"] }'
                text = manifest.read_text()
                assert text.count(old) == 1
                manifest.write_text(text.replace(old, 'egg = { path = "../egg", features = ["serde-1"] }'))
                # Current rustc rejects an outer Self constructor in a nested generic fn.
                expr = source / "src/ast_node/expr.rs"
                text = expr.read_text()
                assert text.count("            Self(node)") == 1
                expr.write_text(text.replace("            Self(node)", "            Expr(node)"))
            (source / ".adapter-source.json").write_text(json.dumps(expected, indent=2))
            shutil.move(str(source), destination)

if __name__ == "__main__":
    main()
