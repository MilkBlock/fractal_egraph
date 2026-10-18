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
  var compileSeconds=0.0, bufferSeconds=0.0
  init(_ p: Program, binary:Data? = nil) throws {
    self.p = p
    try require(p.version == 5, "lowered matrix ABI version (recompile source program)")
    guard let d = MTLCreateSystemDefaultDevice(), let q = d.makeCommandQueue() else {
      throw NSError(domain: "Metal unavailable", code: 1)
    }
    device = d
    queue = q
    let bufferStart=ProcessInfo.processInfo.systemUptime
    let initial=binary ?? p.data.withUnsafeBytes{Data($0)}
    for m in p.layout.values {try require(m.offset>=0 && m.rows>=0 && m.cols>0 && (m.offset+max(1,m.rows*m.cols))*4<=initial.count,"matrix outside binary frame")}
    heap = initial.withUnsafeBytes {
      d.makeBuffer(bytes: $0.baseAddress!, length: $0.count, options: .storageModeShared)!
    }
    bufferSeconds=ProcessInfo.processInfo.systemUptime-bufferStart
    let options = MTLCompileOptions()
    if #available(macOS 15.0, *) {
      options.mathMode = .safe
    } else {
      options.fastMathEnabled = false
    }
    let compileStart=ProcessInfo.processInfo.systemUptime
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
    compileSeconds=ProcessInfo.processInfo.systemUptime-compileStart
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

// Resident fragment tables: static rule program once, binary row deltas after it.
final class TableExecutor {
  let device:MTLDevice,queue:MTLCommandQueue,code:MTLBuffer
  let parts:Int,slots:Int,numbers:Int,indexes:Int,rs:Int,ms:Int
  let queryOnly:Bool
  var rows:MTLBuffer!,links:MTLBuffer!,heads:MTLBuffer!,matches:MTLBuffer!,scratch:MTLBuffer!,groups:MTLBuffer!,meta:MTLBuffer!,patch:MTLBuffer!
  var kernels=[String:MTLComputePipelineState]()
  var rowCount=0,rowCap=0,buckets=1,matchCount=0,matchCap=1,groupBuckets=1,epoch=0,ackEpoch=0
  var compileSeconds=0.0,gpuSeconds=0.0,totalRetries=0
  var args=[UInt32](repeating:0,count:11)
  init(_ config:[String:Any]) throws {
    try require(config["version"] as? Int == 1,"table protocol")
    queryOnly=config["query_only"] as? Bool ?? false
    parts=config["parts"] as! Int;slots=config["slots"] as! Int;numbers=config["numbers"] as! Int;indexes=config["indexes"] as! Int
    try require(parts>0 && slots>0,"empty table schema")
    rs=3+slots+2*numbers;ms=4+parts
    guard let d=MTLCreateSystemDefaultDevice(),let q=d.makeCommandQueue() else{throw NSError(domain:"Metal unavailable",code:1)}
    device=d;queue=q
    let words=config["code"] as! [UInt32]
    code=words.withUnsafeBytes{d.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
    rows=try allocate(1);links=try allocate(1);heads=try allocate(1);matches=try allocate(ms);scratch=try allocate(4);groups=try allocate(3);meta=try allocate(8);patch=try allocate(1)
    let start=ProcessInfo.processInfo.systemUptime
    let options=MTLCompileOptions()
    if #available(macOS 15.0, *) {options.mathMode = .safe}else{options.fastMathEnabled=false}
    let library=try d.makeLibrary(source:config["shader"] as! String,options:options)
    for name in ["table_patch","table_clear_indexes","table_index","table_join","table_pending","table_clear_groups","table_group_links","table_reduce","table_acknowledge"] {
      kernels[name]=try d.makeComputePipelineState(function:library.makeFunction(name:name)!)
    }
    compileSeconds=ProcessInfo.processInfo.systemUptime-start
  }
  func allocate(_ words:Int)throws->MTLBuffer {
    try require(words>=0 && words<Int(UInt32.max),"table buffer address range exhausted")
    guard let b=device.makeBuffer(length:max(1,words)*4,options:.storageModeShared)else{throw NSError(domain:"table allocation failed",code:1)}
    return b
  }
  func pow2(_ n:Int)throws->Int {
    try require(n>0 && n<Int(UInt32.max)/2,"table dimension exhausted")
    var p=1;while p<n{p*=2};return p
  }
  func submit(_ cb:MTLCommandBuffer)throws {
    cb.commit();cb.waitUntilCompleted();if let e=cb.error{throw e}
    if cb.gpuEndTime>cb.gpuStartTime {gpuSeconds+=cb.gpuEndTime-cb.gpuStartTime}
  }
  func grow(_ old:MTLBuffer,_ words:Int,_ preserve:Int)throws->MTLBuffer {
    if old.length>=max(1,words)*4{return old}
    let next=try allocate(words)
    if preserve>0 {
      let cb=queue.makeCommandBuffer()!,blit=cb.makeBlitCommandEncoder()!
      blit.copy(from:old,sourceOffset:0,to:next,destinationOffset:0,size:preserve*4);blit.endEncoding();try submit(cb)
    }
    return next
  }
  func word(_ b:MTLBuffer,_ i:Int)->UInt32{b.contents().assumingMemoryBound(to:UInt32.self)[i]}
  func encode(_ name:String,_ count:Int,_ cb:MTLCommandBuffer){
    if count==0{return};let e=cb.makeComputeCommandEncoder()!,k=kernels[name]!
    e.setComputePipelineState(k)
    for (i,b) in [code,rows!,links!,heads!,matches!,scratch!,groups!,meta!].enumerated(){e.setBuffer(b,offset:0,index:i)}
    args.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:8)};e.setBuffer(patch,offset:0,index:9)
    e.dispatchThreads(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,k.maxTotalThreadsPerThreadgroup),height:1,depth:1));e.endEncoding()
  }
  func advance(_ data:Data,audit:Bool,seeded:Int=0)throws->([String:Any],[UInt32]){
    try require(data.count>=16 && data.count%4==0,"malformed table delta")
    func read(_ i:Int)->UInt32{data.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:i*4,as:UInt32.self).littleEndian}}
    let nextEpoch=Int(read(0)),added=Int(read(1)),removed=Int(read(2)),total=Int(read(3)),ps=1+slots+2*numbers
    try require(nextEpoch>epoch && total==rowCount+added && data.count/4==4+added*ps+removed,"table epoch/length mismatch")
    try require(seeded>=0 && seeded<=added && (seeded==0 || rowCount==0),"seed rows only at initial handoff")
    for i in 0..<added{try require(read(4+i*ps)<parts,"table part outside schema")}
    for i in 0..<removed{try require(read(4+added*ps+i)<rowCount,"retired row outside previous snapshot")}
    let beforeGpu=gpuSeconds,oldRows=rowCount,oldMatches=matchCount
    let capacity=try pow2(max(1,total)),nextBuckets=try pow2(max(1,total*2))
    let rehash=nextBuckets != buckets || rowCap==0
    rows=try grow(rows,capacity*rs,rowCount*rs);links=try grow(links,capacity*max(1,indexes),rowCount*max(1,indexes))
    rowCap=capacity
    if rehash{heads=try allocate(nextBuckets*max(1,indexes));buckets=nextBuckets}
    patch=data.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
    epoch=nextEpoch;rowCount=total
    args=[UInt32(total),UInt32(oldRows),UInt32(added),UInt32(epoch),UInt32(buckets),UInt32(matchCap),UInt32(oldMatches),UInt32(matchCount),1,rehash ? 1:0,UInt32(seeded)]
    memset(meta.contents(),0,32)
    let update=queue.makeCommandBuffer()!;encode("table_patch",max(added,removed),update)
    if rehash{encode("table_clear_indexes",buckets*indexes,update)}
    encode("table_index",total,update);try submit(update)
    // Storage grows losslessly. Overflow retries only the uncommitted new batch;
    // previously completed match records and their receipt flags are preserved.
    matchCap=max(matchCap,try pow2(max(1,oldMatches+added)))
    matches=try grow(matches,matchCap*ms,oldMatches*ms)
    var retries=0
    while true {
      args[5]=UInt32(matchCap);meta.contents().assumingMemoryBound(to:UInt32.self)[0]=UInt32(oldMatches)
      meta.contents().assumingMemoryBound(to:UInt32.self)[1]=0
      let cb=queue.makeCommandBuffer()!;encode("table_join",added,cb);try submit(cb)
      try require(word(meta,1)&2==0,"too many concrete matches for table address space")
      matchCount=Int(word(meta,0))
      if word(meta,1)==0{break}
      matchCap=try pow2(max(1,matchCount));matches=try grow(matches,matchCap*ms,oldMatches*ms);retries+=1
    }
    totalRetries+=retries;groupBuckets=queryOnly ? 1 : try pow2(max(1,matchCount*2))
    scratch=try grow(scratch,3*matchCap+groupBuckets,0);groups=try grow(groups,queryOnly ? 1 : 3*matchCap,0)
    args[5]=UInt32(matchCap);args[7]=UInt32(matchCount);args[8]=UInt32(groupBuckets)
    let reduce=queue.makeCommandBuffer()!
    encode("table_pending",matchCount,reduce);encode("table_clear_groups",groupBuckets,reduce)
    if !queryOnly {encode("table_group_links",matchCount,reduce);encode("table_reduce",groupBuckets,reduce)}
    try submit(reduce)
    let ng=Int(word(meta,3)),error=word(meta,2) != 0
    var body=[UInt32](),na=0
    if audit || queryOnly {
      for i in 0..<matchCount where word(scratch,2*matchCap+i) != 0 && (audit || word(matches,i*ms) != 0) {
        body.append(UInt32(i));for j in 0..<ms{body.append(word(matches,i*ms+j))};na+=1
      }
    }
    for g in 0..<ng {
      let m=Int(word(groups,g*3));body += [UInt32(m),word(groups,g*3+1),word(groups,g*3+2)]
      for p in 0..<parts {body.append(word(matches,m*ms+4+p))}
    }
    try require(body.count<=Int(UInt32.max)/4,"binary table response exceeds frame range")
    return (["epoch":epoch,"rows":rowCount,"new_rows":added,"retired_rows":removed,"match_records":matchCount,
      "new_matches":matchCount-oldMatches,"active_matches":Int(word(meta,7)),"pending_matches":Int(word(meta,6)),
      "audit_rows":na,"groups":ng,"seeded_rows":seeded,"query_only":queryOnly,"pending_only_supported":true,"arithmetic_error":error,"index_lookups":Int(word(meta,5)),"candidate_row_visits":Int(word(meta,4)),
      "retry_count":retries,"shader_compiles":1,"gpu_seconds":gpuSeconds-beforeGpu,
      "matrix_bytes":[code,rows!,links!,heads!,matches!,scratch!,groups!,meta!,patch!].reduce(0){$0+$1.length},"device":device.name],body)
  }
  func ack(_ e:Int)throws {
    try require(e==epoch && word(meta,2)==0,"cannot acknowledge failed/stale table batch")
    if ackEpoch==e{return}
    let cb=queue.makeCommandBuffer()!;encode("table_acknowledge",matchCount,cb);try submit(cb);ackEpoch=e
  }
}
// One device/queue/store for a whole family. Output is scoped to one frozen
// ruleset boundary; native row timestamps replace historical match receipts.
final class FamilyExecutor {
 let d:MTLDevice,q:MTLCommandQueue,code:MTLBuffer,c:Int,p:Int,s:Int,ni:Int,nq:Int,rs:Int,nc:Int,cs:Int,ck:Int,ec:Bool
 let cellTables:Set<UInt32>,portMeta:[UInt32]
 var rows:MTLBuffer!,events:MTLBuffer!,links:MTLBuffer!,heads:MTLBuffer!,output:MTLBuffer!,counts:MTLBuffer!,packet:MTLBuffer!,cells:MTLBuffer!,watches:MTLBuffer!,wheads:MTLBuffer!,dirty:MTLBuffer!,state:MTLBuffer!,ports:MTLBuffer!,previous:MTLBuffer!
 var kernels=[String:MTLComputePipelineState](),args=[UInt32](repeating:0,count:16)
 var rowSlots=0,rowCap=0,eventCount=0,eventCap=0,buckets=1,outCap=1,epoch=0
 var watchCount=0,watchCap=0,watchBuckets=1,portCount=0,portCap=0,watchGCs=0,portGCs=0
 var pcursors:[Int],pendingPortEnd=0
 var cursors:[Int],pending:[Int]=[],pendingEnd=0,compactions=0,gpu=0.0,compileSeconds=0.0
 init(_ x:[String:Any])throws{
  try require(x["version"] as? Int==2,"family protocol")
  c=x["columns"] as! Int;p=x["parts"] as! Int;s=x["slots"] as! Int;ni=x["indexes"] as! Int;nq=x["queries"] as! Int;rs=4+3*c;nc=x["cell_plans"] as? Int ?? 0;cs=x["cell_stride"] as? Int ?? 1;ck=x["cell_steps"] as! Int;ec=(x["event_cells"] as? Bool ?? false) && nc>0
  cellTables=Set(x["cell_tables"] as! [UInt32]);portMeta=x["port_meta"] as! [UInt32]
  guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{throw NSError(domain:"Metal unavailable",code:1)}
  d=device;q=queue;cursors=Array(repeating:0,count:nq);pcursors=Array(repeating:0,count:nq)
  let words=x["code"] as! [UInt32];code=words.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
  rows=try alloc(1);events=try alloc(2);links=try alloc(1);heads=try alloc(1);output=try alloc(1+s);counts=try alloc(20);packet=try alloc(1);cells=try alloc(1);memset(cells.contents(),0,cells.length)
  watches=try alloc(1);wheads=try alloc(1);dirty=try alloc(1);state=try alloc(16);memset(state.contents(),0,state.length);ports=try alloc(2);previous=try alloc(1)
  let t=ProcessInfo.processInfo.systemUptime,options=MTLCompileOptions()
  if #available(macOS 15.0, *) {options.mathMode = .safe}else{options.fastMathEnabled=false}
  let lib=try d.makeLibrary(source:x["shader"] as! String,options:options)
  let names=["family_patch","family_rebase","family_clear","family_index","family_join"] + (ec ? ["family_wake","family_watch_clear","family_watch_compact","family_cell_dispatch","family_cell_events","family_port_compact","family_port_dispatch","family_port_join"] : (nc>0 ? ["family_cells"] : []))
  for name in names{kernels[name]=try d.makeComputePipelineState(function:lib.makeFunction(name:name)!)}
  compileSeconds=ProcessInfo.processInfo.systemUptime-t
 }
 func alloc(_ n:Int)throws->MTLBuffer{try require(n>=0 && n<Int(UInt32.max),"family address space exhausted");guard let b=d.makeBuffer(length:max(1,n)*4,options:.storageModeShared)else{throw NSError(domain:"family allocation",code:1)};return b}
 func pow2(_ n:Int)throws->Int{try require(n>0 && n<Int(UInt32.max)/2,"family capacity exhausted");var k=1;while k<n{k*=2};return k}
 func submit(_ b:MTLCommandBuffer)throws{b.commit();b.waitUntilCompleted();if let e=b.error{throw e};if b.gpuEndTime>b.gpuStartTime{gpu+=b.gpuEndTime-b.gpuStartTime}}
 func grow(_ old:MTLBuffer,_ words:Int,_ preserve:Int)throws->MTLBuffer{
  if old.length>=max(1,words)*4{return old};let next=try alloc(words)
  if preserve>0{let cb=q.makeCommandBuffer()!,e=cb.makeBlitCommandEncoder()!;e.copy(from:old,sourceOffset:0,to:next,destinationOffset:0,size:preserve*4);e.endEncoding();try submit(cb)};return next
 }
 func encode(_ name:String,_ n:Int,_ cb:MTLCommandBuffer,_ indirect:Int?=nil){
  if n==0{return};let e=cb.makeComputeCommandEncoder()!,k=kernels[name]!
  e.setComputePipelineState(k)
  for (i,b) in [code,rows!,events!,links!,heads!,output!,counts!].enumerated(){e.setBuffer(b,offset:0,index:i)}
  args.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:7)};e.setBuffer(packet,offset:0,index:8);e.setBuffer(cells,offset:0,index:9)
  for(i,b) in [watches!,wheads!,dirty!,state!,ports!,previous!].enumerated(){e.setBuffer(b,offset:0,index:10+i)}
  if let off=indirect {e.dispatchThreadgroups(indirectBuffer:state,indirectBufferOffset:off,threadsPerThreadgroup:MTLSize(width:128,height:1,depth:1))}
  else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,k.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
 }
 func advance(_ data:Data,ack:Int)throws->([String:Any],[UInt32]){
  try require(ack==epoch,"missing native commit acknowledgment")
  for id in pending{cursors[id]=pendingEnd;pcursors[id]=pendingPortEnd};pending=[]
  try require(data.count>=16 && data.count%4==0,"family packet size")
  let w=data.withUnsafeBytes{Array($0.bindMemory(to:UInt32.self))};let total=Int(w[0]),changes=Int(w[1]),n=Int(w[2]),nextEpoch=Int(w[3])
  try require(nextEpoch==epoch+1 && total>=rowSlots && w.count==4+4*n+changes*(1+rs),"family epoch/shape")
  let ids=(0..<n).map{Int(w[4+4*$0])};try require(Set(ids).count==n && ids.allSatisfy{$0<nq},"family query IDs")
  for i in 0..<changes{try require(w[4+4*n+i*(1+rs)]<total,"family row slot")}
  let oldRowSlots=rowSlots,oldData=rows.contents().assumingMemoryBound(to:UInt32.self)
  let relevant=ec && (0..<changes).contains{ i in
   let at=4+4*n+i*(1+rs),r=Int(w[at]);return cellTables.contains(w[at+1]) || (r<oldRowSlots && cellTables.contains(oldData[r*rs]))
  }
  let before=gpu,newRowCap=try pow2(max(1,total)),newEventCap=try pow2(max(2,2*newRowCap)),newBuckets=try pow2(max(1,2*total))
  // At capacity, rebuild from live row slots, not historical versions. Doubling
  // leaves amortized room for another live-set-sized wave of changes.
  let compact=eventCap==0 || eventCount+changes>eventCap || newEventCap>eventCap || newBuckets != buckets
  rows=try grow(rows,newRowCap*rs,rowSlots*rs)
  if nc>0 && cells.length<newRowCap*nc*cs*4 {
   let old=cells!,next=try alloc(newRowCap*nc*cs);memset(next.contents(),0,next.length)
   if rowSlots>0 {memcpy(next.contents(),old.contents(),rowSlots*nc*cs*4)};cells=next
  }
  let oldEvents=eventCount
  if compact {
   if eventCap>0{compactions+=1}
   eventCap=max(eventCap,newEventCap);buckets=newBuckets
   events=try alloc(2*eventCap);links=try alloc(eventCap*ni);heads=try alloc(buckets*ni)
   eventCount=total;cursors=Array(repeating:0,count:nq)
  }else{eventCount+=changes}
  let oldWatch=watches!,oldPorts=ports!,oldWatchCount=watchCount,oldPortCount=portCount,cellCap=newRowCap*nc
  var wgc=false,pgc=false
  if relevant {
   let nextWatchCap=try pow2(max(2,4*cellCap*ck)),nextWatchBuckets=try pow2(max(1,2*cellCap)),nextPortCap=try pow2(max(2,4*cellCap))
   wgc=watchCap==0 || watchCount+total*nc*ck>watchCap || nextWatchCap>watchCap || nextWatchBuckets != watchBuckets
   pgc=portCap==0 || portCount+total*nc>portCap || nextPortCap>portCap
   dirty=try grow(dirty,max(1,cellCap),0)
   if wgc {if watchCap>0{watchGCs+=1};watchCap=nextWatchCap;watchBuckets=nextWatchBuckets;watches=try alloc(watchCap*(6+c));wheads=try alloc(watchBuckets);watchCount=0}
   if pgc {if portCap>0{portGCs+=1};portCap=nextPortCap;ports=try alloc(2*portCap);portCount=0;pcursors=Array(repeating:0,count:nq)}
  }
  let ctrl=state.contents().assumingMemoryBound(to:UInt32.self);ctrl[0]=UInt32(watchCount);ctrl[1]=0;ctrl[2]=UInt32(portCount)
  rowSlots=total;rowCap=newRowCap;epoch=nextEpoch
  packet=data.withUnsafeBytes{d.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
  args=[UInt32(total),UInt32(oldEvents),UInt32(changes),UInt32(n),UInt32(buckets),UInt32(outCap),UInt32(eventCount),compact ? 1:0,0,UInt32(epoch),UInt32(watchBuckets),UInt32(oldRowSlots),UInt32(oldWatchCount),0,UInt32(oldPortCount),0]
  let words=packet.contents().assumingMemoryBound(to:UInt32.self)
  var span=0;for (i,id) in ids.enumerated(){words[6+4*i]=UInt32(cursors[id]);words[7+4*i]=UInt32(pcursors[id]);span=max(span,eventCount-cursors[id])}
  args[8]=UInt32(span);args[6]=UInt32(eventCount);args[4]=UInt32(buckets)
  let portWork=ec && ids.enumerated().contains{ i,id in portMeta[id*3] != UInt32.max && (words[5+4*i]>0 || portMeta[id*3+2]==0) && (relevant || pcursors[id]<portCount) }
  var retries=0,count=0
  memset(counts.contents(),0,counts.length)
  while true{
   memset(counts.contents(),0,5*4);memset(counts.contents().advanced(by:13*4),0,3*4);args[5]=UInt32(outCap)
   let cb=q.makeCommandBuffer()!
   if retries==0 {
    if relevant {
     if wgc {previous=oldWatch;encode("family_watch_clear",watchBuckets,cb);encode("family_watch_compact",oldWatchCount,cb)}
     if pgc {previous=oldPorts;encode("family_port_compact",oldPortCount,cb)}
     encode("family_wake",changes,cb)
    }
    encode("family_patch",changes,cb)
    if compact{encode("family_rebase",total,cb);encode("family_clear",buckets*ni,cb)}
    encode("family_index",compact ? eventCount : changes,cb)
    if relevant {encode("family_cell_dispatch",1,cb);encode("family_cell_events",1,cb,16)}
    else if !ec && nc>0 && (changes>0 || compact){encode("family_cells",total*nc,cb)}
   }
   encode("family_join",span*p*n,cb);if portWork {encode("family_port_dispatch",1,cb);encode("family_port_join",1,cb,32)};try submit(cb)
   let flags=counts.contents().assumingMemoryBound(to:UInt32.self)[1]
   try require(flags&(~UInt32(1))==0,"family count or revision overflow");count=Int(counts.contents().assumingMemoryBound(to:UInt32.self)[0])
   if flags==0{break};outCap=try pow2(max(1,count));output=try alloc(outCap*(1+s));retries+=1
  }
  let result=Array(UnsafeBufferPointer(start:output.contents().assumingMemoryBound(to:UInt32.self),count:count*(1+s)))
  try require(result.count<=Int(UInt32.max)/4,"family response too large")
  watchCount=Int(ctrl[0]);portCount=Int(ctrl[2]);pending=ids;pendingEnd=eventCount;pendingPortEnd=portCount
  previous=code // completed GC must not retain an otherwise dead old log
  return (["epoch":epoch,"queries":n,"bindings":count,"row_slots":rowSlots,"row_capacity":rowCap,"events":eventCount,"event_capacity":eventCap,
   "compactions":compactions,"compacted":compact,"old_events":oldEvents,"retries":retries,"gpu_seconds":gpu-before,
   "cell_cache_bytes":cells.length,"cell_plans":nc,"event_cells":ec,"cell_tables_affected":relevant,"port_work":portWork,"watch_count":watchCount,"watch_capacity":watchCap,"port_count":portCount,"port_capacity":portCap,"watch_compactions":watchGCs,"port_compactions":portGCs,"dirty_cells":ctrl[1],
   "cell_work":["join_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[2],"skipped_join_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[3],"certified_anchor_hits":counts.contents().assumingMemoryBound(to:UInt32.self)[4],"certification_attempts":counts.contents().assumingMemoryBound(to:UInt32.self)[5],"certificate_reuses":counts.contents().assumingMemoryBound(to:UInt32.self)[6],"shape_fallbacks":counts.contents().assumingMemoryBound(to:UInt32.self)[7],"invalidated_certificates":counts.contents().assumingMemoryBound(to:UInt32.self)[8],"certification_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[9],"watch_wakes":counts.contents().assumingMemoryBound(to:UInt32.self)[10],"dirty_cells":counts.contents().assumingMemoryBound(to:UInt32.self)[11],"prefix_rows_reused":counts.contents().assumingMemoryBound(to:UInt32.self)[12],"port_bindings":counts.contents().assumingMemoryBound(to:UInt32.self)[13],"delegated_branches":counts.contents().assumingMemoryBound(to:UInt32.self)[14],"port_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[15],"ports_published":counts.contents().assumingMemoryBound(to:UInt32.self)[16],"missing_key_waits":counts.contents().assumingMemoryBound(to:UInt32.self)[17],"watch_entries_checked":counts.contents().assumingMemoryBound(to:UInt32.self)[18],"direct_row_reuses":counts.contents().assumingMemoryBound(to:UInt32.self)[19]],
   "device_bytes":[code,rows!,events!,links!,heads!,output!,counts!,packet!,cells!,watches!,wheads!,dirty!,state!,ports!].reduce(0){$0+$1.length},"device":d.name,"retained_matches":0],result)
 }
}

func sendBinary(_ words:[UInt32]) {
  var n=UInt32(words.count*4).littleEndian
  FileHandle.standardOutput.write(Data(bytes:&n,count:4));words.withUnsafeBytes{FileHandle.standardOutput.write(Data($0))}
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
    var tables:TableExecutor? = nil
    var family:FamilyExecutor? = nil
    try sendFrame(["ready":true,"protocol":10])
    while true {
      let raw=try readFrame()
      let parseStart=ProcessInfo.processInfo.systemUptime
      let message=try JSONSerialization.jsonObject(with:raw) as! [String:Any]
      let parseSeconds=ProcessInfo.processInfo.systemUptime-parseStart
      do {
        switch message["op"] as? String {
        case "install","install_binary":
          let decodeStart=ProcessInfo.processInfo.systemUptime
          let data=try JSONSerialization.data(withJSONObject:message["program"]!)
          let p=try JSONDecoder().decode(Program.self,from:data)
          let binary = message["op"] as? String == "install_binary" ? try readFrame() : nil
          if let binary=binary {try require(binary.count==(message["word_count"] as! Int)*4,"binary word count mismatch")}
          let decodeSeconds=parseSeconds+ProcessInfo.processInfo.systemUptime-decodeStart
          let next=try Executor(p,binary:binary)
          let inheritStart=ProcessInfo.processInfo.systemUptime
          try next.inherit(current)
          let inheritSeconds=ProcessInfo.processInfo.systemUptime-inheritStart
          current=next
          try sendFrame(["installed":true,"cells":next.m("live").rows,"channels":next.m("decisions").rows,
            "install_profile":["decode_seconds":decodeSeconds,"buffer_seconds":next.bufferSeconds,
              "shader_compile_seconds":next.compileSeconds,"inherit_seconds":inheritSeconds]])
        case "advance","resume":
          guard let e=current else {throw NSError(domain:"no matrix program",code:1)}
          try sendFrame(e.step(resume:message["op"] as? String == "resume",disable:message["disable"] as? [Int] ?? []))
        case "family_init":
          try require(family==nil && tables==nil && current==nil,"family already installed")
          family=try FamilyExecutor(message["config"] as! [String:Any])
          try sendFrame(["installed":true,"compile_seconds":family!.compileSeconds,"workers":1])
        case "family_step":
          guard let f=family else{throw NSError(domain:"no family",code:1)}
          let (report,body)=try f.advance(readFrame(),ack:message["ack"] as! Int)
          try sendFrame(report);sendBinary(body)
        case "table_init":
          try require(tables == nil && current == nil,"table plan already installed")
          tables=try TableExecutor(message["config"] as! [String:Any])
          try sendFrame(["installed":true,"shader_compiles":1,"compile_seconds":tables!.compileSeconds])
        case "table_delta":
          guard let t=tables else{throw NSError(domain:"missing table plan",code:1)}
          let (report,body)=try t.advance(readFrame(),audit:message["audit"] as? Bool ?? false,seeded:message["seeded"] as? Int ?? 0)
          try sendFrame(report);sendBinary(body)
        case "table_ack":
          guard let t=tables else{throw NSError(domain:"missing table plan",code:1)}
          try t.ack(message["epoch"] as! Int);try sendFrame(["acknowledged":true])
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
