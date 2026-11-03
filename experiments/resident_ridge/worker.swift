// Resident opaque-input Cell frames + unchanged ridge operator. One command
// buffer advances several certified macros; no CPU state read between them.
import Foundation
import Metal
func require(_ ok:Bool,_ why:String)throws{if !ok{throw NSError(domain:why,code:1)}}
let args=CommandLine.arguments
try require(args.count==4,"worker program.json kernel.metal learned|negative|positive")
let p=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:args[1]))) as! [String:Any]
try require(p["schema"] as? String=="resident-ridge/v1","schema")
let sizes=p["sizes"] as! [UInt32],offsets=p["state_offsets"] as! [UInt32]
let slots=p["slots"] as! Int,parameters=(p["parameter_names"] as! [String]).count
let ts=p["transitions"] as! [[String:Any]],features=p["features"] as! [[UInt32]],edgeCount=ts.count
let fresh=p["fresh_base"] as! UInt32,stride=p["fresh_stride"] as! UInt32
let codeValues=(p["codes"] as! [[Int32]]).flatMap{$0}
let ridge=p["ridge"] as! [String:Any],weights=ridge["weights"] as! [[UInt16]]
var bias=(ridge["bias"] as! [NSNumber]).map{$0.floatValue}
try require(weights.count==256 && weights.allSatisfy{$0.count==edgeCount},"ridge dimensions")
try require(["learned","negative","positive"].contains(args[3]),"weight mode")
var packed:[UInt16]=[]
for row in weights{packed += (args[3]=="learned" ? row:[UInt16](repeating:0,count:edgeCount));packed += [UInt16](repeating:0,count:((edgeCount+7)/8)*8-edgeCount)}
if args[3] != "learned"{bias=[Float](repeating:args[3]=="negative" ? -1:1,count:edgeCount)}
var edges:[UInt32]=[],edgeParams:[UInt32]=[],partition:[UInt32]=[],mapping:[Int32]=[],next=[Int32](repeating:-1,count:sizes.count)
for (i,t) in ts.enumerated(){
    let source=t["source"] as! UInt32,target=t["target"] as! UInt32,ps=t["parameters"] as! [UInt32]
    edges += [source,target,UInt32(edgeParams.count),UInt32(ps.count),UInt32(partition.count),UInt32(mapping.count)]
    edgeParams+=ps;partition+=t["partition"] as! [UInt32];mapping+=t["mapping"] as! [Int32]
    try require(next[Int(source)] == -1,"ambiguous single-page transition");next[Int(source)]=Int32(i)
}
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{fatalError("Metal unavailable")}
func buffer<T>(_ data:[T])->MTLBuffer{data.withUnsafeBytes{r in r.count==0 ? device.makeBuffer(length:4,options:.storageModeShared)! : device.makeBuffer(bytes:r.baseAddress!,length:r.count,options:.storageModeShared)!}}
let source="#define SLOTS \(slots)\n"+(try String(contentsOfFile:args[2],encoding:.utf8))
let opts=MTLCompileOptions();opts.fastMathEnabled=false
let library=try device.makeLibrary(source:source,options:opts)
var pipes:[String:MTLComputePipelineState]=[:]
for name in ["initialize","add_ready","predict","advance","update_coordinates","readback","audit_scores"]{pipes[name]=try device.makeComputePipelineState(function:library.makeFunction(name:name)!)}
let wb=buffer(packed),bb=buffer(bias),eb=buffer(edges),epb=buffer(edgeParams),pb=buffer(partition),mb=buffer(mapping),sz=buffer(sizes),ob=buffer(offsets),nb=buffer(next),fb=buffer(features.flatMap{$0}),codes=buffer(codeValues)
var rows=0,cfg:[UInt32]=[],buffers:[String:MTLBuffer]=[:]
var residentBytes=0
func allocate(_ name:String,_ bytes:Int){let b=device.makeBuffer(length:max(4,bytes),options:.storageModePrivate)!;buffers[name]=b;residentBytes+=b.length}
func launch(_ cb:MTLCommandBuffer,_ name:String,_ bs:[MTLBuffer],_ count:Int,_ grouped:Bool=false,_ extra:UInt32?=nil){
    if count==0{return};let e=cb.makeComputeCommandEncoder()!,pipe=pipes[name]!;e.setComputePipelineState(pipe)
    for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
    cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
    if var x=extra{e.setBytes(&x,length:4,index:bs.count+1)}
    if grouped{e.dispatchThreadgroups(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
    else{e.dispatchThreads(MTLSize(width:count,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,pipe.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
}
let input=FileHandle.standardInput,output=FileHandle.standardOutput
func read(_ count:Int)throws->Data{var d=Data();while d.count<count{guard let more=try input.read(upToCount:count-d.count),!more.isEmpty else{throw NSError(domain:"truncated request",code:1)};d.append(more)};return d}
try output.write(contentsOf:Data("RDR1".utf8))
while true{
    let head=try read(4),length=Int(head.withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)});if length==0{break}
    let data=try read(length);try require(length%4==0,"word packet")
    let words=data.withUnsafeBytes{r in (0..<length/4).map{r.loadUnaligned(fromByteOffset:4*$0,as:UInt32.self)}}
    let op=words[0],cb=queue.makeCommandBuffer()!;var copied:MTLBuffer?=nil
    var count=0
    if op==1 {
        rows=Int(words[1]);try require(rows>0 && words.count==2+rows*(parameters+1),"initial context shape")
        try require(UInt64(fresh)+UInt64(rows)*UInt64(stride)<UInt64(UInt32.max),"symbol range exhausted")
        for row in 0..<rows{for i in 0..<parameters{try require(words[2+row*(parameters+1)+i]<fresh,"input/symbol namespace overlap")};try require(words[2+row*(parameters+1)+parameters] < (1<<parameters),"ready mask")}
        buffers=[:];residentBytes=0
        cfg=[UInt32(rows),UInt32(slots),UInt32(parameters),UInt32(sizes.count),UInt32(edgeCount),UInt32(features.count),fresh,stride,UInt32(edgeCount)]
        allocate("meta",rows*6*4);allocate("params",rows*parameters*4);allocate("frame",rows*slots*4)
        allocate("sum",rows*256*4);allocate("x",rows*256*2);allocate("scores",rows*edgeCount*4);allocate("events",rows*edgeCount*4);allocate("stats",rows*8*4)
        let blit=cb.makeBlitCommandEncoder()!;for b in buffers.values{blit.fill(buffer:b,range:0..<b.length,value:0)};blit.endEncoding()
        launch(cb,"initialize",[buffer(Array(words.dropFirst(2))),buffers["meta"]!,buffers["params"]!,buffers["sum"]!,buffers["x"]!,fb,codes],rows)
    } else {
        try require(rows>0,"context not initialized")
        let meta=buffers["meta"]!,frame=buffers["frame"]!,params=buffers["params"]!,sum=buffers["sum"]!,x=buffers["x"]!,scores=buffers["scores"]!,events=buffers["events"]!,stats=buffers["stats"]!
        if op==2 {
            try require(words.count==2 && words[1]<=UInt32(p["sweeps"] as! Int),"macro sweep count")
            for _ in 0..<Int(words[1]) {
                launch(cb,"predict",[x,wb,bb,scores],((rows+7)/8)*((edgeCount+7)/8),true)
                launch(cb,"advance",[meta,frame,params,scores,eb,epb,pb,mb,sz,ob,nb,events,stats],rows)
                launch(cb,"update_coordinates",[meta,sum,x,codes,fb,params],rows)
            }
        } else if op==3 {
            let n=Int(words[1]);try require(words.count==2+n*2,"patch size");var seen=Set<UInt32>()
            for i in 0..<n{let r=words[2+2*i],mask=words[3+2*i];try require(r<UInt32(rows) && mask<(1<<parameters) && seen.insert(r).inserted,"patch bounds/duplicate row")}
            launch(cb,"add_ready",[buffer(Array(words.dropFirst(2))),meta,params,sum,x,fb,codes],n,false,UInt32(n))
        } else if op==4 {
            try require(words.count==1,"readback shape");count=rows*(4+parameters+slots+edgeCount+8)
            copied=device.makeBuffer(length:count*4,options:.storageModeShared)!
            launch(cb,"readback",[meta,frame,params,events,stats,copied!],rows)
        } else if op==5 {
            try require(words.count==1,"audit shape");count=rows*(256+edgeCount)
            copied=device.makeBuffer(length:count*4,options:.storageModeShared)!
            launch(cb,"predict",[x,wb,bb,scores],((rows+7)/8)*((edgeCount+7)/8),true)
            launch(cb,"audit_scores",[x,scores,copied!],count)
        } else {throw NSError(domain:"unknown command",code:1)}
    }
    cb.commit();cb.waitUntilCompleted();try require(cb.status == .completed,"device execution")
    var result=Data();var n=UInt32(count),dt=cb.gpuEndTime-cb.gpuStartTime,bytes=UInt64(residentBytes)
    withUnsafeBytes(of:&n){result.append(contentsOf:$0)};withUnsafeBytes(of:&dt){result.append(contentsOf:$0)};withUnsafeBytes(of:&bytes){result.append(contentsOf:$0)}
    if let b=copied{result.append(Data(bytes:b.contents(),count:count*4))};try output.write(contentsOf:result)
}
