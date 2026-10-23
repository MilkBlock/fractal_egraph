import Foundation
import Metal

// Binary table protocol. No JSON graph, floating point or unordered reduction.
let magic: UInt64 = 0x5449474552434f53
func fail(_ text: String) -> Never {
    FileHandle.standardError.write(Data((text + "\n").utf8)); exit(1)
}
let data = FileHandle.standardInput.readDataToEndOfFile()
guard data.count >= 32 && data.count % 8 == 0 else { fail("invalid extraction packet length") }
let words: [UInt64] = data.withUnsafeBytes { bytes in
    (0..<(bytes.count / 8)).map { bytes.loadUnaligned(fromByteOffset: $0 * 8, as: UInt64.self).littleEndian }
}
guard words[0] == magic else { fail("invalid extraction packet schema") }
let n = words[1], m = words[2], refs = words[3]
guard n <= UInt64(Int32.max), m <= UInt64(Int32.max), refs <= UInt64(Int32.max),
      UInt64(words.count) == 4 + n + 4*m + refs else { fail("invalid extraction table sizes") }
let header = 4 + Int(n), ports = header + 4*Int(m)
for i in 0..<Int(m) {
    let b = header + 4*i, kind = words[b], off = words[b+2], len = words[b+3]
    guard kind <= 2, off <= refs, len <= refs-off,
          (kind != 1 || len >= 2), (kind != 2 || len == 1) else { fail("invalid cost recipe") }
}
for p in words[ports...] { if p >= n { fail("invalid cost slot") } }
guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else { fail("Metal unavailable") }
let source = """
#include <metal_stdlib>
using namespace metal;
kernel void evaluate(device const ulong *p [[buffer(0)]],
                     device ulong *out [[buffer(1)]], uint id [[thread_position_in_grid]]) {
    if (ulong(id) >= p[2]) return;
    ulong h = 4 + p[1] + 4*ulong(id), kind = p[h], base = p[h+1];
    ulong off = 4 + p[1] + 4*p[2] + p[h+2], len = p[h+3];
    if (kind == 2) { out[id] = p[4+p[off]] * 500ul; return; }
    ulong count = len - (kind == 1 ? 2ul : 0ul), value = base;
    for (ulong j = 0; j < count; ++j) value += p[4+p[off+j]];
    if (kind == 1) {
        ulong a = p[4+p[off+count]], b = p[4+p[off+count+1]];
        value += max(a,b) + (min(a,b) >> 2);
    }
    out[id] = value;
}
"""
do {
    let library = try device.makeLibrary(source: source, options: nil)
    guard let f = library.makeFunction(name: "evaluate") else { fail("missing kernel") }
    let pipeline = try device.makeComputePipelineState(function: f)
    guard let input = device.makeBuffer(bytes: words, length: data.count, options: .storageModeShared),
          let output = device.makeBuffer(length: max(8, Int(m)*8), options: .storageModeShared),
          let command = queue.makeCommandBuffer(), let encoder = command.makeComputeCommandEncoder() else { fail("Metal allocation failed") }
    encoder.setComputePipelineState(pipeline); encoder.setBuffer(input, offset: 0, index: 0); encoder.setBuffer(output, offset: 0, index: 1)
    if m > 0 {
        encoder.dispatchThreads(MTLSize(width: Int(m), height: 1, depth: 1),
                                threadsPerThreadgroup: MTLSize(width: min(256, pipeline.maxTotalThreadsPerThreadgroup), height: 1, depth: 1))
    }
    encoder.endEncoding(); command.commit(); command.waitUntilCompleted()
    if let error = command.error { fail("Metal extraction failed: \(error)") }
    let prefix: [UInt64] = [magic, m]
    prefix.withUnsafeBytes { FileHandle.standardOutput.write(Data($0)) }
    FileHandle.standardOutput.write(Data(bytes: output.contents(), count: Int(m)*8))
    FileHandle.standardError.write(Data(("TIGER_MARKER_GPU {\"kernel_seconds\":\(command.gpuEndTime-command.gpuStartTime),\"recipes\":\(m)}\n").utf8))
} catch { fail("Metal compilation failed: \(error)") }
