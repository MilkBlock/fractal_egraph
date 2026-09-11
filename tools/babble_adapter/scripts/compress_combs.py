"""Learn libraries over generated rule-comb classes, then render their uses."""
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[3]
OUT = ROOT / "experiments/comb_compression"
def run(cmd, **kw):
    subprocess.run(cmd, cwd=ROOT, check=True, **kw)

run([sys.executable, str(Path(__file__).with_name("bootstrap.py"))])
OUT.mkdir(exist_ok=True)
run(["cargo", "run", "--bin", "comb_corpus", "--", "experiments/binding_program/math.json",
     "experiments/annotated_export/math_microbenchmark/profile.json", str(OUT / "corpus.json")])
run(["cargo", "build", "--release", "--locked", "--manifest-path", "tools/babble_adapter/Cargo.toml"])
run(["tools/babble_adapter/target/release/combine-babble", str(OUT / "corpus.json"),
     str(OUT / "results.json"), "comb-mining"], timeout=180)

result = json.loads((OUT / "results.json").read_text())
programs = json.loads((OUT / "results.json.programs.json").read_text())
corpus = json.loads((OUT / "corpus.json").read_text())
def render(e):
    op = e["op"]
    if op.startswith("Data("):
        op, _ = json.JSONDecoder().raw_decode(op[5:])
        if op.startswith("term:"):
            sort, name = json.loads(op[5:])
            op = name
    if not e["args"]:
        return op
    return "(" + op + " " + " ".join(map(render, e["args"])) + ")"

lines = ["# 从生成的 rule comb 中学到的子组合", "",
         "这里统计压缩后语料的显式库引用，不是新的运行时 rule apply 次数。", ""]
usage = {}
for lib in result["libraries"]:
    ident = lib["id"]
    uses = []
    def visit(e, entry, split):
        head, arguments = e, []
        while head["op"] == "Apply":
            arguments.append(head["args"][1])
            head = head["args"][0]
        if head["op"] == f"Ref({ident})" and len(arguments) == arity:
            uses.append({"split": split, "class": entry["class"],
                         "arguments": [render(a) for a in reversed(arguments)], "occurrence_weight": entry["occurrences"]})
        for a in e["args"]:
            visit(a, entry, split)
    definition = lib["definition"]
    body, arity = definition, 0
    while body["op"] == "Lambda":
        arity += 1
        body = body["args"][0]
    for split in ("train", "test"):
        body = programs[split]
        while body["op"].startswith("Lib("):
            body = body["args"][1]
        entries = [e for e in corpus["entries"] if e["split"] == split]
        assert body["op"] == "List" and len(body["args"]) == len(entries)
        for entry, e in zip(entries, body["args"]):
            visit(e, entry, split)
    usage[str(ident)] = uses
    lines += [f"## F{ident}", "", "```text", render(definition), "```", "",
              f"学习集引用：{sum(u['split']=='train' for u in uses)}；留出集引用：{sum(u['split']=='test' for u in uses)}。", ""]
    for u in uses[:3]:
        lines += [f"- `{u['class']}`，参数 `{u['arguments']}`。"]
    lines.append("")
(OUT / "library_usage.json").write_text(json.dumps(usage, indent=2, ensure_ascii=False) + "\n")
(OUT / "libraries.md").write_text("\n".join(lines) + "\n")
