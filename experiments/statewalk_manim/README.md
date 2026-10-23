# Statewalk, animated with Manim Community

An original Chinese mathematical explainer using **actual Manim Community 0.19.0**,
not a slide deck or the earlier Canvas renderer. Dark background, semantic color,
TeX notation and continuous object transformations make the dependencies visible.
The earlier `experiments/statewalk_video` editions remain available.

The film follows real eggcc `raytrace::vec_add`: its Load tuple projections,
selected read order, different extractability information at the same e-class,
a rejected Write, an accepted Write from a different state, the actual DP graph,
then immutable shared cost bags and their merge. The cost tokens 3/1/2 are
explicitly illustrative. State availability is **extractability**, not evaluation
of the program by the extractor; pure closure is not rewrite saturation.

The native trace contains 808 retained states, 1,864 candidate visits and 399
dependency rejections. Its pure-closure cache hits are zero. The new cost-bag
representation is distinct from that cache. It does not integrate matching
markers, RipSat proofs or online extraction. Statewalk's estimates do not promise
globally optimal DAG extraction.

## Reproduction

Requires macOS Tingting, FFmpeg, Cairo/Pango, TeX Live, dvisvgm and Python 3.12.
No voice API or downloaded imagery/music. Native narration is disclosed as synthetic.

```sh
uv venv --python /opt/homebrew/bin/python3.12 out/manim-ce-venv
uv pip install --python out/manim-ce-venv/bin/python -r experiments/statewalk_manim/requirements.txt
out/manim-ce-venv/bin/python experiments/statewalk_manim/prepare.py --output out/statewalk-manim
out/manim-ce-venv/bin/python experiments/statewalk_manim/render.py --output out/statewalk-manim --quality preview
out/manim-ce-venv/bin/python experiments/statewalk_manim/render.py --output out/statewalk-manim --quality final
out/manim-ce-venv/bin/python experiments/statewalk_manim/finish.py --output out/statewalk-manim
```

Preparation uses the checked `out/statewalk-film-v2-trace-checked` capture and
`out/boundary-loop/extraction/gpu.egg`. To regenerate the native trace from the
saved Pass-3 graph, use `experiments/statewalk_video/capture.py --output <fresh-dir>`,
then pass `--trace <fresh-dir>` to `prepare.py`. The shared `real_ir.py` and
`trace_data.py` verify the exact IR, state IDs, dependencies and spoken counts.
This is a frozen evidence-backed film, not a general narrator for arbitrary traces.

`render.py` scopes `TEXMFROOT`/`TEXMFCNF` to the child renderer so Homebrew
dvisvgm finds the installed TeX Live files. It uses DVI, not the incompatible
Ghostscript PDF conversion route. Unchanged speech clips are reused. Preview
animations may be cached; final rendering disables Manim's animation cache
because version 0.19 can skip narration and accumulate timeline drift when
reusing clips. Final frame/audio checks remain strict.

`finish.py` copies the Manim video frames unchanged, normalizes/pads narration,
adds full Chinese subtitle and chapter tracks, and creates a local chapter player.
It validates dimensions, frame count, duration and chapters, then decodes the
entire finished video. Short explanatory captions are burned in; full narration
subtitles are optional. Review stills live under `out/statewalk-manim/stills`.

## Evidence and scope

- Production source: `check_statewalk@ae80f82a`, `dag_in_context/src/tiger/`.
- Actual example: `benchmarks/passing/raytrace/raytrace.rs:193`.
- Full trace output equals the saved production extraction byte-for-byte.
- Timing source: `experiments/tiger_extraction_markers/results.json`.
- Saved single-run `off` vs `bag`: global estimate 407.059 → 29.404 ms;
  full Tiger extraction 2.271746 → 1.735388 s; identical full output.
- These are production CPU extraction measurements, not the animation runtime,
  a new benchmark, total eggcc compilation, or a GPU speedup.

The search diagram plots retained predecessor depth against the six-value read
mask solely for display. It preserves native edges and queue-pop order; it is
not a depiction of every considered candidate edge. The highlighted path has
22 native states. The rejected and accepted Write scenes explicitly use different
states (#71 vs #797); missing dependencies are never silently waived.

Only reproducible sources and text validation results belong in Git. Rendered
movies, audio, caches, the virtual environment and trace files remain under `out/`.

## Detailed edition: extraction into executable dataflow

The detailed edition adds five chapters and a runnable Python companion:

1. A real transition #73 → #200: Load b1 (c21) enables the Get value (c72),
   then FAdd (c37). Pure alternatives are OR; the children of one candidate
   are AND. This step adds 11 extractable classes, including the selected Load.
2. The queue loop: local effect successors, dependency rejection, estimated
   cost 1225 + 611 = 1836, relevant-state deduplication and predecessor updates.
   On-screen pseudocode intentionally omits native data structures/pruning.
3. `rebuild_egraph_statewalk` expands the repeated effectful c9 occurrences
   into separate positions and rewires each state edge to its predecessor.
4. `prune_unextractable_enodes` and `statewalk_greedy_extraction` complete the
   graph, collect missing dependencies, output them topologically, and reuse
   previously emitted results. This explanation is source-backed; we do not
   claim to have instrumented the final greedy queue in the existing trace.
5. Python generated from the actual selected IR executes on concrete arrays.
   The selected read order remains b0, a1, b1, a2, b2, a0. Output is [12,24,36].

`dataflow.py` is a deliberately narrow exporter and independent direct
interpreter for this frozen vec_add IR. It rejects unsupported computation.
It generates `vec_add_plain.py` (standalone), `vec_add_python.py` (explicit state
tokens) and its small `dataflow_runtime.py`. Types are omitted, and Get/PtrAdd
aliases are inlined. Every emitted statement retains its original node ID.
Integer state tokens illustrate the selected ordering; they are not eggcc's
native State values. General pointer aliasing and allocation identity are not
modeled. Tests cover 131 finite input pairs, including signed zeros and wide
magnitudes, comparing double representations and effect sequences against the
direct selected-IR interpreter. This is not a general equivalence proof.

The Python pure-closure checker independently replays **all 1,864 candidate
dependency checks, 1,465 closure deltas, and 807 publication cost additions**.
It operates on native node records. It does not rerun native DP, reproduce its
liveness/hash optimizations, or claim a new optimum. The explanatory closure
uses repeated scans; Tiger uses reverse dependencies, counters and persistent
sets. These are evidence checks, not timing benchmarks.

`walkthrough.html` lets the reader step through actual recorded Python locals,
switch between plain and state-token code, and inspect each original IR node.
Its data comes from executing the generated function with recording statements;
the browser is a trace player, not an embedded Python interpreter.

`syntax.py` uses Python's standard tokenizer to share syntax colors between the
Manim code scenes and both HTML code views. Token spans preserve the exact code;
Pango/HTML text is escaped. The current execution line remains a separate yellow
frame/background and does not overwrite keyword/function/number colors. No CDN
or browser-side syntax library is needed.

To preserve the first edition, use a different output directory:

```sh
out/manim-ce-venv/bin/python experiments/statewalk_manim/prepare.py --output out/statewalk-manim-detail
out/manim-ce-venv/bin/python experiments/statewalk_manim/render.py --output out/statewalk-manim-detail --quality final
out/manim-ce-venv/bin/python experiments/statewalk_manim/finish.py --output out/statewalk-manim-detail
python3 out/statewalk-manim-detail/vec_add_plain.py
python3 out/statewalk-manim-detail/vec_add_python.py
```
