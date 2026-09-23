# Event-Driven Hierarchical Rule Composition and Saturation

The English paper is authored in `event-driven-hierarchical-rule-composition.tex`.
The current PDF is `event-driven-hierarchical-rule-composition_v0.2.5.pdf`.
Earlier versioned PDFs remain historical artifacts.

## September 23 revision

- Separates full tier0 provenance, effect-only local ripen observation, and optional
  local Coarse/Smooth history.
- Introduces a **proposed** finite origin-template contract: template + caller
  binding + actual fact/version slot map + residual obligations. Association and
  commutation from `math.egg` illustrate it. Derivational justification does not
  claim that a deduplicated fact was freshly produced in tier0.
- Documents shared 50 ms / 200K observation defaults, native admission checkpoints,
  commit/rebuild completion, and `ObservedUnfinished` rather than false saturation.
- Explains exact existential/freshness probes, query reuse, clone notification
  isolation, parallel graph-free execution, and separate SVG rendering.
- Reports the three measured mini3 process times (1.33, 1.06, 1.07 s), their scope,
  79 completed-state comparisons and 488 completed-source weak-dominate decisions.
  The sub-second target is **not** claimed met; unfinished prefixes remain
  wall-clock dependent.
- Keeps historical Use/candidate-budget and symbolic Bake experiments explicitly
  separate from the current native runtime. Automatic origin-template feedback,
  arbitrary-depth induction, and live tier0 replacement remain unimplemented.

`implementation-update-2026-09-23.json` freezes the source/evidence files used for
this update. `references-verified.json` describes the earlier bibliography and
historical evidence checks; its older hashes are not relabelled as current.

## Build locally without network access

```sh
make -C docs/papers pdf
```

The `pdf` target reuses existing native figure assets and does not regenerate
experiment outputs. TeX intermediates go to `out/paper-latex`. To explicitly
regenerate the native Add figures, use `make -C docs/papers native-egraph`.
`make -C docs/papers` retains that full regeneration workflow.

Requirements: latexmk, pdfLaTeX, TikZ, newtx, and the packages named in the source.
New provenance/runtime diagrams are in `figures/provenance-runtime.tex`.
Blue/orange mark LHS/RHS fact slots; purple/dashed teal mark coarse/smooth
applications in the template figure. Architecture diagrams use neutral boxes.

## Evidence boundaries

The native math illustrations use scoped fragments of
`egglog/tests/web-demo/math.egg`, with records in `examples/evidence.json`.
They are not saturation of the full optimizer. Earlier performance/storage and
Top-N coverage figures remain labelled historical. The current timing evidence
is `experiments/math_mini3/near_second.json`; no new benchmark was run to edit
this paper, and no inference-time acceleration over bare egglog is claimed.

The optional local history is potential training material for origin-template
reuse, not an implemented tier0 dispatch mechanism. The DABCABC/branching tests
protect growth and reentry observations; they do not claim automatic discovery
of the minimal ABC loop body.
