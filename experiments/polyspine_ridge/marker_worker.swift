import Foundation
import Metal
func need(_ v:Bool,_ s:String)throws{if !v{throw NSError(domain:s,code:1)}}
try need(CommandLine.arguments.count==4,"worker manifest.json output-dir kernel.metal")
let url=URL(fileURLWithPath:CommandLine.arguments[1]),folder=url.deletingLastPathComponent(),out=URL(fileURLWithPath:CommandLine.arguments[2])
try FileManager.default.createDirectory(at:out,withIntermediateDirectories:true)
let manifest=try JSONSerialization.jsonObject(with:Data(contentsOf:url)) as! [String:Any]
let device=MTLCreateSystemDefaultDevice()!,queue=device.makeCommandQueue()!,options=MTLCompileOptions();options.fastMathEnabled=false
let lib=try device.makeLibrary(source:String(contentsOfFile:CommandLine.arguments[3],encoding:.utf8),options:options)
var pipes:[String:MTLComputePipelineState]=[:]
for name in ["gather","mma","candidate","reset","reduce","features"]{pipes[name]=try device.makeComputePipelineState(function:lib.makeFunction(name:name)!)}
func launch(_ cb:MTLCommandBuffer,_ name:String,_ bs:[MTLBuffer],_ cfg:[UInt32],_ n:Int,_ groups:Bool=false,_ initCost:Int64?=nil){
 if n==0{return};let e=cb.makeComputeCommandEncoder()!,p=pipes[name]!;e.setComputePipelineState(p)
 for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
 cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
 if var cost=initCost{e.setBytes(&cost,length:8,index:bs.count+1)}
 if groups{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
 else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,p.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
}
var result:[[String:Any]]=[]
for frame in manifest["frames"] as! [[String:Any]]{
 let data=try Data(contentsOf:folder.appendingPathComponent(frame["packet"] as! String));var at=8
 try need(String(data:data.prefix(8),encoding:.utf8)=="MRKLINK1","schema")
 func word()->UInt32{defer{at+=4};return data.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:at,as:UInt32.self)}}
 let cfg=(0..<5).map{_ in word()},m=Int(cfg[0]),n=Int(cfg[1]),queries=Int(cfg[2]),width=Int(cfg[3]);let initializer=data.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:at,as:Int64.self)};at+=8
 func bytes(_ size:Int)->MTLBuffer{defer{at+=size};if size==0{return device.makeBuffer(length:8,options:.storageModeShared)!};return data.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!+at,length:size,options:.storageModeShared)!}}
 let rows=bytes(m*6*4),initial=bytes(n*4*8),offset=bytes((n+1)*4),index=bytes(m*4),stat=bytes(n*4*4),weight=bytes(width*8*2)
 try need(at==data.count,"trailing bytes")
 func alloc(_ size:Int)->MTLBuffer{device.makeBuffer(length:max(8,size),options:.storageModeShared)!}
 var current=device.makeBuffer(bytes:initial.contents(),length:initial.length,options:.storageModeShared)!,next=alloc(n*4*8)
 let x=alloc(m*width*2),y=alloc(m*4),c=alloc(m*3*8),f=alloc(queries*24*4),changed=alloc(4)
 var step=0;var records:[[String:Any]]=[];let id=frame["id"] as! Int
 while true{
  let stop=step<8 ? (step==0 ? 1:step*2):step+8,cb=queue.makeCommandBuffer()!,begin=ProcessInfo.processInfo.systemUptime
  while step<stop{
   launch(cb,"reset",[changed],cfg,1);launch(cb,"gather",[rows,current,x],cfg,m);launch(cb,"mma",[x,weight,y],cfg,(m+7)/8,true)
   launch(cb,"candidate",[rows,current,y,c],cfg,m,false,initializer);launch(cb,"reduce",[current,next,c,offset,index,changed],cfg,n)
   swap(&current,&next);step+=1
  }
  launch(cb,"features",[rows,initial,current,next,c,offset,index,stat,f],cfg,queries)
  cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"GPU \(String(describing:cb.error))")
  let stable=changed.contents().load(as:UInt32.self)==0
  for (suffix,b,size) in [("state",current,n*4*8),("candidate",c,m*3*8),("features",f,queries*24*4)]{
   try Data(bytes:b.contents(),count:size).write(to:out.appendingPathComponent("frame-\(id)-depth-\(step)-\(suffix).bin"))
  }
  records.append(["depth":step,"stable":stable,"gpu_seconds":cb.gpuEndTime-cb.gpuStartTime,"submit_wait_seconds":ProcessInfo.processInfo.systemUptime-begin])
  if stable&&step>=8{break}
  // With nonnegative additive hyperedges, a minimum-cost witness can remove
  // cycles. This is an invariant check, not truncation accepted as saturation.
  try need(step<=n+8,"nonnegative projection failed its finite-carrier bound")
 }
 result.append(["id":id,"markers":m,"slots":n,"queries":queries,"steps":records,"depth":step,"resident_state_bytes":initial.length+current.length+next.length])
 print("frame \(id): stable at checkpoint \(step)")
}
try JSONSerialization.data(withJSONObject:["device":device.name,"frames":result,"scope":"offline cost/availability projection on pre-stage marker carrier, not native closure"],options:[.sortedKeys,.prettyPrinted]).write(to:out.appendingPathComponent("gpu.json"))
