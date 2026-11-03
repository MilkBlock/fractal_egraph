# Repository workflow

- Default to the local debugger workflow: `python3 tools/egglog_debugger/server.py --port 8080`.
  Do not push `gh-pages`, rebuild the published site, or fetch the published URL for
  verification unless the user explicitly asks for a release; the network budget is limited
  and each deploy/verify ships and downloads tens of MB.

- Maintain Git history as part of completing implementation work: after a coherent change passes its relevant checks, create a focused Conventional Commit with a scope. Report its short commit ID.
- Preserve the immutable `egglog-baseline` tag. Keep local kernel changes, experiment infrastructure, and experiment results distinguishable from the pristine upstream import.
- Inspect the index and working tree before staging. Commit only work belonging to the current task; preserve unrelated changes and do not rewrite existing history without user authorization.
- Do not track build directories, Python caches, generated binary mutation streams, or credentials. Commit the reproducible experiment sources and relevant text results.
- Document whether a result measures a standalone prototype or the actual egglog runtime. Do not describe logical rule matches as committed mutations.

## Branches and manual integration

- Use `main` as the integration branch. For a new change, start a topic branch from the intended base and name that base in the PR. Treat `master` as the historical baseline.
- Push commits to their same-named topic branches. Preserve the immutable `egglog-baseline` tag.
- Only the repository owner, MilkBlock, manually merges a PR into `main`. Agents may prepare and update PRs. Agents must never merge, enable auto-merge, enqueue a merge, or bypass branch rules, including when authenticated as MilkBlock.
- Keep configuration/entry-provider consolidation, ChangeBatch publication, and residual-filter integration in separately reviewable PRs. For a stacked PR, identify its parent and retarget after the owner merges that parent.
- Include the commit range, validation and known failures in the PR. For performance changes, include the workload and paired control. Retain source branches until the owner decides to retire them.
