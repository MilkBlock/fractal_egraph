// MPS execution of the generated matrix program. The host never interprets
// bindings or branch truth: it reads compacted dimensions to configure MPS.
import Foundation
import Metal
import MetalPerformanceShaders

struct Matrix: Decodable {
  let dtype: String, rows: Int, cols: Int, offset: Int
  let fields: [String]
}
struct Stage: Decodable { let start: Int, count: Int, levels: [Int] }
struct Program: Decodable {
  let version: Int, layout: [String: Matrix], data: [UInt32], shader: String, stages: [Stage],
    epochs: [[Int]], groups: Int
}
func require(_ b: Bool, _ s: String) throws { if !b { throw NSError(domain: s, code: 1) } }
final class Executor {
  let p: Program, device: MTLDevice, queue: MTLCommandQueue, heap: MTLBuffer
  var kernels = [String: MTLComputePipelineState](), gpuSeconds = 0.0, hostCountReads = 0
  var calls = 0, positions = 0, macs = 0
  init(_ p: Program) throws {
    self.p = p
    try require(p.version == 1, "matrix program version")
    guard let d = MTLCreateSystemDefaultDevice(), let q = d.makeCommandQueue() else {
      throw NSError(domain: "Metal unavailable", code: 1)
    }
    device = d
    queue = q
    heap = p.data.withUnsafeBytes {
      d.makeBuffer(bytes: $0.baseAddress!, length: $0.count, options: .storageModeShared)!
    }
    let options = MTLCompileOptions()
    if #available(macOS 15.0, *) {
      options.mathMode = .safe
    } else {
      options.fastMathEnabled = false
    }
    let library = try d.makeLibrary(source: p.shader, options: options)
    for n in [
      "update_ports", "invalidate_features", "invalidate_conditions", "clear_dirty", "admit",
      "clear_requests", "reset_channels", "classify", "prepare_group", "build_features", "project",
      "collect", "targets", "histogram",
    ] {
      kernels[n] = try d.makeComputePipelineState(function: library.makeFunction(name: n)!)
    }
  }
  func m(_ name: String) -> Matrix { p.layout[name]! }
  func words(_ name: String) -> [UInt32] {
    let x = m(name)
    return Array(
      UnsafeBufferPointer(
        start: heap.contents().assumingMemoryBound(to: UInt32.self).advanced(by: x.offset),
        count: x.rows * x.cols))
  }
  func zero(_ name: String) {
    if name == "counters" {
      calls = 0
      positions = 0
      macs = 0
    }
    let x = m(name)
    memset(heap.contents().advanced(by: x.offset * 4), 0, x.rows * x.cols * 4)
  }
  func encode(_ n: String, _ size: Int, _ cfg: [UInt32], _ cb: MTLCommandBuffer) {
    if size == 0 { return }
    let e = cb.makeComputeCommandEncoder()!
    let k = kernels[n]!
    e.setComputePipelineState(k)
    e.setBuffer(heap, offset: 0, index: 0)
    cfg.withUnsafeBytes { e.setBytes($0.baseAddress!, length: $0.count, index: 1) }
    e.dispatchThreads(
      MTLSize(width: size, height: 1, depth: 1),
      threadsPerThreadgroup: MTLSize(
        width: min(128, k.maxTotalThreadsPerThreadgroup), height: 1, depth: 1))
    e.endEncoding()
  }
  func submit(_ cb: MTLCommandBuffer) throws {
    cb.commit()
    cb.waitUntilCompleted()
    if let e = cb.error { throw e }
    if cb.gpuEndTime > cb.gpuStartTime { gpuSeconds += cb.gpuEndTime - cb.gpuStartTime }
  }
  func invalidation(_ cb: MTLCommandBuffer) {
    let z = [UInt32](repeating: 0, count: 4)
    encode("invalidate_features", m("valid").rows * m("valid").cols, z, cb)
    encode("invalidate_conditions", m("evidence").rows, z, cb)
    encode("clear_dirty", m("dirty_ports").rows * m("dirty_ports").cols, z, cb)
  }
  func summary(_ stage: Stage, _ depth: Int?) throws -> [String: Any] {
    zero("histogram")
    let cb = queue.makeCommandBuffer()!
    let cfg = [UInt32(stage.start), UInt32(stage.count), UInt32.max, 0]
    encode("classify", stage.count, cfg, cb)
    encode("histogram", stage.count, cfg, cb)
    try submit(cb)
    let h = words("histogram")
    let c = words("counters")
    var work = [String: Int]()
    for (i, n) in m("counters").fields.enumerated() { work[n] = Int(c[i]) }
    work["matrix_calls"] = calls
    work["matrix_positions"] = positions
    work["matrix_macs"] = macs
    return [
      "depth": depth as Any? ?? NSNull(), "complete": Int(h[1]), "rejected": Int(h[2]),
      "continued": Int(h[0]), "work": work,
    ]
  }
  func run() throws -> [String: Any] {
    var saved = [[[String: Any]]]()
    var snapshots = [[[String: MTLBuffer]]]()
    for epoch in p.epochs {
      let cb = queue.makeCommandBuffer()!
      let cfg = [UInt32(epoch[0]), UInt32(epoch[1]), 0, 0]
      encode("update_ports", epoch[1], cfg, cb)
      invalidation(cb)
      encode("admit", m("live").rows, cfg, cb)
      try submit(cb)
      var stageResults = [[String: Any]]()
      var stageSnapshots = [[String: MTLBuffer]]()
      for stage in p.stages {
        let start = queue.makeCommandBuffer()!
        let cfg = [UInt32(stage.start), UInt32(stage.count), UInt32.max, 0]
        encode("reset_channels", stage.count, cfg, start)
        try submit(start)
        zero("counters")
        var levels = [try summary(stage, nil)]
        for depth in stage.levels {
          zero("counters")
          for group in 0..<p.groups {
            let cfg = [UInt32(stage.start), UInt32(stage.count), UInt32(depth), UInt32(group)]
            let prepare = queue.makeCommandBuffer()!
            encode("clear_requests", m("requested").rows, cfg, prepare)
            encode("classify", stage.count, cfg, prepare)
            encode("prepare_group", 1, cfg, prepare)
            try submit(prepare)
            hostCountReads += 1
            let n = Int(words("g\(group)_count")[0])
            if n == 0 { continue }
            let rows = m("g\(group)_rows")
            let gram = m("g\(group)_gram")
            let groups = words("groups")
            let gs = m("groups")
            let wi = gs.fields.firstIndex(of: "width")!
            let width = Int(groups[group * gs.cols + wi])
            let mul = queue.makeCommandBuffer()!
            encode("build_features", m("live").rows, cfg, mul)
            encode("project", n, cfg, mul)
            let xd = MPSMatrixDescriptor(
              rows: n, columns: width, rowBytes: rows.cols * 4, dataType: .float32)
            let gd = MPSMatrixDescriptor(
              rows: n, columns: n, rowBytes: gram.cols * 4, dataType: .float32)
            let x = MPSMatrix(buffer: heap, offset: rows.offset * 4, descriptor: xd)
            let g = MPSMatrix(buffer: heap, offset: gram.offset * 4, descriptor: gd)
            let gemm = MPSMatrixMultiplication(
              device: device, transposeLeft: false, transposeRight: true, resultRows: n,
              resultColumns: n, interiorColumns: width, alpha: 1, beta: 0)
            gemm.encode(commandBuffer: mul, leftMatrix: x, rightMatrix: x, resultMatrix: g)
            encode("collect", m("g\(group)_conditions").rows, cfg, mul)
            try submit(mul)
            calls += 1
            positions += n * n
            macs += n * n * width
          }
          levels.append(try summary(stage, depth))
        }
        let commit = queue.makeCommandBuffer()!
        encode("targets", stage.count, cfg, commit)
        invalidation(commit)
        // Audit snapshots stay on the device until the full program has completed.
        var snap = [String: MTLBuffer]()
        let blit = commit.makeBlitCommandEncoder()!
        for name in ["bindings", "live", "decisions", "target_status"] {
          let x = m(name)
          let b = device.makeBuffer(
            length: max(4, x.rows * x.cols * 4), options: .storageModeShared)!
          if x.rows * x.cols > 0 {
            blit.copy(
              from: heap, sourceOffset: x.offset * 4, to: b, destinationOffset: 0,
              size: x.rows * x.cols * 4)
          }
          snap[name] = b
        }
        blit.endEncoding()
        try submit(commit)
        stageResults.append(["levels": levels])
        stageSnapshots.append(snap)
      }
      saved.append(stageResults)
      snapshots.append(stageSnapshots)
    }
    let ids = words("stage_channels")
    let cells = m("bindings")
    func read(_ snap: [String: MTLBuffer], _ name: String) -> [UInt32] {
      let x = m(name)
      return Array(
        UnsafeBufferPointer(
          start: snap[name]!.contents().assumingMemoryBound(to: UInt32.self), count: x.rows * x.cols
        ))
    }
    for epoch in 0..<saved.count {
      for (si, stage) in p.stages.enumerated() {
        let s = snapshots[epoch][si]
        let v = read(s, "bindings")
        let d = read(s, "decisions")
        let t = read(s, "target_status")
        let selected = Array(ids[stage.start..<stage.start + stage.count]).map { Int($0) }
        saved[epoch][si]["decisions"] = selected.map { d[$0] }
        saved[epoch][si]["target_status"] = selected.map { t[$0] }
        saved[epoch][si]["live"] = read(s, "live")
        saved[epoch][si]["cells"] = (0..<cells.rows).map {
          Array(v[$0 * cells.cols..<($0 + 1) * cells.cols])
        }
      }
    }
    return [
      "backend": "metal-mps", "epochs": saved, "device": device.name, "gpu_seconds": gpuSeconds,
      "host_count_reads": hostCountReads, "intermediate_binding_readbacks": 0,
      "matrix_bytes": heap.length,
      "scope":
        "generated matrix program, GPU predicates/frontier/targets; host reads dimensions for compact MPS dispatch; no native runtime speedup claim",
    ]
  }
}
do {
  try require(CommandLine.arguments.count == 3, "usage: matrix-worker lowered.json report.json")
  let p = try JSONDecoder().decode(
    Program.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
  let e = try Executor(p)
  let r = try e.run()
  try JSONSerialization.data(withJSONObject: r, options: [.sortedKeys]).write(
    to: URL(fileURLWithPath: CommandLine.arguments[2]))
} catch {
  fputs("matrix worker: \(error)\n", stderr)
  exit(1)
}
