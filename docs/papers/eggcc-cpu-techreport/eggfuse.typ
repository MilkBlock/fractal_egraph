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
#let data=json("report-data.json")
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
#let repo="https://github.com/MilkBlock/eggfuse/blob/70b291077252a8a6cfd9a00ebdd98f9599cd0c62/"
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
#text(size:9pt,fill:muted)[N/C is original unmarked egglog replay time divided by Eggfuse replay time. Ratios above 1 favor Eggfuse. Each row uses all paired successes in that scope. Appendix A gives every task. Appendix B gives the original eggcc production timings.]

Across #data.n tasks, the three captured schedules took #data.native_s s in native egglog and #data.cell_s s in Cell in aggregate. Preparation of the Cell targets took an additional #data.prep_s s in this experiment. Raytrace contributed 82.6% of the aggregate time saved. Excluding it gives a time-weighted ratio of 1.029×. A fixed sum-bits block catalog was transferred to every target without per-target block training. The survey measures isolated, single-thread inference. Each next pass starts from eggcc's captured production input. A separately repeated comparison appears in §6.

#tip[*The adoption decision.* Prototype shared entry packets and checked register joins beneath egglog's existing evaluator. The current standalone backend has a concrete sum-bits endpoint discrepancy even though projected table counts agree. Treat the timing differences as architecture evidence. Require the native observable-result and extraction gates in §7 before enabling an alternative backend for production.]
#v(5pt)
#text(size:9pt,fill:muted)[Revision of 6 November. Section 9 adds a separate two-workload memory study. The full-suite measurements below use their original snapshot.]
#text(size:9pt,fill:muted)[Source snapshot #raw("70b2910") · Apple M3 Max · arm64 macOS 27.0.1 · Rust 1.94.0. The corpus has 96 Bril/Rust workloads and 288 pass inputs. Scope, individual timings, replay scripts, witness and input digests are included in the companion artifact.]

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
= 5. What explains the gain
The strongest historical improvement over the earlier Cell implementation came from compiling fast per-pivot register joins, specializing single writes, and reducing frame and index overhead. The comparison below uses frozen 64-block artifacts and single-thread release builds. The numerator is a previous *Cell* engine. §6 gives the independent egglog comparison. #src(bench+"INFERENCE_THROUGHPUT.md","Paired historical measurements")

#grid(("Workload","Earlier Cell","Optimized Cell","Ratio","Pairs above 2×"),(("sum-bits","79.34 ms","37.07 ms","2.14×","8 / 9"),("Raytrace","33.81 s","15.50 s","2.18×","5 / 5")),(1.15fr,1fr,1.18fr,.72fr,1.2fr))
On Raytrace the register plans produced 2,650,667 bindings and the single-write plans avoided 1,017,798 transient frame registrations. Peak process memory fell from 4,122 to 3,265 MiB in that paired study. The reported candidate-work counter actually rose. A guess about fewer row visits would therefore give the wrong explanation for this result.

*What RHS specialization buys separately.* With the fixed Raytrace catalog, the later guarded switch avoided 30,896 constructor requests and 17,152 complete RHS frames. Its comparison against dispatch alone was faster in one of four matched rounds. Fewer operations and a demonstrated throughput gain require separate measurements. #src(bench+"RHS_COMPLETION.md","RHS ablation")

*Why context retention stays optional.* In a five-atom loop-invariant rule, a #raw("DoWhile") root keyed reads of input context, output context and type. Retaining up to 84 correlated prefixes let the final #raw("Const") expansion join against the changed side. Rule search dropped from 149.31 to 50.45 ms and emitted the same 143,282 bindings. Whole-pass Raytrace inference was 13.837 versus 13.844 s. Generalizing the compiler found cuts in 481 of 582 rules. Enabling all cuts increased full-pass visits from 8.42 to 10.33 million relative to narrow retention. The portable path used in this report leaves the context experiments off. #src(bench+"LOCAL_CONTEXT.md","Loop-invariant trace") · #src(bench+"CONTEXT_CUTS.md","Admission study")

*Break-even condition.* Retaining a prefix pays when avoided probes, decodes and sorting exceed work to observe changes, validate roots, refresh prefixes and route answers. It matters how often old prefixes meet new terminal rows. Counting only distinct roots hides this join multiplicity. Query hints also need a lifetime budget and a fallback for changing workloads. The present single-thread measurements supply no scaling curve for a parallel implementation.

#pagebreak()
= 6. Three measurements of eggcc workloads
*Production.* The capture ran eggcc at #raw("ae80f82") with its pinned #raw("egglog-experimental 08771f9") and actual Tiger extraction feeding the next pass. The pass driver was instrumented. Appendix B sums time inside ordinary Run and outer Saturate packets once, then separately reports extraction and execution including setup. These are archived measurements on original emitted programs. They answer where eggcc spent time. #link("https://github.com/egraphs-good/eggcc/blob/ae80f82a403c1209fb084b115ee89470c7e66d0e/Cargo.toml")[Pinned dependency]

*Matched replay.* For every captured pass the experimental frontend resolved the original program, printed the resulting flat commands, and normalized generated names. Both timed lanes start from that unmarked source. Native Free Join uses the project's immutable #raw("egglog-baseline 62ae6baa"), with an external timing adapter and compiled-out work counters. The native clock starts at the first Run schedule after bootstrap and includes subsequent commands. Eggfuse's Rust compiler transforms the same flat input and emits a marked typed recipe. Its clock covers schedule execution after graph setup. The timings exclude training, parsing, final export and Tiger. Raw experimental syntax needs the experimental frontend; passing it directly to the tag runner fails the prelude/type environment on the first corpus input. #src("tools/cell-compiler/benchmarks/free-join/README.md","Native adapter and counter boundaries")

*Fixed treatment.* One thread, ordinary storage, shared nodes, planned Free Join, Sieve, intersection, range guards, static RHS aliases, Stage Spine and receipts. All targets use one frozen, small marker-training program exercising terms and helpers, and one iterative block donor learned from real sum-bits Pass 3 executions. These supply different compiler inputs. The target receives 59 deployable units in each Pass 1 and Pass 2 program and 64 in each Pass 3 program. Cell setup derives and checks each target's plans. This is within-workload transfer for the donor benchmark and cross-workload deployment elsewhere. The suite uses this policy throughout. Runtime settings, input hashes, repetitions and compiler costs are recorded with the data.

#grid(("Repeated Pass 3","Pairs","Native ms","Cell ms","Ratio","Cell wins"),data.pairs,(1.5fr,.65fr,1fr,1fr,.8fr,1fr))
#text(size:9pt,fill:muted)[One excluded warm-up per arm, then four alternated fresh-process pairs. Ratios divide the two medians. The corpus survey executes one sample per arm and alternates order by workload index. Close single-sample outcomes should be interpreted at their measured precision.]
#v(5pt)
At the process boundary, native / Cell median wall time was 104.6 / 145.5 ms for sum-bits, 170.3 / 208.7 ms for fenwick_tree, and 23.673 / 16.196 s for Raytrace. The Cell CLI also exports the final graph and detailed report. These observations expose startup and output costs; the two CLIs have different output duties.

Earlier case-specific catalogs produced native-original / Cell inference medians of 46.43 / 36.68 ms on sum-bits and 24.192 / 14.015 s on Raytrace in a separate matched experiment. Native-on-marked medians in that study were 54.85 ms and 21.697 s. The original-program column is the baseline for an architectural claim. The marked column measures how the transformation changes the work egglog sees. The companion archive includes the historical samples.

#pagebreak()
= The upstream baseline on its own terms
The first numeric column below is the time eggcc's pinned engine spent in scheduled packets on the original inputs. Its driver ran each pass, extracted with Tiger, and supplied that result to the next pass. The capture also separates parsing and non-schedule setup. Columns N and C are fresh replays of those captured boundaries. They answer the narrower question of what an alternative execution layer costs on a common input representation. Historical production runs and fresh replays have different harnesses and sampling dates.

#image("figures/boundary.svg",width:100%)
#cap(5,[The solid path is the measured production pipeline. The dashed branch reruns that specific pass input under two execution engines. Replaying the three captured inputs isolates inference from drift in preceding extracted programs.])
#grid(("All workloads","Production sched, s","Tiger wall, s","N replay, s","C replay, s"),data.production,(1fr,1.25fr,1fr,1.05fr,1.05fr))
#text(size:9pt,fill:muted)[Production figures cover all archived cases per pass. Replay totals cover completed pairs shown on the first page. The original driver profile does not record an explicit thread setting. Only the fresh N/C comparisons here enforce one thread.]

#kicker("WHAT EGGLOG ALREADY DOES")
The tagged engine decomposes a query into join stages. During Free Join execution, a chosen cover enumerates keys or projected rows, other atoms probe indexes, and intersected domains constrain later stages. Completed substitutions reach an action buffer. The rule bridge invokes native merge and rebuild machinery, including incremental versus nonincremental rebuild selection. Shared indexes, an incremental frontier, compiled join plans and vectorized actions are features of the baseline. The proposed extension reuses correlated cross-rule entry packets and physical producer receipts across Run boundaries. #link("https://github.com/MilkBlock/eggfuse/tree/egglog-baseline/egglog/core-relations/src/free_join")[Native Free Join] · #link("https://github.com/MilkBlock/eggfuse/blob/egglog-baseline/egglog/egglog-bridge/src/lib.rs")[Native rule bridge and rebuild]

#kicker("A COUNTER IS A UNIT OF WORK")
On historical frozen sum-bits, instrumented native Free Join on the original source recorded 40,425 enumerated index keys, 5,918 projected scan rows, 871 materializations and 34,698 probes. The 8,763 bindings delivered to the action buffer were counted separately. Eggfuse's visits sum direct candidates, residual witnesses or key attempts, merge and Spine candidates and shared predicate checks. Comparing the two integers as numbers of rows eliminated would mix different operations. Timings here use the native runner with counters compiled out. #src("tools/cell-compiler/benchmarks/free-join/README.md","Definitions and native ablation")

The storage proposal can be evaluated incrementally. First keep native Free Join and table storage as the oracle and share one entry computation for consumers with identical normalized atom shapes. Then compare full bindings from register kernels at the action-buffer interface. Introduce a committed fact handle only for operations that can prove its identity, visibility and canonical key. This sequence isolates which interface earns its implementation cost.

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
= 8. Distribution and cost of portability
#image("figures/suite.svg",width:100%)
#cap(6,[Equal-height bars correspond to equal multiplicative changes. The horizontal reference is parity. All cases contribute one observation irrespective of program size.])
#grid(("Three-pass tails","Native / Cell","Native sum, ms","Cell sum, ms"),data.tails.map(row=>(row.at(0).split("/").last(),..row.slice(1))),(1.6fr,1fr,1.1fr,1.1fr))
The first three rows are the largest regressions among the covered tasks; the last three are the largest improvements. Reporting them alongside the aggregate makes the deployment trade-off visible. The geomean weights one workload once. The time-weighted ratio answers how long the entire sequential sweep spends in inference. Both use sums of three frozen inference intervals per case.

*Interpreting preparation.* The general compiler re-runs the fixed marker Bake, selects markers, lowers the target, then validates and links the donor blocks for each independent artifact. This deliberately straightforward driver repeats preparation that a service might cache. It does not measure such a cache. If all measured preparation is additional and startup/export differences are set to zero, the illustrative break-even reuse count is preparation time divided by positive per-execution inference savings. Over this suite the implied break-even is about 48 replays of the same prepared artifacts. Compiling each program once for a single inference run would lose the observed 16.62 s saving many times over. Sharing plans across input programs is therefore an integration requirement worth measuring. When inference is slower, repeated runs do not repay preparation.

*Reproducibility.* The data package contains the 288 flat programs, raw eggcc inputs, compact production summaries, the frozen donor, full per-run JSONL and CSV, and Cargo-backed replay code. Keys are workload path plus pass number. Duplicate basenames remain distinct. Source links point to the tested commit. The unpinned workstation had concurrent system activity. Alternating order and repeating three anchors reduces some timing noise; the full-corpus single samples have no per-task confidence interval. Automatic context cuts, reference views, local saturation, union grafts and GPU execution were outside this fixed policy. The suite provides evidence about this pinned revision and machine. Other egglog releases, architectures and thread counts require their own measurements.

#pagebreak()
= 9. Memory and cross-Run lifetime
#kicker("6 NOVEMBER 2026   /   TWO-WORKLOAD UPDATE")
This follow-up measures the standalone Cell CPU engine at #raw("27f098a2") and the lifetime changes implemented in #raw("4d9ea942"). It uses the frozen sum-bits and Raytrace Pass 3 artifacts, their original 64-unit Bake catalog, one CPU thread and ordinary release builds. These runs add evidence about memory to the earlier suite. Their timing matrix has its own control; no PGO is used here.

#grid(("Pass 3","Control ms","Reclaimed ms","Control MiB","Reclaimed MiB"),
 mem.outcomes,(1.0fr,1fr,1.05fr,1fr,1.1fr),sz:8.8pt,pad:5pt)
Raytrace's peak RSS fell by *#mem.reduction_percent%*. In the same matrix, complete CLI medians were 10.373 and 10.374 s. The median of paired inference differences was +16.663 ms, with only one of four reclaimed runs faster. A lower ratio of medians therefore supplies little evidence of a latency improvement.

#kicker("RECHECKING THE NATIVE MEMORY GAP")
Two new runs of eggcc's pinned native egglog #raw("ebba7bb") executed the original unmarked Raytrace source with a verified one-worker pool. macOS #raw("time -l") recorded #mem.native_samples.at(0) and #mem.native_samples.at(1) MiB peak RSS; the median was #mem.native_rss MiB. Native timing included parsing, initialization and inference, without a graph export. Cell RSS was sampled after inference and before its export, including its artifact loader and generated tables. The control/native memory ratio was approximately *#mem.old_native_ratio×*, falling to *#mem.new_native_ratio×* with reclamation. Source and binary hashes accompany the measurements.

#kicker("COUNT LIVE ALLOCATIONS ONCE")
A diagnostic allocator attributes requested bytes to the allocating operation and subtracts them on deallocation. A shared Rc allocation is charged once. Reallocation retains its original category. At Raytrace inference completion it counted *#mem.allocation_total MiB* still live.

#grid(("Allocation origin","Live requested MiB"),mem.allocation.slice(0,8),(2.7fr,1.3fr),sz:8.6pt,pad:3pt)
The largest remaining categories include RHS (62.6 MiB), Sieve (22.1 MiB) and captured history (2.3 MiB). Allocation origin can differ from the eventual owner: matched parameters can become persistent Cell slots. Category peaks occur at different times. The ledger excludes allocator fragmentation and C allocations; its instrumented timing and RSS are excluded from the speed comparison.

#text(size:8.5pt,fill:muted)[Evidence: #raw("evidence/memory-lifetimes.json"), rendered by #raw("reproduce/render-memory-data.py"). The original 96-workload tables retain their earlier source revision and measurement dates.]

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

These are within-Cell checks. The grounded cross-engine witness in §7 requires its own resolution. The memory comparison alone establishes neither native semantic equivalence nor a completed twofold speed target.

#kicker("WHAT CROSS-RUN COMPLETION CAN STILL ABSORB")
History-reference trimming can recover only a few MiB in this measurement. Larger structures hold graph records, parameters, indexes and shared entry state. Future blocks should expose reader demand and last-use ports, then measure reconstruction costs against another retained row copy.

Generated view and completion rows are the next candidate. A compressed form must still support required canonical-key lookups, union forwarding, last-writer publication and exports. Their generated origin alone cannot justify removal.

#tip[*Reproduce the update.* #raw("reproduce/memory-replay.sh") accepts checkout #raw("4d9ea942") and two frozen program files. Features #raw("entry-retirement") and #raw("origin-demand") select lifetime optimizations; #raw("--release-source-artifact") releases validation IR. Bundled data contains all three matrices and their hashes.]

#pagebreak()
#let per=24
#for start in range(0,data.tasks.len(),step:per) {
  if start>0 {pagebreak()}
  kicker("APPENDIX A   /   FROZEN REPLAY")
  heading(level:1)[Per-workload inference]
  [Milliseconds. Within each pass, N denotes native egglog on unmarked flat input and C denotes the standalone Cell executor. The final column divides the three-pass sums. LA abbreviates #emph[linear-algebra] in the paths.]
  v(6pt)
  grid(("ID","eggcc passing workload","P1 N","P1 C","P2 N","P2 C","P3 N","P3 C","N/C"),data.tasks.slice(start,calc.min(start+per,data.tasks.len())),(.35fr,2.85fr,.71fr,.71fr,.71fr,.71fr,.8fr,.8fr,.55fr),sz:8pt,pad:5pt)
}
#pagebreak()
#for start in range(0,data.archived.len(),step:per) {
  if start>0 {pagebreak()}
  kicker("APPENDIX B   /   PRODUCTION EGGCC CAPTURE")
  heading(level:1)[Native schedules and Tiger extraction]
  [Milliseconds in eggcc's original captured production run. P1–P3 are scheduled execution only. Extract is the three-pass sum of extraction wall time. Exec is the sum of recorded pass execution time including setup. Keep these columns separate from the fresh replay ratios in Appendix A.]
  v(6pt)
  grid(("ID","eggcc passing workload","P1","P2","P3","Extract","Exec"),data.archived.slice(start,calc.min(start+per,data.archived.len())),(.35fr,3.15fr,.9fr,.9fr,.95fr,1fr,1fr),sz:8.2pt,pad:5pt)
}

#pagebreak()
#set text(font:"Hiragino Sans GB",lang:"zh",size:10.7pt)
#set par(leading:.78em)
#kicker("MAINTAINER BRIEF   /   中文摘要")
#text(size:21pt,weight:"bold",fill:navy)[建议先在 egglog 的动作缓冲区前接入编译计划]
#v(7pt)
eggcc 的真实生产链和独立重放给出了两个不同的答案。附录 B 记录原始 eggcc 程序如何在固定版本的生产引擎中执行、再由 Tiger 把结果传给下一 pass。附录 A 在同一个已经展开的输入上测试两个执行器。原生 egglog 直接执行未加 marker 的程序；Cell 由编译器生成 marker、共享入口、寄存器连接计划和跨 Run 的指令块。这个设计可以把推理收益同上一个 pass 选出了什么程序分开。

#v(5pt)
#text(weight:"bold",fill:green)[覆盖全部输入后，收益主要集中在哪里？]
共 96 个 passing 任务、288 个 pass，双方全部完成。三段推理相加，原生 egglog 共 143.43 秒，Cell 共 126.80 秒；73 个任务更快，23 个更慢。等权几何平均为 1.138×，按总耗时为 1.131×。Raytrace 占总节省时间的 82.6%。去掉它，按总时间仍有 1.029×。算法必须连同失利的例子一起看。3mm 的三段合计从 5.40 秒升至 6.02 秒。当前离线准备累计约 792 秒。若每个新程序都承担完整准备成本，只运行一次会慢于原生路径。

#v(5pt)
#text(weight:"bold",fill:green)[在哪个接口开始改最容易验证？]
先让编译器把同一输入变化编码为一份带槽位、事实引用和 epoch 的只读数据包，按规则原来的 Run 交给消费者。让已证明的完整键读取走寄存器计划，让剩余入口继续走原生 Free Join。两条路径都向 egglog 原有的动作缓冲区交付完整、有序的 RHS 输入。这样可以在保留原生 merge、rebuild 和 extraction 的条件下，直接比较每条绑定。

第二步才引入跨 Run 的事实引用。生产者已经构造过的结果，必须在下一消费者真正被调度时，验证其可见性、当前等价类和完整键。通过检查就切换到复用指令块，缺少任何必需端口就走普通路径。图 4 给出了实际的 SubTuple、Get、Single 链。

#v(5pt)
#text(weight:"bold",fill:green)[现有证据支持多强的结论？]
sum-bits 两端投影到原始表后都是 3,206 行，但正文给出的具体 TermSingle 在 Cell 端可见，原生 egglog 的查询失败。相同 check 在原生执行 marked 源码时也失败。要声称可以替换后端，还需要定位首次语义分歧，核对可见事实和等价类，并用各自的 Tiger 结果独立驱动完整三 pass，对比提取程序及其行为。本报告支持一次可以逐步验收的上游架构实验。

#pagebreak()
#set text(font:"Hiragino Sans GB",lang:"zh",size:10.7pt)
#kicker("6 NOVEMBER UPDATE   /   内存与跨 Run 生命周期")
#text(size:21pt,weight:"bold",fill:navy)[让消费关系决定状态保留多久]
#v(8pt)
本轮从 #raw("27f098a2") 出发，在固定的 sum-bits 与 Raytrace Pass 3 上测试独立 Cell CPU 引擎。Raytrace 峰值 RSS 从 1,856.79 降到 1,432.68 MiB，减少 22.8%。原生 egglog 在未加 marker 的对应源码上，两次单线程测得 462.375 和 450.047 MiB，均包含解析、初始化和推理，未导出图。Cell 的峰值在导出前采样，包含编译产物加载。两类表示的内存比约从 4.07 倍降到 3.14 倍。

编译器首先列出共享入口的全部消费者。所有消费者都越过某个 epoch 后，引擎释放该 epoch 的旧材料化行，并保留准确的成员基数。后来的变更可以重建行；请求更早的 watermark 也会触发重建。Raytrace 留存的入口包从 665,584 降到 82,355。

另一项分析检查 Unit 完成表是否存在声明的读者。无人读取来源的完成记录继续保留键、值和修复身份，同时解除对生产者 Cell 的保活。完整 Cell 与紧凑来源快照合计从 687,913 降到 648,737。第三项显式选项在运行结构初始化后释放验证用 JSON 和输入缓冲；后续离线 Bake 需要重载原文件。

合用三项改动后，Raytrace 推理中位数为 6.746 → 6.717 秒，但逐对差异的中位数为 +16.663 ms，四对中只有一对更快。因此本轮证据主要支持内存收益。逐 Run 的 RHS、有效写入、Spine 接续、调度和最终图与冻结 Cell 参考一致。第 7 节的跨引擎具体语义差异仍需要单独解决。

分配统计还显示，History 产生的存活申请量约 2.3 MiB。更大的空间在图存储、参数、索引和共享入口里。最终图中生成的 View 与 Applied 表共占约 32.4% 的行；进一步压缩这些记录，需要证明它们在后续查询、union 转发、锚点发布与导出中的可观察行为。
