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
Ghostscript PDF conversion route. Unchanged speech clips and Manim animations
can be reused on later renders.

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
