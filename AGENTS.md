# Repository workflow

- Validate the minimal CPU branch using the local Cargo compile, run and bench commands in README.md. The debugger and historical experiments live on the research branch. Published site releases require explicit user instruction.

- Maintain Git history as part of completing implementation work: after a coherent change passes its relevant checks, create a focused Conventional Commit with a scope. Report its short commit ID.
- Preserve the immutable `egglog-baseline` tag. Keep local kernel changes, experiment infrastructure, and experiment results distinguishable from the pristine upstream import.
- Inspect the index and working tree before staging. Commit only work belonging to the current task; preserve unrelated changes and do not rewrite existing history without user authorization.
- Do not track build directories, Python caches, generated binary mutation streams, or credentials. Commit the reproducible experiment sources and relevant text results.
- Document whether a result measures a standalone prototype or the actual egglog runtime. Do not describe logical rule matches as committed mutations.

Before optimizing the Cell CPU path, check whether a new iteration can learn from the last catalog's actual executions, which work a scheduled block switch could discharge across Runs, and which typed compiler facts can prove that work redundant. Record the opportunity, the obligations that remain live, and the measured change against the current branch.
