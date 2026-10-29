"""Bind the existing private GPU schedule executor to resident ABI7 buffers.

The CPU dispatches a compiled region and allocates buffers; it does not execute
its Rules/Repeat/Saturate, enumerate bindings or provide inferred facts.
"""
from pathlib import Path

ROOT=Path(__file__).resolve().parents[2]

def runtime_source():
    text=(ROOT/'engine/metal/scheduled_matrix.swift').read_text()
    def replace(before,after):
        nonlocal text
        if text.count(before)!=1:raise ValueError('scheduled runtime anchor changed: '+before[:90])
        text=text.replace(before,after)
    text=text.replace('program["scheduled_matrix"]','program["resident_schedules"]')
    text=text.replace('device.makeComputePipelineState(function:lib.makeFunction(name:$0)!)','loadPipeline($0)')
    text=text.replace('"sm_dispatch"]','"sm_dispatch","resident_schedule_index"]')
    text=text.replace('(program["sets"] as! [String])','(scheduled["all_sets"] as! [String])')
    text=text.replace('let setCount=(scheduled["all_sets"] as! [String]).count','let setCount=(scheduled["all_sets"] as! [String]).count')
    text=text.replace('watermarks.length','max(1,(program["rules"] as! Int))*4')
    text=text.replace('from:watermarks,sourceOffset:0','from:uints,sourceOffset:layout["watermarks"]!*4')
    text=text.replace('to:watermarks,destinationOffset:0','to:uints,destinationOffset:layout["watermarks"]!*4')
    text=text.replace('ProcessInfo.processInfo.environment["EGGFUSE_MATRIX_AUDIT"]=="1"','program["batch_audit_numeric"] as? Bool == true')
    text=text.replace('privateBuffer(64),readback=shared([UInt32](repeating:0,count:16))','privateBuffer(80),readback=shared([UInt32](repeating:0,count:20))')
    text=text.replace('range:0..<64','range:0..<80').replace('size:64','size:80')
    text=text.replace('capacity:16),count:16','capacity:20),count:20')
    text=text.replace('historyCapacity*32','historyCapacity*48').replace('256*32','256*48')
    text=text.replace('rounds*8','rounds*12').replace('rounds*32','rounds*48')
    replace('  if let stage=stage{', '''  if index==14 {e.setBuffer(uints,offset:layout["parent"]!*4,index:7);e.setBuffer(uints,offset:layout["fp"]!*4,index:8)}
  if let stage=stage{''')
    replace('let result=Array(UnsafeBufferPointer(start:status.contents().bindMemory(to:UInt32.self,capacity:16),count:9))', 'let result=u32(status,0,9)')
    replace('let indexCount=status.contents().advanced(by:36).loadUnaligned(as:Int64.self)', 'let indexCount=i64(rows,program["index_counter_offset"] as! Int,1)[0]')
    # Publication creates only fresh opaque predicate rows. The normal resident
    # index builder owns its next rebuild; no stale parallel hash is patched.
    before=' if new>0&&needsRowIndex{encode(commit,rowIndexPipelines[0],1,set);encode(commit,rowIndexPipelines[1],program["hash_capacity"] as! Int,set);encode(commit,rowIndexPipelines[2],max(1,nrows),set);encode(commit,rowIndexPipelines[3],1,set)}'
    replace(before,'')
    replace('launch(commit,14,[order,birth,parent,factParent],new);launch(commit,15,[order],new);launch(commit,16,[history],1)', 'launch(commit,14,[order,birth,uints,uints],new);launch(commit,22,[],new);launch(commit,16,[history],1)')
    replace('let status=shared([UInt32](repeating:0,count:17)),flags=privateBuffer(64);let wb=commit.makeBlitCommandEncoder()!;', 'let status=shared([UInt32](repeating:0,count:9));let wb=commit.makeBlitCommandEncoder()!;')
    replace('wb.endEncoding();copyStatus(commit,status,flags)', 'wb.copy(from:metas,sourceOffset:0,to:status,destinationOffset:0,size:36);wb.endEncoding()')
    replace(' if new>0{needsRowIndex=false}','')
    pre='''
// Private regions reuse the original certified schedule/matrix executor.
struct MatrixSatResult {let accepted:Bool;let reason:UInt32;let info:[UInt32];let history:[UInt32];let phases:[Double];let indexCount:Int64;let markerCount:Int;let scratchBytes:Int}
func need(_ ok:Bool,_ message:String)throws {try require(ok,message)}
func shared<T>(_ a:[T])->MTLBuffer {let b=zeros(a.count*MemoryLayout<T>.stride);copy(a,b);return b}
func privateBuffer(_ bytes:Int)->MTLBuffer {device.makeBuffer(length:max(8,bytes),options:.storageModePrivate)!}
var meta:MTLBuffer {metas}
var hostMeta=[UInt32]()
let msDescription=(program["resident_schedules"] as? [String:Any])?["matrix"] as? [String:Any]
let msPlans=msDescription?["programs"] as? [[String:Any]] ?? []
let msWeights=shared((msDescription?["weights"] as? [NSNumber] ?? []).map{$0.uint16Value})
var msLogicalRuns=0,msPublished=0,msPeakBytes=0,smForwardedTotal=0
'''
    text=text.replace('smLastForwarded=Int(info[15]);', 'smLastForwarded=Int(info[15]);smForwardedTotal+=smLastForwarded;')
    return pre+text


def worker_source():
    return (ROOT/'engine/metal/generic_cell.swift').read_text().replace('// RESIDENT_SCHEDULE_RUNTIME',runtime_source())
