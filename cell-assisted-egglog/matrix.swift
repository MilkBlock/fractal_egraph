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
 let d:MTLDevice,q:MTLCommandQueue,code:MTLBuffer,c:Int,p:Int,s:Int,ni:Int,nq:Int,rs:Int,nc:Int,cs:Int,ck:Int,ec:Bool,locality:Int,routingMode:Int
 let cellTables:Set<UInt32>,portMeta:[UInt32],portJoinGroup:[UInt32],phaseTiming:Bool,seedProfile:Bool
 var rows:MTLBuffer!,events:MTLBuffer!,links:MTLBuffer!,heads:MTLBuffer!,output:MTLBuffer!,counts:MTLBuffer!,packet:MTLBuffer!,cells:MTLBuffer!,watches:MTLBuffer!,wheads:MTLBuffer!,dirty:MTLBuffer!,state:MTLBuffer!,ports:MTLBuffer!,previous:MTLBuffer!,seedQueue:MTLBuffer!,orderedSeeds:MTLBuffer!,routing:MTLBuffer!,rowCells:MTLBuffer!,instanceMeta:MTLBuffer!,freeCells:MTLBuffer!,registry:MTLBuffer!
 let depthMetrics:[String],cellMetrics:[String],searchMetrics:[String],queryAtoms:[Int]
 let queryTables:[[Int]],emptyTablePruning:Bool,commitMarkers:Bool
 let domainTables:[UInt32]
 var domains:MTLBuffer!,domainLayout:[UInt32]=[],domainWords=0
 let indexTables:[UInt32],tableDegree:[Int],queryShapes:[Int],shapeCount:Int
 var tableLive:[Int],tableBuckets:[Int],slotTable:[Int]=[],slotLive:[Bool]=[],linkCount=0,linkCapacity=1,headLayout:[UInt32]=[]
 var kernels=[String:MTLComputePipelineState](),args=[UInt32](repeating:0,count:31)
 var rowSlots=0,rowCap=0,eventCount=0,eventCap=0,buckets=1,outCap=1,epoch=0,seedCap=1
 var watchCount=0,watchCap=0,watchBuckets=1,portCount=0,portCap=0,watchGCs=0,portGCs=0,cellCap=1,cellSlotsReused=0
 var pcursors:[Int],pendingPortEnd=0
 var cursors:[Int],pending:[Int]=[],pendingEnd=0,compactions=0,gpu=0.0,compileSeconds=0.0
 init(_ x:[String:Any])throws{
  try require(x["version"] as? Int==2 && x["family_layout"] as? Int==18,"family layout: rebuild both Rust and Swift")
  c=x["columns"] as! Int;p=x["parts"] as! Int;s=x["slots"] as! Int;ni=x["indexes"] as! Int;nq=x["queries"] as! Int;rs=4+3*c;nc=x["cell_plans"] as? Int ?? 0;cs=x["cell_stride"] as? Int ?? 1;ck=x["cell_steps"] as! Int;ec=(x["event_cells"] as? Bool ?? false) && nc>0
  queryShapes=x["query_shapes"] as! [Int];shapeCount=(x["shape_sizes"] as! [[Int]]).count
  cellMetrics=x["cell_metrics"] as! [String];searchMetrics=x["search_metrics"] as! [String]
  depthMetrics=x["depth_metrics"] as! [String];queryAtoms=x["query_atoms"] as! [Int]
  commitMarkers=x["commit_markers"] as? Bool ?? false
  queryTables=x["query_tables"] as! [[Int]];emptyTablePruning=x["empty_table_pruning"] as! Bool
  indexTables=x["index_tables"] as! [UInt32]
  domainTables=x["domain_tables"] as! [UInt32]
  let nt=x["table_count"] as! Int
  try require(queryTables.count==nq && queryTables.allSatisfy{$0.allSatisfy{$0>=0 && $0<nt}},"query required tables")
  var degrees=Array(repeating:0,count:nt)
  for t in indexTables where t != UInt32.max {degrees[Int(t)]+=1}
  tableDegree=degrees;tableLive=Array(repeating:0,count:nt);tableBuckets=Array(repeating:1,count:nt)
  routingMode=x["query_routing"] as? Int ?? 0;try require((0...3).contains(routingMode),"query routing mode")
  phaseTiming=x["phase_timing"] as? Bool ?? false;seedProfile=x["profile"] as? Bool ?? false
  locality=x["port_locality"] as? Int ?? 0;try require((0...2).contains(locality),"Cell locality mode")
  cellTables=Set(x["cell_tables"] as! [UInt32]);portMeta=x["port_meta"] as! [UInt32];portJoinGroup=x["port_join_group"] as! [UInt32]
  guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{throw NSError(domain:"Metal unavailable",code:1)}
  d=device;q=queue;cursors=Array(repeating:0,count:nq);pcursors=Array(repeating:0,count:nq)
  let words=x["code"] as! [UInt32];code=words.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
  rows=try alloc(1);events=try alloc(2);links=try alloc(1);heads=try alloc(1);output=try alloc(1+s);counts=try alloc(64+(seedProfile ? 3*nq+nq*p*depthMetrics.count+nq*searchMetrics.count+nc*cellMetrics.count : 0));packet=try alloc(1);cells=try alloc(cs);memset(cells.contents(),0,cells.length)
  domains=try alloc(1)
  watches=try alloc(1);wheads=try alloc(1);memset(wheads.contents(),255,wheads.length);dirty=try alloc(1);state=try alloc(16);memset(state.contents(),0,state.length);ports=try alloc(2);previous=try alloc(1)
  rowCells=try alloc(1);memset(rowCells.contents(),255,rowCells.length);instanceMeta=try alloc(3);freeCells=try alloc(1);registry=try alloc(10);memset(registry.contents(),0,registry.length)
  seedQueue=try alloc(8+p);orderedSeeds=try alloc(8+p);routing=try alloc(28+shapeCount*6);memset(routing.contents(),0,routing.length)
  let t=ProcessInfo.processInfo.systemUptime,options=MTLCompileOptions()
  if #available(macOS 15.0, *) {options.mathMode = .safe}else{options.fastMathEnabled=false}
  let lib=try d.makeLibrary(source:x["shader"] as! String,options:options)
  var names=["family_patch","family_rebase","family_clear","family_index","family_join","family_domain_clear"]
  if nc>0 {names += ["family_cell_retire_count","family_cell_allocate","family_registry_finish","family_install_markers","family_marker_count","family_marker_allocate"]}
  if routingMode>0 {names += ["family_classify","family_classify_tables","family_unit","family_route_dispatch","family_route_scatter","family_routed_join"]}
  if routingMode==3 {names += ["family_shape_dispatch","family_shape_scatter"];names += (0..<shapeCount).map{"family_shape_\($0)"}}
  if ec {names += ["family_wake","family_watch_clear","family_watch_compact","family_cell_dispatch","family_cell_events","family_cell_publish","family_watch_count","family_port_count","family_port_compact","family_port_dispatch","family_port_join","family_port_locality"]}
  else if nc>0 {names += ["family_cells"]}
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
  // ABI slots 0...6 are fixed. Optional summaries bind at 23, never in this list.
  for (i,b) in [code,rows!,events!,links!,heads!,output!,counts!].enumerated(){e.setBuffer(b,offset:0,index:i)}
  args.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:7)};e.setBuffer(packet,offset:0,index:8);e.setBuffer(cells,offset:0,index:9)
  for(i,b) in [watches!,wheads!,dirty!,state!,ports!,previous!].enumerated(){e.setBuffer(b,offset:0,index:10+i)}
  for(i,b) in [seedQueue!,orderedSeeds!,routing!].enumerated(){e.setBuffer(b,offset:0,index:16+i)}
  for(i,b) in [rowCells!,instanceMeta!,freeCells!,registry!].enumerated(){e.setBuffer(b,offset:0,index:19+i)}
  e.setBuffer(domains,offset:0,index:23)
  if let off=indirect {e.dispatchThreadgroups(indirectBuffer:(name=="family_routed_join" || name.hasPrefix("family_shape_")) ? routing : state,indirectBufferOffset:off,threadsPerThreadgroup:MTLSize(width:name=="family_port_locality" ? k.threadExecutionWidth : 128,height:1,depth:1))}
  else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,k.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
 }
 // The host only sizes buffers from GPU counts. It does not inspect bindings
 // or maintain the Cell registry. Returned work publishes dependencies before
 // matching; the caller commits every query result at the original boundary.
 func prepareCells(_ relevant:Bool,_ cellRelevant:Bool,_ changes:Int,_ compact:Bool)throws->MTLCommandBuffer {
  let ctrl=state.contents().assumingMemoryBound(to:UInt32.self),reg=registry.contents().assumingMemoryBound(to:UInt32.self)
  let first=q.makeCommandBuffer()!
  if relevant{encode("family_wake",changes,first)}
  encode("family_patch",changes,first)
  if cellRelevant{encode("family_cell_retire_count",changes,first);if commitMarkers{encode("family_marker_count",Int(args[30]),first)}}
  if compact{encode("family_rebase",rowSlots,first);encode("family_clear",buckets,first);encode("family_domain_clear",domainWords,first)}
  encode("family_index",compact ? eventCount : changes,first)
  if !cellRelevant{return first}
  try submit(first);try require(counts.contents().assumingMemoryBound(to:UInt32.self)[1]==0,"Cell retirement failed")
  let oldSize=Int(reg[0]),available=Int(reg[1]),needed=Int(reg[2]),newSize=oldSize+max(0,needed-available)
  let capacity=try pow2(max(1,newSize))
  if capacity>cellCap {
   cells=try grow(cells,capacity*cs,oldSize*cs)
   memset(cells.contents().advanced(by:oldSize*cs*4),0,(capacity-oldSize)*cs*4)
   instanceMeta=try grow(instanceMeta,capacity*3,oldSize*3)
   freeCells=try grow(freeCells,capacity,available)
   cellCap=capacity
  }
  dirty=try grow(dirty,cellCap,Int(ctrl[1]));args[22]=UInt32(cellCap)
  cellSlotsReused+=min(available,needed)
  let evaluate=q.makeCommandBuffer()!
  if needed>0{encode(commitMarkers ? "family_marker_allocate" : "family_cell_allocate",commitMarkers ? Int(args[30]) : changes,evaluate)}
  encode("family_registry_finish",1,evaluate)
  if args[30]>0{encode("family_install_markers",Int(args[30]),evaluate)}
  if ec{encode("family_cell_dispatch",1,evaluate);encode("family_cell_events",1,evaluate,16)}
  else{encode("family_cells",newSize,evaluate)}
  try submit(evaluate);try require(counts.contents().assumingMemoryBound(to:UInt32.self)[1]==0,"Cell evaluation failed")
  try require(Int(reg[0])==newSize && reg[0]==reg[1]+reg[4],"sparse Cell registry accounting")
  let publish=q.makeCommandBuffer()!
  if !ec{return publish}
  let needWatch=Int(reg[6]),needPort=Int(reg[7]),retired=Int(reg[5])
  let wgc=watchCount+needWatch>watchCap || (retired>0 && watchCount>0)
  let pgc=portCount+needPort>portCap || (retired>0 && portCount>0)
  let oldWatch=watches!,oldPorts=ports!,oldWatchCount=watchCount,oldPortCount=portCount
  var liveWatch=watchCount,livePort=portCount
  if wgc || pgc {
   let count=q.makeCommandBuffer()!
   if wgc{encode("family_watch_count",watchCount,count)}
   if pgc{encode("family_port_count",portCount,count)}
   try submit(count)
   if wgc{liveWatch=Int(reg[8])};if pgc{livePort=Int(reg[9])}
  }
  if wgc {
   if watchCap>0{watchGCs+=1}
   let capacity=try pow2(max(1,liveWatch+needWatch)),bucketCount=try pow2(max(1,2*(liveWatch+needWatch)))
   // No survivors: the old storage can be overwritten after the wake phase.
   if liveWatch>0 || capacity != watchCap {watches=try alloc(capacity*(6+c))}
   if bucketCount != watchBuckets || wheads.length<bucketCount*4 {wheads=try alloc(bucketCount)}
   watchCap=capacity;watchBuckets=bucketCount;ctrl[0]=0
   args[10]=UInt32(watchBuckets);args[20]=UInt32(watchCap)
   encode("family_watch_clear",watchBuckets,publish)
   if liveWatch>0{previous=oldWatch;encode("family_watch_compact",oldWatchCount,publish)}
  }
  if pgc {
   if portCap>0{portGCs+=1}
   let capacity=try pow2(max(1,livePort+needPort))
   if livePort>0 || capacity != portCap{ports=try alloc(2*capacity)}
   portCap=capacity;ctrl[2]=0;pcursors=Array(repeating:0,count:nq);args[21]=UInt32(portCap)
   if livePort>0{previous=oldPorts;encode("family_port_compact",oldPortCount,publish)}
  }
  args[20]=UInt32(max(1,watchCap));args[21]=UInt32(max(1,portCap))
  if Int(ctrl[1])>0{encode("family_cell_publish",1,publish,16)}
  watchCount=liveWatch+needWatch;portCount=livePort+needPort
  return publish
 }
 func advance(_ data:Data,ack:Int)throws->([String:Any],[UInt32]){
  try require(ack==epoch,"missing native commit acknowledgment")
  for id in pending{cursors[id]=pendingEnd;pcursors[id]=pendingPortEnd};pending=[]
  try require(data.count>=16 && data.count%4==0,"family packet size")
  var w=data.withUnsafeBytes{Array($0.bindMemory(to:UInt32.self))};let total=Int(w[0]),changes=Int(w[1]),nextEpoch=Int(w[3]);var n=Int(w[2])
  let markerStart=4+4*n+changes*(1+rs)
  try require(nextEpoch==epoch+1 && total>=rowSlots && w.count>markerStart,"family epoch/shape")
  let markerData=Array(w[markerStart...]),markerCount=Int(markerData[0])
  try require(markerData.count==1+markerCount*(4+2*p),"marker receipt shape")
  w=Array(w[..<markerStart])
  var ids=(0..<n).map{Int(w[4+4*$0])};try require(Set(ids).count==n && ids.allSatisfy{$0<nq},"family query IDs")
  let requestedIDs=ids;var queryData=w.withUnsafeBytes{Data($0)}
  for i in 0..<changes{try require(w[4+4*n+i*(1+rs)]<total,"family row slot")}
  let oldRowSlots=rowSlots,oldData=rows.contents().assumingMemoryBound(to:UInt32.self)
  let cellRelevant=nc>0 && (markerCount>0 || (0..<changes).contains{ i in
   let at=4+4*n+i*(1+rs),r=Int(w[at]);return cellTables.contains(w[at+1]) || (r<oldRowSlots && cellTables.contains(oldData[r*rs]))
  })
  let relevant=ec && cellRelevant
  let before=gpu,newRowCap=try pow2(max(1,total)),newEventCap=try pow2(max(2,2*newRowCap))
  if total>slotTable.count {slotTable += Array(repeating:0,count:total-slotTable.count);slotLive += Array(repeating:false,count:total-slotLive.count)}
  for i in 0..<changes {
   let at=4+4*n+i*(1+rs),slot=Int(w[at]),table=Int(w[at+1]),live=w[at+2] != 0
   try require(table<tableDegree.count,"table ID")
   if slotLive[slot]{tableLive[slotTable[slot]]-=1}
   slotTable[slot]=table;slotLive[slot]=live;if live{tableLive[table]+=1}
  }
  // Generic backend gate, independent of Cell plans and certificates. Use
  // final visible counts AFTER all deltas in this packet, never an intermediate
  // delete/reinsert state. Zero-atom queries have no required tables and survive.
  let emptyQueries=emptyTablePruning ? ids.filter{id in queryTables[id].contains{tableLive[$0]==0}} : []
  if !emptyQueries.isEmpty {
   let blocked=Set(emptyQueries),rowStart=4+4*n
   var filtered=[UInt32(total),UInt32(changes),UInt32(n-emptyQueries.count),UInt32(nextEpoch)]
   for (i,id) in ids.enumerated() where !blocked.contains(id){filtered.append(contentsOf:w[(4+4*i)..<(8+4*i)])}
   filtered.append(contentsOf:w[rowStart...]);ids.removeAll{blocked.contains($0)};n=ids.count
   w=filtered;queryData=w.withUnsafeBytes{Data($0)}
  }
  var rehash=false
  for t in tableLive.indices where tableDegree[t]>0 && tableLive[t]>tableBuckets[t] {
   tableBuckets[t]=try pow2(max(1,2*tableLive[t]));rehash=true
  }
  // Only table identities/counts drive allocation on the host. Binding search,
  // hash construction and Cell state maintenance remain on the device.
  let compact=eventCap==0 || eventCount+changes>eventCap || newEventCap>eventCap || rehash
  rows=try grow(rows,newRowCap*rs,rowSlots*rs)
  if nc>0 && rowCells.length<newRowCap*4 {
   let old=rowCells!,next=try alloc(newRowCap);memset(next.contents(),255,next.length)
   if rowSlots>0{memcpy(next.contents(),old.contents(),rowSlots*4)};rowCells=next
  }
  let oldEvents=eventCount
  var linkBases=[UInt32]();linkBases.reserveCapacity(compact ? total : changes)
  let previousLinks=linkCount
  if compact {linkCount=0}
  for i in 0..<(compact ? total : changes) {
   let slot=compact ? i : Int(w[4+4*n+i*(1+rs)])
   linkBases.append(UInt32(linkCount))
   if slotLive[slot]{linkCount+=tableDegree[slotTable[slot]]}
  }
  try require(linkCount<Int(UInt32.max),"index link address space exhausted")
  let neededLinks=try pow2(max(1,linkCount))
  if compact {
   if eventCap>0{compactions+=1}
   eventCap=max(eventCap,newEventCap);eventCount=total;cursors=Array(repeating:0,count:nq)
   events=try alloc(3*eventCap);links=try alloc(neededLinks);linkCapacity=neededLinks
   headLayout=[];buckets=0
   for t in indexTables {
    let count=t==UInt32.max ? 1 : tableBuckets[Int(t)]
    headLayout += [UInt32(buckets),UInt32(count-1)];buckets+=count
   }
   heads=try alloc(buckets)
   domainLayout=[];domainWords=0
   for table in domainTables {
    let count=tableBuckets[Int(table)];try require(domainWords<Int(UInt32.max)-count,"summary address space exhausted");domainLayout += [UInt32(domainWords),UInt32(count-1)];domainWords+=count
   }
   domains=try alloc(domainWords)
  }else{
   eventCount+=changes
   links=try grow(links,neededLinks,previousLinks);linkCapacity=max(linkCapacity,neededLinks)
  }
  let oldWatchCount=watchCount,oldPortCount=portCount
  let ctrl=state.contents().assumingMemoryBound(to:UInt32.self),reg=registry.contents().assumingMemoryBound(to:UInt32.self)
  ctrl[0]=UInt32(watchCount);ctrl[1]=0;ctrl[2]=UInt32(portCount)
  reg[2]=0;reg[3]=0;for i in 5..<10{reg[i]=0}
  rowSlots=total;rowCap=newRowCap;epoch=nextEpoch
  var expanded=queryData;expanded.append(Data(count:n*8+nq*4))
  let linkBaseOffset=expanded.count/4
  linkBases.withUnsafeBytes{expanded.append(contentsOf:$0)}
  let headLayoutOffset=expanded.count/4
  headLayout.withUnsafeBytes{expanded.append(contentsOf:$0)}
  let domainLayoutOffset=expanded.count/4
  domainLayout.withUnsafeBytes{expanded.append(contentsOf:$0)}
  let markerOffset=expanded.count/4+1
  markerData.withUnsafeBytes{expanded.append(contentsOf:$0)}
  packet=expanded.withUnsafeBytes{d.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
  args=[UInt32(total),UInt32(oldEvents),UInt32(changes),UInt32(n),UInt32(buckets),UInt32(outCap),UInt32(eventCount),compact ? 1:0,0,UInt32(epoch),UInt32(watchBuckets),UInt32(oldRowSlots),UInt32(oldWatchCount),0,UInt32(oldPortCount),UInt32(locality),UInt32(routingMode),0,UInt32(seedCap),0,UInt32(max(1,watchCap)),UInt32(max(1,portCap)),UInt32(cellCap),0,0,UInt32(linkBaseOffset),UInt32(headLayoutOffset),UInt32(domainLayoutOffset),UInt32(domainWords),UInt32(markerOffset),UInt32(markerCount)]
  memset(counts.contents(),0,counts.length)
  var maintenance=try prepareCells(relevant,cellRelevant,changes,compact)
  var maintenanceSeconds=0.0
  if phaseTiming {
   try submit(maintenance);maintenanceSeconds=gpu-before
   maintenance=q.makeCommandBuffer()!
  }
  let expectedWatch=watchCount,expectedPort=portCount
  let words=packet.contents().assumingMemoryBound(to:UInt32.self)
  var span=0;for (i,id) in ids.enumerated(){words[6+4*i]=UInt32(cursors[id]);words[7+4*i]=UInt32(pcursors[id]);span=max(span,eventCount-cursors[id])}
  // Group consumers once per batch, not once per Cell. Equal residual shape,
  // native watermark AND port cursor imply identical delta ownership.
  let groupBase=4+4*n+changes*(1+rs)
  var groups=[[UInt32]:(Int,Int)]()
  for (i,id) in ids.enumerated(){
   words[groupBase+i]=UInt32(i);words[groupBase+n+i]=UInt32.max
   if locality<2{continue}
   let key=[portJoinGroup[id],words[5+4*i],words[7+4*i]]
   if let (head,tail)=groups[key]{words[groupBase+i]=UInt32(head);words[groupBase+n+tail]=UInt32(i);groups[key]=(head,i)}
   else{groups[key]=(i,i)}
  }
  // A zero-atom query has one initial unit binding, even with zero events.
  args[23]=UInt32(groupBase+2*n)
  args[24]=UInt32(ids.map{cursors[$0]}.min() ?? eventCount)
  for id in 0..<nq{words[Int(args[23])+id]=UInt32.max}
  for (i,id) in ids.enumerated(){words[Int(args[23])+id]=UInt32(i)}
  span=max(1,span)
  args[8]=UInt32(span);args[6]=UInt32(eventCount);args[4]=UInt32(buckets)
  let portWork=ec && ids.enumerated().contains{ i,id in portMeta[id*3] != UInt32.max && (words[5+4*i]>0 || portMeta[id*3+2]==0) && (relevant || pcursors[id]<portCount) }
  var retries=0,count=0,routeRetries=0,routedCount=0,passes=0
  let activeShapes=Set(ids.map{queryShapes[$0]}).sorted()
  var routeReady=false
  while true{
   memset(counts.contents(),0,5*4);memset(counts.contents().advanced(by:13*4),0,3*4);args[5]=UInt32(outCap)
   counts.contents().assumingMemoryBound(to:UInt32.self)[41]=0
   let cb=passes==0 ? maintenance : q.makeCommandBuffer()!;passes+=1
   var work=cb
   if n==0 {
    // Still commit table/index/Cell maintenance and acknowledge every request.
    // No classify, scatter or query kernels are needed for a proved-empty batch.
    memset(routing.contents(),0,routing.length);try submit(work)
    try require(counts.contents().assumingMemoryBound(to:UInt32.self)[1]==0,"empty batch maintenance failed")
    break
   }
   if routingMode==3 {
    if !routeReady {
     memset(routing.contents(),0,routing.length);args[18]=UInt32(seedCap)
     encode("family_classify_tables",eventCount-Int(args[24]),work)
     encode("family_shape_dispatch",1,work)
     // The GPU counts and dispatches its actual seeds without a host readback.
     // Overflow cancels this attempt and requests lossless buffer growth.
     encode("family_shape_scatter",1,work,12*4)
    }
    if portWork {encode("family_port_dispatch",1,work);encode(locality>0 ? "family_port_locality" : "family_port_join",1,work,32)}
    encode("family_unit",n,work)
    for g in activeShapes {encode("family_shape_\(g)",1,work,(28+g*6+1)*4)}
   } else if routingMode>0 {
    if retries==0 {
     memset(routing.contents(),0,routing.length);args[18]=UInt32(seedCap)
     encode(routingMode==3 ? "family_classify_tables" : "family_classify",routingMode==3 ? eventCount-Int(args[24]) : span*p*n,work);try submit(work)
     routedCount=Int(routing.contents().assumingMemoryBound(to:UInt32.self)[0])
     if routedCount>seedCap {
      routeRetries+=1;seedCap=try pow2(routedCount);seedQueue=try alloc(seedCap*(8+p));orderedSeeds=try alloc(seedCap*(8+p));args[18]=UInt32(seedCap)
      memset(routing.contents(),0,routing.length);work=q.makeCommandBuffer()!;encode(routingMode==3 ? "family_classify_tables" : "family_classify",routingMode==3 ? eventCount-Int(args[24]) : span*p*n,work);try submit(work)
     }
     try require(routing.contents().assumingMemoryBound(to:UInt32.self)[7]==0,"routing capacity changed within immutable snapshot")
     let rc=routing.contents().assumingMemoryBound(to:UInt32.self)
     try require(rc[0]==rc[1]+rc[2]+rc[3] && rc[24]==rc[0]+rc[4]+rc[5]+rc[27],"partial-query routing lost or duplicated a seed")
     work=q.makeCommandBuffer()!;encode("family_route_dispatch",1,work);encode("family_route_scatter",routedCount,work)
    }
    // Each tier reads the same immutable snapshot; effects still commit together.
    memset(counts.contents().advanced(by:29*4),0,12*4)
    if portWork {encode("family_port_dispatch",1,work);encode(locality>0 ? "family_port_locality" : "family_port_join",1,work,32)}
    if routingMode==3 {encode("family_unit",n,work)}
    for tier in 0..<3 {args[17]=UInt32(tier);encode("family_routed_join",1,work,(12+tier*4)*4)}
   } else {encode("family_join",span*p*n,work)}
   if routingMode==0 && portWork {encode("family_port_dispatch",1,work);encode(locality>0 ? "family_port_locality" : "family_port_join",1,work,32)};try submit(work)
   if routingMode==3 {
    let rc=routing.contents().assumingMemoryBound(to:UInt32.self);routedCount=Int(rc[0])
    if rc[7] != 0 {
     routeRetries+=1;seedCap=try pow2(max(1,routedCount));seedQueue=try alloc(seedCap*(8+p));orderedSeeds=try alloc(seedCap*(8+p));continue
    }
    try require(rc[0]==rc[3] && rc[24]==rc[0],"table routing lost seeds")
    routeReady=true
   }
   let flags=counts.contents().assumingMemoryBound(to:UInt32.self)[1]
   try require(flags&(~UInt32(1))==0,"family count or revision overflow");count=Int(counts.contents().assumingMemoryBound(to:UInt32.self)[0])
   if flags==0{break};outCap=try pow2(max(1,routingMode==3 ? Int(counts.contents().assumingMemoryBound(to:UInt32.self)[41]) : count));output=try alloc(outCap*(routingMode==3 ? 1 : 1+s));retries+=1
  }
  let outputWords=routingMode==3 ? Int(counts.contents().assumingMemoryBound(to:UInt32.self)[41]) : count*(1+s)
  let result=Array(UnsafeBufferPointer(start:output.contents().assumingMemoryBound(to:UInt32.self),count:outputWords))
  try require(result.count<=Int(UInt32.max)/4,"family response too large")
  try require(Int(ctrl[0])==expectedWatch && Int(ctrl[2])==expectedPort,"Cell dependency publication count mismatch")
  watchCount=Int(ctrl[0]);portCount=Int(ctrl[2]);pending=requestedIDs;pendingEnd=eventCount;pendingPortEnd=portCount
  previous=code // completed GC must not retain an otherwise dead old log
  var seedWork=[[UInt32]](),depthWork=[[UInt32]](),cellPlanWork=[[UInt32]](),querySearchWork=[[UInt32]]()
  if seedProfile {
   let v=counts.contents().assumingMemoryBound(to:UInt32.self)
   var totals=[UInt64](repeating:0,count:3)
   for id in ids {
    let at=64+3*id
    if v[at]>0{seedWork.append([UInt32(id),v[at],v[at+1],v[at+2]])}
    for k in 0..<3{totals[k]+=UInt64(v[at+k])}
   }
   let width=depthMetrics.count,base=64+3*nq
   let slot=Dictionary(uniqueKeysWithValues:depthMetrics.enumerated().map{($0.element,$0.offset)})
   var depthTotals=[UInt64](repeating:0,count:width)
   for id in ids {
    for depth in 0..<queryAtoms[id] {
     let at=base+(id*p+depth)*width
     let values=(0..<width).map{v[at+$0]}
     if values.contains(where:{$0>0}){depthWork.append([UInt32(id),UInt32(depth)]+values)}
     for k in 0..<width{depthTotals[k]+=UInt64(values[k])}
     let prefixes=values[slot["prefixes"]!],refuted=values[slot["refutations"]!]
     try require(refuted<=prefixes,"depth rejection exceeds prefixes")
     if depth+1<queryAtoms[id]{try require(v[at+width+slot["entries"]!]==prefixes-refuted,"depth frontier accounting mismatch")}
    }
   }
   for (field,counter) in [("link_visits",44),("live_rows",45),("key_checks",42),("domain_checks",46),("signature_refutations",48),("connection_checks",52),("connection_refutations",53),("connection_unknown",54),("range_checks",55),("range_refutations",56)] {
    try require(depthTotals[slot[field]!]==UInt64(v[counter]),"depth/global accounting mismatch: \(field)")
   }
   if let deepest=slot["deepest_seeds"]{try require(depthTotals[deepest]==UInt64(v[49]),"deepest-seed accounting mismatch")}
   try require(depthTotals[slot["refutations"]!]==UInt64(v[43])+UInt64(v[47])+UInt64(v[53])+UInt64(v[56]),"depth/global refutation mismatch")
   for k in 0..<3{try require(totals[k]==UInt64(v[49+k]),"seed profile accounting mismatch")}
   try require(totals[1]+totals[2]<=totals[0],"seed profile outcomes overlap")
   let searchBase=64+nq*(3+p*depthMetrics.count),cellBase=searchBase+nq*searchMetrics.count
   for id in ids {
    let values=(0..<searchMetrics.count).map{v[searchBase+id*searchMetrics.count+$0]}
    if values.contains(where:{$0>0}){querySearchWork.append([UInt32(id)]+values)}
   }
   for id in 0..<nc {
    let values=(0..<cellMetrics.count).map{v[cellBase+id*cellMetrics.count+$0]}
    if values.contains(where:{$0>0}){cellPlanWork.append([UInt32(id)]+values)}
   }
  }
  return (["epoch":epoch,"queries":requestedIDs.count,"matched_queries":n,"empty_table_queries":emptyQueries,"empty_table_pruning":emptyTablePruning,"bindings":count,"compact_bindings":routingMode==3,"output_words":outputWords,"port_locality":locality,"query_routing":routingMode,"route_retries":routeRetries,"routed_seeds":routedCount,"row_slots":rowSlots,"row_capacity":rowCap,"events":eventCount,"event_capacity":eventCap,"index_count":ni,"index_link_words":linkCount,"index_link_bytes":links.length,"index_head_bytes":heads.length,"refutation_domain_bytes":domains.length,"sparse_indexes":true,"query_shapes":shapeCount,"active_shapes":activeShapes.count,
   "compactions":compactions,"compacted":compact,"old_events":oldEvents,"retries":retries,"gpu_seconds":gpu-before,"phase_timing":phaseTiming,"gpu_maintenance_seconds":maintenanceSeconds,"gpu_matching_seconds":phaseTiming ? gpu-before-maintenanceSeconds : 0,
   "cell_cache_bytes":cells.length,"cell_plans":nc,"cell_instances":Int(reg[4]),"cell_instance_slots":Int(reg[0]),"cell_instance_capacity":cellCap,"cell_free_slots":Int(reg[1]),"cell_slots_reused":cellSlotsReused,"cell_directory_bytes":rowCells.length+instanceMeta.length+freeCells.length+registry.length,"cell_watch_bytes":watches.length+wheads.length,"cell_port_bytes":ports.length,"cell_dirty_bytes":dirty.length,"sparse_cells":true,"event_cells":ec,"cell_tables_affected":relevant,"port_work":portWork,"watch_count":watchCount,"watch_capacity":watchCap,"port_count":portCount,"port_capacity":portCap,"watch_compactions":watchGCs,"port_compactions":portGCs,"dirty_cells":ctrl[1],
   "cell_plan_work":cellPlanWork,"query_search_work":querySearchWork,"query_depth_work":depthWork,"query_seed_work":seedWork,"cell_work":["range_checks":counts.contents().assumingMemoryBound(to:UInt32.self)[55],"range_refutations":counts.contents().assumingMemoryBound(to:UInt32.self)[56],"connection_checks":counts.contents().assumingMemoryBound(to:UInt32.self)[52],"connection_refutations":counts.contents().assumingMemoryBound(to:UInt32.self)[53],"connection_unknown":counts.contents().assumingMemoryBound(to:UInt32.self)[54],"eligible_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[49],"refuted_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[50],"productive_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[51],"signature_refuted_branches":counts.contents().assumingMemoryBound(to:UInt32.self)[48],"domain_checks":counts.contents().assumingMemoryBound(to:UInt32.self)[46],"domain_refuted_branches":counts.contents().assumingMemoryBound(to:UInt32.self)[47],"join_link_visits":counts.contents().assumingMemoryBound(to:UInt32.self)[44],"join_live_row_visits":counts.contents().assumingMemoryBound(to:UInt32.self)[45],"refutation_checks":counts.contents().assumingMemoryBound(to:UInt32.self)[42],"refuted_branches":counts.contents().assumingMemoryBound(to:UInt32.self)[43],"join_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[2],"skipped_join_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[3],"certified_anchor_hits":counts.contents().assumingMemoryBound(to:UInt32.self)[4],"certification_attempts":counts.contents().assumingMemoryBound(to:UInt32.self)[5],"certificate_reuses":counts.contents().assumingMemoryBound(to:UInt32.self)[6],"shape_fallbacks":counts.contents().assumingMemoryBound(to:UInt32.self)[7],"invalidated_certificates":counts.contents().assumingMemoryBound(to:UInt32.self)[8],"certification_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[9],"watch_wakes":counts.contents().assumingMemoryBound(to:UInt32.self)[10],"dirty_cells":counts.contents().assumingMemoryBound(to:UInt32.self)[11],"prefix_rows_reused":counts.contents().assumingMemoryBound(to:UInt32.self)[12],"port_bindings":counts.contents().assumingMemoryBound(to:UInt32.self)[13],"delegated_branches":counts.contents().assumingMemoryBound(to:UInt32.self)[14],"port_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[15],"ports_published":counts.contents().assumingMemoryBound(to:UInt32.self)[16],"missing_key_waits":counts.contents().assumingMemoryBound(to:UInt32.self)[17],"watch_entries_checked":counts.contents().assumingMemoryBound(to:UInt32.self)[18],"direct_row_reuses":counts.contents().assumingMemoryBound(to:UInt32.self)[19],"port_context_loads":counts.contents().assumingMemoryBound(to:UInt32.self)[20],"port_context_words":counts.contents().assumingMemoryBound(to:UInt32.self)[21],"port_join_executions":counts.contents().assumingMemoryBound(to:UInt32.self)[22],"port_consumer_checks":counts.contents().assumingMemoryBound(to:UInt32.self)[23],"port_shared_joins_saved":counts.contents().assumingMemoryBound(to:UInt32.self)[24],"port_candidate_rows":counts.contents().assumingMemoryBound(to:UInt32.self)[25],"routing_locator_probes":counts.contents().assumingMemoryBound(to:UInt32.self)[26],"routing_locator_rows":counts.contents().assumingMemoryBound(to:UInt32.self)[27],"routing_unique_hints":counts.contents().assumingMemoryBound(to:UInt32.self)[28],"ready_productive_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[29],"suspected_productive_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[30],"unmapped_productive_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[31],"ready_matches":counts.contents().assumingMemoryBound(to:UInt32.self)[32],"suspected_matches":counts.contents().assumingMemoryBound(to:UInt32.self)[33],"unmapped_matches":counts.contents().assumingMemoryBound(to:UInt32.self)[34],"ready_reuse_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[35],"suspected_reuse_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[36],"unmapped_reuse_seeds":counts.contents().assumingMemoryBound(to:UInt32.self)[37],"ready_reused_atoms":counts.contents().assumingMemoryBound(to:UInt32.self)[38],"suspected_reused_atoms":counts.contents().assumingMemoryBound(to:UInt32.self)[39],"unmapped_reused_atoms":counts.contents().assumingMemoryBound(to:UInt32.self)[40],"routed_ready":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[1]) : 0,"routed_suspected":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[2]) : 0,"routed_unmapped":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[3]) : 0,"routing_no_next_atom":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[4]) : 0,"routing_delegated_to_port":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[5]) : 0,"routing_ambiguous":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[6]) : 0,"routing_valid_seeds":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[24]) : 0,"ready_covered_atom_sum":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[25]) : 0,"ready_query_atom_sum":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[26]) : 0,"routing_earlier_delta_owner":routingMode>0 ? Int(routing.contents().assumingMemoryBound(to:UInt32.self)[27]) : 0],
   "device_bytes":[code,rows!,events!,links!,heads!,domains!,output!,counts!,packet!,cells!,watches!,wheads!,dirty!,state!,ports!,seedQueue!,orderedSeeds!,routing!,rowCells!,instanceMeta!,freeCells!,registry!].reduce(0){$0+$1.length},"device":d.name,"retained_matches":0],result)
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
          try sendFrame(["installed":true,"compile_seconds":family!.compileSeconds,"workers":1,"family_layout":18,"port_locality":family!.locality,"phase_timing":family!.phaseTiming])
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
