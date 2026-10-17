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
struct Job: Decodable { let name: String; let plan: Plan; let masks: [[Int]]?; let emits: [[Int]]? }
struct Input: Decodable { let circuit: Circuit; let ports: [[[Float]]]; let jobs: [Job]; let half_inputs: Bool? }
struct Result: Encodable { let name: String; let outputs: [[UInt8]]; let emitted: [[UInt8]]; let gpu_seconds: Double; let submit_wait_seconds: Double; let feature_values: [Float] }
struct Report: Encodable { let device: String; let backend: String; let results: [Result] }

func require(_ condition: Bool, _ message: String) throws {
    if !condition { throw NSError(domain: message, code: 1) }
}

let metalSource = """
#include <metal_stdlib>
using namespace metal;
// instruction = op, offset, width, a_offset, a_width, b_offset, slot
kernel void build(device const float *input [[buffer(0)]], device float *tape [[buffer(1)]],
                  device const uint *ops [[buffer(2)]], constant uint4 &cfg [[buffer(3)]],
                  constant uint &op_count [[buffer(4)]], uint id [[thread_position_in_grid]]) {
    if (id >= cfg.x) return;
    uint bits=cfg.y, slots=cfg.z, stride=cfg.w, base=id*stride;
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
"""

do {
    try require(CommandLine.arguments.count==3, "usage: gpu input.json output.json")
    let input = try JSONDecoder().decode(Input.self, from: Data(contentsOf: URL(fileURLWithPath:CommandLine.arguments[1])))
    guard let device=MTLCreateSystemDefaultDevice(), let queue=device.makeCommandQueue() else { throw NSError(domain:"Metal unavailable",code:1) }
    let options=MTLCompileOptions()
    if #available(macOS 15.0, *) { options.mathMode = .safe } else { options.fastMathEnabled=false }
    let library=try device.makeLibrary(source:metalSource,options:options)
    var pipelines=[String:MTLComputePipelineState]()
    for name in ["build","project","project_half","zero_distance","accumulate","decide","emit"] {
        pipelines[name]=try device.makeComputePipelineState(function:library.makeFunction(name:name)!)
    }
    func buffer<T>(_ values: [T]) -> MTLBuffer {
        return values.withUnsafeBytes { device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)! }
    }
    func empty(_ bytes: Int) -> MTLBuffer { device.makeBuffer(length:max(bytes,4),options:.storageModeShared)! }
    let c=input.circuit, count=input.ports.count, half=input.half_inputs ?? false
    try require(count>0 && input.ports.allSatisfy{$0.count==c.slots && $0.allSatisfy{$0.count==c.bits && $0.allSatisfy{$0 == 1 || $0 == -1}}}, "invalid input code shape")
    let codes=buffer(input.ports.flatMap{$0.flatMap{$0}})
    let opIDs=["port":0,"lift2":1,"mean":2,"concat":3,"moments":4]
    let ops=buffer(c.nodes.flatMap { n -> [UInt32] in
        let a=n.args.isEmpty ? nil : c.nodes[n.args[0]], b=n.args.count<2 ? nil : c.nodes[n.args[1]]
        return [opIDs[n.op]!,n.offset,n.width,a?.offset ?? 0,a?.width ?? 0,b?.offset ?? 0,max(n.slot,0)].map{UInt32($0)}
    })
    var results=[Result]()
    for job in input.jobs {
        try require(job.plan.guards.count==count,"Cell guard count mismatch")
        for (i,expected) in job.plan.guards.enumerated() {
            let ports=Array(input.ports[i].prefix(expected.count))
            let observed=ports.map{ports.firstIndex(of:$0)!}
            try require(observed==expected,"Cell input partition invalidates this Bake; specialize before dispatch")
        }
        let tape=empty(count*c.tape_width*4)
        let cmd=queue.makeCommandBuffer()!
        var keep=[MTLBuffer]()
        func dispatch(_ name:String,_ bindings:[MTLBuffer],_ constants:[[UInt32]],_ size:Int) {
            let enc=cmd.makeComputeCommandEncoder()!, pipeline=pipelines[name]!
            enc.setComputePipelineState(pipeline)
            for (i,b) in bindings.enumerated(){enc.setBuffer(b,offset:0,index:i)}
            for (i,v) in constants.enumerated(){v.withUnsafeBytes{enc.setBytes($0.baseAddress!,length:$0.count,index:bindings.count+i)}}
            enc.dispatchThreads(MTLSize(width:size,height:1,depth:1),threadsPerThreadgroup:MTLSize(width:min(128,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1))
            enc.endEncoding()
        }
        dispatch("build",[codes,tape,ops],[[UInt32(count),UInt32(c.bits),UInt32(c.slots),UInt32(c.tape_width)],[UInt32(c.nodes.count)]],count)
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
        let start=ProcessInfo.processInfo.systemUptime
        cmd.commit();cmd.waitUntilCompleted()
        let end=ProcessInfo.processInfo.systemUptime
        if let error=cmd.error{throw error}
        let rows=zip(outputs,sizes).map{b,n in Array(UnsafeBufferPointer(start:b.contents().assumingMemoryBound(to:UInt8.self),count:n))}
        // Debug readback is outside GPU matching time, deliberately explicit.
        let features=Array(UnsafeBufferPointer(start:tape.contents().assumingMemoryBound(to:Float.self),count:count*c.tape_width))
        let selected=zip(emitted,emitSizes).map{b,n in Array(UnsafeBufferPointer(start:b.contents().assumingMemoryBound(to:UInt8.self),count:n))}
        results.append(Result(name:job.name,outputs:rows,emitted:selected,gpu_seconds:cmd.gpuEndTime-cmd.gpuStartTime,submit_wait_seconds:end-start,feature_values:features))
        withExtendedLifetime(keep){}
    }
    let report=Report(device:device.name,backend:"Metal feature circuit + MPS \(half ? "FP16 inputs / FP32 output" : "FP32") GEMM + exact distance kernels",results:results)
    try JSONEncoder().encode(report).write(to:URL(fileURLWithPath:CommandLine.arguments[2]))
    print("Metal verified pipeline completed on \(device.name); \(results.count) plans")
} catch { fputs("\(error)\n",stderr);exit(1) }
