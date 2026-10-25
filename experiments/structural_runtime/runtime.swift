// Persistent binary service. Weights upload once; requests contain only unseen
// structural token signatures. Reuses the tested structural MMA kernel source.
import Foundation
import Metal
func require(_ b:Bool,_ s:String)throws{if !b{throw NSError(domain:s,code:1)}}
let args=CommandLine.arguments
try require(args.count==3,"runtime kernel.metal model.json")
let lib=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:args[2]))) as! [String:Any]
let models=lib["models"] as! [[String:Any]]
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{fatalError("no Metal")}
let options=MTLCompileOptions();options.fastMathEnabled=false
let code=try device.makeLibrary(source:String(contentsOfFile:args[1],encoding:.utf8),options:options)
let encode=try device.makeComputePipelineState(function:code.makeFunction(name:"encode_spine")!)
let mma=try device.makeComputePipelineState(function:code.makeFunction(name:"grouped_mma")!)
let fill=try device.makeComputePipelineState(function:code.makeFunction(name:"fill_prior")!)
func buffer<T>(_ values:[T])->MTLBuffer{values.withUnsafeBytes{r in
    if r.isEmpty{return device.makeBuffer(length:8,options:.storageModeShared)!}
    return device.makeBuffer(bytes:r.baseAddress!,length:r.count,options:.storageModeShared)!
}}
var weights:[UInt16]=[],bias:[Float]=[]
for m in models{
    let w=m["weights"] as! [UInt16];try require(w.count==256,"model width")
    for v in w{weights.append(v);weights += [UInt16](repeating:0,count:7)}
    bias.append((m["bias"] as! NSNumber).floatValue)
}
let wb=buffer(weights),bb=buffer(bias),map=buffer([UInt32](repeating:0,count:models.count))
let input=FileHandle.standardInput,output=FileHandle.standardOutput
func read(_ n:Int)throws->Data{var d=Data();while d.count<n{guard let p=try input.read(upToCount:n-d.count),!p.isEmpty else{throw NSError(domain:"truncated binary request",code:1)};d.append(p)};return d}
func launch(_ cb:MTLCommandBuffer,_ p:MTLComputePipelineState,_ bs:[MTLBuffer],_ cfg:[UInt32],_ n:Int,_ grouped:Bool=false){
    if n==0{return};let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(p)
    for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
    cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
    if grouped{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
    else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,p.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
}
try output.write(contentsOf:Data("RDY1".utf8))
while true{
    let head=try read(4);let size=head.withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)};if size==0{break}
    let data=try read(Int(size));var pos=0
    func word()->UInt32{defer{pos+=4};return data.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:pos,as:UInt32.self)}}
    func long()->UInt64{defer{pos+=8};return data.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:pos,as:UInt64.self)}}
    let n=Int(word());var requests=[[(Int,[UInt64])]](repeating:[],count:models.count)
    for i in 0..<n{let m=Int(word()),count=Int(word());try require(m<models.count,"bad model");requests[m].append((i,(0..<count).map{_ in long()}))}
    try require(pos==data.count,"trailing request")
    var keys:[UInt64]=[],bags:[UInt32]=[],desc:[UInt32]=[],route:[Int]=[];var rows=0,tiles=0
    for (m,rs) in requests.enumerated() where !rs.isEmpty{
        let start=rows;for (i,ts) in rs{
            var divisor=1;while divisor*divisor<ts.count{divisor*=2}
            bags += [UInt32(keys.count),UInt32(ts.count),UInt32(divisor)];keys+=ts;route.append(i);rows+=1
        }
        tiles+=(rs.count+7)/8
        desc += [rs.count,1,1,start,m*256*8,m,m,start,tiles,rows,8,256].map(UInt32.init)
    }
    let kb=buffer(keys),bagsb=buffer(bags),db=buffer(desc)
    let coords=device.makeBuffer(length:max(8,rows*256*2),options:.storageModePrivate)!
    let result=device.makeBuffer(length:max(8,rows*4),options:.storageModeShared)!
    let cb=queue.makeCommandBuffer()!;let jobs=desc.count/12
    launch(cb,encode,[kb,bagsb,coords],[UInt32(rows),256],rows)
    launch(cb,fill,[db,bb,result],[UInt32(jobs),UInt32(rows)],rows)
    launch(cb,mma,[db,coords,wb,map,bb,result],[UInt32(jobs)],tiles,true)
    cb.commit();cb.waitUntilCompleted();try require(cb.status == .completed,"GPU execution failed")
    let floats=result.contents().bindMemory(to:Float.self,capacity:rows)
    var scores=[Float](repeating:0,count:n);for i in 0..<rows{scores[route[i]]=floats[i]}
    var answer=Data();var count=UInt32(n),dt=cb.gpuEndTime-cb.gpuStartTime
    withUnsafeBytes(of:&count){answer.append(contentsOf:$0)};withUnsafeBytes(of:&dt){answer.append(contentsOf:$0)}
    scores.withUnsafeBytes{answer.append(contentsOf:$0)};try output.write(contentsOf:answer)
}
