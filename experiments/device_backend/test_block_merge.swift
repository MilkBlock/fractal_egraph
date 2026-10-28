// GPU primitive test using the real generated worker shader. These synthetic
// UF blocks test merge algebra, not recognition of a Cell or an eggcc benchmark.
import Foundation
import Metal
let program=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:CommandLine.arguments[1]))) as! [String:Any]
let device=MTLCreateSystemDefaultDevice()!,queue=device.makeCommandQueue()!
let options=MTLCompileOptions();options.fastMathEnabled=false
let lib=try device.makeLibrary(source:program["shader"] as! String,options:options)
let pipelines=try ["cu_block_begin","cu_block_union","cu_block_finish"].map{try device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)}
func buffer<T>(_ values:[T])->MTLBuffer{values.withUnsafeBytes{device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)!}}
let cap=program["capacity"] as! Int,stride=program["stride"] as! Int,idcap=program["id_capacity"] as! Int
let words=cap*stride+(program["set_words"] as! Int)+(program["index_words"] as! Int)
let state=(program["index_counter_offset"] as! Int)+5+4*cap
let unionWords=program["union_words"] as! Int
let generation=unionWords-1 // IDCAP + 2 + 2*UNION_CAP
let eqSet=(((program["joint_polyspine"] as! [String:Any])["program"] as! [String:Any])["set_ids"] as! [Int])[2]
var cases:[[String:Any]]=[]
for order in [[0,1],[1,0]]{
 var r=[Int64](repeating:0,count:words);r[state]=1
 let rows=buffer(r),meta=buffer([UInt32(0),5,100,0,0,0,0,1,5]),wm=buffer([UInt32](repeating:0,count:program["rules"] as! Int)),control=buffer([UInt32](repeating:0,count:16)),flat=buffer([UInt32(0)])
 var p=[UInt32](repeating:0,count:idcap+unionWords);for i in 0..<idcap{p[i]=UInt32(i)};p[4]=1;p[generation+1+4]=1
 let parents=buffer(p),locals=[buffer([UInt32(2),1]),buffer([UInt32(3),2])]
 var deltas:[UInt32]=[]
 for which in order+[order[0]]{
  let c=buffer([UInt32](repeating:0,count:16)),cfg:[UInt32]=[UInt32(eqSet),0,5,1,0,2,0,0]
  let cb=queue.makeCommandBuffer()!
  let args=[[meta],[locals[which],parents,meta],[parents,meta,wm,control]]
  for k in 0..<3{let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(pipelines[k]);e.setBuffer(rows,offset:0,index:0);e.setBuffer(c,offset:0,index:1);cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:32,index:2)};for (i,b) in args[k].enumerated(){e.setBuffer(b,offset:0,index:i+3)};e.setBuffer(flat,offset:0,index:28);e.setBuffer(rows,offset:0,index:30);e.dispatchThreads(MTLSize(width:k==0 ? 1:5,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1));e.endEncoding()}
  cb.commit();cb.waitUntilCompleted();precondition(cb.status == .completed,"GPU block join failed")
  deltas.append(c.contents().bindMemory(to:UInt32.self,capacity:16)[13])
 }
 let actual=Array(UnsafeBufferPointer(start:parents.contents().bindMemory(to:UInt32.self,capacity:5),count:5))
 precondition(actual==[0,1,1,1,1],"stale block overwrote a newer union")
 precondition(deltas==[1,1,0],"replaying an equality block added a duplicate union")
 precondition(meta.contents().bindMemory(to:UInt32.self,capacity:9)[7]==3)
 precondition(parents.contents().bindMemory(to:UInt32.self,capacity:idcap+unionWords)[generation+2]==1,"merge-key flag was not propagated")
 cases.append(["order":order,"parents":actual,"committed_union_deltas":deltas])
}
let out:[String:Any]=["scope":"real GPU block-merge kernels, synthetic UF inputs","cases":cases,"passed":true]
print(String(data:try JSONSerialization.data(withJSONObject:out,options:[.prettyPrinted,.sortedKeys]),encoding:.utf8)!)
