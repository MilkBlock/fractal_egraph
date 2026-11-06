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
#let mem=json("memory-update.json")
#let perf=json("performance-update.json")
#let tip(body)=block(width:100%,inset:(x:11pt,y:8pt),fill:light,stroke:(left:2pt+green),body)
#let cap(i,body)={v(1pt);text(font:"Avenir Next",size:8.8pt,fill:muted)[*Fig. #i*  #body];v(3pt)}
#let kicker(s)=block(above:6pt,below:3pt,text(font:"Avenir Next",size:9pt,weight:"bold",fill:green,tracking:1pt,s))
#let grid(head,rows,widths,sz:8.3pt,pad:4pt) = block(width:100%)[
 #set text(font:"Avenir Next",size:sz)
 #table(columns:widths,stroke:(bottom:0.35pt+rgb("#cad8dc")),inset:(x:3pt,y:pad),align:(x,y)=>if x==0 or head.at(x)=="eggcc passing workload" or head.at(0) in ("Physical object","Proposed seam") {left+horizon} else {right+horizon},fill:(x,y)=>if y==0 {navy} else if calc.rem(y,2)==1 {rgb("#f2f6f6")},
 table.header(..head.map(h=>text(fill:white,weight:"bold",h))),
 ..rows.flatten().map(x=>text(x)))
]
#let repo="https://github.com/MilkBlock/eggfuse/blob/70b291077252a8a6cfd9a00ebdd98f9599cd0c62/"
#let src(path,label)=link(repo+path)[#label]
#let bench="tools/cell-compiler/benchmarks/runtime-modules/"
#let code="engine/cell-runtime/src/"
#kicker("TECHNICAL REPORT   /   CPU BACKENDS")
#v(4pt)
#text(font:"Avenir Next",size:29pt,weight:"bold",fill:navy)[Compiling scheduled egglog rules into reusable CPU execution blocks]
#v(4pt)
#text(font:"Avenir Next",size:12pt,fill:muted)[An architecture proposal for egglog and eggcc, evaluated on frozen sum-bits and Raytrace Pass 3 workloads.]
#v(9pt)
Eggfuse compiles rule-local joins and effects into typed recipes, shares entry work across consumers, and carries checked references to committed constructor results across scheduled Runs. It leaves Free Join responsible for residual matches. The architectural proposal is to expose these operations as optional physical plans inside egglog, where the native engine can retain ownership of scheduling, merges and extraction.

#grid(("Latest Pass 3","Paired old ms","Candidate ms","Fixed ratio","Paired ratio"),perf.headline,(1.0fr,1.1fr,1.1fr,.95fr,1fr),sz:8.5pt,pad:5pt)
#text(size:9pt,fill:muted)[Latest verified checkpoint #raw("502e29cd"). The control is the earlier Cell CPU engine. Fixed ratios use the predeclared reference times; paired ratios use the control rerun in the same session. Candidate builds include native CPU, LTO and held-out PGO. Section 5 gives the full method and data.]

#tip[*The adoption decision.* Prototype shared entry packets and checked register joins beneath egglog's existing evaluator. The pinned cross-engine audit has a concrete sum-bits endpoint discrepancy even though projected table counts agree. Treat the timing differences as architecture evidence. Require the native observable-result and extraction gates in §7 before enabling an alternative backend for production.]
#v(5pt)
#text(size:9pt,fill:muted)[Performance checkpoint #raw("502e29cd") · memory implementation #raw("4d9ea942") · Apple M3 Max · Rust 1.94.0. Sections 5–6 report performance; Section 8 reports memory. Sample data, source identities and validation scripts are included.]

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
*Concrete saving.* Rule 143 witnesses #raw("Get(t,i)"), constructs its #raw("Single") and equates that result with #raw("SubTuple(t,i,1)"). A later rule 137 requires the same constructor key. The compiler records the donor instruction, distance, and consumer destination. The runtime validates the current row and canonical keys before reusing it. A whole-RHS preflight checks every required receipt before any effect is committed. An unexpected replacement chooses the ordinary arm. This avoids failed speculation partially applying a write. #src(code+"result_handoff.rs","Switch construction") · #src(bench+"RHS_COMPLETION.md","Measured pair")

*Learning larger blocks.* Start with 32 applied consumer/pivot leaves. A training round counts witnessed adjacent units on a winning lineage. A proposed binary unit names a left unit, the publishing instruction and a right unit. Freeze the catalog during each run, bound its size, and let an actually applied macro become a later operand. The donor used here has a 64-unit ceiling, at most four primitive steps, eight nodes, up to eight additions per round and a 512-entry candidate budget. When transferring, the harness asks the compiler to resolve each leaf's typed rule signature on the target. It remaps only reachable joins and rebuilds every RHS handoff against that target. #src(code+"compiler/spine_blocks.rs","Typed leaf transfer") · #src(code+"iterative.rs","Bounded composition")

#pagebreak()
= 5. Latest verified performance checkpoint
#kicker("502e29cd   /   FROZEN PASS 3 ACCEPTANCE")
The latest complete fixed-target acceptance file in the inspected branch records implementation #raw("502e29cd"). It compares two frozen Pass 3 programs with the original Cell executable at #raw("c8ee1a3f"), whose binary SHA was verified. Each case retains 32 primitive block leaves and 32 learned joins. One warm-up per arm is excluded, followed by 11 measured sum-bits pairs and seven Raytrace pairs in fresh, alternating processes.

#grid(("Pass 3","Paired old ms","Candidate ms","Fixed ratio","Paired ratio"),
 perf.headline,(1fr,1.1fr,1.1fr,.95fr,1fr),sz:9pt,pad:5pt)

#kicker("TWO DISTINCT ACCEPTANCE DENOMINATORS")
The fixed ratio divides the previously declared original median by the new candidate median. The paired ratio divides the original median remeasured in the same session by that candidate median. Both use the original Cell engine as control. Section 6 defines the timed intervals and build settings.

#grid(("Pass 3","Fixed reference ms","Fixed 2× cutoff ms","Candidates below cutoff"),
 perf.limits,(1fr,1.4fr,1.4fr,1.4fr),sz:8.5pt,pad:5pt)
Both candidate medians passed the fixed thresholds. The concurrent paired ratios were *1.849×* and *1.988×*. Thus the fixed-target checkpoint is complete, while a twofold improvement against both contemporaneous controls remains unestablished. The original executable ran faster in this session than in the session that defined the fixed thresholds.

#kicker("CLI, MEMORY AND LOGICAL WORK")
#grid(("Pass 3","Old CLI ms","New CLI ms","Old RSS MiB","New RSS MiB"),
 perf.execution,(.95fr,1.15fr,1.15fr,1fr,1fr),sz:8.5pt,pad:5pt)
#grid(("Pass 3","Old visits","New visits","Pairs","Runs","Resumes / chain"),
 perf.counts,(.9fr,1.15fr,1.15fr,.5fr,.8fr,1.3fr),sz:8.1pt,pad:4pt)
CLI time includes loading and exporting the same Cell graph. RSS is sampled after inference. Logical visits are the existing Cell work counter. Both independent full audits and forced-collision sum-bits matched the frozen RHS, effective-write, schedule, graph and Spine digests. The recorded test counts were 26 all-feature runtime, 12 ordinary runtime, nine minimal runtime and 15 compiler integration tests.

#tip[*Build treatment.* The candidate uses fat LTO, one codegen unit, native CPU code generation and offline PGO trained solely on the other workload. LLVM's value-profile counter capacity is 16 during training. The measured binary collects no profile. These comparisons combine execution and build changes; they do not isolate PGO's contribution.]

#pagebreak()
= 6. Benchmark method and interpretation
The performance evaluation covers the frozen sum-bits and Raytrace Pass 3 programs. Each executable runs the same source schedule on one CPU thread. Cell input consists of the checked recipe artifact, the marker representation and a fixed catalog of 32 primitive leaves plus 32 learned joins. The artifact, recipe, executable and opposite-workload profile hashes are bundled with the accepted measurements.

#kicker("TIMED INTERVALS")
Inference begins after artifact loading and runtime initialization and ends after the scheduled engine execution. It includes matching, RHS evaluation, rebuild and incremental publication. Complete CLI time also includes loading, diagnostics and graph export. Offline Bake and PGO training are outside these clocks. Each measured process starts fresh; one warm-up per arm is excluded, and control/candidate order alternates.

#kicker("CONTROLS AND BUILD SETTINGS")
The paired control is the original Cell executable at #raw("c8ee1a3f"), verified by SHA-256. The candidate checkpoint is #raw("502e29cd"). Its compiler configuration includes native CPU generation, fat LTO, one codegen unit and held-out PGO. The profile used for sum-bits comes from Raytrace; the profile used for Raytrace comes from sum-bits. These ratios measure the combined executable changes. A PGO-only claim requires a pair with every other build setting fixed.

#kicker("FIXED TARGETS AND PAIRED RATIOS")
The predeclared reference medians were 33.675 ms and 13,508.078 ms. Their twofold cutoffs are 16.8375 ms and 6,754.039 ms. The accepted candidate medians, 16.379 ms and 6,140.271 ms, pass both. The original remeasured in that session was 30.290 ms and 12,208.315 ms, yielding paired ratios of 1.849× and 1.988×. The measured pairs answer present-machine improvement; the fixed cutoffs answer progress against the declared target.

#kicker("OBSERVATIONS THAT MUST AGREE")
The candidate preserves the complete source Run sequence, ordered RHS inputs, effective write order, final logical graph and winning Spine lineage against the frozen Cell reference. The audit executable is independent of the timed build. Forced fingerprint and Sieve collisions exercise exact fallback checks. Counts of candidate visits, bindings, constructor requests and committed writes describe separate work units. A reduction in one count does not by itself establish a reduction in time.

#kicker("MEMORY HAS ITS OWN MATCHED EXPERIMENT")
Section 8 tests reader-lifetime reclamation and source-artifact release at ordinary release settings. Its control is #raw("27f098a2") and its implementation is #raw("4d9ea942"). The 1,432.68 MiB Raytrace peak belongs to that configuration; the performance checkpoint's 6.140 s and 1,978.20 MiB belong together. Each table reports its own treatment and sample counts.

#tip[*Rebuild the performance tables.* #raw("reproduce/render-performance-data.py") validates all measured samples and generates the tables from #raw("evidence/performance-acceptance.json"). The archived full-suite inputs and results remain in the companion data package. The current PDF's performance tables use the two-workload checkpoint.]

#pagebreak()
= 7. Semantic boundary and upstream decision
The current within-engine audits compare ordered RHS inputs, complete schedule boundaries, committed operation counts, final logical graphs and winning Spine lineages against frozen Cell references. Sum-bits has 1,428 audited Runs, 3,034 resumed steps and maximum depth 12. Raytrace has 16,883 Runs, 663,403 resumed steps and depth 29. These tests catch an illegal early activation or a dropped binding between Cell configurations. Cross-engine replacement also needs comparison against egglog.

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
= 8. Memory and cross-Run lifetime
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

These are within-Cell checks. The grounded cross-engine witness in §7 requires its own resolution. The memory comparison establishes within-Cell equivalence. Section 5 gives the independent fixed-target and contemporaneous paired performance checks.

#kicker("WHAT CROSS-RUN COMPLETION CAN STILL ABSORB")
History-reference trimming can recover only a few MiB in this measurement. Larger structures hold graph records, parameters, indexes and shared entry state. Future blocks should expose reader demand and last-use ports, then measure reconstruction costs against another retained row copy.

Generated view and completion rows are the next candidate. A compressed form must still support required canonical-key lookups, union forwarding, last-writer publication and exports. Their generated origin alone cannot justify removal.

#tip[*Reproduce the update.* #raw("reproduce/memory-replay.sh") accepts checkout #raw("4d9ea942") and two frozen program files. Features #raw("entry-retirement") and #raw("origin-demand") select lifetime optimizations; #raw("--release-source-artifact") releases validation IR. Bundled data contains all three matrices and their hashes.]

#pagebreak()
#set text(font:"Hiragino Sans GB",lang:"zh",size:10.7pt)
#set par(leading:.78em)
#kicker("MAINTAINER BRIEF   /   中文摘要")
#text(size:21pt,weight:"bold",fill:navy)[当前性能与内存结果]
#v(8pt)
本报告的性能主表采用已验证的 #raw("502e29cd") checkpoint，覆盖固定的 sum-bits 和 Raytrace Pass 3。推理中位数分别为 16.379 ms 和 6.140 s，相对预先声明的旧 Cell 基线达到 2.056× 与 2.200×。这组验收有 11 对 sum-bits 和 7 对 Raytrace 实测，均在排除预热后交替启动新进程。

同场重跑旧 Cell 得到 30.290 ms 和 12.208 s，因此同场中位数之比分别为 1.849× 和 1.988×。两个固定阈值都已通过；两者相对同场旧版都达到 2×，仍需要进一步证据。候选构建同时使用 native CPU、fat LTO、单 codegen unit，以及仅在另一个负载上训练的 PGO，这组比较反映它们与引擎改动的合计效果。

内存实验另以 #raw("27f098a2") 为控制，使用普通 release 配对。Raytrace 峰值 RSS 从 1,856.79 降到 1,432.68 MiB，减少 22.8%。改动包括在所有消费者越过入口 epoch 后释放旧材料化行，解除无读者 Unit 完成记录对生产者的保活，以及在运行结构初始化后释放验证 JSON 和输入缓冲。推理中位数为 6.746 → 6.717 s，逐对结果有波动，本轮主要支持内存收益。

对应原始未加 marker 的 Raytrace 源码在原生 egglog 上，两次单线程峰值约为 462.375 和 450.047 MiB。两边均包含解析、初始化和推理。原生未导出图，Cell 在导出前采样。内存比约从 4.07 倍降到 3.14 倍。生成的 View 与 Applied 表占最终 Cell 图约 32.4% 的行，后续压缩必须保住查询、修复转发、锚点发布和导出的可观察行为。

本轮各 Cell 配置的逐 Run RHS、有效写入、Spine 接续、调度和最终图与冻结参考一致。第 7 节保留了跨引擎检查中的具体 TermSingle 差异。原生后端替换还需定位该差异并验证提取结果。当前性能表的覆盖范围是这两个 Pass 3；原有全套输入与结果仍作为历史证据保存在数据附件中。
