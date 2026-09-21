# Event-Driven Hierarchical Rule Composition and Closure Sharing

The English article is authored directly in
`event-driven-hierarchical-rule-composition.tex`. Its compiled PDF has the same
basename. Figures are inline TikZ; there are no downloaded assets or template
packages. The manuscript uses the standard `article` class, not a conference's
submission template.

Build locally without network access:

```sh
make -C docs/papers
```

Requirements: an installed TeX distribution with latexmk, pdfLaTeX, TikZ,
newtx, and the other standard packages named in the source. Build intermediates
are placed in `out/paper-latex`, outside the tracked paper directory.

Implementation descriptions are pinned to commit `258eef4`. The text distinguishes
implemented finite mechanisms, narrow Bake query proofs, mathematical definitions,
and proposed general FractalRule-to-ripen feedback. The existing experimental
records are not presented as a same-revision controlled ablation or as native
egglog compression measurements. `references-verified.json` records the offline
bibliography and source-evidence checks.
