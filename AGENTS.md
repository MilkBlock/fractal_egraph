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
