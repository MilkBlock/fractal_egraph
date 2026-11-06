#set document(title: "Compiling scheduled egglog rules into reusable CPU execution blocks",author:"Eggfuse research project",date:datetime(year:2026,month:11,day:6))
#let navy=rgb("#17394b")
#let green=rgb("#126f65")
#let muted=rgb("#506772")
#let light=rgb("#eef4f5")
#set page(paper:"a4",margin:(x:18mm,top:19mm,bottom:19mm),header:context [#set text(font:"Avenir Next",size:8pt,fill:muted); #align(left)[EGGFUSE   /   SYSTEMS RESEARCH #h(1fr) EXECUTION ARCHITECTURE]],footer:context [#line(length:100%,stroke:0.45pt+rgb("#bbcbd0"))#v(3pt)#set text(font:"Avenir Next",size:8pt,fill:muted); 6 November 2026 #h(1fr) Technical report   ·   #counter(page).display("1")])
#set text(font:"Palatino",size:10.5pt,fill:rgb("#172f39"),lang:"en")
#set par(leading:0.66em,justify:false)
#show heading: set text(font:"Avenir Next",fill:navy)
#show heading.where(level:1):set text(size:21pt,weight:"bold")
#show heading.where(level:2):set text(size:13pt,weight:"semibold")
#show raw:set text(font:"Menlo",size:8.3pt)
#show link:set text(fill:green)
#let data=json("current-report-data.json")
#let mem=json("memory-update.json")
#let tip(body)=block(width:100%,inset:(x:11pt,y:8pt),fill:light,stroke:(left:2pt+green),body)
#let cap(i,body)={v(1pt);text(font:"Avenir Next",size:8.8pt,fill:muted)[*Fig. #i*  #body];v(3pt)}
#let kicker(s)=block(above:6pt,below:3pt,text(font:"Avenir Next",size:9pt,weight:"bold",fill:green,tracking:1pt,s))
#let grid(head,rows,widths,sz:8.3pt,pad:4pt) = block(width:100%)[
 #set text(font:"Avenir Next",size:sz)
 #table(columns:widths,stroke:(bottom:0.35pt+rgb("#cad8dc")),inset:(x:3pt,y:pad),align:(x,y)=>if x==0 or head.at(x)=="eggcc passing workload" or head.at(0) in ("Physical object","Proposed seam") {left+horizon} else {right+horizon},fill:(x,y)=>if y==0 {navy} else if calc.rem(y,2)==1 {rgb("#f2f6f6")},
 table.header(..head.map(h=>text(fill:white,weight:"bold",h))),
 ..rows.flatten().map(x=>text(x)))
]
#let repo="https://github.com/MilkBlock/eggfuse/blob/27f098a251b87449d25cc833aee7c32a7411d340/"
#let src(path,label)=link(repo+path)[#label]
#let bench="tools/cell-compiler/benchmarks/runtime-modules/"
#let code="engine/cell-runtime/src/"
#kicker("TECHNICAL REPORT   /   CPU BACKENDS")
#v(4pt)
#text(font:"Avenir Next",size:29pt,weight:"bold",fill:navy)[Compiling scheduled egglog rules into reusable CPU execution blocks]
#v(4pt)
#text(font:"Avenir Next",size:12pt,fill:muted)[An architecture proposal for egglog and eggcc, evaluated on every workload in the passing suite at eggcc revision #raw("ae80f82").]
#v(9pt)
Eggfuse compiles rule-local joins and effects into typed recipes, shares entry work across consumers, and carries checked references to committed constructor results across scheduled Runs. It leaves Free Join responsible for residual matches. The architectural proposal is to expose these operations as optional physical plans inside egglog, where the native engine can retain ownership of scheduling, merges and extraction.

#grid(("Frozen inference","Tasks","Cell faster","Geomean N/C","Total N/C"), data.summary,(1.5fr,.65fr,1fr,1.18fr,1fr))
#text(size:9pt,fill:muted)[N/C is original unmarked egglog replay time divided by Eggfuse replay time. Ratios above 1 favor Eggfuse. All 288 inputs have three measured pairs. Appendix A plots every workload; Appendix B lists every pass.]

Across #data.n workloads, the sum of per-pass median inference times is #data.native_s s for native egglog and #data.cell_s s for Cell. Raytrace contributes #data.raytrace_saved_share% of the time saved; excluding it gives a time-weighted ratio of #data.without_raytrace_ratio×. A fixed sum-bits block catalog transfers to all targets without per-target block training. Each pass starts from eggcc's captured production input. Offline preparation adds #data.prep_s s across the suite. Section 6 separates inference, process time, preparation and memory costs.

#tip[*The adoption decision.* Prototype shared entry packets and checked register joins beneath egglog's existing evaluator. The current standalone backend has a concrete sum-bits endpoint discrepancy even though projected table counts agree. Treat the timing differences as architecture evidence. Require the native observable-result and extraction gates in §7 before enabling an alternative backend for production.]
#v(5pt)
#text(size:9pt,fill:muted)[Cell #raw("27f098a2") · Native #raw("62ae6baa") · Apple M3 Max · arm64 macOS 27.0.1 · Rust 1.94.0. The corpus has 96 Bril/Rust workloads and 288 pass inputs. Scope, individual timings, replay scripts, witness and input digests are included in the companion artifact.]

#pagebreak()
= 1. Where the compiler meets the engine
Eggcc supplies a scheduled equality-saturation program and uses an extractor to choose a result for its next pass. Egglog supplies incremental relational evaluation and congruence closure. A useful extension point is the compiled rule: it already identifies variables, table calls, key columns, effect destinations and ruleset ownership. Eggfuse makes those relationships explicit for a standalone executor. #link("https://github.com/egraphs-good/eggcc/tree/ae80f82a403c1209fb084b115ee89470c7e66d0e")[eggcc source] · #link("https://doi.org/10.1145/3591239")[egglog semantics]

#image("figures/architecture.svg",width:100%)
#cap(1,[The compiler produces a checked artifact. The colored runtime path repeats at the caller's next legal Run. Dashed inputs carry training evidence or refreshed state across a boundary. Native extraction sits outside the replay timer.])
*Two kinds of prior knowledge.* The compiler proves facts such as “this key is bound” or “this RHS requests the same constructor as that producer.” Training observes how often a path actually wins. The first supplies the legality conditions. The second decides whether spending memory and compilation effort on the path is worthwhile. #src("tools/cell-compiler/src/lib.rs","Compiler entry") · #src(code+"program.rs","Artifact loader")

*What sharing can save.* Suppose $d$ changed rows satisfy one atom shape watched by $m$ consumers. If all $m$ separately decode and test that atom, its entry work is $d m c$, where $c$ is the cost per decode and test. With a shared entry it is approximately $d c + d sum_i p_i$, where $p_i$ is the cost of projecting and delivering the packet to consumer $i$. Each consumer still pays for its residual join, sorting and effects. This local model predicts a saving only when dispatch and maintenance cost less than the redundant entry work. Egglog can already share physical indexes, so that common cost is outside this comparison.

#pagebreak()
= 2. From a rule to a reusable plan
#kicker("SOURCE TRANSFORMATION")
The Rust front end reads the flattened pass, installs explicitly requested structural-key selection, runs Bake on a compatible training input, selects markers, and lowers the transformed program to Recipe IR. A marker represents a positive conjunction with explicit ports. The compiler emits maintenance rules to populate it before dependent consumers run. Its selection problem trades saved consumer atoms against maintenance cost, with structural estimates or measured inputs. Observing a Cell during Bake proposes reuse. The source transformation still needs a rule-derived positive view and a legal place in the schedule. #src("tools/cell-compiler/src/markers.rs","Marker selection") · #src("tools/cell-compiler/src/witness.rs","Cell witnesses")

#kicker("PHYSICAL LOWERING")
A rule becomes typed query atoms, input slots, effect destinations and a source-owned control tree. The compiler enumerates legal pivot-specific orders. A register kernel can admit a connected query with one indexed expansion, subsequent full-key reads and primitives whose operands are bound. Fresh columns write registers. Repeated variables, literals and previously assigned columns generate equality checks. The loader reconstructs physical plans from the typed recipe and checks serialized plans. #src(code+"row_kernel.rs","Kernel derivation") · #src(code+"execute_kernel.rs","Kernel interpreter")

#image("figures/join.svg",width:100%)
#cap(2,[Two consumers can subscribe to the same normalized atom shape. An arrival supplies a correlated row, its witness and epoch. Consumer projections map packet positions to local slots. An update to the other atom can also be the rule's pivot.])

For a changed row $r$ and consumer watermark $w$, entry selection restricts the delta to visible rows with $r."epoch" >= w$. The Sieve can prove that necessary domains do not intersect. Planned Free Join can probe compatible keys before materializing seeds and can start at a selective indexed domain. A fingerprint narrows the candidates. The matcher checks full values before emitting a binding. Ownership by epoch, witness and pivot plus source-slot deduplication eliminates duplicate derivations at the Run boundary. #src(code+"entry_schedule.rs","Shared nodes") · #src(code+"free_join.rs","Join routing") · #src(code+"sieve.rs","Sieve")

#pagebreak()
= 3. Cells, repair and the Run boundary
A *Cell template* contains an ordered effect recipe. An *instance* records its template ID, bound slots, optional parent and live fact positions. A constructor fact can be named by a #raw("FactRef {cell, op}"). The recipe tells the engine which relation and slot operands that position denotes. Decoding those operands produces the current logical row. A mutable function has a current key, value, owner and epoch. Query indexes and table widths still matter: storing facts through shared recipes trades duplicate representation for decoding, liveness tests and reference maintenance. #src(code+"graph.rs","Graph and repair") · #src(code+"slots.rs","Frame storage")

#image("figures/storage.svg",width:100%)
#cap(3,[The address of a constructor row combines a concrete Cell instance and an instruction position. A mutable function row has its own current owner. Purple shows the repair path after a key changes.])
#grid(("Physical object","What consumers may reuse","What can invalidate it"),(
 ("Cell fact position","A committed constructor witness","Deletion, subsumption, movement or a changed canonical key"),
 ("Function receipt","The current committed function row","An ordered merge or replacement of its current value"),
 ("Index bucket / packet","Candidate witnesses for a key and epoch","Membership, visibility or representative change"),
 ("Spine cursor","Bounded producer lineage at a live anchor","A stale receipt or failed consumer precondition")
),(1fr,1.35fr,1.65fr),sz:9pt)

#kicker("EXECUTION CONTRACT")
#block(fill:rgb("#f2f6f6"),inset:10pt,width:100%)[
```text
for each Run selected by the source schedule:
    read all rule LHSs against the current snapshot
    seal complete bindings in the rule's original input order
    apply their RHS instructions in order
    repair congruence and rekey affected rows
    publish visible changes, update subscribers and watermarks
    evaluate the next repeat/saturation condition
```
]
On union, reverse dependencies identify constructor and function keys involving an affected representative. The engine invalidates decoded payloads, handles rekeying or coalescence and delivers changes to the relevant indexes. Consumers must be able to observe old-key removal, new-key arrival, visibility and forwarding. A compiled block does not remove this repair obligation. A cheap RHS path can avoid registration of a temporary frame when a typed single function write can be completed directly. A failure of its preconditions resumes ordinary effects. #src(code+"runtime.rs","Run and RHS execution") · #src(bench+"INFERENCE_THROUGHPUT.md","Storage and kernel ablation")

#pagebreak()
= 4. How a block crosses Runs
A *Spine* follows committed facts that serve as anchors for subsequent rules. The compiler records which consumer pivot can read a producer's output and whether its remaining ports can be obtained exhaustively. The producer publishes a weak reference when its fact becomes visible. A later consumer can use the reference only at its own scheduled occurrence. The next matched rule can have other LHS entries, other matches and ordinary effects in the same Run.

#image("figures/handoff.svg",width:100%)
#cap(4,[An observed producer/consumer pair from Raytrace. The published SubTuple fact enables a later pivot. Producer equalities and constructor receipts can also satisfy part of the consumer RHS. The dashed edge requests a switch. The current runtime state decides whether it succeeds.])
*Concrete saving.* Rule 143 witnesses #raw("Get(t,i)"), constructs its #raw("Single") and equates that result with #raw("SubTuple(t,i,1)"). A later rule 137 requires the same constructor key. The compiler records the donor instruction, distance, and consumer destination. The runtime validates the current row and canonical keys before reusing it. A whole-RHS preflight checks every required receipt before any effect is committed. An unexpected replacement chooses the ordinary arm. This avoids failed speculation partially applying a write. #src(code+"result_handoff.rs","Switch construction") · #link("https://github.com/MilkBlock/eggfuse/blob/70b291077252a8a6cfd9a00ebdd98f9599cd0c62/tools/cell-compiler/benchmarks/runtime-modules/RHS_COMPLETION.md")[Original pair study]

*Learning larger blocks.* Start with 32 applied consumer/pivot leaves. A training round counts witnessed adjacent units on a winning lineage. A proposed binary unit names a left unit, the publishing instruction and a right unit. Freeze the catalog during each run, bound its size, and let an actually applied macro become a later operand. The donor used here has a 64-unit ceiling, at most four primitive steps, eight nodes, up to eight additions per round and a 512-entry candidate budget. When transferring, the harness asks the compiler to resolve each leaf's typed rule signature on the target. It remaps only reachable joins and rebuilds every RHS handoff against that target. #src(code+"compiler/spine_blocks.rs","Typed leaf transfer") · #src(code+"iterative.rs","Bounded composition")

#pagebreak()
= 5. Performance against native egglog
This study replays all 96 passing workloads at eggcc revision #raw("ae80f82"). Each of the three captured pass inputs is run in a fresh native process and a fresh Cell process three times. Per-pass values are medians. A three-pass workload value sums those three medians. The native runner executes the original unmarked flat program; Cell compiles that input and links the same donor catalog through checked rule signatures.

#grid(("Frozen inference","Workloads","Cell faster","Geomean N/C","Total N/C"),data.summary,(1.45fr,.8fr,1fr,1.15fr,1fr),sz:8.6pt,pad:5pt)
#image("figures/suite-current.svg",width:100%)
#cap(5,[One bar per workload, sorted by the ratio of native egglog to Cell time for three frozen pass inputs. Values above 1 favor Cell. Appendix A labels every workload and gives both totals; Appendix B gives all per-pass medians.])

Across the corpus, native inference totals #data.native_s s and Cell inference totals #data.cell_s s. Cell is faster on #data.wins of 96 three-pass totals and slower on #data.losses. The equal-weight geometric mean is #data.geomean×. Raytrace contributes #data.raytrace_saved_share% of the time saved; excluding it gives a time-weighted ratio of #data.without_raytrace_ratio×.

#kicker("REPEATED PASS 3 ANCHORS")
#grid(("Workload","Pairs","Native ms","Cell ms","N/C","Cell faster"),
 data.pairs,(1.3fr,.55fr,1.1fr,1.1fr,.8fr,.9fr),sz:8.8pt,pad:5pt)
These anchors are drawn from the same repeated full-suite study. They use ordinary release builds with one thread. Offline compilation and linking are recorded separately. The comparison combines the checked compiler representation with its execution engine, so an upstream trial should isolate the matcher, action and storage interfaces before adopting them.

#tip[*Interpretation.* Both sides receive the same captured pass boundary. The refreshed Cell endpoints match the archived Cell endpoints in #data.unchanged_endpoints of 288 inputs. Section 7 documents the independent native/Cell membership discrepancy. The timing evidence motivates an architecture experiment; the native-observable correctness gate still applies.]
#pagebreak()
= 6. A matched replay protocol
#kicker("INPUTS AND EXECUTABLES")
Eggcc's captured production run used revision #raw("ae80f82") and its pinned #raw("egglog-experimental 08771f9"), with Tiger extraction supplying the next pass. The supplied archive contains the resulting 288 flat pass inputs. This experiment freezes those boundaries. It does not substitute a new extractor into the production chain.

The native comparator is the immutable #raw("egglog-baseline") snapshot #raw("62ae6baa"). Its Cargo-built helper sets one worker and times execution from the first #raw("RunSchedule") onward. Native search/merge counter hooks are compiled out. The Cell compiler and executor use source #raw("27f098a2"), ordinary release settings and the sparse storage/shared-view execution features. Neither arm uses PGO or a machine-specific code-generation override in this suite.

#kicker("COMPILER TREATMENT")
Every Cell target receives the same small marker-training source and the same iterative donor learned from real sum-bits Pass 3 executions. Sum-bits is the donor workload; the other workloads test transfer. Marker training and block composition supply different information. The linker resolves typed rule signatures, retains reachable joins and rebuilds RHS handoff proofs against each target. The catalog contains 59 deployable units on Pass 1/2 and 64 on Pass 3. There is no per-target block training or per-workload runtime exception.

#kicker("TIMING AND OUTPUT")
For each input the harness compiles and links once, then alternates three fresh native/Cell process pairs. It records all samples, failures, source hashes, deployed units, Cell Run counts and endpoint hashes. The pass replay order is P3, P1, P2. The workstation is unisolated, so close results need additional repetitions.

Inference begins after input loading and initialization. Cell's interval includes scheduled search, RHS, repair and publication. Complete CLI adds loading and output; Cell also exports the full graph. Native RSS covers its whole process without graph export, and Cell RSS is sampled immediately after inference, before export. The process-level columns therefore describe each executable's actual obligations.

#grid(("Pass 3","Native RSS MiB","Cell RSS MiB","Native CLI ms","Cell CLI ms"),
 data.memory_rows,(1.1fr,1.15fr,1.1fr,1.15fr,1.15fr),sz:8.1pt,pad:5pt)
Across all 288 inputs, summed CLI medians are #data.native_cli_s s for native and #data.cell_cli_s s for Cell, a #data.cli_ratio× ratio before offline preparation. This process-level comparison includes their different output duties and is distinct from the inference ratio.

#kicker("PREPARATION AND AMORTIZATION")
Preparation totaled #data.prep_s s across the 288 Cell artifacts. The runner repeats the shared Bake and lowering for each target; caching this preparation is a future integration choice. Comparing this cost with the #data.saved_s s aggregate inference saving gives an illustrative break-even of #data.break_even replays of the entire prepared suite, assuming unchanged startup and export costs. One compilation per one execution has a different end-to-end trade-off.

The hashes and raw repetitions are in #raw("suite-current.jsonl") and #raw("evidence/suite-current-provenance.json"). #raw("reproduce/refresh-suite.sh") builds all four executables with Cargo; its Python driver orchestrates the measurements.
#pagebreak()
= 7. Semantic boundary and upstream decision
Archived within-Cell audits compare ordered RHS inputs, schedules, committed effects, final graphs and winning Spine lineages. Sum-bits has 1,428 audited Runs, 3,034 resumed steps and maximum depth 12; Raytrace has 16,883 Runs, 663,403 steps and depth 29. All 288 refreshed Cell endpoints match their archived references. Repetitions also agree on schedule hashes, bindings and resumed steps. These checks do not establish equivalence with egglog.

#tip[*A reproducible difference.* On sum-bits, both the original native export and current fixed-64 Cell export have 3,206 rows after projection to original table names; all per-table counts agree. Grounded reconstruction nevertheless finds a visible Cell #raw("TermSingle") with no matching native key. All 19 constructor occurrences in the witness resolve to visible Cell rows. Appending the ground check to the original flat source gives #raw("CheckError") in the timed baseline binary. The same check fails on the marked input in native egglog. The archive includes the concrete term, both exports and replay commands.]

The diverging term can be written compactly with #raw("g(i) = TermGet(TermArg(), i)") and #raw("b(op,x,y) = TermBop(op(),x,y)"):
#block(fill:rgb("#f2f6f6"),inset:7pt,width:100%)[
```text
q = b(Div, g(1), g(2))
p = b(Mul, q, TermConst(Int(2)))
witness = TermSingle(b(Add, g(0), b(Sub, g(1), p)))
```
]
This locates an observable membership difference. It leaves open whether the changed graph produces a different Tiger-extracted program and which implementation decision first caused the divergence. In particular, an order-sensitive merge can depend on an earlier schedule or binding-order difference. The table below identifies a smaller upstream experiment that would distinguish these causes.

#grid(("Proposed seam","Retain in egglog","Acceptance evidence"),(
 ("Compiled LHS entry","Native Run snapshot, watermark and action-buffer order","Shadow each pivot and compare complete substitutions, including misses and ties"),
 ("Register / writer plans","Native value interning, conflict resolution and application order","Compare key reads and ordered effects for insert, replace and no-op"),
 ("Receipt publication","Native rebuild, subsumption and canonicalization","Invalidate on old/new keys, visibility and forwarding; test deleted and merged witnesses"),
 ("Cross-Run block","Native scheduling and extraction","Check guards against current rows. Compare grounded queries and extracted IR through all passes")
),(1.05fr,1.28fr,1.72fr),sz:8.8pt)
The first implementation milestone is an opt-in alternative matcher feeding egglog's ordinary action buffer. Run the exact counterexample as a regression and capture the first divergent source Run. Only after equality partitions, function outputs, visibility and selected terms agree should a production trial replace native effect or storage handling. A final trial should include offline cost amortization, total compiler wall time, peak memory and execution of the resulting binaries.

#pagebreak()
= 8. Distribution and upstream adoption
#grid(("Workload","Native total ms","Cell total ms","Native / Cell"),data.tails,(2.8fr,1.15fr,1.15fr,1fr),sz:8pt,pad:5pt)
The first four entries have the smallest ratios and the last four the largest. Every workload contributes equally to the geometric mean, while total time weights longer programs more. Appendix A shows all 96 cases on a common log-ratio scale and gives exact totals. Appendix B shows where each result arises across the three passes.

#kicker("KEEP EGGLOG'S SEMANTIC OWNERSHIP")
Native egglog already provides Free Join, incremental indexes, vectorized action buffers and merge/rebuild machinery. The proposed integration adds compiler-proved shared entry packets and references to committed producer results at those interfaces. An upstream implementation can preserve native scheduling, interning, merge semantics and extraction while independently selecting the physical plan.

An entry compiler can share a normalized input across consumers, carry checked parameter projections, and discharge functional reads before a residual join. A successful match still enters the original ordered action buffer. A cross-Run continuation can reuse constructor results only after validating visibility and canonical keys at the consumer's scheduled occurrence.

#kicker("THE MEMORY COST REMAINS PART OF THE PROPOSAL")
The standalone representation also retains typed compilation metadata, producer slots, generated view/completion rows and auxiliary indexes. The native/Cell peak memory columns in Section 6 belong to this complete suite configuration. Section 9 measures a separate lifetime optimization on two inputs. Its lower RSS cannot be attached to the faster suite executable without another matched run.

A native integration should first test whether the existing native row storage can own the shared entry and completion metadata. The evidence favors measuring the amount of shared work and its lifecycle cost together. It leaves the grounded semantic gate in Section 7, extracted-program behavior, and input-to-output compiler wall time as explicit acceptance requirements.

#tip[*A reviewer can inspect every case.* Appendix A labels all workload ratios and includes their native/Cell totals. The per-pass table and CSV preserve every case and its source identity. The raw JSONL records all three pairs for every input.]
#pagebreak()
= 9. Memory and cross-Run lifetime
#kicker("6 NOVEMBER 2026   /   TWO-WORKLOAD MEMORY STUDY")
This follow-up measures the standalone Cell CPU engine at #raw("27f098a2") and the lifetime changes implemented in #raw("4d9ea942"). It uses the frozen sum-bits and Raytrace Pass 3 artifacts, their original 64-unit Bake catalog, one CPU thread and ordinary release builds. The memory matrix has its own control and ordinary release configuration; no PGO is used here.

#grid(("Pass 3","Control ms","Reclaimed ms","Control MiB","Reclaimed MiB"),
 mem.outcomes,(1.0fr,1fr,1.05fr,1fr,1.1fr),sz:8.8pt,pad:5pt)
Raytrace's peak RSS fell by *#mem.reduction_percent%*. In the same matrix, complete CLI medians were 10.373 and 10.374 s. The median of paired inference differences was +16.663 ms, with only one of four reclaimed runs faster. A lower ratio of medians therefore supplies little evidence of a latency improvement.

#kicker("RECHECKING THE NATIVE MEMORY GAP")
Two new runs of eggcc's pinned native egglog #raw("ebba7bb") executed the original unmarked Raytrace source with a verified one-worker pool. macOS #raw("time -l") recorded #mem.native_samples.at(0) and #mem.native_samples.at(1) MiB peak RSS; the median was #mem.native_rss MiB. Native timing included parsing, initialization and inference, without a graph export. Cell RSS was sampled after inference and before its export, including its artifact loader and generated tables. The control/native memory ratio was approximately *#mem.old_native_ratio×*, falling to *#mem.new_native_ratio×* with reclamation. Source and binary hashes accompany the measurements.

#kicker("COUNT LIVE ALLOCATIONS ONCE")
A diagnostic allocator attributes requested bytes to the allocating operation and subtracts them on deallocation. A shared Rc allocation is charged once. Reallocation retains its original category. At Raytrace inference completion it counted *#mem.allocation_total MiB* still live.

#grid(("Allocation origin","Live requested MiB"),mem.allocation.slice(0,8),(2.7fr,1.3fr),sz:8.6pt,pad:3pt)
The largest remaining categories include RHS (62.6 MiB), Sieve (22.1 MiB) and captured history (2.3 MiB). Allocation origin can differ from the eventual owner: matched parameters can become persistent Cell slots. Category peaks occur at different times. The ledger excludes allocator fragmentation and C allocations; its instrumented timing and RSS are excluded from the speed comparison.

#text(size:8.5pt,fill:muted)[Evidence: #raw("evidence/memory-lifetimes.json"), rendered by #raw("reproduce/render-memory-data.py"). Program identities and all three memory matrices are recorded in the evidence file.]

#pagebreak()
= Consumers determine retention
A shared entry node owns a set of declared rule consumers. After a source ruleset occurrence, each rule's logical watermark advances, including rules omitted by the ready set. The node can release a materialized packet once its epoch is *strictly below every consumer's watermark*. An equal epoch can still seed a match. A rule appearing in several sets advances with its latest actual occurrence.

The implementation keeps a compact membership header so join planning sees the same cardinality. A changed fact can rebuild its packet through the existing publication stream. An explicitly older watermark request marks the node cold and reconstructs its visible rows. Reclamation can run immediately or conservatively every 32 graph epochs. The latter delays release without advancing the readers.

On Raytrace, immediate reclamation reduced retained entry packets from *#mem.entry_packets.at(0)* to *#mem.entry_packets.at(1)*. Entry-origin requested storage fell from 149.0 to 72.4 MiB. There were 882,328 retirements over the schedule, including repeated incarnations of some witnesses.

#kicker("COMPLETION RECEIPTS NEED FEWER PRODUCER OWNERS")
The final Raytrace Cell graph has 1,034,514 logical rows. Generated #raw("CellSpineView*") tables account for 213,334; #raw("CellSpineApplied*") tables account for 121,502. Together they contribute *#mem.generated_percent%* of the rows. This table-name grouping describes the measurement. The optimization uses executable read contracts.

The three populated Applied tables have no declared query or RHS readers. Their Unit values use an old-value merge. The compiler derives origin demand from all rule queries, executable table reads and calls in merge expressions. It checks an emitted certificate against the target program. An unread receipt can keep its key, value and rebuild identity while releasing its optional creating-frame reference. An assertion or later declared rule that reads the table retains that producer.

#grid(("Pass 3","Full live Cells","Embedded snapshots","Both forms"),
 mem.physical,(.75fr,1.45fr,1.45fr,1.5fr),sz:8.0pt,pad:5pt)
Some remaining producers change from full Cells into embedded snapshots after an unnecessary second owner is removed. The combined count measures that distinction. Every completion-table row stays in the logical export. A caller choosing the new mode can receive #raw("None") from the optional origin accessor for an unread receipt.

#kicker("RELEASE VALIDATION IR AFTER INITIALIZATION")
A teardown probe freed about 323.7 MiB by dropping the original Raytrace validation JSON, plus 17.1 MiB for its input buffer. The explicit #raw("--release-source-artifact") option releases these after runtime plans are initialized. Typed plans, structural-key nodes and the original source SHA remain available. The residual object has a provenance-only schema, which the source loader rejects. Offline Bake or revalidation requires reloading the original artifact.

For sum-bits, the combined live-requested footprint fell from 131.99 to 22.08 MiB, while peak RSS changed by much less. Its parser/setup high-water had already occurred. This separates the benefit of reclaiming completed input state from the cost of initially loading it.

#pagebreak()
= Cost, correctness and remaining scope
The primary Raytrace matrix isolates entry retirement from validation-IR release.

#grid(("Arm","Inference s","Peak RSS MiB"),mem.factorial,(2fr,1fr,1.2fr),sz:9pt,pad:5pt)
The final matrix combines all three changes, with 32-epoch retirement. It rotates eight processes per sum-bits arm and four per Raytrace arm. Compilation and diagnostic runs are outside the measurements.

#grid(("Pass 3","Paired delta","Faster pairs","Control CLI ms","Reclaimed CLI ms"),mem.paired,(.9fr,1.1fr,.8fr,1.2fr,1.3fr),sz:8.3pt,pad:5pt)
The primary sum-bits retirement-only arm was slower in all eight pairs. The evidence supports a memory reduction with a small, workload-dependent maintenance cost. Both lifetime features remain opt-in.

#kicker("AUDIT THE SAME SCHEDULED OBSERVATIONS")
Full audits matched the frozen graph, schedule, ordered RHS inputs, effective writes and Spine trajectory. They include the combined mode and forced fingerprint/Sieve collisions. Twenty-eight runtime tests and fifteen compiler tests passed, covering reader lifetimes, backward watermark requests and rejected certificates.

These are within-Cell checks. The grounded cross-engine witness in §7 requires its own resolution. The memory comparison establishes within-Cell equivalence. Sections 5–6 give the separate 96-workload native/Cell replay comparison.

#kicker("WHAT CROSS-RUN COMPLETION CAN STILL ABSORB")
History-reference trimming can recover only a few MiB in this measurement. Larger structures hold graph records, parameters, indexes and shared entry state. Future blocks should expose reader demand and last-use ports, then measure reconstruction costs against another retained row copy.

Generated view and completion rows are the next candidate. A compressed form must still support required canonical-key lookups, union forwarding, last-writer publication and exports. Their generated origin alone cannot justify removal.

#tip[*Reproduce the update.* #raw("reproduce/memory-replay.sh") accepts checkout #raw("4d9ea942") and two frozen program files. Features #raw("entry-retirement") and #raw("origin-demand") select lifetime optimizations; #raw("--release-source-artifact") releases validation IR. Bundled data contains all three matrices and their hashes.]

#pagebreak()
#for (i, path) in data.workload_charts.enumerate() {
 if i>0 {pagebreak()}
 kicker("APPENDIX A   /   NATIVE EGGLOG VERSUS EGGFUSE")
 heading(level:1)[Every workload · #(i+1) / 4]
 [Three-pass frozen-input inference, ordered by native / Cell time. Each pass uses the median of three fresh processes. All four pages share the same log-ratio scale. A point right of 1× favors EggFuse. LA abbreviates #emph[linear-algebra].]
 v(6pt)
 image(path,width:100%)
}
#pagebreak()
#let per=24
#for start in range(0,data.tasks.len(),step:per) {
  if start>0 {pagebreak()}
  kicker("APPENDIX B   /   FROZEN REPLAY")
  heading(level:1)[Per-workload inference]
  [Median milliseconds from three fresh processes per backend. Within each pass, N denotes native egglog on unmarked flat input and C denotes the standalone Cell executor. The final column divides the three-pass sums. LA abbreviates #emph[linear-algebra] in the paths.]
  v(6pt)
  grid(("ID","eggcc passing workload","P1 N","P1 C","P2 N","P2 C","P3 N","P3 C","N/C"),data.tasks.slice(start,calc.min(start+per,data.tasks.len())),(.35fr,2.85fr,.71fr,.71fr,.71fr,.71fr,.8fr,.8fr,.55fr),sz:8pt,pad:5pt)
}
#pagebreak()
#set text(font:"Hiragino Sans GB",lang:"zh",size:10.7pt)
#set par(leading:.78em)
#kicker("MAINTAINER BRIEF   /   中文摘要")
#text(size:21pt,weight:"bold",fill:navy)[从逐 workload 的原生对照判断架构收益]
#v(7pt)
本报告比较的是原生 egglog 执行原始、未加 marker 的源码，与 EggFuse 独立 Cell CPU 执行器编译并执行同一份输入。共 96 个 workload、288 个冻结的 pass 输入，每个输入双方各运行三个独立进程。附录 A 用四页图逐个列出全部 workload、双方耗时和比值；附录 B 给出每个 pass 的原始中位数。

#v(5pt)
#text(weight:"bold",fill:green)[速度收益是否普遍，而非由单个大案例主导？]
三段推理中位数相加，原生 egglog 共 #data.native_s 秒，Cell 共 #data.cell_s 秒；#data.wins 个 workload 更快，#data.losses 个更慢。等权几何平均为 #data.geomean×。Raytrace 占总节省时间的 #data.raytrace_saved_share%。去掉它，按总时间仍为 #data.without_raytrace_ratio×。每个 pass 的输入均由原始 eggcc 生产运行提供；这些数值不代表两套后端分别驱动提取器后的完整编译时间。

#v(5pt)
#text(weight:"bold",fill:green)[如何把收益接进 egglog，而不是要求重写整个引擎？]
先在原生动作缓冲区前加入可选的编译入口和寄存器查询计划。共享入口向规则投递已解码参数，剩余查询继续使用 Free Join。所有匹配仍进入原生的有序动作缓冲区，保留 interning、merge、rebuild 与 extraction。第二步再引入跨 Run 的事实引用：生产者已提交的结果只能在消费者被原调度激活时，经过可见性、当前等价类和完整键检查后复用。

#v(5pt)
#text(weight:"bold",fill:green)[内存、准备成本和正确性必须一起验收。]
本轮全套对照使用普通 release、单线程，不用 PGO。离线准备共 #data.prep_s 秒，单次编译的总墙钟成本仍需单独评估。第 6 节的原生／Cell RSS 来自这套完整配置；第 9 节的回收实验另有配对控制，不把它的低内存与本轮最快时间拼成一个配置。

288 个新 Cell 终态与历史 Cell 终态一致，但这只证明 Cell 内部结果稳定。sum-bits 两端虽然都有 3,206 个投影行，具体 TermSingle 成员查询仍存在差异。上游试验应定位首次分歧，比较有序 RHS、可见事实、函数值、等价类和提取程序行为。本报告的证据支持一次可逐步验收的架构试验；正式替换后端还需要通过这一语义门槛。
