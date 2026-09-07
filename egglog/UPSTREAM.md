# Upstream baseline

This source tree was imported from `saulshanabrook/egg-smol` commit
`ebba7bb902bdc1b0f377b6bb22c06ac305912674`, the egglog 2.0 core revision used
by `egraphs-good/egglog-experimental` commit `08771f9` and the adjacent
`check_statewalk` checkout.

Layout-specific clustering is intentionally kept outside the execution core.
The core changes expose loss-aware rule and mutation traces from which layout
tools can derive communities, ports, leakage, and stability metrics.
