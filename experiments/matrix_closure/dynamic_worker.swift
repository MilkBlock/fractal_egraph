import Foundation
import Metal
func need(_ ok:Bool,_ why:String)throws{if !ok{throw NSError(domain:why,code:1)}}
let args=CommandLine.arguments
try need(args.count==4,"dynamic_worker library.json kernel.metal dynamic.metal")
let libraryDescription=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:args[1]))) as! [String:Any]
try need(libraryDescription["schema"] as? String=="matrix-dynamic/v1","library schema")
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue() else{fatalError("Metal unavailable")}
let options=MTLCompileOptions();options.fastMathEnabled=false
let source=try String(contentsOfFile:args[2],encoding:.utf8)+String(contentsOfFile:args[3],encoding:.utf8)
let library=try device.makeLibrary(source:source,options:options)
let mode=ProcessInfo.processInfo.environment["MATRIX_CLOSURE_MODE"] ?? "matrix"
try need(mode=="matrix" || mode=="direct","execution mode")
var pipes:[String:MTLComputePipelineState]=[:]
for name in ["begin_run","closure_gemm","dyn_initialize","dyn_alias","dyn_seed","dyn_reset_birth","dyn_rows","dyn_gather","dyn_publish","dyn_direct","dyn_finish"]{
 pipes[name]=try device.makeComputePipelineState(function:library.makeFunction(name:name)!)
}
func shared<T>(_ a:[T])->MTLBuffer{a.withUnsafeBytes{b in b.isEmpty ? device.makeBuffer(length:4,options:.storageModeShared)! : device.makeBuffer(bytes:b.baseAddress!,length:b.count,options:.storageModeShared)!}}
func priv(_ n:Int)->MTLBuffer{device.makeBuffer(length:max(4,n),options:.storageModePrivate)!}
func capacity(_ old:Int,_ count:Int)->Int{
 if count==0{return 0};var c=max(8,old);while c<count{c*=2};while c>8 && count<=c/4{c/=2};return c
}
struct Model{let weights:MTLBuffer;let threshold:Float}
var models:[Int:Model]=[:]
for spec in libraryDescription["separators"] as! [[String:Any]]{
 let k=spec["arity"] as! Int,theta=(spec["coefficients"] as! [NSNumber]).map{$0.uint16Value}
 try need(theta.count==k+1,"separator shape")
 let v=theta.map{Double(Float16(bitPattern:$0))},threshold=(spec["threshold"] as! NSNumber).doubleValue
 let pos=v.reduce(0,+),neg=k==0 ? pos-1 : pos-(v.dropFirst().min() ?? 0)
 let error=8*Double(Float.ulpOfOne)*v.map{abs($0)}.reduce(0,+)
 try need(v.allSatisfy{$0.isFinite} && threshold.isFinite && (k==0 || v.dropFirst().allSatisfy{$0>0}) && pos-threshold>error && threshold-neg>error,"uncertified separator")
 var w=[UInt16](repeating:0,count:8*(k+1)*8)
 for block in 0..<8{for j in 0...k{w[(block*(k+1)+j)*8+block]=theta[j]}}
 models[k]=Model(weights:shared(w),threshold:Float(threshold))
}
struct Bank{
 var count:Int;let capacity:Int;let rows:MTLBuffer;let x:MTLBuffer;let scores:MTLBuffer;let receipts:MTLBuffer
 init(_ device:MTLDevice,_ k:Int,_ n:Int,_ cap:Int){
  func allocate(_ bytes:Int)->MTLBuffer{device.makeBuffer(length:max(4,bytes),options:.storageModePrivate)!}
  count=n;capacity=cap;rows=allocate(cap*(k+2)*4);x=allocate(((cap+7)/8)*8*(k+1)*2);scores=allocate(cap*4);receipts=allocate(cap*12)
 }
 var bytes:Int{rows.length+x.length+scores.length+receipts.length}
}
var banks:[Int:Bank]=[:],epoch:UInt32=0,factCount=0,factCapacity=0,types:[UInt32]=[]
var state=priv(4),next=priv(4),hidden=priv(4),parent=priv(4),birth=priv(4)
let meta=priv(32),status=device.makeBuffer(length:32,options:.storageModeShared)!
var lastMeta=[UInt32](repeating:0,count:8)
var poisoned=false
func launch(_ cb:MTLCommandBuffer,_ name:String,_ buffers:[MTLBuffer],_ constants:[UInt32],_ count:Int,_ grouped:Bool=false,_ threshold:Float?=nil){
 let e=cb.makeComputeCommandEncoder()!,pipe=pipes[name]!;e.setComputePipelineState(pipe);e.label=name
 for (i,b) in buffers.enumerated(){e.setBuffer(b,offset:0,index:i)}
 constants.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:buffers.count)}
 if var threshold=threshold{e.setBytes(&threshold,length:4,index:buffers.count+1)}
 if grouped{e.dispatchThreadgroups(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
 else{e.dispatchThreads(MTLSize(width:max(1,count),height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,pipe.maxTotalThreadsPerThreadgroup),height:1,depth:1))}
 e.endEncoding()
}
func complete(_ cb:MTLCommandBuffer)throws->Double{
 cb.commit();cb.waitUntilCompleted();if cb.status != .completed{poisoned=true}
 try need(cb.status == .completed,"GPU command failure; recreate runtime")
 return cb.gpuEndTime-cb.gpuStartTime
}
struct BankPatch{let arity:Int;let old:Int;let count:Int;let updates:[UInt32]}
func apply(_ words:[UInt32])throws->[String:Any]{
 var at=1
 func take()throws->UInt32{try need(at<words.count,"truncated patch");let v=words[at];at+=1;return v}
 let expected=try take(),newEpoch=try take(),newCount=Int(try take()),newTypes=Int(try take()),mergeCount=Int(try take()),seedCount=Int(try take()),bankCount=Int(try take())
 try need(expected==epoch && epoch<UInt32.max && newEpoch==epoch+1,"stale event epoch")
 try need(newCount>=factCount && newTypes==newCount-factCount,"fact growth")
 var nextTypes=types
 for _ in 0..<newTypes{let t=try take();try need(t<UInt32((libraryDescription["predicates"] as! [String]).count),"fact predicate type");nextTypes.append(t)}
 var merges:[UInt32]=[],seeds:[UInt32]=[],patches:[BankPatch]=[],seenBanks=Set<Int>()
 for _ in 0..<mergeCount{
  let a=try take(),b=try take();try need(a<UInt32(newCount) && b<UInt32(newCount) && nextTypes[Int(a)]==nextTypes[Int(b)],"cross-predicate alias")
  merges += [a,b]
 }
 for _ in 0..<seedCount{
  let id=try take(),present=try take(),hide=try take();try need(id<UInt32(newCount) && present==1 && hide<=1,"external fact seed");seeds += [id,present,hide]
 }
 for _ in 0..<bankCount{
  let k=Int(try take()),old=Int(try take()),n=Int(try take()),u=Int(try take())
  try need(models[k] != nil && seenBanks.insert(k).inserted && (banks[k]?.count ?? 0)==old,"bank version or arity")
  var updates:[UInt32]=[],slots=Set<UInt32>()
  for _ in 0..<u{
   let slot=try take(),generation=try take(),target=try take()
   try need(slot<UInt32(n) && slots.insert(slot).inserted && generation==newEpoch && target<UInt32(newCount),"stale context slot or target")
   updates += [slot,generation,target]
   for _ in 0..<k{let dep=try take();try need(dep<UInt32(newCount),"context dependency");updates.append(dep)}
  }
  if n>old{try need((old..<n).allSatisfy{slots.contains(UInt32($0))},"uninitialized grown context")}
  patches.append(BankPatch(arity:k,old:old,count:n,updates:updates))
 }
 try need(at==words.count,"extra patch payload")
 // Every input check precedes mutation. Control metadata never contains truth.
 let cb=queue.makeCommandBuffer()!;var resized=0,cleared=0,written=0
 let newCapacity=capacity(factCapacity,newCount)
 if newCapacity != factCapacity{
  let ns=priv(newCapacity*4),nn=priv(newCapacity*4),nh=priv(newCapacity*4),np=priv(newCapacity*4),nb=priv(newCapacity*4)
  let b=cb.makeBlitCommandEncoder()!
  if factCount>0{for (from,to) in [(state,ns),(hidden,nh),(parent,np)]{b.copy(from:from,sourceOffset:0,to:to,destinationOffset:0,size:factCount*4)}}
  b.endEncoding();state=ns;next=nn;hidden=nh;parent=np;birth=nb;factCapacity=newCapacity;resized+=1
 }
 if newCount>factCount{launch(cb,"dyn_initialize",[state,hidden,parent,birth],[UInt32(factCount),UInt32(newCount)],newCount-factCount)}
 if mergeCount>0{launch(cb,"dyn_alias",[state,hidden,parent,shared(merges)],[UInt32(mergeCount)],1)}
 if seedCount>0{launch(cb,"dyn_seed",[state,hidden,parent,shared(seeds)],[UInt32(seedCount)],seedCount)}
 for change in patches{
  let k=change.arity,n=change.count
  if n==0{cleared+=change.old;banks.removeValue(forKey:k);continue}
  let old=banks[k],cap=capacity(old?.capacity ?? 0,n)
  var bank:Bank
  if cap != old?.capacity{
   bank=Bank(device,k,n,cap);resized+=1
   let b=cb.makeBlitCommandEncoder()!
   for buffer in [bank.rows,bank.x,bank.scores,bank.receipts]{b.fill(buffer:buffer,range:0..<buffer.length,value:0)}
   if let old=old{let keep=min(old.count,n);if keep>0{b.copy(from:old.rows,sourceOffset:0,to:bank.rows,destinationOffset:0,size:keep*(k+2)*4)}}
   b.endEncoding()
  }else{
   bank=old!;bank.count=n
   if n<change.old{
    let b=cb.makeBlitCommandEncoder()!
    for (buffer,stride) in [(bank.rows,(k+2)*4),(bank.x,(k+1)*2),(bank.scores,4),(bank.receipts,12)]{b.fill(buffer:buffer,range:n*stride..<change.old*stride,value:0)}
    b.endEncoding()
   }
  }
  cleared+=max(0,change.old-n);written+=change.updates.count/(k+3)
  if !change.updates.isEmpty{launch(cb,"dyn_rows",[shared(change.updates),bank.rows],[UInt32(change.updates.count/(k+3)),UInt32(k+2)],change.updates.count/(k+3))}
  banks[k]=bank
 }
 launch(cb,"dyn_reset_birth",[state,parent,birth],[UInt32(newCount)],newCount)
 let b=cb.makeBlitCommandEncoder()!,reset=shared([UInt32(1),0,0,0,0,newEpoch,0,0])
 b.copy(from:reset,sourceOffset:0,to:meta,destinationOffset:0,size:32);b.endEncoding()
 let seconds=try complete(cb)
 epoch=newEpoch;factCount=newCount;types=nextTypes;lastMeta=[1,0,0,0,0,epoch,0,0]
 return ["gpu_seconds":seconds,"slots_cleared":cleared,"slots_written":written,"buffers_resized":resized]
}
func run(_ rounds:Int?)throws->[String:Any]{
 let started=CFAbsoluteTimeGetCurrent();let oldRuns=lastMeta[2],oldFacts=lastMeta[4]
 var seconds=0.0,batches=0,remaining=rounds ?? Int.max
 // An explicit new Run after a quiet boundary is still a legal quiet Run.
 let wake=queue.makeCommandBuffer()!,b=wake.makeBlitCommandEncoder()!,one=shared([UInt32(1)])
 b.copy(from:one,sourceOffset:0,to:meta,destinationOffset:0,size:4);b.endEncoding();seconds+=try complete(wake)
 repeat{
  let cb=queue.makeCommandBuffer()!,n=min(8,remaining)
  for _ in 0..<n{
   launch(cb,"begin_run",[state,next,meta],[UInt32(factCount)],factCount)
   for k in banks.keys.sorted(){
    let bank=banks[k]!,c=[UInt32(bank.count),UInt32(k),UInt32(8*(k+1)),UInt32((bank.count+7)/8)]
    if mode=="matrix"{
     launch(cb,"dyn_gather",[state,hidden,parent,bank.rows,bank.x,bank.receipts,meta],c,bank.count)
     launch(cb,"closure_gemm",[bank.x,models[k]!.weights,bank.scores,meta],c,(Int(c[3])+7)/8,true)
     launch(cb,"dyn_publish",[bank.scores,bank.rows,bank.receipts,parent,hidden,next,birth,meta],c,bank.count,false,models[k]!.threshold)
    }else{launch(cb,"dyn_direct",[state,hidden,parent,bank.rows,next,birth,meta],c,bank.count)}
   }
   launch(cb,"dyn_finish",[meta],[UInt32(factCount)],1);swap(&state,&next)
  }
  let b=cb.makeBlitCommandEncoder()!;b.copy(from:meta,sourceOffset:0,to:status,destinationOffset:0,size:32);b.endEncoding()
  seconds+=try complete(cb);batches+=1;remaining-=n
  lastMeta=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:8),count:8))
  try need(lastMeta[3]==0,"finite closure bound failed")
 }while lastMeta[0] != 0 && remaining>0
 return ["gpu_seconds":seconds,"control_seconds":CFAbsoluteTimeGetCurrent()-started,"batches":batches,
         "runs_in_epoch":lastMeta[2],"runs_executed":lastMeta[2]-oldRuns,"updated":lastMeta[1] != 0,
         "any_updated":lastMeta[4]>oldFacts,"new_facts":lastMeta[4]-oldFacts,"new_facts_in_epoch":lastMeta[4],"stale_receipts_rejected":lastMeta[7]]
}
func snapshot()throws->Data{
 let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!,data=device.makeBuffer(length:max(4,factCount*16),options:.storageModeShared)!
 if factCount>0{for (i,buffer) in [state,hidden,parent,birth].enumerated(){b.copy(from:buffer,sourceOffset:0,to:data,destinationOffset:i*factCount*4,size:factCount*4)}}
 b.endEncoding();_ = try complete(cb);return Data(bytes:data.contents(),count:factCount*16)
}
let input=FileHandle.standardInput,output=FileHandle.standardOutput
func read(_ n:Int)throws->Data{var d=Data();while d.count<n{guard let part=try input.read(upToCount:n-d.count),!part.isEmpty else{throw NSError(domain:"truncated command",code:1)};d+=part};return d}
func respond(_ info:[String:Any],_ payload:Data=Data())throws{
 let json=try JSONSerialization.data(withJSONObject:info,options:[.sortedKeys])
 let sizes=[UInt32(json.count),UInt32(payload.count)]
 try sizes.withUnsafeBytes{try output.write(contentsOf:Data($0))};try output.write(contentsOf:json);try output.write(contentsOf:payload)
}
try output.write(contentsOf:Data("MCD2".utf8))
while true{
 let size=Int(try read(4).withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)});if size==0{break}
 let data=try read(size);try need(size%4==0,"word packet")
 let words=data.withUnsafeBytes{r in (0..<size/4).map{r.loadUnaligned(fromByteOffset:$0*4,as:UInt32.self)}}
 do{
  var payload=Data()
  var info=try autoreleasepool{()->[String:Any] in
   try need(!words.isEmpty,"empty command")
   try need(!poisoned,"GPU state unavailable; recreate runtime")
   if words[0]==1{return try apply(words)}
   try need(epoch>0,"initialize with an event batch first")
   try need(words.count>=2 && words[1]==epoch,"stale command epoch")
   if words[0]==2{try need(words.count==3 && words[2]>0,"Run count");return try run(Int(words[2]))}
   if words[0]==3{try need(words.count==2,"Saturate shape");return try run(nil)}
   if words[0]==4{try need(words.count==2,"snapshot shape");payload=try snapshot();return [:]}
   throw NSError(domain:"unknown command",code:1)
  }
  info["ok"]=true;info["epoch"]=epoch;info["fact_count"]=factCount;info["mode"]=mode
  info["live_contexts"]=banks.values.map{$0.count}.reduce(0,+)
  info["context_reserved_bytes"]=banks.values.map{$0.bytes}.reduce(0,+)
  info["context_capacity"]=banks.values.map{$0.capacity}.reduce(0,+)
  info["fact_capacity"]=factCapacity
  info["device_allocated_bytes"]=device.currentAllocatedSize
  try respond(info,payload)
 }catch{try respond(["ok":false,"epoch":epoch,"error":String(describing:error)])}
}
