// Exact int32 parameter DAGs. Standalone Metal worker; no floating-point IDs.
import Foundation
import Metal

struct Template: Decodable { let ops: [[Int32]]; let guard_end: Int; let guards: [Int]; let outputs: [Int] }
struct Task: Decodable {
    let template: Int; let values: [Int32]
    init(from decoder: Decoder) throws {
        var c = try decoder.unkeyedContainer()
        template = try c.decode(Int.self); values = try c.decode([Int32].self)
    }
}
struct Input: Decodable { let templates: [Template]; let tasks: [Task] }
struct Result: Encodable { let status: Int32; let values: [Int32] }
struct Report: Encodable { let device: String; let results: [Result] }

let source = """
#include <metal_stdlib>
using namespace metal;
constant int lo = (-2147483647 - 1), hi = 2147483647;
kernel void run(device const int *ops [[buffer(0)]], device const uint *templates [[buffer(1)]],
                device const uint *guards [[buffer(2)]], device const uint *outputs [[buffer(3)]],
                device const int *tasks [[buffer(4)]], device int *tape [[buffer(5)]],
                device int *result [[buffer(6)]], constant uint4 &cfg [[buffer(7)]],
                uint id [[thread_position_in_grid]]) {
    if(id>=cfg.x)return;
    uint task=id*cfg.y, t=uint(tasks[task])*7, base=id*cfg.z, dest=id*cfg.w;
    uint start=templates[t], count=templates[t+1], end=templates[t+2];
    uint gs=templates[t+3], gn=templates[t+4], os=templates[t+5], on=templates[t+6];
    result[dest]=0;
    for(uint i=0;i<=count;i++) {
        if(i==end)for(uint g=0;g<gn;g++)if(!tape[base+guards[gs+g]])return;
        if(i==count)break;
        uint p=(start+i)*3; int op=ops[p], a=ops[p+1], b=ops[p+2],v=0;
        if(op==0)v=tasks[task+1+uint(a)];
        else if(op==1)v=a;
        else {
            int x=tape[base+uint(a)], y=tape[base+uint(b)]; bool ok=true;
            if(op==2){ok=!((y>0 && x>hi-y)||(y<0 && x<lo-y));if(ok)v=x+y;}
            else if(op==3){ok=!((y>0 && x<lo+y)||(y<0 && x>hi+y));if(ok)v=x-y;}
            else if(op==4){
                bool neg=(x<0)!=(y<0); uint ax=x<0?0u-uint(x):uint(x), ay=y<0?0u-uint(y):uint(y);
                uint limit=neg?2147483648u:2147483647u; ok=ay==0 || ax<=limit/ay;
                if(ok){uint z=ax*ay;v=neg?(z==2147483648u?lo:-int(z)):int(z);}
            }
            else if(op==5)v=int(x>y); else if(op==6)v=int(x>=y);
            else if(op==7)v=int(x<y); else if(op==8)v=int(x<=y);
            else if(op==9)v=int(x==y); else v=int(x!=y);
            if(!ok){result[dest]=2;return;}
        }
        tape[base+i]=v;
    }
    result[dest]=1;
    for(uint i=0;i<on;i++)result[dest+1+i]=tape[base+outputs[os+i]];
}
"""

do {
    guard let device=MTLCreateSystemDefaultDevice(), let queue=device.makeCommandQueue() else {
        throw NSError(domain:"Metal unavailable",code:1)
    }
    let lib=try device.makeLibrary(source:source,options:nil)
    let pipeline=try device.makeComputePipelineState(function:lib.makeFunction(name:"run")!)
    func buffer<T>(_ a: [T]) -> MTLBuffer {
        if a.isEmpty { return device.makeBuffer(length:4,options:.storageModeShared)! }
        return a.withUnsafeBytes { device.makeBuffer(bytes:$0.baseAddress!,length:$0.count,options:.storageModeShared)! }
    }
    while let line=readLine() {
        do {
            let input=try JSONDecoder().decode(Input.self,from:Data(line.utf8))
            let width=1+(input.tasks.map{$0.values.count}.max() ?? 0)
            let stride=max(1,input.templates.map{$0.ops.count}.max() ?? 0)
            let outWidth=1+(input.templates.map{$0.outputs.count}.max() ?? 0)
            var ops=[Int32](), meta=[UInt32](), guards=[UInt32](), outputs=[UInt32]()
            for t in input.templates {
                meta += [ops.count/3,t.ops.count,t.guard_end,guards.count,t.guards.count,outputs.count,t.outputs.count].map{UInt32($0)}
                ops += t.ops.flatMap{$0}; guards += t.guards.map{UInt32($0)}; outputs += t.outputs.map{UInt32($0)}
            }
            let tasks=input.tasks.flatMap { [Int32($0.template)]+$0.values+Array(repeating:Int32(0),count:width-1-$0.values.count) }
            let bufs=[buffer(ops),buffer(meta),buffer(guards),buffer(outputs),buffer(tasks),
                      device.makeBuffer(length:max(4,input.tasks.count*stride*4),options:.storageModeShared)!,
                      device.makeBuffer(length:max(4,input.tasks.count*outWidth*4),options:.storageModeShared)!]
            var cfg=SIMD4<UInt32>(UInt32(input.tasks.count),UInt32(width),UInt32(stride),UInt32(outWidth))
            let cb=queue.makeCommandBuffer()!, enc=cb.makeComputeCommandEncoder()!
            enc.setComputePipelineState(pipeline)
            for (i,b) in bufs.enumerated(){enc.setBuffer(b,offset:0,index:i)}
            enc.setBytes(&cfg,length:MemoryLayout.size(ofValue:cfg),index:7)
            enc.dispatchThreads(MTLSize(width:input.tasks.count,height:1,depth:1),
                                threadsPerThreadgroup:MTLSize(width:min(128,pipeline.maxTotalThreadsPerThreadgroup),height:1,depth:1))
            enc.endEncoding(); cb.commit(); cb.waitUntilCompleted()
            if let error=cb.error {throw error}
            let ptr=bufs[6].contents().bindMemory(to:Int32.self,capacity:input.tasks.count*outWidth)
            let results=input.tasks.enumerated().map { i,t in
                Result(status:ptr[i*outWidth],values:ptr[i*outWidth]==1 ?
                       (0..<input.templates[t.template].outputs.count).map{ptr[i*outWidth+1+$0]}:[])
            }
            print(String(data:try JSONEncoder().encode(Report(device:device.name,results:results)),encoding:.utf8)!)
            fflush(stdout)
        } catch {
            let data=try JSONSerialization.data(withJSONObject:["error":String(describing:error)])
            print(String(data:data,encoding:.utf8)!);fflush(stdout)
        }
    }
} catch {
    FileHandle.standardError.write(Data("\(error)\n".utf8));exit(1)
}
