import Foundation
import Metal
func need(_ ok:Bool,_ message:String)throws{if !ok{throw NSError(domain:message,code:1)}}
let a=CommandLine.arguments
try need(a.count==2,"worker program.json")
var program=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:a[1]))) as! [String:Any]
try need(program["schema"] as? String == "egglog-device-storage/v1","schema")
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue() else{fatalError("Metal unavailable")}
let opts=MTLCompileOptions();opts.fastMathEnabled=false
let lib=try device.makeLibrary(source:program["shader"] as! String,options:opts)
let initPipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"initialize")!)
let stepPipelines=try (program["kernel_names"] as! [String]).map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
let capacity=program["capacity"] as! Int,stride=program["stride"] as! Int,ids=program["id_capacity"] as! Int
let matchCapacity=program["match_capacity"] as! Int,matchStride=program["match_stride"] as! Int,nrules=program["rules"] as! Int
func privateBuffer(_ bytes:Int)->MTLBuffer{device.makeBuffer(length:max(8,bytes),options:.storageModePrivate)!}
func shared<T>(_ x:[T])->MTLBuffer{x.withUnsafeBytes{b in b.isEmpty ? device.makeBuffer(length:8,options:.storageModeShared)! : device.makeBuffer(bytes:b.baseAddress!,length:b.count,options:.storageModeShared)!}}
let setWords=program["set_words"] as! Int
let rows=privateBuffer((capacity*stride+setWords+(program["index_words"] as! Int))*8),old=privateBuffer(capacity*stride*8),parent=privateBuffer(ids*4),factParent=privateBuffer(ids*4)
let meta=privateBuffer(36),matches=privateBuffer(matchCapacity*matchStride*8),watermarks=privateBuffer(nrules*4)
let instructions=privateBuffer(max(8,(program["action_code"] as! [NSNumber]).count*8)),registers=privateBuffer((program["register_count"] as! Int)*8)
let queryHeads=privateBuffer((program["query_head_words"] as! Int)*4),queryLinks=privateBuffer((program["query_link_words"] as! Int)*4)
let initialMeta=(program["meta"] as! [NSNumber]).map{$0.uint32Value}
func initializeState() throws {
let initial=(program["initial_rows"] as! [NSNumber]).map{$0.int64Value}
let initCmd=queue.makeCommandBuffer()!,blit=initCmd.makeBlitCommandEncoder()!
for b in [rows,old,parent,factParent,meta,matches,watermarks,instructions,registers]{blit.fill(buffer:b,range:0..<b.length,value:0)}
let actionStage=shared((program["action_code"] as! [NSNumber]).map{$0.int64Value})
blit.copy(from:actionStage,sourceOffset:0,to:instructions,destinationOffset:0,size:actionStage.length)
let staging=shared(initial),stagingMeta=shared(initialMeta),stagingSets=shared((program["initial_sets"] as! [NSNumber]).map{$0.int64Value})
blit.copy(from:stagingSets,sourceOffset:0,to:rows,destinationOffset:capacity*stride*8,size:stagingSets.length)
if !initial.isEmpty{blit.copy(from:staging,sourceOffset:0,to:rows,destinationOffset:0,size:initial.count*8)}
blit.copy(from:stagingMeta,sourceOffset:0,to:meta,destinationOffset:0,size:36);blit.endEncoding()
let encoder=initCmd.makeComputeCommandEncoder()!;encoder.setComputePipelineState(initPipeline);encoder.setBuffer(parent,offset:0,index:0);encoder.setBuffer(factParent,offset:0,index:1)
encoder.dispatchThreads(MTLSize(width:ids,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,initPipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1));encoder.endEncoding();initCmd.commit();initCmd.waitUntilCompleted()
try need(initCmd.status == .completed,"initialize failed")
}
try initializeState()
program.removeValue(forKey:"initial_rows");program.removeValue(forKey:"initial_sets");program.removeValue(forKey:"action_code")
let input=FileHandle.standardInput,output=FileHandle.standardOutput
func read(_ n:Int)throws->Data{var d=Data();while d.count<n{guard let p=try input.read(upToCount:n-d.count),!p.isEmpty else{throw NSError(domain:"truncated command",code:1)};d+=p};return d}
func send(_ x:[UInt32])throws{try x.withUnsafeBytes{try output.write(contentsOf:Data($0))}}
var hostMeta=initialMeta
try output.write(contentsOf:Data("DVB1".utf8))
while true{
 let d=try read(8),words=d.withUnsafeBytes{Array($0.bindMemory(to:UInt32.self))};let op=words[0],arg=words[1]
 if op==0{break}
 if op==1{
  try need(Int(arg)<(program["sets"] as! [String]).count,"ruleset")
  let cb=queue.makeCommandBuffer()!,e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(stepPipelines[Int(arg)])
  for (i,b) in [rows,old,parent,factParent,meta,matches,watermarks].enumerated(){e.setBuffer(b,offset:0,index:i)}
  e.setBuffer(instructions,offset:0,index:8);e.setBuffer(registers,offset:0,index:9);e.setBuffer(queryHeads,offset:0,index:10);e.setBuffer(queryLinks,offset:0,index:11)
  var set=arg;withUnsafeBytes(of:&set){e.setBytes($0.baseAddress!,length:4,index:7)}
  e.dispatchThreads(MTLSize(width:1,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:1,height:1,depth:1));e.endEncoding()
  let status=device.makeBuffer(length:36,options:.storageModeShared)!,b=cb.makeBlitCommandEncoder()!;b.copy(from:meta,sourceOffset:0,to:status,destinationOffset:0,size:36);b.endEncoding()
  cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"device command failed")
  hostMeta=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:9),count:9));try send(hostMeta)
 }else if op==2{
  // Explicit checkpoint only. No table contents are read by the control loop.
  let cb=queue.makeCommandBuffer()!,b=cb.makeBlitCommandEncoder()!
  let setSnapshot=device.makeBuffer(length:setWords*8,options:.storageModeShared)!
  b.copy(from:rows,sourceOffset:capacity*stride*8,to:setSnapshot,destinationOffset:0,size:setWords*8)
  let bytes=Int(hostMeta[0])*stride*8,psize=Int(hostMeta[1])*4,fsize=Int(hostMeta[8])*4
  let r=device.makeBuffer(length:max(8,bytes),options:.storageModeShared)!,p=device.makeBuffer(length:max(8,psize),options:.storageModeShared)!,f=device.makeBuffer(length:max(8,fsize),options:.storageModeShared)!
  if bytes>0{b.copy(from:rows,sourceOffset:0,to:r,destinationOffset:0,size:bytes)}
  b.copy(from:parent,sourceOffset:0,to:p,destinationOffset:0,size:psize);b.copy(from:factParent,sourceOffset:0,to:f,destinationOffset:0,size:fsize);b.endEncoding();cb.commit();cb.waitUntilCompleted();try need(cb.status == .completed,"snapshot failed")
  try send(hostMeta);try output.write(contentsOf:Data(bytes:r.contents(),count:bytes));try output.write(contentsOf:Data(bytes:p.contents(),count:psize));try output.write(contentsOf:Data(bytes:f.contents(),count:fsize));try output.write(contentsOf:Data(bytes:setSnapshot.contents(),count:setWords*8))
 }else{throw NSError(domain:"unknown control command",code:1)}
}
