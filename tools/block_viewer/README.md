# Native program block inspector

```
cargo run --bin egg_trace -- egglog/tests/web-demo/cyk.egg results/block_viewer/cyk.json
python3 tools/block_viewer/render.py results/block_viewer/cyk.json --output results/block_viewer/cyk.html
```

Open the resulting HTML or load the JSON through the viewer. Snapshots are taken
at native command boundaries, not individual iterations inside a schedule.
The original schedules, checks, scopes, includes and command order execute in
egglog. Runtime errors are exported and still produce a failing process status.
Scope rollback retires prior block certificates, including rollback inside an
included program. Restored historical rows are boundary inputs, not new producers.

The generic runner registers exact runtime rule descriptions as opaque specs:
it observes dependencies but does not certify equality shortcuts. An invalidated
side output does not invalidate a different committed output of an opaque rule.
Composed rewrites retain the stronger whole-recipe validity gate.

Full CYK (all native checks passed): active blocks before the three scope pops:
23, 23, 45. All earlier blocks are retired after each pop. These counts describe
diagnostic blocks, not compression or measured speedup. The viewer lists rejected
match candidates, including action guards that filtered them.

UI regression uses Python Playwright with installed Chrome; run
`python tools/block_viewer/test_ui.py` after generating index.html and cyk.html.
