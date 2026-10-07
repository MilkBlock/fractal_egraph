# RipSatCell egraph viewer

The local debugger now exposes RipSatCell directly. Retired CS layer, CSCS/CCSS
and old catalog-mode controls have been removed. The analysis-directory dropdown
groups runs by exact current-source content and defaults to the newest matching
run. Switching source clears unrelated results; finishing a web run refreshes
and selects its output automatically. Other sources remain manually selectable.
No publishing or separate website is needed.

```sh
EGG_LAYOUT_GRAPHS=0 cargo run --offline --release --bin egg_layout -- analyze --source experiments/cell_viewer/demo.egg --output out/cell-egraph-view-demo
python3 tools/egglog_debugger/server.py --port 8080
```

Open:

http://127.0.0.1:8080/?layer_run=out/cell-egraph-view-demo&layer_kind=cells&cell_unit=1

The viewer reads `analysis.json` plus lazily selected Cell state files. It does
not run analyze or infer new paths when a state is clicked. Existing directories
can be loaded even without pre-generated DOT/SVG or round files.

## Views

* Cell eclasses and their actual `CoarseApply(CellParents, contract)` alternatives.
* Expandable AND/OR predecessor choices: all registered alternatives remain
  accessible; shared/cyclic references do not cause infinite path enumeration.
* Predecessors are ranked by indegree, defined here as the number of registered
  CoarseApply alternatives entering their eclass. Alternative constructions are
  ranked by the sum of predecessor indegrees. This is a display order only.
* Typst renders the chosen construction DAG. Links expand individual RipSat
  states. Another button renders the selected coarse/smooth application through
  the existing Eggplant plugin Typst pipeline.
* Initial and saturated internal egraphs, rendered by the existing Graphviz host.
* Replay, completion, certified-vs-observed export and symbolic-condition evidence
  are shown separately. No generic runtime-transition certificate is invented.

New preparations record `origin.lhs_evidence`, retaining each application's
ruleset and grounded LHS call positions. The selected construction excludes
inherited parent members when choosing coarse triggers. Matching preserves full
arguments/literals and visibility; it never colors by operator name alone.
Orange means selected coarse LHS. Green means other closure facts, NOT proven
per-node smooth provenance. Old runs lacking bindings explicitly show that the
highlight is unavailable. Unresolved terms are reported rather than guessed.

## Validation

```sh
python3 tools/egglog_debugger/test_cell_egraph.py
node tools/egglog_debugger/test_cell_egraph.mjs
NODE_PATH=/path/to/node_modules node tools/egglog_debugger/test_cell_egraph_browser.cjs
```

The browser test uses installed Chrome and Playwright; it checks eclass selection,
DOT links, Typst path rendering, existing rule templates, bound LHS coloring,
and honest missing-evidence labels on the saved eggcc run. No generated images
or run artifacts are tracked. The focused native preparation suite passed 29 tests.

The directory index reads source files and file timestamps, not every large
analysis JSON. Source hashes are cached by file version. Browser runs preserve
input.egg for matching. Partial runs with round snapshots are readable as well.
`test_cell_directory_browser.cjs` checks switching source, latest-run selection,
no-result clearing, removal of old controls, and selection after a fresh web run.

## Round snapshots and independent viewer

The snapshot selector reads saved round JSON independently of the final report.
Web runs feed each emitted round into the same panel before completion. The latest
option follows new rounds; selecting a specific round pins it. Old CLI runs that
did not save rounds expose only their final result. Selecting a historical round
never substitutes the final registry or a future saturated body.

The editor page opens `/cell-viewer.html` in a new tab, carrying the selected run,
round and Cell. The independent page reuses the same graph module and DOT/Typst
renderers, with the graph on the left and inspector on the right. It contains no
editor or trace-log panel. Click CoarseApply for Typst or an eclass for its body.
The original page never changes layout. New rounds are announced through a
same-origin BroadcastChannel; the independent viewer follows them when its
snapshot selector is set to latest.

`test_cell_fullscreen_browser.cjs` verifies new-tab navigation, identical graph
SVG, absence of editor/logs, both click routes and historical snapshot isolation.
