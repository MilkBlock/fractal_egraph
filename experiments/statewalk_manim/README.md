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

## MamboTTS narration

`voice.py` supports the legacy GPT-SoVITS `POST /` API used by
[MamboTTS](https://github.com/Tsukimisaka/MamboTTS). Run the intended Mambo model
and reference on the engine host first; the client does not silently switch a
shared server's models. An SSH tunnel can expose that server only on local
loopback. The upstream Mambo launcher supplies `manbo-e10.ckpt`,
`manbo_e8_s168.pth`, and its default `refer.wav` / transcription.

```sh
# A short real preview, before generating the full narration.
out/manim-ce-venv/bin/python experiments/statewalk_manim/voice.py \
  --mambo-url http://127.0.0.1:19880 --output out/mambo-preview.wav

# Use a separate output directory to retain the Tingting edition.
out/manim-ce-venv/bin/python experiments/statewalk_manim/prepare.py \
  --output out/statewalk-mambo --speech-backend mambotts \
  --mambo-url http://127.0.0.1:19880 --mambo-speed 1.0
out/manim-ce-venv/bin/python experiments/statewalk_manim/render.py --output out/statewalk-mambo --quality final
out/manim-ce-venv/bin/python experiments/statewalk_manim/finish.py --output out/statewalk-mambo
```

The engine's reference path is server-side, not a local file upload. Override it
only with both `--mambo-reference` and its exact `--mambo-prompt`. Bump
`--voice-revision` when changing server weights/reference under the same name.
Cache identity includes backend, text, voice settings and output checksum.
Responses must be WAV and successfully decode before replacing an existing clip;
an unavailable engine never substitutes silence or the old voice. Narration
durations drive the existing animation/subtitle timing. Credits and player labels
identify the actual backend and disclose synthesized speech.

### Identifier pronunciation

`narration.json` keeps the actual displayed identifiers (`a0`, `b1`, `A`, `B`,
`t`) in its full narration text. `pronunciation.py` derives a separate TTS string:
for example, `b0` becomes `字母 B，零`. It never renames `a/b` to `甲/乙`.
The adapter uses the engine's Chinese/English segmentation, where standalone
uppercase `A`, `B`, `T` have letter pronunciations. Chinese numerical subscripts
are explicitly separated from the letter.

`prepare.py` records both `subtitle` and `speech` for every clip. Manim and the
subtitle packager use `subtitle` directly; they do not reverse transliterations
or derive display text from speech. Final packaging rejects a subtitle mismatch.
Changed speech invalidates only that clip's audio cache, while the final render
recomputes the timeline for new narration durations.

```sh
out/manim-ce-venv/bin/python -m unittest discover \
  -s experiments/statewalk_manim -p 'test_pronunciation.py'
```

The correction regenerates 16 of 50 Mambo clips, including the shared-cost `A/B/t`
example. Frontend checks verify letter segmentation and `A=EY1`, `B=B IY1`,
`T=T IY1` phonemes. These checks and audio decoding are not an acoustic ASR proof;
the short `first-read-preview.wav` is retained for listening.

## Appended lessons: pure closure, bags, and If

`closure_appendix.py` renders an independent Manim continuation. It covers:

- State-carrying tuples versus ordinary value projections; Tiger's type flag is
  distinct from eggcc's `ExprIsPure` analysis.
- A maximal connected pure region becoming extractable in two separate waves.
- An integer expression gaining a constant-zero alternative, and why that does
  not authorize deleting effects still required by the state chain.
- Cyclic candidate e-graphs even for a loop-free selected program.
- Real vec_add records #11 and #73: both at c9, with 49 versus 52 extractable
  classes. The displayed read values are explicitly only a subset.
- Availability versus candidate-cost bookkeeping, and a sharing counterexample
  to greedy local choice (8 versus 6, with illustrative costs).
- The actual `(If pred inputs then else)` interface, scoped `InIf` assumptions,
  one conditional output without unioning distinct branch states, and the
  guarded state-edge-passthrough transformation.

The new geometric examples are teaching diagrams, not fabricated native trace
events. The two vec_add records are checked against the saved actual trace.
Relevant upstream sources at `ae80f82a` are `schema.egg`, `add_context.rs`,
`utility/context_of.egg`, `interpreter.rs`, `interval_analysis.egg`,
`optimizations/passthrough.egg`, and the Tiger type/closure/cost implementations.
The original film's benchmark claims are not changed or refreshed.

```sh
out/manim-ce-venv/bin/python experiments/statewalk_manim/prepare.py \
  --output out/statewalk-closure-appendix \
  --narration experiments/statewalk_manim/closure_narration.json \
  --speech-backend mambotts --mambo-url http://127.0.0.1:19880 \
  --voice-revision mambo-v1.2.1-426ac73f-80668f16
out/manim-ce-venv/bin/python experiments/statewalk_manim/render.py \
  --output out/statewalk-closure-appendix --part closure --quality final
out/manim-ce-venv/bin/python experiments/statewalk_manim/append.py \
  --base out/statewalk-mambo-identifiers \
  --extension out/statewalk-closure-appendix \
  --output out/statewalk-mambo-extended
```

The base is the complete local production directory, with `audio.json`, edit,
stills and evidence, rather than only the distributed MP4. `append.py` preserves
the old video packets, offsets the new chapters/captions by the exact old frame
count, and aligns the two narration tracks to 48-kHz sample boundaries. It uses
lossless ALAC for the intermediate audio before the usual final normalization.
It decodes the finished movie and compares every old frame's decoded hash with
the new movie's prefix. The old MP4 is never overwritten. The new full narration
mix is normalized; bit-identical old audio samples are not claimed.

Tests: `python -m unittest discover -s experiments/statewalk_manim -p 'test_*.py'`.

## Stepwise known / live_known continuation

`known_appendix.py` expands the two teaching pseudocode lines into individual
operations, using the same node-lighting style as the pure-region lesson:

- Record #73 starts with 52 known classes. Candidate c21 reads c45/c55/c9, all
  present. Set union adds membership for an already-existing class; it is not
  egglog equality union, graph insertion, or concrete execution of Load.
- One chosen effect result plus four pure waves of 1/4/3/2 classes gives 63
  known classes. c72 (Get) and c37 (FAdd) are shown with their exact inputs.
  Wave grouping is pedagogical, not a claim about native queue pop order.
- The 63 entries comprise 45 common initial entries, 4 dynamic effect entries
  and 14 dynamic pure entries. All 14 pure entries remain live at this point.
  The other 49 entries are not described as dead computation.
- Record #798, after a Write at c5, retains 96 known classes. Its 41 dynamic
  pure entries project to 16 live entries for keying; the other 25 remain in
  the full record. Static initial entries and effect position are explained
  separately. Availability is not an already-paid cost bag.
- Future relevance follows all possible effect successors in the fixed regional
  candidate graph, then recursively traces their pure dependencies. It is not
  prediction of future egglog rewrites and not just the immediately next step.
- Toy comparisons explain same-position key reuse, retaining the cheaper arrival,
  and why a possibly-needed missing input must keep keys distinct.

`known_evidence.py` reconstructs the known sets from the saved diagnostic trace
and verifies all 1,465 closure deltas. It independently compiles the **unchanged
liveness source block** from the pinned Tiger against that same 113-class region.
All 28 effect-position masks must match the readable Python reconstruction.
This checks the masks, not collision-freedom or every runtime hash update. The
movie explicitly distinguishes mathematical sets from finite hash summaries.
No original eggcc source is modified and no inference benchmark is rerun.

```sh
out/manim-ce-venv/bin/python experiments/statewalk_manim/prepare.py \
  --output out/statewalk-known-appendix \
  --narration experiments/statewalk_manim/known_narration.json \
  --speech-backend mambotts --mambo-url http://127.0.0.1:19880 \
  --voice-revision mambo-v1.2.1-426ac73f-80668f16
out/manim-ce-venv/bin/python experiments/statewalk_manim/known_evidence.py \
  --trace out/statewalk-known-appendix/statewalk-trace.jsonl \
  --output out/statewalk-known-appendix/known-evidence.json \
  --oracle-dir out/statewalk-known-appendix/liveness-check
out/manim-ce-venv/bin/python experiments/statewalk_manim/render.py \
  --output out/statewalk-known-appendix --part known --quality final
out/manim-ce-venv/bin/python experiments/statewalk_manim/append.py \
  --base out/statewalk-mambo-extended --extension out/statewalk-known-appendix \
  --part known --output out/statewalk-mambo-known
```

Successive additions retain their provenance in `appendices`. As before, the
complete prior movie's decoded frames and its subtitle cues are preserved.
