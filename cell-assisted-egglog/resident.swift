import Foundation
import Metal

func exact(_ n: Int) throws -> Data {
  var d = Data()
  while d.count < n {
    guard let x = try FileHandle.standardInput.read(upToCount: n - d.count), !x.isEmpty else {
      throw NSError(domain: "EOF", code: 1)
    }
    d.append(x)
  }
  return d
}
func frame() throws -> Data {
  let h = try exact(4)
  let n = h.withUnsafeBytes { Int($0.loadUnaligned(as: UInt32.self)) }
  return try exact(n)
}
func send(_ v: [String: Any]) throws {
  let d = try JSONSerialization.data(withJSONObject: v, options: [.sortedKeys])
  var n = UInt32(d.count)
  FileHandle.standardOutput.write(Data(bytes: &n, count: 4))
  FileHandle.standardOutput.write(d)
}
func pow2(_ x: Int) -> Int {
  var v = 1
  while v < x { v *= 2 }
  return v
}
let NS = 25
let META = 0
let PARENT = 1
let FACT = 2
let FH = 3
let INST = 4
let IH = 5
let WATCH = 6
let WH = 7
let WS = 8
let ROWS = 9
let UNIONS = 10
let CLEARS = 11
let EVENTS = 12
let SEEDS = 13
let WORK = 14
let EXT = 15
let PART = 16
let PH = 17
let REF_NEXT = 18
let INDEX = 19
let XH = 20
let OUTPUT = 21
let NUM = 22
let NUM_UPDATES = 23
let NH = 24

final class Resident {
  let device: MTLDevice, queue: MTLCommandQueue, library: MTLLibrary, program: MTLBuffer
  let defs: [String: Int], strides: [Int]
  var kernels = [String: MTLComputePipelineState]()
  var heap: MTLBuffer!, caps = [Int](repeating: 0, count: NS),
    offsets = [Int](repeating: 0, count: NS)
  var epoch = 0, nrows = 0, nunions = 0, nevents = 0, nclears = 0, oldparents = 0, nnums = 0
  var gpuSeconds = 0.0, dispatches = 0, resizeCount = 0, queryRuns = 0
  init(_ config: [String: Any]) throws {
    guard (config["version"] as? NSNumber)?.intValue == 2 else {
      throw NSError(domain:"resident protocol mismatch; rebuild Rust and Swift together",code:1)
    }
    guard let d = MTLCreateSystemDefaultDevice(), let q = d.makeCommandQueue() else {
      throw NSError(domain: "Metal unavailable", code: 1)
    }
    device = d
    queue = q
    defs = (config["definitions"] as! [String: NSNumber]).mapValues { $0.intValue }
    let C = defs["MAX_COLS"]!
    let V = defs["MAX_VALUES"]!
    let P = defs["MAX_PARAMS"]!
    let K = defs["VARS"]!
    let F = defs["MAX_FIELDS"]!
    strides = [
      1, 1, 4 + C, 1, 4 + P + V, 1, 4, 1, 2, 4 + C, 2, 1, 1 + F, 2 + P, 1, 2, 4 + K, 1, 1, 3, 1, K,
      4, 3, 1,
    ]
    var source = ""
    for (name, value) in defs { source += "#define \(name) \(value)\n" }
    for (name, value) in config["offsets"] as! [String: NSNumber] {
      source += "#define \(name) \(value.intValue)\n"
    }
    source += config["shader"] as! String
    let options = MTLCompileOptions()
    if #available(macOS 15.0, *) {
      options.mathMode = .safe
    } else {
      options.fastMathEnabled = false
    }
    library = try d.makeLibrary(source: source, options: options)
    let words = (config["program"] as! [NSNumber]).map { $0.uint32Value }
    program = words.withUnsafeBytes {
      d.makeBuffer(bytes: $0.baseAddress!, length: max(4, $0.count), options: .storageModeShared)!
    }
    for name in [
      "init_parents", "clear_heads", "rehash", "clear_work", "apply_unions", "apply_clears",
      "load_numbers", "row_heads", "apply_rows", "seed_heads", "apply_seeds", "resolve_cells",
      "projection_heads",
      "intern_parts", "clear_indexes", "build_indexes", "match_delta", "finish_query",
    ] {
      kernels[name] = try d.makeComputePipelineState(function: library.makeFunction(name: name)!)
    }
    var initial = [Int](repeating: 1, count: NS)
    initial[META] = 64
    initial[PARENT] = 32
    initial[FACT] = 32
    initial[INST] = 32
    initial[WATCH] = 128
    initial[EXT] = 32
    initial[PART] = 32
    initial[OUTPUT] = 32
    try ensure(initial)
  }
  func meta(_ i: Int) -> Int { Int(heap.contents().bindMemory(to: UInt32.self, capacity: 64)[i]) }
  func set(_ i: Int, _ v: Int) {
    heap.contents().bindMemory(to: UInt32.self, capacity: 64)[i] = UInt32(v)
  }
  func encode(_ steps: [(String, Int)], into cb: MTLCommandBuffer) {
    var args = offsets.map { UInt32($0) } + caps.map { UInt32($0) }
    args += [epoch, nrows, nunions, nevents, nclears, oldparents, nnums].map { UInt32($0) }
    let parameters = args.withUnsafeBytes {
      device.makeBuffer(bytes: $0.baseAddress!, length: $0.count, options: .storageModeShared)!
    }
    for (name, n) in steps where n > 0 {
      let enc = cb.makeComputeCommandEncoder()!
      let pipe = kernels[name]!
      enc.setComputePipelineState(pipe)
      enc.setBuffer(heap, offset: 0, index: 0)
      enc.setBuffer(parameters, offset: 0, index: 1)
      enc.setBuffer(program, offset: 0, index: 2)
      enc.dispatchThreads(
        MTLSize(width: n, height: 1, depth: 1),
        threadsPerThreadgroup: MTLSize(
          width: min(128, pipe.maxTotalThreadsPerThreadgroup), height: 1, depth: 1))
      enc.endEncoding()
      dispatches += 1
    }
  }
  func finish(_ cb: MTLCommandBuffer) throws {
    cb.commit()
    cb.waitUntilCompleted()
    if let e = cb.error { throw e }
    if cb.gpuEndTime > cb.gpuStartTime { gpuSeconds += cb.gpuEndTime - cb.gpuStartTime }
    if meta(15) != 0 { throw NSError(domain: "GPU invariant error \(meta(15))", code: meta(15)) }
  }
  func run(_ steps: [(String, Int)]) throws {
    let cb = queue.makeCommandBuffer()!
    encode(steps, into: cb)
    try finish(cb)
  }
  // Capacity growth is lossless resource management. No candidate is dropped.
  // Persistent arrays are copied by GPU blits, never interpreted by the host.
  func ensure(_ requested: [Int]) throws {
    var next = caps
    for i in 0..<NS { next[i] = max(caps[i], pow2(max(1, requested[i]))) }
    next[WS] = max(next[WS], next[INST] * (defs["MAX_READS"]! + defs["MAX_VALUES"]!))
    let buckets = pow2(max(64, 2 * max(next[FACT], next[INST], next[PART], next[NUM])))
    for i in [FH, IH, WH, PH, WORK, XH, NH] { next[i] = buckets }
    if next == caps { return }
    var no = [Int](repeating: 0, count: NS)
    var words = 0
    for i in 0..<NS {
      no[i] = words
      words += next[i] * strides[i]
    }
    guard words < Int(UInt32.max),
      let nh = device.makeBuffer(length: words * 4, options: .storageModeShared)
    else { throw NSError(domain: "GPU allocation too large", code: 1) }
    let rehash = next[FH] != caps[FH]
    let count = heap == nil ? 0 : max(meta(0), meta(1), meta(2), meta(3), meta(21))
    let cb = queue.makeCommandBuffer()!
    let blit = cb.makeBlitCommandEncoder()!
    blit.fill(buffer: nh, range: 0..<words * 4, value: 0)
    if let old = heap {
      for i in 0..<NS where caps[i] > 0 {
        blit.copy(
          from: old, sourceOffset: offsets[i] * 4, to: nh, destinationOffset: no[i] * 4,
          size: caps[i] * strides[i] * 4)
      }
    }
    blit.endEncoding()
    oldparents = caps[PARENT]
    heap = nh
    caps = next
    offsets = no
    resizeCount += 1
    // Links store row indices, not pointers. Moving arrays does not invalidate
    // their hash tables. Rebuild only when the bucket count actually changes.
    var steps = [("init_parents", caps[PARENT] - oldparents)]
    if rehash { steps += [("clear_heads", buckets), ("rehash", count)] }
    encode(steps, into: cb)
    try finish(cb)
  }
  func copy(_ words: [UInt32], _ start: Int, _ count: Int, _ section: Int) {
    if count == 0 { return }
    _ = words.withUnsafeBytes { p in
      memcpy(
        heap.contents().advanced(by: offsets[section] * 4), p.baseAddress!.advanced(by: start * 4),
        count * 4)
    }
  }
  func batch(_ bytes: Data) throws {
    let words = bytes.withUnsafeBytes { Array($0.bindMemory(to: UInt32.self)) }
    guard words.count >= 7 else { throw NSError(domain: "short event packet", code: 1) }
    let newEpoch = Int(words[0])
    guard newEpoch > epoch else { throw NSError(domain: "stale event packet", code: 1) }
    epoch = newEpoch
    nrows = Int(words[1])
    nunions = Int(words[2])
    nevents = Int(words[3])
    nclears = Int(words[4])
    nnums = Int(words[5])
    let rowWords = nrows * strides[ROWS]
    let unionWords = nunions * 2
    let eventWords = nevents * strides[EVENTS]
    guard words.count == 7 + rowWords + unionWords + eventWords + nclears + 3 * nnums else {
      throw NSError(domain: "bad event packet shape", code: 1)
    }
    let maxValue = words[6]  // equality-sort IDs only, never primitive payload IDs
    var want = caps
    want[PARENT] = max(want[PARENT], Int(maxValue) + 1)
    want[NUM] = max(want[NUM], meta(21) + nnums)
    want[NUM_UPDATES] = max(1, nnums)
    want[FACT] = max(want[FACT], meta(0) + nrows)
    want[INST] = max(want[INST], meta(1) + nevents * defs["TEMPLATE_COUNT"]!)
    want[EXT] = max(want[EXT], meta(4) + nrows * defs["PART_COUNT"]!)
    want[ROWS] = max(1, nrows)
    want[UNIONS] = max(1, nunions)
    want[EVENTS] = max(1, nevents)
    want[CLEARS] = max(1, nclears)
    want[SEEDS] = max(1, nevents * defs["TEMPLATE_COUNT"]!)
    try ensure(want)
    var at = 7
    copy(words, at, rowWords, ROWS)
    at += rowWords
    copy(words, at, unionWords, UNIONS)
    at += unionWords
    copy(words, at, eventWords, EVENTS)
    at += eventWords
    copy(words, at, nclears, CLEARS)
    at += nclears
    copy(words, at, 3 * nnums, NUM_UPDATES)
    set(5, 0)
    set(6, 0)
    set(7, 0)
    try run([
      ("load_numbers", nnums), ("apply_unions", nunions),
      ("apply_clears", nclears == 0 ? 0 : meta(0)),
      ("clear_work", caps[WORK]), ("row_heads", nrows), ("apply_rows", nrows == 0 ? 0 : caps[WORK]),
      ("clear_work", caps[WORK]), ("seed_heads", nevents * defs["TEMPLATE_COUNT"]!),
      ("apply_seeds", nevents == 0 ? 0 : caps[WORK]), ("resolve_cells", caps[INST]),
    ])
    while meta(6) != 0 {
      guard meta(6) == 1 else { throw NSError(domain: "unexpected resolver overflow", code: 1) }
      let needed = meta(2)
      set(2, min(needed, caps[WATCH]))
      var w = caps
      w[WATCH] = max(needed, caps[WATCH] * 2)
      try ensure(w)
      set(6, 0)
      try run([("resolve_cells", meta(1))])
    }
    if meta(5) != 0 {
      queryRuns += 1
      let refs = meta(1) * defs["MAX_LENSES"]! + meta(4)
      var w = caps
      w[REF_NEXT] = max(1, refs)
      w[PART] = max(1, meta(3) + refs)
      try ensure(w)
      try run([
        ("clear_work", caps[WORK]), ("projection_heads", refs), ("intern_parts", caps[WORK]),
      ])
      w = caps
      w[INDEX] = max(1, meta(3) * defs["INDEX_COUNT"]!)
      try ensure(w)
      set(12, 0)
      try run([("clear_indexes", caps[XH]), ("build_indexes", meta(3))])
      while true {
        set(6, 0)
        set(7, 0)
        try run([("match_delta", meta(3))])
        if meta(6) == 0 { break }
        guard meta(6) == 2 else { throw NSError(domain: "unexpected query overflow", code: 1) }
        w = caps
        w[OUTPUT] = max(meta(7), caps[OUTPUT] * 2)
        try ensure(w)
        // Only OUTPUT grew; the existing tuple indexes remain valid.
      }
      try run([("finish_query", 1)])
    }
    let n = meta(7)
    let K = defs["VARS"]!
    try send([
      "ok": true, "epoch": epoch, "bindings": n, "facts_stored": meta(0), "instances": meta(1),
      "active_instances": meta(18), "pending_instances": meta(1) - meta(18),
      "watches_stored": meta(2), "fragment_bindings_stored": meta(3), "external_refs": meta(4),
      "numeric_values": meta(21), "guard_checks": meta(19), "guard_rejected": meta(20),
      "watch_hits": meta(8), "cell_validations": meta(9), "device_fact_lookups": meta(10),
      "seed_reuses": meta(11), "join_candidates": meta(13), "emitted_total": meta(16),
      "delta_anchors": meta(17), "query_runs": queryRuns, "kernel_dispatches": dispatches,
      "buffer_resizes": resizeCount, "resident_bytes": heap.length, "gpu_seconds": gpuSeconds,
    ])
    if n > 0 {
      FileHandle.standardOutput.write(
        Data(bytes: heap.contents().advanced(by: offsets[OUTPUT] * 4), count: n * K * 4))
    }
  }
}
do {
  let config = try JSONSerialization.jsonObject(with: frame()) as! [String: Any]
  let runtime = try Resident(config)
  try send(["ready": true, "protocol":2, "device": runtime.device.name])
  while true {
    let packet: Data
    do { packet = try frame() } catch { break }
    try runtime.batch(packet)
  }
} catch {
  try? send(["ok": false, "ready": false, "error": String(describing: error)])
  fputs("resident Metal: \(error)\n", stderr)
  exit(1)
}
