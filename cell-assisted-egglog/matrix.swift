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
    epochs: [[Int]], groups: Int, numeric: Bool
}
func require(_ b: Bool, _ s: String) throws { if !b { throw NSError(domain: s, code: 1) } }
final class Executor {
  let p: Program, device: MTLDevice, queue: MTLCommandQueue, heap: MTLBuffer
  var kernels = [String: MTLComputePipelineState](), gpuSeconds = 0.0, hostCountReads = 0
  var calls = 0, positions = 0, macs = 0
  init(_ p: Program) throws {
    self.p = p
    try require(p.version == 5, "lowered matrix ABI version (recompile source program)")
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
      "collect", "compare_pairs", "targets", "histogram", "revoke", "admit_new",
      "numeric_begin", "numeric_guard", "numeric_propose", "numeric_reduce", "numeric_clear_selected",
    ] {
      do { kernels[n] = try d.makeComputePipelineState(function: library.makeFunction(name: n)!) }
      catch { throw NSError(domain:"pipeline \(n): \(error)",code:1) }
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
  func matchStage(_ stage: Stage, reset: Bool) throws -> [[String:Any]] {
    let start = queue.makeCommandBuffer()!
    let cfg = [UInt32(stage.start), UInt32(stage.count), UInt32.max, 0]
    if reset {
      encode("reset_channels", stage.count, cfg, start)
      if p.numeric { encode("numeric_begin",stage.count,cfg,start) }
    }
    if p.numeric { zero("numeric_counters"); zero("numeric_error") }
    try submit(start)
    zero("counters")
    var levels = [try summary(stage, nil)]
    for depth in stage.levels {
      zero("counters")
      if p.numeric {
        let cb=queue.makeCommandBuffer()!
        encode("numeric_guard",stage.count,[UInt32(stage.start),UInt32(stage.count),UInt32(depth),0],cb)
        try submit(cb)
      }
      for group in 0..<p.groups {
        let cfg = [UInt32(stage.start), UInt32(stage.count), UInt32(depth), UInt32(group)]
        let prepare = queue.makeCommandBuffer()!
        encode("clear_requests", m("requested").rows, cfg, prepare)
        encode("classify", stage.count, cfg, prepare)
        encode("prepare_group", 1, cfg, prepare)
        let groups = words("groups")
        let gs = m("groups")
        func meta(_ field:String)->Int {Int(groups[group*gs.cols+gs.fields.firstIndex(of:field)!])}
        let method=meta("method")
        if method == 1 {
          encode("build_features",m("live").rows,cfg,prepare)
          encode("compare_pairs",meta("condition_count"),cfg,prepare)
          try submit(prepare)
          continue
        }
        try submit(prepare)
        hostCountReads += 1
        let counts=words("g\(group)_count")
        let n = Int(counts[0])
        if n == 0 { continue }
        let rows = m("g\(group)_rows")
        let gram = m("g\(group)_gram")
        let width = meta("width")
        let left=method==2 ? Int(counts[1]) : n
        let right=method==2 ? n-left : n
        let mul = queue.makeCommandBuffer()!
        encode("build_features", m("live").rows, cfg, mul)
        encode("project", n, cfg, mul)
        let xd = MPSMatrixDescriptor(
          rows: left, columns: width, rowBytes: rows.cols * 4, dataType: .float32)
        let yd = MPSMatrixDescriptor(
          rows: right, columns: width, rowBytes: rows.cols * 4, dataType: .float32)
        let gd = MPSMatrixDescriptor(
          rows: left, columns: right, rowBytes: gram.cols * 4, dataType: .float32)
        let x = MPSMatrix(buffer: heap, offset: rows.offset * 4, descriptor: xd)
        let y = MPSMatrix(buffer: heap, offset: (rows.offset + (method==2 ? left*rows.cols : 0))*4,descriptor:yd)
        let g = MPSMatrix(buffer: heap, offset: gram.offset * 4, descriptor: gd)
        let gemm = MPSMatrixMultiplication(
          device: device, transposeLeft: false, transposeRight: true, resultRows: left,
          resultColumns: right, interiorColumns: width, alpha: 1, beta: 0)
        gemm.encode(commandBuffer: mul, leftMatrix: x, rightMatrix: y, resultMatrix: g)
        encode("collect", m("g\(group)_conditions").rows, cfg, mul)
        try submit(mul)
        calls += 1
        positions += left * right
        macs += left * right * width
      }
      levels.append(try summary(stage, depth))
    }
    return levels
  }
  func numericCommit(_ stage:Stage,_ cb:MTLCommandBuffer) {
    if !p.numeric {return}
    let cfg=[UInt32(stage.start),UInt32(stage.count),0,0]
    encode("numeric_clear_selected",m("numeric_selected").rows,cfg,cb)
    encode("numeric_propose",stage.count,cfg,cb)
    encode("numeric_reduce",m("numeric_groups").rows,cfg,cb)
  }
  let numericNames=["numeric_state","numeric_evidence","numeric_proposals","numeric_error","numeric_counters"]
  func layoutWork()->[String:Int] {
    let c=words("layout_counters")
    return ["paired_comparisons":Int(c[0]),"paired_components":Int(c[1]),"packed_components":Int(c[2])]
  }
  func numericReport(_ read:(String)->[UInt32])->[String:Any] {
    let state=read("numeric_state"),props=read("numeric_proposals")
    func column(_ name:String,_ field:String)->Int {m(name).fields.firstIndex(of:field)!}
    func word(_ data:[UInt32],_ name:String,_ row:Int,_ field:String)->UInt32 {
      data[row*m(name).cols+column(name,field)]
    }
    func value(_ data:[UInt32],_ name:String,_ row:Int)->Int64 {
      Int64(bitPattern:UInt64(word(data,name,row,"lo")) | UInt64(word(data,name,row,"hi"))<<32)
    }
    var work=[String:Int]()
    for (i,key) in m("numeric_counters").fields.enumerated(){work[key]=Int(read("numeric_counters")[i])}
    return ["values":(0..<m("numeric_state").rows).map {word(state,"numeric_state",$0,"present")==0 ? NSNull() as Any : value(state,"numeric_state",$0) as Any},
      "versions":(0..<m("numeric_state").rows).map {word(state,"numeric_state",$0,"version")},
      "guards":read("numeric_evidence"),
      "proposals":(0..<m("numeric_proposals").rows).map {word(props,"numeric_proposals",$0,"status")==1 ? value(props,"numeric_proposals",$0) as Any : NSNull() as Any},
      "status":(0..<m("numeric_proposals").rows).map {word(props,"numeric_proposals",$0,"status")},
      "arithmetic_error":read("numeric_error")[0] != 0,"work":work]
  }
  func run() throws -> [String: Any] {
    var saved = [[[String: Any]]]()
    var snapshots = [[[String: MTLBuffer]]]()
    epochs: for epoch in p.epochs {
      let cb = queue.makeCommandBuffer()!
      let cfg = [UInt32(epoch[0]), UInt32(epoch[1]), 0, 0]
      encode("update_ports", epoch[1], cfg, cb)
      invalidation(cb)
      encode("admit", m("live").rows, cfg, cb)
      try submit(cb)
      var stageResults = [[String: Any]]()
      var stageSnapshots = [[String: MTLBuffer]]()
      for stage in p.stages {
        let cfg = [UInt32(stage.start), UInt32(stage.count), UInt32.max, 0]
        let levels = try matchStage(stage, reset: true)
        let commit = queue.makeCommandBuffer()!
        numericCommit(stage,commit)
        encode("targets", stage.count, cfg, commit)
        invalidation(commit)
        // Audit snapshots stay on the device until the full program has completed.
        var snap = [String: MTLBuffer]()
        let blit = commit.makeBlitCommandEncoder()!
        for name in ["bindings", "live", "decisions", "target_status", "specialization_targets"] + (p.numeric ? numericNames : []) {
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
        if p.numeric && words("numeric_error")[0] != 0 {
          saved.append(stageResults); snapshots.append(stageSnapshots);break epochs
        }
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
      for si in 0..<saved[epoch].count {
        let stage=p.stages[si]
        let s = snapshots[epoch][si]
        let v = read(s, "bindings")
        let d = read(s, "decisions")
        let t = read(s, "target_status")
        let sp = read(s, "specialization_targets")
        let selected = Array(ids[stage.start..<stage.start + stage.count]).map { Int($0) }
        if p.numeric {saved[epoch][si]["numeric"]=numericReport {read(s,$0)}}
        saved[epoch][si]["decisions"] = selected.map { d[$0] }
        saved[epoch][si]["target_status"] = selected.map { t[$0] }
        saved[epoch][si]["specialization_targets"] = selected.map {
          sp[$0] == UInt32.max ? NSNull() as Any : Int(sp[$0]) as Any
        }
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
      "layout_work":layoutWork(),
      "scope":
        "generated matrix program, GPU predicates/frontier/targets; host reads dimensions for compact MPS dispatch; no native runtime speedup claim",
    ]
  }
  // Only stable semantic rows are copied. Compaction/Gram scratch is regenerated.
  // The Rust driver verifies immutable prefixes before requesting installation.
  func inherit(_ old: Executor?) throws {
    let cb = queue.makeCommandBuffer()!
    if let old = old {
      let blit = cb.makeBlitCommandEncoder()!
      for name in ["bindings","live","features","valid","evidence","decisions","target_status","specialization_targets","execution_counts","numeric_values","numeric_defined","numeric_valid","numeric_state","numeric_evidence","numeric_proposals"] {
        let a=old.m(name), b=m(name)
        try require(b.rows>=a.rows && b.cols>=a.cols,"shrinking live matrix")
        for row in 0..<a.rows where a.cols>0 {
          blit.copy(from:old.heap,sourceOffset:(a.offset+row*a.cols)*4,
                    to:heap,destinationOffset:(b.offset+row*b.cols)*4,size:a.cols*4)
        }
      }
      blit.endEncoding()
    }
    let start=old?.m("live").rows ?? 0
    encode("admit_new", m("live").rows-start, [UInt32(start), UInt32(m("live").rows-start),0,0], cb)
    try submit(cb)
  }

  func step(resume:Bool, disable:[Int]) throws -> [String:Any] {
    zero("layout_counters")
    for i in disable {
      try require(i>=0 && i<m("live").rows,"invalid retired Cell")
      heap.contents().assumingMemoryBound(to:UInt32.self)[m("live").offset+i]=0
    }
    let beforeGpu=gpuSeconds, beforeReads=hostCountReads
    var allLevels=[[[String:Any]]]()
    for stage in p.stages {
      let cfg=[UInt32(stage.start),UInt32(stage.count),UInt32.max,0]
      let revoke=queue.makeCommandBuffer()!
      encode("revoke",stage.count,cfg,revoke)
      try submit(revoke)
      if resume && p.numeric {zero("numeric_counters");zero("numeric_error")}
      allLevels.append(resume ? [] : try matchStage(stage,reset:false))
      let commit=queue.makeCommandBuffer()!
      numericCommit(stage,commit)
      encode("targets",stage.count,cfg,commit)
      invalidation(commit)
      try submit(commit)
      if p.numeric && words("numeric_error")[0] != 0 {break}
    }
    // These are terminal/pending outputs needed for native commits or Bake.
    // No feature matrix is returned to the host.
    let ds=words("decisions"),ts=words("target_status"),sp=words("specialization_targets")
    let ops=words("channel_ops"),vals=words("bindings"),live=words("live"),counts=words("execution_counts")
    let fields=m("channel_ops").fields
    func op(_ ci:Int,_ field:String)->Int { Int(ops[ci*fields.count+fields.firstIndex(of:field)!]) }
    var outputs=[[String:Any]]()
    for ci in 0..<m("decisions").rows {
      if ds[ci] != 1 { continue }
      let target=op(ci,"target"),arity=op(ci,"gather_count")
      if target==Int(UInt32.max) {continue}
      outputs.append(["channel":ci,"status":Int(ts[ci]),"binding":Array(vals[target*m("bindings").cols..<target*m("bindings").cols+arity]),
        "specialized":sp[ci]==UInt32.max ? NSNull() as Any : Int(sp[ci]) as Any])
    }
    var result:[String:Any] = ["outputs":outputs,"decisions":ds,"live":live,"levels":allLevels,
      "execution_counts":stride(from:0,to:counts.count,by:2).map{Array(counts[$0..<$0+2])},
      "resume_only":resume,"gpu_seconds":gpuSeconds-beforeGpu,"host_count_reads":hostCountReads-beforeReads,
      "matrix_bytes":heap.length,"device":device.name,"layout_work":layoutWork()]
    if p.numeric {result["numeric"]=numericReport {words($0)}}
    return result
  }

}

func readFrame() throws -> Data {
  func exact(_ n:Int) throws -> Data {
    var d=Data()
    while d.count<n { let b=FileHandle.standardInput.readData(ofLength:n-d.count); if b.isEmpty {throw NSError(domain:"pipe closed",code:1)}; d.append(b) }
    return d
  }
  let head=try exact(4)
  let n=head.enumerated().reduce(0){ $0 | (Int($1.element)<<($1.offset*8)) }
  return try exact(n)
}
func sendFrame(_ object:[String:Any]) throws {
  let data=try JSONSerialization.data(withJSONObject:object,options:[.sortedKeys])
  var n=UInt32(data.count).littleEndian
  FileHandle.standardOutput.write(Data(bytes:&n,count:4))
  FileHandle.standardOutput.write(data)
}
do {
  if CommandLine.arguments.count==2 && CommandLine.arguments[1]=="--serve" {
    var current:Executor? = nil
    try sendFrame(["ready":true,"protocol":5])
    while true {
      let message=try JSONSerialization.jsonObject(with:readFrame()) as! [String:Any]
      do {
        switch message["op"] as? String {
        case "install":
          let data=try JSONSerialization.data(withJSONObject:message["program"]!)
          let p=try JSONDecoder().decode(Program.self,from:data)
          let next=try Executor(p)
          try next.inherit(current)
          current=next
          try sendFrame(["installed":true,"cells":next.m("live").rows,"channels":next.m("decisions").rows])
        case "advance","resume":
          guard let e=current else {throw NSError(domain:"no matrix program",code:1)}
          try sendFrame(e.step(resume:message["op"] as? String == "resume",disable:message["disable"] as? [Int] ?? []))
        case "close": try sendFrame(["closed":true]); exit(0)
        default: throw NSError(domain:"unknown live matrix operation",code:1)
        }
      } catch { try sendFrame(["error":String(describing:error)]) }
    }
  } else {
    try require(CommandLine.arguments.count == 3, "usage: matrix-worker lowered.json report.json | --serve")
    let p = try JSONDecoder().decode(Program.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[1])))
    let e = try Executor(p)
    let r = try e.run()
    try JSONSerialization.data(withJSONObject: r, options: [.sortedKeys]).write(to: URL(fileURLWithPath: CommandLine.arguments[2]))
  }
} catch { fputs("matrix worker: \(error)\n", stderr); exit(1) }
