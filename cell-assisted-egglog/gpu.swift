import Foundation
import Metal
import MetalPerformanceShaders

func exact(_ n: Int) throws -> Data {
    var d = Data()
    while d.count < n {
        guard let x = try FileHandle.standardInput.read(upToCount: n - d.count), !x.isEmpty else {
            throw NSError(domain: "EOF", code: 1)
        }
        d.append(x)
    }
    return d
}
guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
    fatalError("Metal device unavailable")
}
FileHandle.standardOutput.write(Data("READY \(device.name)\n".utf8))
while true {
    let h: Data
    do { h = try exact(12) } catch { break }
    let header = h.withUnsafeBytes { Array($0.bindMemory(to: UInt32.self)) }
    let n = Int(header[0]), m = Int(header[1]), w = Int(header[2]), k = w * 32
    guard n > 0 && m > 0 && n <= 256 && m <= 256 && w > 0 && k <= (1 << 24) else { fatalError("bad matrix dimensions") }
    let d = try exact((n+m)*w*4)
    let ids = d.withUnsafeBytes { Array($0.bindMemory(to: UInt32.self)) }
    // Signed-bit encoding is lossless for u32 identity, including values >2^24.
    let a = device.makeBuffer(length:n*k*4, options:.storageModeShared)!
    let b = device.makeBuffer(length:m*k*4, options:.storageModeShared)!
    let ap = a.contents().bindMemory(to:Float.self, capacity:n*k)
    let bp = b.contents().bindMemory(to:Float.self, capacity:m*k)
    for i in 0..<(n+m) { for j in 0..<w { for bit in 0..<32 {
        let v:Float = (ids[i*w+j] >> bit)&1 == 0 ? -1 : 1
        if i < n { ap[i*k+j*32+bit] = v } else { bp[(i-n)*k+j*32+bit] = v }
    }}}
    let c = device.makeBuffer(length:n*m*4, options:.storageModeShared)!
    let am = MPSMatrix(buffer:a, descriptor:MPSMatrixDescriptor(rows:n, columns:k, rowBytes:k*4, dataType:.float32))
    let bm = MPSMatrix(buffer:b, descriptor:MPSMatrixDescriptor(rows:m, columns:k, rowBytes:k*4, dataType:.float32))
    let cm = MPSMatrix(buffer:c, descriptor:MPSMatrixDescriptor(rows:n, columns:m, rowBytes:m*4, dataType:.float32))
    let multiply = MPSMatrixMultiplication(device:device, transposeLeft:false, transposeRight:true,
        resultRows:n, resultColumns:m, interiorColumns:k, alpha:1, beta:0)
    let command = queue.makeCommandBuffer()!
    multiply.encode(commandBuffer:command, leftMatrix:am, rightMatrix:bm, resultMatrix:cm)
    command.commit();command.waitUntilCompleted()
    if let error=command.error { fatalError("Metal failed: \(error)") }
    FileHandle.standardOutput.write(Data(bytes:c.contents(), count:n*m*4))
}
