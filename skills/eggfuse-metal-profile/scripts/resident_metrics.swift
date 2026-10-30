// Diagnostic instrumentation only. The input program, quantum, shader and
// private schedule execution are unchanged. No checkpoint/stop option is added.
try require(device.supportsCounterSampling(.atStageBoundary), "encoder timestamp counters unavailable")
let pfSet=(device.counterSets ?? []).first{$0.name==MTLCommonCounterSet.timestamp.rawValue}
try require(pfSet != nil,"timestamp counter set unavailable")
let pfQuantum=program["quantum"] as? Int ?? 8
// The inspected device caps one sample buffer at 32 KiB. Use 16 KiB pages;
// additional encoders allocate additional pages rather than dropping samples.
let pfEncodersPerPage=1024
final class PFRecord {
 let cb:MTLCommandBuffer
 let made:Double
 let before:[UInt32]
 let pc:UInt32
 let outer:Int
 var sent:Double=0
 var counters=[MTLCounterSampleBuffer]()
 var names=[String]()
 var cpuWait:Double=0
 init(_ c:MTLCommandBuffer){cb=c;made=CFAbsoluteTimeGetCurrent();before=(0..<n).map{u32(states,$0*stateWords+3,1)[0]};pc=u32(states,0,1)[0];outer=dispatches}
}
var pfPending=[PFRecord](),pfCommands=[[String:Any]](),pfDurations=[String:[Double]]()
var pfWaitCount=0,pfWaitWall=0.0,pfEncodeWall=0.0,pfCounterResolveWall=0.0
func pfCommand()->MTLCommandBuffer {
 let cb=queue.makeCommandBuffer()!;pfPending.append(PFRecord(cb));return cb
}
func pfRecord(_ cb:MTLCommandBuffer)->PFRecord {
 guard let r=pfPending.first(where:{$0.cb === cb}) else{fatalError("unregistered profile command")};return r
}
func pfEncoder(_ cb:MTLCommandBuffer,_ name:String)->MTLComputeCommandEncoder {
 let r=pfRecord(cb)
 let page=r.names.count/pfEncodersPerPage,index=r.names.count%pfEncodersPerPage
 if page==r.counters.count {
  let d=MTLCounterSampleBufferDescriptor();d.counterSet=pfSet;d.storageMode = .shared;d.sampleCount=2*pfEncodersPerPage
  r.counters.append(try! device.makeCounterSampleBuffer(descriptor:d))
 }
 let d=MTLComputePassDescriptor();d.sampleBufferAttachments[0].sampleBuffer=r.counters[page]
 d.sampleBufferAttachments[0].startOfEncoderSampleIndex=2*index
 d.sampleBufferAttachments[0].endOfEncoderSampleIndex=2*index+1
 r.names.append(name)
 let e=cb.makeComputeCommandEncoder(descriptor:d)!;e.label="fn=\(name) resident-profile";return e
}
func pfCommit(_ cb:MTLCommandBuffer){let r=pfRecord(cb);r.sent=CFAbsoluteTimeGetCurrent();pfEncodeWall+=r.sent-r.made;cb.commit()}
func pfDrain() {
 let started=CFAbsoluteTimeGetCurrent()
 var retained=[PFRecord]()
 for r in pfPending {
  if r.cb.status != .completed {retained.append(r);continue}
  var phases=[String:Double](),counts=[String:Int](),intervals=[(UInt64,UInt64)]()
  if !r.names.isEmpty {
   var ts=[UInt64]()
   for (page,c) in r.counters.enumerated(){
    let count=2*min(pfEncodersPerPage,r.names.count-page*pfEncodersPerPage)
    let data=try! c.resolveCounterRange(0..<count)
    precondition(data != nil,"timestamp resolve returned nil")
    ts+=data!.withUnsafeBytes{v in (0..<count).map{v.loadUnaligned(fromByteOffset:$0*8,as:UInt64.self)}}
   }
   for (i,name) in r.names.enumerated(){let a=ts[2*i],b=ts[2*i+1]
    precondition(a != UInt64.max && b != UInt64.max && b>=a,"invalid encoder timestamp")
    let dt=Double(b-a)*1e-9
    phases[name,default:0]+=dt;counts[name,default:0]+=1;pfDurations[name,default:[]].append(dt);intervals.append((a,b))
   }
  }
  intervals.sort{$0.0<$1.0};var union:UInt64=0,end:UInt64=0
  for (a,b) in intervals {if b>end{union+=b-max(a,end);end=b}}
  let gpu=r.cb.gpuEndTime-r.cb.gpuStartTime
  pfCommands.append(["index":pfCommands.count,"run_before":r.before,"run_after_observed":(0..<n).map{u32(states,$0*stateWords+3,1)[0]},"pc_before":r.pc,"outer_before":r.outer,"label":r.cb.label ?? "", "gpu_seconds":gpu,"gpu_start":r.cb.gpuStartTime,"gpu_end":r.cb.gpuEndTime,"cpu_encode_wall":r.sent-r.made,"cpu_wait_wall":r.cpuWait,"encoder_seconds":phases,"encoder_calls":counts,"encoder_interval_union_seconds":Double(union)*1e-9])
 }
 pfPending=retained;pfCounterResolveWall+=CFAbsoluteTimeGetCurrent()-started
}
func pfWait(_ cb:MTLCommandBuffer){let t=CFAbsoluteTimeGetCurrent();cb.waitUntilCompleted();let dt=CFAbsoluteTimeGetCurrent()-t;pfWaitCount+=1;pfWaitWall+=dt;pfRecord(cb).cpuWait+=dt;pfDrain()}
func pfReport()->[String:Any]{
 var phases=[String:[String:Any]]()
 for (name,values) in pfDurations {
  let s=values.sorted();func q(_ f:Double)->Double{s[min(s.count-1,Int(Double(s.count-1)*f))]}
  phases[name]=["issued_encoders":s.count,"zero_duration_encoders":s.filter{$0==0}.count,"seconds":s.reduce(0,+),"p50_us":q(0.5)*1e6,"p95_us":q(0.95)*1e6,"p99_us":q(0.99)*1e6,"max_us":s.last!*1e6]
 }
 return ["schema":"eggfuse-full-encoder-profile/v1","base_worker_sha256":"__BASE_WORKER_HASH__","quantum":pfQuantum,"phases":phases,"commands":pfCommands,"cpu_wait_calls":pfWaitCount,"cpu_wait_wall_seconds":pfWaitWall,"cpu_command_encoding_wall_seconds":pfEncodeWall,"counter_resolution_wall_seconds":pfCounterResolveWall,"pending_commands":pfPending.count,"scope":"diagnostic encoder boundaries; original full control/private schedules; timestamps in nanoseconds, not Metal4 raw ticks"]
}
