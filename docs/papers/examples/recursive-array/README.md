# Recursive DSL to array summary

This mini corpus is used by the paper's infi/dissipative example. It contains two
samples for a bounded cursor and two samples for a ternary recursive frontier.
Bake discovers the repeated families from native execution, then checks the
structural array laws.

```sh
cargo run --offline -- bake \
  docs/papers/examples/recursive-array/manifest.json \
  out/paper-recursive-array-v1
cargo run --offline -- bake-eval \
  out/paper-recursive-array-v1/library.egg \
  fractal_0000 3 11 out/paper-recursive-array-v1/scan-query.json
cargo run --offline -- bake-eval \
  out/paper-recursive-array-v1/library.egg \
  fractal_0001 4 --depth 4 out/paper-recursive-array-v1/tree-query.json
```

The checked outputs are sum `60` for the cursor values `4..11`, and sum `29484`
for the 81-value ternary frontier `324..404`. Both queries use zero tier0 rule
applications and zero element/prefix expansion. Template IDs are local to this
library; inspect `bake.json` when reproducing the run.
