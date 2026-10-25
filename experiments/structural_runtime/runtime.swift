// Persistent shared-coordinate/condition-column matrix service. Binary v2 sends
// each current coordinate once. No changes to the trained scores or threshold.
import Foundation
import Metal
func require(_ b:Bool,_ s:String)throws{if !b{throw NSError(domain:s,code:1)}}
let args=CommandLine.arguments
try require(args.count==3,"runtime kernel.metal model.json")
let lib=try JSONSerialization.jsonObject(with:Data(contentsOf:URL(fileURLWithPath:args[2]))) as! [String:Any]
let models=lib["models"] as! [[String:Any]]
let mode=ProcessInfo.processInfo.environment["STRUCTURAL_MATRIX_LAYOUT"] ?? "single"
try require(["single","shared","resident"].contains(mode),"matrix layout")
guard let device=MTLCreateSystemDefaultDevice(),let queue=device.makeCommandQueue()else{fatalError("no Metal")}
let options=MTLCompileOptions();options.fastMathEnabled=false
let code=try device.makeLibrary(source:String(contentsOfFile:args[1],encoding:.utf8),options:options)
let encode=try device.makeComputePipelineState(function:code.makeFunction(name:"encode_spine")!)
let mma=try device.makeComputePipelineState(function:code.makeFunction(name:"shared_condition_mma")!)
func buffer<T>(_ values:[T])->MTLBuffer{values.withUnsafeBytes{r in
    if r.isEmpty{return device.makeBuffer(length:8,options:.storageModeShared)!}
    return device.makeBuffer(bytes:r.baseAddress!,length:r.count,options:.storageModeShared)!
}}
let rawWeights=models.map{$0["weights"] as! [UInt16]}
try require(rawWeights.allSatisfy{$0.count==256},"model width")
var weights:[UInt16]=[]
for k in 0..<256{for w in rawWeights{weights.append(w[k])}}
let bias=models.map{($0["bias"] as! NSNumber).floatValue}
let wb=buffer(weights),bb=buffer(bias)
let input=FileHandle.standardInput,output=FileHandle.standardOutput
func read(_ n:Int)throws->Data{var d=Data();while d.count<n{guard let p=try input.read(upToCount:n-d.count),!p.isEmpty else{throw NSError(domain:"truncated binary request",code:1)};d.append(p)};return d}
func launch(_ cb:MTLCommandBuffer,_ p:MTLComputePipelineState,_ bs:[MTLBuffer],_ cfg:[UInt32],_ n:Int,_ grouped:Bool=false){
    if n==0{return};let e=cb.makeComputeCommandEncoder()!;e.setComputePipelineState(p)
    for (i,b) in bs.enumerated(){e.setBuffer(b,offset:0,index:i)}
    cfg.withUnsafeBytes{e.setBytes($0.baseAddress!,length:$0.count,index:bs.count)}
    if grouped{e.dispatchThreadgroups(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:32,height:1,depth:1))}
    else{e.dispatchThreads(MTLSize(width:n,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(256,p.maxTotalThreadsPerThreadgroup),height:1,depth:1))};e.endEncoding()
}
struct Block {let rows:[Int];let columns:[Int];var tiles:Int{((rows.count+7)/8)*((columns.count+7)/8)};var cells:Int{rows.count*columns.count}}
struct Pair:Hashable{let row:Int;let column:Int}
if mode=="resident"{try residentLoop();exit(0)}
try output.write(contentsOf:Data("RDY2".utf8))
while true{
    let head=try read(4);let size=head.withUnsafeBytes{$0.loadUnaligned(as:UInt32.self)};if size==0{break}
    let data=try read(Int(size));var pos=0
    func word()->UInt32{defer{pos+=4};return data.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:pos,as:UInt32.self)}}
    func long()->UInt64{defer{pos+=8};return data.withUnsafeBytes{$0.loadUnaligned(fromByteOffset:pos,as:UInt64.self)}}
    let n=Int(word()),rowCount=Int(word());var keys:[UInt64]=[],bags:[UInt32]=[],sorts:[UInt64]=[]
    for _ in 0..<rowCount{
        let count=Int(word());try require(count>0,"coordinate needs sort token")
        let ts=(0..<count).map{_ in long()};var divisor=1;while divisor*divisor<count{divisor*=2}
        bags += [UInt32(keys.count),UInt32(count),UInt32(divisor)];keys+=ts;sorts.append(ts[0])
    }
    var requests:[Pair]=[],demand=[Set<Int>](repeating:[],count:rowCount)
    for _ in 0..<n{
        let row=Int(word()),m=Int(word());try require(row<rowCount && m<models.count,"request bounds")
        requests.append(Pair(row:row,column:m));demand[row].insert(m)
    }
    try require(pos==data.count,"trailing request")
    var bySort:[UInt64:[Int]]=[:]
    for row in 0..<rowCount where !demand[row].isEmpty{bySort[sorts[row],default:[]].append(row)}
    var blocks:[Block]=[],singleTiles=0
    for sort in bySort.keys.sorted(){
        let rs=bySort[sort]!,cs=Set(rs.flatMap{demand[$0]}).sorted()
        let single=cs.map{c in Block(rows:rs.filter{demand[$0].contains(c)},columns:[c])}
        singleTiles+=single.reduce(0){$0+$1.tiles}
        if mode=="single"{blocks+=single;continue}
        // Three exact, finite layout alternatives. Compare actual MMA tile
        // count, then logical output cells; no learned cost or density budget.
        var sameDemand:[[Int]:[Int]]=[:]
        for row in rs{sameDemand[demand[row].sorted(),default:[]].append(row)}
        let exact=sameDemand.keys.sorted{$0.lexicographicallyPrecedes($1)}.map{Block(rows:sameDemand[$0]!,columns:$0)}
        let bank=[Block(rows:rs,columns:cs)]
        let choices=[single,exact,bank]
        let best=choices.enumerated().min{a,b in
            let ta=a.element.reduce(0){$0+$1.tiles},tb=b.element.reduce(0){$0+$1.tiles}
            if ta != tb{return ta<tb}
            let ca=a.element.reduce(0){$0+$1.cells},cb=b.element.reduce(0){$0+$1.cells}
            if ca != cb{return ca<cb};return a.offset<b.offset
        }!.element
        blocks+=best
    }
    var desc:[UInt32]=[],rowIDs:[UInt32]=[],colIDs:[UInt32]=[],route:[Pair:Int]=[:]
    var tiles=0,cells=0,multi=0,maxRows=0,maxCols=0
    for b in blocks{
        let rowStart=rowIDs.count,colStart=colIDs.count
        rowIDs+=b.rows.map(UInt32.init);colIDs+=b.columns.map(UInt32.init)
        for (i,r) in b.rows.enumerated(){for (j,c) in b.columns.enumerated() where demand[r].contains(c){
            let p=Pair(row:r,column:c);try require(route[p]==nil,"overlapping requested matrix cells")
            route[p]=cells+i*b.columns.count+j
            if b.columns.count>1{multi+=1}
        }}
        tiles+=b.tiles;desc += [b.rows.count,b.columns.count,rowStart,colStart,cells,tiles,models.count].map(UInt32.init)
        cells+=b.cells;maxRows=max(maxRows,b.rows.count);maxCols=max(maxCols,b.columns.count)
    }
    try require(route.count==demand.reduce(0){$0+$1.count},"missing requested matrix cells")
    let kb=buffer(keys),bagsb=buffer(bags),db=buffer(desc),rb=buffer(rowIDs),cbm=buffer(colIDs)
    let coords=device.makeBuffer(length:max(8,rowCount*256*2),options:.storageModePrivate)!
    let result=device.makeBuffer(length:max(8,cells*4),options:.storageModeShared)!
    let cb=queue.makeCommandBuffer()!
    launch(cb,encode,[kb,bagsb,coords],[UInt32(rowCount),256],rowCount)
    launch(cb,mma,[db,coords,wb,rb,cbm,bb,result],[UInt32(blocks.count),UInt32(models.count)],tiles,true)
    cb.commit();cb.waitUntilCompleted();try require(cb.status == .completed,"GPU execution failed")
    let floats=result.contents().bindMemory(to:Float.self,capacity:cells)
    let scores=requests.map{floats[route[$0]!]}
    var answer=Data();var count=UInt32(n),dt=cb.gpuEndTime-cb.gpuStartTime
    withUnsafeBytes(of:&count){answer.append(contentsOf:$0)};withUnsafeBytes(of:&dt){answer.append(contentsOf:$0)}
    let metrics=[n,rowCount,route.count,cells,tiles*64,blocks.count,multi,maxCols,maxRows,singleTiles*64].map(UInt64.init)
    metrics.withUnsafeBytes{answer.append(contentsOf:$0)}
    scores.withUnsafeBytes{answer.append(contentsOf:$0)};try output.write(contentsOf:answer)
}
