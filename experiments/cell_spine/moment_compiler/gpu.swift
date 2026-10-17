// Native Metal/MPS backend: fixed feature circuit, GEMM, then exact distances.
// No network dependencies. Compile with: swiftc -O gpu.swift -o <untracked path>.
import Foundation
import Metal
import MetalPerformanceShaders

struct Node: Decodable { let op: String; let args: [Int]; let width: Int; let bound: Int; let slot: Int; let offset: Int }
struct Circuit: Decodable { let bits: Int; let slots: Int; let tape_width: Int; let nodes: [Node] }
struct Block: Decodable { let width: Int; let wires: [[Int]]; let max_norm: Int }
struct Term: Decodable { let block: Int; let rows: [Int] }
struct Query: Decodable { let wires: [[Int]]; let terms: [Term] }
struct Plan: Decodable { let blocks: [Block]; let queries: [Query]; let guards: [[Int]] }
struct Transition: Decodable { let target: Int; let sources: [Int]; let conditions: [[Int]]; let bindings: [[Int]]; let partition: [Int] }
struct Job: Decodable { let name: String; let plan: Plan; let masks: [[Int]]?; let emits: [[Int]]?; let transitions: [Transition]? }
struct Input: Decodable { let circuit: Circuit; let ports: [[[Float]]]; let jobs: [Job]; let half_inputs: Bool?; let active: [UInt8]?; let skip_inactive: Bool?; let chain_stages: Bool? }
struct Result: Encodable { let name: String; let outputs: [[UInt8]]; let emitted: [[UInt8]]; let gpu_seconds: Double?; let submit_wait_seconds: Double?; let feature_values: [Float]; let transition_status: [UInt8]; let cell_values: [Float]?; let cell_valid: [UInt8]? }
struct Report: Encodable { let device: String; let backend: String; let results: [Result]; let chained: Bool; let program_gpu_seconds: Double?; let program_submit_wait_seconds: Double? }

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw NSError(domain: message, code: 1) }
}

let metalSource = """
#include <metal_stdlib>
using namespace metal;
// instruction = op, offset, width, a_offset, a_width, b_offset, slot
kernel void build(device const float *input [[buffer(0)]], device float *tape [[buffer(1)]],
                  device const uint *ops [[buffer(2)]], device const uchar *live [[buffer(3)]],
                  constant uint4 &cfg [[buffer(4)]], constant uint &op_count [[buffer(5)]],
                  constant uint &skip [[buffer(6)]], uint id [[thread_position_in_grid]]) {
    if (id >= cfg.x) return;
    uint bits=cfg.y, slots=cfg.z, stride=cfg.w, base=id*stride;
    if(skip && !live[id]){for(uint i=0;i<stride;i++)tape[base+i]=0;return;}
    for (uint n=0;n<op_count;n++) {
        uint p=n*7, op=ops[p], out=base+ops[p+1], w=ops[p+2];
        uint a=base+ops[p+3], aw=ops[p+4], b=base+ops[p+5];
        if (op==0) {
            for(uint j=0;j<w;j++) tape[out+j]=input[(id*slots+ops[p+6])*bits+j];
        } else if (op==1) {
            uint k=0;
            for(uint j=0;j<aw;j++) tape[out+k++]=tape[a+j];
            for(uint j=0;j<aw;j++) for(uint l=j+1;l<aw;l++) tape[out+k++]=tape[a+j]*tape[a+l];
        } else if (op==2) {
            for(uint j=0;j<w;j++) tape[out+j]=(tape[a+j]+tape[b+j])*0.5f;
        } else if (op==3) {
            for(uint j=0;j<aw;j++) tape[out+j]=tape[a+j];
            for(uint j=aw;j<w;j++) tape[out+j]=tape[b+j-aw];
        } else {
            uint k=0;
            for(uint j=0;j<aw;j++) tape[out+k++]=tape[a+j]+tape[b+j];
            for(uint j=0;j<aw;j++) for(uint l=j;l<aw;l++) tape[out+k++]=tape[a+j]*tape[a+l]+tape[b+j]*tape[b+l];
        }
    }
}
kernel void project(device const float *tape [[buffer(0)]], device const uint *sources [[buffer(1)]],
                    device float *rows [[buffer(2)]], device float *norms [[buffer(3)]],
                    constant uint4 &cfg [[buffer(4)]], uint id [[thread_position_in_grid]]) {
    if(id>=cfg.x) return;
    uint source=sources[id], width=cfg.y, stride=cfg.z;
    float norm=0;
    for(uint j=0;j<width;j++){float v=tape[source+j];rows[id*stride+j]=v;norm+=v*v;}
    norms[id]=norm;
}
kernel void project_half(device const float *tape [[buffer(0)]], device const uint *sources [[buffer(1)]],
                         device half *rows [[buffer(2)]], device float *norms [[buffer(3)]],
                         constant uint4 &cfg [[buffer(4)]], uint id [[thread_position_in_grid]]) {
    if(id>=cfg.x)return;
    uint source=sources[id],width=cfg.y,stride=cfg.z;float norm=0;
    for(uint j=0;j<width;j++){float v=tape[source+j];rows[id*stride+j]=half(v);norm+=v*v;}
    norms[id]=norm;
}
kernel void zero_distance(device float *distance [[buffer(0)]], constant uint &size [[buffer(1)]],
                          uint id [[thread_position_in_grid]]) { if(id<size)distance[id]=0; }
kernel void accumulate(device const float *gram [[buffer(0)]], device const float *norms [[buffer(1)]],
                       device const uint *map [[buffer(2)]], device float *distance [[buffer(3)]],
                       constant uint2 &cfg [[buffer(4)]], uint id [[thread_position_in_grid]]) {
    uint n=cfg.x;
    if(id>=n*n)return;
    uint a=map[id/n],b=map[id%n];
    distance[id]+=norms[a]+norms[b]-2*gram[a*cfg.y+b];
}
kernel void decide(device const float *distance [[buffer(0)]], device const uchar *mask [[buffer(1)]],
                   device uchar *out [[buffer(2)]], constant uint &size [[buffer(3)]],
                   uint id [[thread_position_in_grid]]) {
    if(id<size)out[id]=uchar(distance[id]==0 && mask[id]!=0);
}
kernel void emit(device const uchar *answers [[buffer(0)]], device const uint *index [[buffer(1)]],
                 device uchar *out [[buffer(2)]], constant uint &size [[buffer(3)]],
                 uint id [[thread_position_in_grid]]) {if(id<size)out[id]=answers[index[id]];}
// Status: 0=no matching live source, 1=valid target, 2=target needs alias Bake.
// Source and target rows are disjoint in a stage; cells are append-only slots.
kernel void transition(device float *codes [[buffer(0)]], device uchar *live [[buffer(1)]],
                       device const uchar *answers [[buffer(2)]], device const uint *ops [[buffer(3)]],
                       device const uint *sources [[buffer(4)]], device const uint *conditions [[buffer(5)]],
                       device const uint *bindings [[buffer(6)]], device const uint *partition [[buffer(7)]],
                       device uchar *status [[buffer(8)]], constant uint3 &cfg [[buffer(9)]],
                       uint id [[thread_position_in_grid]]) {
    if(id>=cfg.x)return;
    uint p=id*8,target=ops[p],so=ops[p+1],sn=ops[p+2],co=ops[p+3],cn=ops[p+4],bo=ops[p+5],arity=ops[p+6],po=ops[p+7];
    uint bits=cfg.y,slots=cfg.z;live[target]=0;status[id]=0;
    for(uint i=0;i<sn;i++)if(!live[sources[so+i]])return;
    for(uint i=0;i<cn;i++)if(!answers[conditions[co+i]])return;
    for(uint i=0;i<slots*bits;i++)codes[target*slots*bits+i]=-1;
    for(uint i=0;i<arity;i++)for(uint j=0;j<bits;j++)
        codes[(target*slots+i)*bits+j]=codes[bindings[bo+i]*bits+j];
    for(uint i=0;i<arity;i++)for(uint j=0;j<i;j++){
        bool equal=true;
        for(uint k=0;k<bits;k++)equal=equal && codes[(target*slots+i)*bits+k]==codes[(target*slots+j)*bits+k];
        if(equal!=(partition[po+i]==partition[po+j])){status[id]=2;return;}
    }
    live[target]=1;status[id]=1;
}
"""

do {
    try require(CommandLine.arguments.count==3, "usage: gpu input.json output.json")
    let input = try JSONDecoder().decode(Input.self, from: Data(contentsOf: URL(fileURLWithPath:CommandLine.arguments[1])))
    guard let device=MTLCreateSystemDefaultDevice(), let queue=device.makeCommandQueue() else { throw NSError(domain:"Metal unavailable",code:1) }
    let options=MTLCompileOptions()
    if #available(macOS 15.0, *) { options.mathMode = .safe } else { options.fastMathEnabled=false }
    let library=try device.makeLibrary(source:metalSource,options:options)
    var pipelines=[String:MTLComputePipelineState]()
    for name in ["build","project","project_half","zero_distance","accumulate","decide","emit","transition"] {
        pipelines[name]=try device.makeComputePipelineState(function:library.makeFunction(name:name)!)
    }
    func buffer<T>(_ values: [T]) -> MTLBuffer {
        return values.withUnsafeBytes { device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)! }
    }
    func empty(_ bytes: Int) -> MTLBuffer { device.makeBuffer(length:max(bytes,4),options:.storageModeShared)! }
    let c=input.circuit, count=input.ports.count, half=input.half_inputs ?? false
    try require(count>0 && input.ports.allSatisfy{$0.count==c.slots && $0.allSatisfy{$0.count==c.bits && $0.allSatisfy{$0 == 1 || $0 == -1}}}, "invalid input code shape")
    let codes=buffer(input.ports.flatMap{$0.flatMap{$0}})
    let initialActive=input.active ?? Array(repeating:UInt8(1),count:count)
    try require(initialActive.count==count && initialActive.allSatisfy{$0<=1},"invalid Cell validity matrix")
    let live=buffer(initialActive)
    var claimed=Set(initialActive.enumerated().filter{$0.element != 0}.map{$0.offset})
    let opIDs=["port":0,"lift2":1,"mean":2,"concat":3,"moments":4]
    let ops=buffer(c.nodes.flatMap { n -> [UInt32] in
        let a=n.args.isEmpty ? nil : c.nodes[n.args[0]], b=n.args.count<2 ? nil : c.nodes[n.args[1]]
        return [opIDs[n.op]!,n.offset,n.width,a?.offset ?? 0,a?.width ?? 0,b?.offset ?? 0,max(n.slot,0)].map{UInt32($0)}
    })
    let chained=input.chain_stages ?? false
    let chain=chained ? queue.makeCommandBuffer() : nil
    var results=[Result](), pending=[() -> Result](), retained=[MTLBuffer]()
    for job in input.jobs {
        try require(job.plan.guards.count==count,"Cell guard count mismatch")
        for (i,expected) in job.plan.guards.enumerated() {
            if initialActive[i]==0 { continue }
            let ports=Array(input.ports[i].prefix(expected.count))
            let observed=ports.map{ports.firstIndex(of:$0)!}
            try require(observed==expected,"Cell input partition invalidates this Bake; specialize before dispatch")
        }
        let tape=empty(count*c.tape_width*4)
        let cmd=chain ?? queue.makeCommandBuffer()!
        var keep=[MTLBuffer]()
        func dispatch(_ name:String,_ bindings:[MTLBuffer],_ constants:[[UInt32]],_ size:Int) {
            let enc=cmd.makeComputeCommandEncoder()!, pipeline=pipelines[name]!
            enc.setComputePipelineState(pipeline)
            for (i,b) in bindings.enumerated(){enc.setBuffer(b,offset:0,index:i)}
            for (i,v) in constants.enumerated(){v.withUnsafeBytes{enc.setBytes($0.baseAddress!,length:$0.count,index:bindings.count+i)}}
            enc.dispatchThreads(MTLSize(width:size,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1))
            enc.endEncoding()
        }
        dispatch("build",[codes,tape,ops,live],[[UInt32(count),UInt32(c.bits),UInt32(c.slots),UInt32(c.tape_width)],[UInt32(c.nodes.count)],[input.skip_inactive == true ? 1 : 0]],count)
        var grams=[MPSMatrix](), norms=[MTLBuffer]()
        for b in job.plan.blocks {
            try require(b.max_norm*4 < 1<<24, "FP32 exactness bound exceeded")
            if half {try require(b.wires.allSatisfy{c.nodes[$0[1]].bound<=2048}, "FP16 input integer exactness bound exceeded")}
            let inputType:MPSDataType=half ? .float16 : .float32
            let n=b.wires.count, rowBytes=MPSMatrixDescriptor.rowBytes(fromColumns:b.width,dataType:inputType)
            let gramBytes=MPSMatrixDescriptor.rowBytes(fromColumns:n,dataType:.float32)
            let rows=empty(n*rowBytes), norm=empty(n*4), values=empty(n*gramBytes)
            let sources=buffer(b.wires.map{UInt32($0[0]*c.tape_width+c.nodes[$0[1]].offset)})
            keep += [rows,norm,values,sources]
            dispatch(half ? "project_half" : "project",[tape,sources,rows,norm],[[UInt32(n),UInt32(b.width),UInt32(rowBytes/(half ? 2 : 4)),0]],n)
            let x=MPSMatrix(buffer:rows,descriptor:MPSMatrixDescriptor(rows:n,columns:b.width,rowBytes:rowBytes,dataType:inputType))
            let g=MPSMatrix(buffer:values,descriptor:MPSMatrixDescriptor(rows:n,columns:n,rowBytes:gramBytes,dataType:.float32))
            let mul=MPSMatrixMultiplication(device:device,transposeLeft:false,transposeRight:true,resultRows:n,resultColumns:n,interiorColumns:b.width,alpha:1,beta:0)
            mul.encode(commandBuffer:cmd,leftMatrix:x,rightMatrix:x,resultMatrix:g)
            grams.append(g);norms.append(norm)
        }
        var outputs=[MTLBuffer](), sizes=[Int](), emitted=[MTLBuffer](), emitSizes=[Int]()
        for (qi,q) in job.plan.queries.enumerated() {
            let n=q.wires.count, size=n*n, distance=empty(size*4), out=empty(size)
            let maskValues=job.masks?[qi] ?? Array(repeating:1,count:size)
            try require(maskValues.count==size, "mask shape mismatch")
            let mask=buffer(maskValues.map{UInt8($0)})
            keep += [distance,out,mask]
            dispatch("zero_distance",[distance],[[UInt32(size)]],size)
            for term in q.terms {
                let map=buffer(term.rows.map{UInt32($0)});keep.append(map)
                dispatch("accumulate",[grams[term.block].data,norms[term.block],map,distance],[[UInt32(n),UInt32(grams[term.block].rowBytes/4)]],size)
            }
            dispatch("decide",[distance,mask,out],[[UInt32(size)]],size)
            outputs.append(out);sizes.append(size)
            let selection=job.emits?[qi] ?? []
            let selected=empty(selection.count)
            if !selection.isEmpty {
                try require(selection.allSatisfy{$0>=0 && $0<size}, "invalid consumer wiring")
                let index=buffer(selection.map{UInt32($0)});keep += [index,selected]
                dispatch("emit",[out,index,selected],[[UInt32(selection.count)]],selection.count)
            }
            emitted.append(selected);emitSizes.append(selection.count)
        }
        let transitions=job.transitions ?? []
        let statuses=empty(transitions.count)
        if !transitions.isEmpty {
            let targets=Set(transitions.map{$0.target})
            try require(targets.count==transitions.count && targets.isDisjoint(with:claimed),"target Cell slots must be fresh")
            var allSources=[UInt32](), allConditions=[UInt32](), allBindings=[UInt32](), partitions=[UInt32](), instructions=[UInt32]()
            var offsets=[Int](), total=0
            for size in sizes {offsets.append(total);total+=size}
            let answers=empty(total)
            let blit=cmd.makeBlitCommandEncoder()!
            for (i,out) in outputs.enumerated(){blit.copy(from:out,sourceOffset:0,to:answers,destinationOffset:offsets[i],size:sizes[i])}
            blit.endEncoding()
            for t in transitions {
                try require(t.target>=0 && t.target<count && t.bindings.count<=c.slots && t.partition.count==t.bindings.count,"invalid target layout")
                try require(t.sources.allSatisfy{claimed.contains($0)} && targets.isDisjoint(with:Set(t.sources)),"stage sources must precede targets")
                try require(t.bindings.allSatisfy{$0.count==2 && t.sources.contains($0[0]) && $0[1]>=0 && $0[1]<c.slots},"invalid transition binding")
                let cond=t.conditions.map{pair -> UInt32 in
                    precondition(pair.count==2 && pair[0]>=0 && pair[0]<sizes.count && pair[1]>=0 && pair[1]<sizes[pair[0]])
                    return UInt32(offsets[pair[0]]+pair[1])
                }
                instructions += [t.target,allSources.count,t.sources.count,allConditions.count,cond.count,allBindings.count,t.bindings.count,partitions.count].map{UInt32($0)}
                allSources += t.sources.map{UInt32($0)};allConditions += cond
                allBindings += t.bindings.map{UInt32($0[0]*c.slots+$0[1])};partitions += t.partition.map{UInt32($0)}
            }
            let op=buffer(instructions),so=buffer(allSources),co=buffer(allConditions.isEmpty ? [0] : allConditions),bi=buffer(allBindings),pa=buffer(partitions)
            keep += [answers,op,so,co,bi,pa,statuses]
            dispatch("transition",[codes,live,answers,op,so,co,bi,pa,statuses],[[UInt32(transitions.count),UInt32(c.bits),UInt32(c.slots)]],transitions.count)
            claimed.formUnion(targets)
        }
        let stateSnapshot=input.active == nil ? nil : empty(count*c.slots*c.bits*4)
        let liveSnapshot=input.active == nil ? nil : empty(count)
        if let stateSnapshot=stateSnapshot, let liveSnapshot=liveSnapshot {
            let blit=cmd.makeBlitCommandEncoder()!
            blit.copy(from:codes,sourceOffset:0,to:stateSnapshot,destinationOffset:0,size:count*c.slots*c.bits*4)
            blit.copy(from:live,sourceOffset:0,to:liveSnapshot,destinationOffset:0,size:count)
            blit.endEncoding();keep += [stateSnapshot,liveSnapshot]
        }
        var elapsed:Double?=nil
        if !chained {
            let start=ProcessInfo.processInfo.systemUptime
            cmd.commit();cmd.waitUntilCompleted()
            elapsed=ProcessInfo.processInfo.systemUptime-start
            if let error=cmd.error{throw error}
        }
        let readback: () -> Result = {
        let rows=zip(outputs,sizes).map{b,n in Array(UnsafeBufferPointer(start:b.contents().assumingMemoryBound(to:UInt8.self),count:n))}
        // Debug readback is outside GPU matching time, deliberately explicit.
        let features=Array(UnsafeBufferPointer(start:tape.contents().assumingMemoryBound(to:Float.self),count:count*c.tape_width))
        let selected=zip(emitted,emitSizes).map{b,n in Array(UnsafeBufferPointer(start:b.contents().assumingMemoryBound(to:UInt8.self),count:n))}
        let status=Array(UnsafeBufferPointer(start:statuses.contents().assumingMemoryBound(to:UInt8.self),count:transitions.count))
        let state=stateSnapshot.map{Array(UnsafeBufferPointer(start:$0.contents().assumingMemoryBound(to:Float.self),count:count*c.slots*c.bits))}
        let validity=liveSnapshot.map{Array(UnsafeBufferPointer(start:$0.contents().assumingMemoryBound(to:UInt8.self),count:count))}
        return Result(name:job.name,outputs:rows,emitted:selected,gpu_seconds:chained ? nil : cmd.gpuEndTime-cmd.gpuStartTime,submit_wait_seconds:elapsed,feature_values:features,transition_status:status,cell_values:state,cell_valid:validity)
        }
        if chained {pending.append(readback);retained += keep+[tape,statuses]+outputs+emitted}
        else {results.append(readback());withExtendedLifetime(keep){}}
    }
    var programElapsed:Double?=nil
    if let chain=chain {
        let start=ProcessInfo.processInfo.systemUptime
        chain.commit();chain.waitUntilCompleted()
        programElapsed=ProcessInfo.processInfo.systemUptime-start
        if let error=chain.error{throw error}
    }
    results += pending.map{$0()};withExtendedLifetime(retained){}
    let report=Report(device:device.name,backend:"Metal feature circuit + MPS \(half ? "FP16 inputs / FP32 output" : "FP32") GEMM + exact distance kernels",results:results,chained:chained,program_gpu_seconds:chain.map{$0.gpuEndTime-$0.gpuStartTime},program_submit_wait_seconds:programElapsed)
    try JSONEncoder().encode(report).write(to:URL(fileURLWithPath:CommandLine.arguments[2]))
    print("Metal verified pipeline completed on \(device.name); \(results.count) plans")
} catch { fputs("\(error)\n",stderr);exit(1) }
