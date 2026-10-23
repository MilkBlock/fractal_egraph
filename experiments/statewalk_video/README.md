# Statewalk: legal paths and shared cost bags

An offline Chinese explainer film, 1920×1080 at 24 fps, with synthesized narration,
burned-in captions, an optional subtitle track, chapter metadata and a local player.
The film distinguishes existing Statewalk caches from the new cost-bag representation.
It does not describe matching-marker/RipSat integration or online extraction as done.

The real IR demo is **raytrace's `vec_add`**, extracted from the already validated
`out/boundary-loop/extraction/gpu.egg`. The demo preserves original node IDs and
source line numbers. It shows the actual chosen read order, the value/state tuple
projections, result allocation, three writes and the shared result pointer.
This is a chosen DAG, not a fabricated display of all pre-extraction alternatives.
`real_ir.py` checks the exact referenced definitions and state chain before rendering.

Sources:

- `check_statewalk@ae80f82a/dag_in_context/src/tiger/statewalkdp.cpp`
- `check_statewalk@ae80f82a/dag_in_context/src/tiger/greedy.cpp`
- `check_statewalk@ae80f82a/dag_in_context/src/interpreter.rs` (`Load`/`Write` semantics)
- `check_statewalk/benchmarks/passing/raytrace/raytrace.rs:193` (`vec_add`)
- `experiments/tiger_extraction_markers/results.json` (single-run measured results)

The A/B/t cost example is explicitly illustrative. The 407→29 ms and
2.272→1.735 s figures refer to the saved Pass-3 extraction comparison, not a new
benchmark, not total eggcc compilation and not a GPU speedup.

## Reproduce locally

Requirements: macOS `say` with the installed Tingting voice, FFmpeg, Python 3,
Node and `@napi-rs/canvas`. No external speech API, downloaded assets or music.
The renderer uses the installed offline Canvas runtime; Remotion was unavailable.
Animations are deterministic functions of the frame number.

```sh
python3 experiments/statewalk_video/prepare.py --output out/statewalk-film
node experiments/statewalk_video/render.cjs out/statewalk-film --preview
node experiments/statewalk_video/render.cjs out/statewalk-film
python3 experiments/statewalk_video/finish.py --output out/statewalk-film
```

Set `CODEX_NODE_MODULES` if the installed Node packages live elsewhere. The
default path is the Codex bundled local runtime. `prepare.py` reuses unchanged
speech clips. Rendering streams raw frames to FFmpeg; it does not save thousands
of intermediate images. `finish.py` checks frame count, audio/video lengths,
dimensions and chapter count. Visual review uses the exported stills and contact
sheet. A native reconstruction check also confirms the included `.egg` fragment
parses and creates the expected `vec_add` function; that self-check is a syntax
and construction check, not a new program-equivalence proof.

Outputs under `out/statewalk-film/`:

- `statewalk-explained.mp4`: the finished film.
- `index.html`: local player with chapter navigation and downloads.
- `captions.srt`: Chinese captions.
- `vec_add-extracted.egg`, `vec_add-source.rs`: the real demo.
- `manifest.json`, `ir-demo.json`: provenance and media validation.
- `poster.jpg`, `contact-sheet.jpg`, `stills/`: visual review assets.

Media/build outputs are not committed. The script, narration and rendering sources
are versioned; the voice is clearly disclosed as synthesized.
