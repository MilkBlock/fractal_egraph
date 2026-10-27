import Foundation
import Metal
func need(_ b:Bool,_ s:String)throws{if !b{throw NSError(domain:s,code:1)}}
try need(CommandLine.arguments.count==4,"worker manifest.json output-dir kernel.metal")
let url=URL(fileURLWithPath:CommandLine.arguments[1]),folder=url.deletingLastPathComponent(),out=URL(fileURLWithPath:CommandLine.arguments[2])
try FileManager.default.createDirectory(at:out,withIntermediateDirectories:true)
let manifest=try JSONSerialization.jsonObject(with:Data(contentsOf:url)) as! [String:Any]
let device=MTLCreateSystemDefaultDevice()!,queue=device.makeCommandQueue()!,options=MTLCompileOptions();options.fastMathEnabled=false
let lib=try device.makeLibrary(source:String(contentsOfFile:CommandLine.arguments[3],encoding:.utf8),options:options)
var pipes:[String:MTLComputePipelineState]=[:]
for s in ["prepare","fused_mma","integers","pool"]{pipes[s]=try device.makeComputePipelineState(function:lib.makeFunction(name:s)!)}
func launch(_ cb:MTLCommandBuffer,_ name:String,_ bs:[MTLBuffer],_ cfg:[UInt32],_ n:Int,_ groups:Bool=false){
 if n==0{return};let e=cb.makeComputeCommandEncoder()!,p=pipes[name]!;e.setComputePipelineState(p)
 for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
 cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
 if groups{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
 else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,p.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
}
var report:[[String:Any]]=[]
for frame in manifest["frames"] as! [[String:Any]]{
 let d=try Data(contentsOf:folder.appendingPathComponent(frame["packet"] as! String));var at=8
 try need(String(data:d.prefix(8),encoding:.utf8)=="AFFINE01","packet magic")
 func word()->UInt32{defer{at+=4};return d.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:at,as:UInt32.self)}}
 let cfg=(0..<4).map{_ in word()},n=Int(cfg[0]),roots=Int(cfg[1]),width=Int(cfg[2])
 func bytes(_ length:Int)->MTLBuffer{defer{at+=length};if length==0{return device.makeBuffer(length:8,options:.storageModeShared)!};return d.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!+at,length:length,options:.storageModeShared)!}}
 let h=bytes(roots*512*2),p=bytes(n*4*8),m=bytes(n*4*4),w=bytes(width*8*2),offset=bytes((roots+1)*4),index=bytes(n*4)
 try need(at==d.count,"trailing bytes")
 func alloc(_ n:Int)->MTLBuffer{device.makeBuffer(length:max(n,8),options:.storageModeShared)!}
 let x=alloc(n*width*2),y=alloc(n*3*4),exact=alloc(n*3*8),f=alloc(roots*24*4),cb=queue.makeCommandBuffer()!
 let start=ProcessInfo.processInfo.systemUptime
 launch(cb,"prepare",[h,p,m,x],cfg,n);launch(cb,"fused_mma",[x,w,y],cfg,(n+7)/8,true)
 launch(cb,"integers",[p,m,y,exact],cfg,n);launch(cb,"pool",[p,m,exact,offset,index,f],cfg,roots)
 cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"GPU failed \(String(describing:cb.error))")
 let host=ProcessInfo.processInfo.systemUptime-start,id=frame["id"] as! Int
 for (suffix,b,size) in [("matrix",y,n*3*4),("exact",exact,n*3*8),("features",f,roots*24*4)]{
  try Data(bytes:b.contents(),count:size).write(to:out.appendingPathComponent("frame-\(id)-\(suffix).bin"))
 }
 report.append(["id":id,"contexts":n,"roots":roots,"gpu_seconds":cb.gpuEndTime-cb.gpuStartTime,"submit_wait_seconds":host,"context_matrix_bytes":x.length])
}
try JSONSerialization.data(withJSONObject:["device":device.name,"frames":report,"scope":"offline fixed-context experiment; not full runtime"],options:[.sortedKeys,.prettyPrinted]).write(to:out.appendingPathComponent("gpu.json"))
