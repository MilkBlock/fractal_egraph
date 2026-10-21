# Cell 代价与复用账表

来源：`/Users/mineralsteins/.codex/worktrees/automatic-refutation/egg_layout/out/cell-ledger-reuse/report.json`。诊断计数：True。核对通过：True。

— 表示未测或不能独立归因。共享片段只记一次；candidate 是候选行访问，不是 match 或新事实。

## Analyze Cell

| ID | 共享片段 | 查询 ID | 独立净收益秒 | 口径 |
|---|---|---|---|---|
| 0 |  |  | — | no compiled projection; no runtime reuse |
| 1 | 0,1,2,3,4,5 | 2,3,5,6,7,368,369,374,375,376,377,380,381 | — | shared projections: follow plan rows; do not duplicate credit |
| 2 | 0,1,2,3,4,5 | 2,3,5,6,7,368,369,374,375,376,377,380,381 | — | shared projections: follow plan rows; do not duplicate credit |
| 3 | 0,1,2,3,4,5 | 2,3,5,6,7,368,369,374,375,376,377,380,381 | — | shared projections: follow plan rows; do not duplicate credit |
| 4 | 0,1,2,3,4,5 | 2,3,5,6,7,368,369,374,375,376,377,380,381 | — | shared projections: follow plan rows; do not duplicate credit |

## 共享片段

| ID | 来源 Cell | 请求 | 命中 | 命中率 | 端口请求 | 端口非空回答 | 端口命中率 | 新读取 | 缓存读取 | 订阅检查 | 跳过读取 |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 0 | 1,2,3,4 | 0 | 0 | — | 0 | 0 | — | 416 | 0 | 0 | 0 |
| 1 | 1,2,3,4 | 0 | 0 | — | 0 | 0 | — | 0 | 0 | 0 | 0 |
| 2 | 1,2,3,4 | 6756 | 76 | 1.12% | 92 | 60 | 65.22% | 11367 | 18 | 0 | 76 |
| 3 | 1,2,3,4 | 0 | 0 | — | 0 | 0 | — | 0 | 0 | 0 | 0 |
| 4 | 1,2,3,4 | 0 | 0 | — | 0 | 0 | — | 416 | 0 | 0 | 0 |
| 5 | 1,2,3,4 | 0 | 0 | — | 0 | 0 | — | 416 | 0 | 0 | 0 |

## 源码编译程序

| ID | 对象 | 请求 | 命中 | 命中率 | 新读取 | 缓存读取 | 缓存率 | 独立净收益秒 |
|---|---|---|---|---|---|---|---|---|
| 0 | Const | 0 | 0 | — | 0 | 0 | — | — |
| 1 | Arg | 0 | 0 | — | 0 | 0 | — | — |
| 2 | Bop | 0 | 0 | — | 47848 | 26806 | 35.91% | — |
| 3 | Top | 0 | 0 | — | 16842 | 8861 | 34.47% | — |
| 4 | Uop | 0 | 0 | — | 22 | 3 | 12.00% | — |
| 5 | Get | 0 | 0 | — | 85314 | 5 | 0.01% | — |
| 6 | Single | 0 | 0 | — | 33058 | 2750 | 7.68% | — |
| 7 | Concat | 0 | 0 | — | 34261 | 11051 | 24.39% | — |
| 8 | PotentialExtractedExpr | 0 | 0 | — | 0 | 0 | — | — |
| 9 | PotentialExtractedExpr | 0 | 0 | — | 536749 | 56030 | 9.45% | — |
| 10 | PotentialExtractedExpr | 0 | 0 | — | 71204 | 0 | 0.00% | — |
| 11 | Smaller | 0 | 0 | — | 0 | 0 | — | — |
| 12 | Smaller | 0 | 0 | — | 0 | 0 | — | — |
| 13 | Smaller | 0 | 0 | — | 0 | 0 | — | — |
| 14 | Function | 1798 | 704 | 39.15% | 1487 | 0 | 0.00% | — |
| 15 | Const | 1798 | 17 | 0.95% | 0 | 0 | — | — |
| 16 | Top | 1798 | 1290 | 71.75% | 143633 | 103094 | 41.78% | — |
| 17 | Bop | 1798 | 1553 | 86.37% | 316085 | 132399 | 29.52% | — |
| 18 | Uop | 1798 | 3 | 0.17% | 7 | 0 | 0.00% | — |
| 19 | Get | 1798 | 1637 | 91.05% | 977455 | 0 | 0.00% | — |
| 20 | Concat | 1798 | 1589 | 88.38% | 539510 | 134947 | 20.01% | — |
| 21 | Single | 1798 | 1586 | 88.21% | 311109 | 0 | 0.00% | — |
| 22 | Switch | 1798 | 0 | 0.00% | 0 | 0 | — | — |
| 23 | If | 1798 | 965 | 53.67% | 20035 | 32701 | 62.01% | — |
| 24 | DoWhile | 1798 | 973 | 54.12% | 7857 | 6591 | 45.62% | — |
| 25 | Arg | 1798 | 5 | 0.28% | 0 | 0 | — | — |
| 26 | Call | 1798 | 972 | 54.06% | 7030 | 0 | 0.00% | — |
| 27 | Empty | 1798 | 2 | 0.11% | 0 | 0 | — | — |
| 28 | Cons | 1798 | 0 | 0.00% | 0 | 0 | — | — |
| 29 | Nil | 1798 | 0 | 0.00% | 0 | 0 | — | — |
| 30 | Alloc | 1798 | 6 | 0.33% | 366 | 0 | 0.00% | — |

## 闭包证明组

| ID | 请求 | 命中 | 命中率 | 独立净收益秒 | 口径 |
|---|---|---|---|---|---|
| terms-joint-closure-proofs | 12 | 0 | 0.00% | — | comparison closures consumed: 1185; marginal time needs cell-credit |

## 原始查询

| ID | 对象 | 共享片段 | 请求 | 原候选访问 | 现候选访问 | 省去候选访问 | 口径 |
|---|---|---|---|---|---|---|---|
| 2 | switch_rewrite | 0,1,2,3 | 2 | 0 | 0 | 0 | generic empty requests: 2; hits column = output bindings, not a rate |
| 3 | switch_rewrite | 0,1,2,3 | 2 | 0 | 0 | 0 | generic empty requests: 2; hits column = output bindings, not a rate |
| 5 | switch_rewrite | 0,1,3 | 2 | 0 | 0 | 0 | generic empty requests: 2; hits column = output bindings, not a rate |
| 6 | switch_rewrite | 4 | 2 | 0 | 0 | 0 | generic empty requests: 2; hits column = output bindings, not a rate |
| 7 | switch_rewrite | 5 | 2 | 0 | 0 | 0 | generic empty requests: 2; hits column = output bindings, not a rate |
| 368 | interval-analysis | 2 | 28 | 9106 | 9022 | 84 | generic empty requests: 1; hits column = output bindings, not a rate |
| 369 | interval-analysis | 2 | 28 | 9106 | 9022 | 84 | generic empty requests: 1; hits column = output bindings, not a rate |
| 374 | interval-analysis | 2,4 | 30 | 0 | 0 | 0 | generic empty requests: 30; hits column = output bindings, not a rate |
| 375 | interval-analysis | 2,5 | 30 | 0 | 0 | 0 | generic empty requests: 30; hits column = output bindings, not a rate |
| 376 | interval-analysis | 2,4 | 30 | 0 | 0 | 0 | generic empty requests: 30; hits column = output bindings, not a rate |
| 377 | interval-analysis | 2,5 | 30 | 0 | 0 | 0 | generic empty requests: 30; hits column = output bindings, not a rate |
| 380 | interval-analysis | 4 | 30 | 0 | 0 | 0 | generic empty requests: 30; hits column = output bindings, not a rate |
| 381 | interval-analysis | 4 | 30 | 0 | 0 | 0 | generic empty requests: 30; hits column = output bindings, not a rate |

## 共享归因组

```json
[
  {
    "id": 0,
    "plans": [
      0,
      1,
      2,
      3,
      4,
      5
    ],
    "consumers": [
      2,
      3,
      5,
      6,
      7,
      368,
      369,
      374,
      375,
      376,
      377,
      380,
      381
    ],
    "owners": [
      1,
      2,
      3,
      4
    ],
    "candidates_saved": 168,
    "net_seconds": null,
    "scope": "query work difference for the connected sharing group; no per-parent credit"
  }
]
```

完整指标、port 命中率和口径见同名 JSON/CSV。组的候选访问差额不自动等于独立 Cell credit。
