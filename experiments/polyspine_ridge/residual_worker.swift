// Fixed-depth PRE-stage residual projection. Raw typed rows + shared column
// programs enter the GPU; no CPU-generated per-instance binding list is read.
import Foundation
import Metal

func need(_ b:Bool,_ message:String)throws{if !b{throw NSError(domain:message,code:1)}}
try need(CommandLine.arguments.count==4,"worker capture.json output-dir kernel.metal")
let manifestURL=URL(fileURLWithPath:CommandLine.arguments[1]),folder=manifestURL.deletingLastPathComponent()
let manifest=try JSONSerialization.jsonObject(with:Data(contentsOf:manifestURL)) as! [String:Any]
let out=URL(fileURLWithPath:CommandLine.arguments[2]);try FileManager.default.createDirectory(at:out,withIntermediateDirectories:true)
let program=manifest["program"] as! [String:Any],family=program["family"] as! [String]
let writes=Set((program["recipes"] as! [[String:Any]]).map{($0["output"] as! [Any])[0] as! String})
let device=MTLCreateSystemDefaultDevice()!,queue=device.makeCommandQueue()!
let options=MTLCompileOptions();options.fastMathEnabled=false
let library=try device.makeLibrary(source:String(contentsOfFile:CommandLine.arguments[3],encoding:.utf8),options:options)
var pipes:[String:MTLComputePipelineState]=[:]
for n in ["residual_copy","residual_step","residual_read"]{pipes[n]=try device.makeComputePipelineState(function:library.makeFunction(name:n)!)}
func buffer<T>(_ values:[T])->MTLBuffer{
 if values.isEmpty{return device.makeBuffer(length:4,options:.storageModeShared)!}
 return values.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}
}
func launch(_ cb:MTLCommandBuffer,_ name:String,_ bs:[MTLBuffer],_ cfg:[UInt32],_ n:Int){
 if n==0{return};let e=cb.makeComputeCommandEncoder()!,p=pipes[name]!;e.setComputePipelineState(p)
 for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
 cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
 e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,p.maxTotalThreadsPerThreadgroup),height:1,depth:1));e.endEncoding()
}
var results:[[String:Any]]=[]
for f in manifest["frames"] as! [[String:Any]]{
 let data=try Data(contentsOf:folder.appendingPathComponent(f["packet"] as! String));var at=8
 try need(String(data:data.prefix(8),encoding:.utf8)=="POLYRES1","packet schema")
 func words(_ n:Int)->[UInt32]{let start=at;at+=4*n;return data.withUnsafeBytes{p in (0..<n).map{p.loadUnaligned(fromByteOffset:start+4*$0,as:UInt32.self)}}}
 let c=words(8).map(Int.init),n=c[0],pred=c[1],groups=c[2],tasks=c[3],nr=c[4],nc=c[5],queries=c[6],rounds=c[7]
 let rawSeed=words(n*pred).map{Float(bitPattern:$0)},desc=buffer(words(groups*9)),rows=buffer(words(nr)),code=buffer(words(nc)),query=buffer(words(queries))
 try need(at==data.count,"trailing packet data")
 for mode in ["prestate","erased_prestate"]{
  var seed=rawSeed
  if mode=="erased_prestate"{for p in 0..<pred where writes.contains(family[p]){for i in 0..<n{seed[p*n+i]=0}}}
  var current=buffer(seed),next=device.makeBuffer(length:current.length,options:.storageModeShared)!
  let fraction=device.makeBuffer(length:current.length,options:.storageModeShared)!
  let features=device.makeBuffer(length:max(4,queries*(1+2*rounds)*4),options:.storageModeShared)!
  let begin=ProcessInfo.processInfo.systemUptime,cb=queue.makeCommandBuffer()!
  launch(cb,"residual_read",[current,fraction,query,features],[UInt32(queries),UInt32(1+2*rounds),0,0],queries)
  for depth in 1...rounds{
   launch(cb,"residual_copy",[current,next,fraction],[UInt32(n*pred)],n*pred)
   launch(cb,"residual_step",[current,next,fraction,desc,rows,code],[UInt32(n),UInt32(groups),UInt32(tasks)],tasks)
   launch(cb,"residual_read",[next,fraction,query,features],[UInt32(queries),UInt32(1+2*rounds),UInt32(depth),0],queries)
   swap(&current,&next)
  }
  cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"projection failed")
  let host=ProcessInfo.processInfo.systemUptime-begin,name="cycle-\(String(format:"%02d",f["cycle"] as! Int))-\(mode).bin"
  try Data(bytes:features.contents(),count:queries*(1+2*rounds)*4).write(to:out.appendingPathComponent(name))
  let bytes=current.length+next.length+fraction.length+features.length+desc.length+rows.length+code.length+query.length
  let row:[String:Any] = ["cycle":f["cycle"]!,"mode":mode,"file":name,"rows":queries,"columns":1+2*rounds,
                  "gpu_seconds":cb.gpuEndTime-cb.gpuStartTime,"host_submit_wait_seconds":host,
                  "row_program_evaluations":tasks*rounds,
                  "resident_bytes":bytes]
  results.append(row)
 }
}
let result:[String:Any]=["scope":"device projection only; fixed pre-stage carrier, not full eggcc execution","device":device.name,"frames":results,"rounds":8,"warmups":0,"retained_runs":1]
try JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys]).write(to:out.appendingPathComponent("projection.json"))
print("projected \(results.count) frames")
