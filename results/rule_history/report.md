# Rule-history retrieval ablation

Controlled synthetic arithmetic programs executed by the local egglog kernel. These are current-snapshot retrieval results, not causal effects, production compression measurements, or hash decoding success.

1536 snapshots; 22 exact one-hop alpha templates; preparation 0.049s. Each local template was decoded with its original port bindings and checked against the source nodes.

Exact target: identical local node sets up to a globally consistent, sort-preserving external-port renaming; SELF fixed. This preserves sharing and self-reference but does not establish semantic equivalence or recursive graph isomorphism.

Near target: Jaccard >= 0.8 under independently canonicalized port coordinates. This is a feasible alignment only, not optimal approximate graph matching. Residual is a node count, not compressed bytes.

Structure-cheap uses per-node operator, within-node equality and SELF flags. Structure-strong additionally sees child operator sets. Structure-incidence averages strong similarity and the similarity of per-port operator/argument-position incidence histograms, computed without any history. History uses real Survived logical matches, not committed changes. Roles combine variable equalities and their current node-position incidences. Bindings-only removes rule identity from those role features. Past-only excludes the current round but re-canonicalizes old IDs at the current snapshot. Known-noop removal removes only the explicit noop rules.

Weights selected using validation exact-top1 only; no test tuning. Five paired dictionary tie orders. Delta ranges below are tie-order sensitivity, NOT confidence intervals. Independent leaf-renaming runs are intentionally easy repeats; they do not demonstrate new-program generalization.

## Results

### renamed

Dictionary / validation / test = 384 / 384 / 768; exact-template availability ceiling = 100.0%.

| Variant | weight | exact @1 | near @1 | exact @4 | residual nodes | Δ exact vs strong | paired range |
|---|---:|---:|---:|---:|---:|---:|---:|
| structure_cheap | 0 | 90.3% | 90.3% | 98.4% | 0.19 | -7.3 pp | [-26.0, -2.1] pp |
| structure_strong | 0 | 97.6% | 97.6% | 99.2% | 0.05 | +0.0 pp | [+0.0, +0.0] pp |
| structure_incidence | 0 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| cheap+rule_roles | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| incidence+rule_set | 0 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| incidence+rule_roles | 0 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| history_only | 1 | 87.6% | 87.6% | 94.7% | 0.25 | -9.9 pp | [-14.3, -7.8] pp |
| strong+rule_set | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| strong+rule_counts | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| strong+bindings_only | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| strong+rule_roles | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| strong+temporal | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| strong+past_only | 0.1 | 98.6% | 98.6% | 99.7% | 0.03 | +1.1 pp | [+0.5, +3.4] pp |
| strong+remove_known_noop | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +2.4 pp | [+1.6, +6.0] pp |
| strong+shuffled_history | 0 | 97.6% | 97.6% | 99.2% | 0.05 | +0.0 pp | [+0.0, +0.0] pp |
| strong+shuffled_matched_weight | 0.1 | 96.1% | 96.1% | 99.3% | 0.08 | -1.4 pp | [-3.4, +2.2] pp |

### unseen_schedule

Dictionary / validation / test = 256 / 64 / 128; exact-template availability ceiling = 100.0%.

| Variant | weight | exact @1 | near @1 | exact @4 | residual nodes | Δ exact vs strong | paired range |
|---|---:|---:|---:|---:|---:|---:|---:|
| structure_cheap | 0 | 99.4% | 99.4% | 100.0% | 0.01 | +0.6 pp | [-3.1, +6.2] pp |
| structure_strong | 0 | 98.8% | 98.8% | 100.0% | 0.03 | +0.0 pp | [+0.0, +0.0] pp |
| structure_incidence | 0 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| cheap+rule_roles | 0 | 99.4% | 99.4% | 100.0% | 0.01 | +0.6 pp | [-3.1, +6.2] pp |
| incidence+rule_set | 0 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| incidence+rule_roles | 0 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| history_only | 1 | 77.5% | 77.5% | 84.4% | 0.57 | -21.2 pp | [-25.0, -7.8] pp |
| strong+rule_set | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| strong+rule_counts | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| strong+bindings_only | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| strong+rule_roles | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| strong+temporal | 1 | 71.2% | 71.2% | 78.1% | 0.69 | -27.5 pp | [-31.2, -14.1] pp |
| strong+past_only | 0.1 | 99.4% | 99.4% | 100.0% | 0.01 | +0.6 pp | [+0.0, +3.1] pp |
| strong+remove_known_noop | 0.1 | 100.0% | 100.0% | 100.0% | 0.00 | +1.2 pp | [+0.0, +6.2] pp |
| strong+shuffled_history | 0 | 98.8% | 98.8% | 100.0% | 0.03 | +0.0 pp | [+0.0, +0.0] pp |
| strong+shuffled_matched_weight | 0.1 | 96.6% | 96.6% | 100.0% | 0.07 | -2.2 pp | [-4.7, +1.6] pp |

### unseen_shape

Dictionary / validation / test = 288 / 48 / 96; exact-template availability ceiling = 83.3%.

| Variant | weight | exact @1 | near @1 | exact @4 | residual nodes | Δ exact vs strong | paired range |
|---|---:|---:|---:|---:|---:|---:|---:|
| structure_cheap | 0 | 75.8% | 75.8% | 83.3% | 1.05 | +20.8 pp | [-4.2, +33.3] pp |
| structure_strong | 0 | 55.0% | 55.0% | 71.7% | 1.91 | +0.0 pp | [+0.0, +0.0] pp |
| structure_incidence | 0 | 83.3% | 83.3% | 83.3% | 0.79 | +28.3 pp | [+8.3, +33.3] pp |
| cheap+rule_roles | 0.1 | 83.3% | 83.3% | 83.3% | 0.90 | +28.3 pp | [+8.3, +33.3] pp |
| incidence+rule_set | 0 | 83.3% | 83.3% | 83.3% | 0.79 | +28.3 pp | [+8.3, +33.3] pp |
| incidence+rule_roles | 0 | 83.3% | 83.3% | 83.3% | 0.79 | +28.3 pp | [+8.3, +33.3] pp |
| history_only | 1 | 70.0% | 70.0% | 77.9% | 1.16 | +15.0 pp | [+2.1, +22.9] pp |
| strong+rule_set | 0.1 | 77.9% | 77.9% | 82.1% | 1.09 | +22.9 pp | [+0.0, +33.3] pp |
| strong+rule_counts | 0.1 | 71.7% | 71.7% | 82.1% | 1.25 | +16.7 pp | [-14.6, +31.2] pp |
| strong+bindings_only | 0.1 | 78.3% | 78.3% | 82.1% | 1.00 | +23.3 pp | [+2.1, +33.3] pp |
| strong+rule_roles | 0.1 | 78.3% | 78.3% | 82.1% | 1.00 | +23.3 pp | [+2.1, +33.3] pp |
| strong+temporal | 0.1 | 78.3% | 78.3% | 82.1% | 1.00 | +23.3 pp | [+2.1, +33.3] pp |
| strong+past_only | 0.1 | 69.2% | 69.2% | 78.3% | 1.25 | +14.2 pp | [-12.5, +25.0] pp |
| strong+remove_known_noop | 0.1 | 76.7% | 76.7% | 81.7% | 1.03 | +21.7 pp | [+0.0, +33.3] pp |
| strong+shuffled_history | 0.1 | 54.0% | 54.0% | 71.9% | 1.82 | -1.0 pp | [-14.6, +8.3] pp |
| strong+shuffled_matched_weight | 0.1 | 54.0% | 54.0% | 71.9% | 1.82 | -1.0 pp | [-14.6, +8.3] pp |

## Scope and reproducibility

Verification: {'traced_untraced_full_snapshot_comparisons': 1536, 'local_template_roundtrips': 1536, 'unit_tests': 6, 'distinct_rule_sets': 24, 'rule_sets_with_multiple_templates': 9, 'repeated_collection_same_features_and_labels': True}. Dataset SHA-256: `1e0e69b0f467da7d53734c671c699f4238306cc107e19a53c9687281ba1ad98b`.

16 hand-written expression shapes × 6 schedules × 4 independent renamed runs × 4 rounds. Renamed: replica 0 dictionary, 1 validation, 2–3 test. Unseen schedule: dictionary schedules 0–3/replica 0; validation schedule 4/replica 1; test schedule 5/replicas 2–3. Unseen shape: dictionary shapes 0–11/replica 0; validation shapes 12 and 14/replica 1; test shapes 13 and 15/replicas 2–3. No snapshots from the same execution cross partitions.

Root provenance is reconstructed from known benchmark rule LHS terms and retained user-variable bindings. This avoids unstable generated variable names, but is not a generic provenance implementation. Historical roots follow subsequent merges. Event occurrence IDs are not treated as causal order; temporal features use explicit execution rounds.

The dictionary is a fixed pool of observations (including repeated templates), equal for every variant. Exact-top4 counts observations, not deduplicated templates. The matched-weight shuffled control uses the weight selected for real rule+role history instead of tuning its own weight. Shuffling occurs independently inside each partition with fixed seeds. Empty history is a real feature: two empty vectors have similarity 1.

No estimate of full e-graph compression is reported: child binding tables, primitive payloads, shared dictionary, reconstruction indexes and trace collection overhead would all need accounting. Exhaustive canonicalization is an offline label oracle bounded to at most six external ports, not the deployed retrieval algorithm.

```sh
cargo run --bin rule_history_data -- results/rule_history/data.jsonl
python3 experiments/rule_history/ablate.py
python3 -m unittest discover -s experiments/rule_history -p "test_*.py"
```

Analysis elapsed: 40.760s; default debug collector timings are in the raw data and are not a production performance benchmark.
