// Standalone batch evaluator; this does not modify or execute the egglog runtime.
// Input: little-endian Int64[18] rows. Output: little-endian Int64[6] rows:
// id, bopLeft, left, bopRight, right, eligible.
// CPU body and Metal source are generated from the shared typed Cell program.
import Foundation
import Metal

private let inputWidth = 18
private let outputWidth = 6
private let maxInput: Int64 = 1 << 30

private struct BackendError: Error, CustomStringConvertible {
    let description: String
    init(_ message: String) { description = message }
}

private func now() -> Double { ProcessInfo.processInfo.systemUptime }

private func evaluate() throws {
    let started = now()
    let arguments = Array(CommandLine.arguments.dropFirst())
    guard arguments.count >= 3, ["cpu", "gpu"].contains(arguments[0]) else {
        throw BackendError("usage: backend cpu|gpu INPUT OUTPUT [--metal-source PATH]; GPU requires --metal-source")
    }
    let backend = arguments[0]
    let inputPath = arguments[1]
    let outputPath = arguments[2]
    var sourcePath: String?
    if arguments.count == 5, arguments[3] == "--metal-source" {
        sourcePath = arguments[4]
    } else if arguments.count != 3 {
        throw BackendError("expected exactly INPUT OUTPUT and optional --metal-source PATH")
    }
    if backend == "gpu", sourcePath == nil {
        throw BackendError("GPU requires --metal-source PATH containing kernel cell_kernel; no CPU fallback is used")
    }

    let readStarted = now()
    let rawInput = try Data(contentsOf: URL(fileURLWithPath: inputPath))
    let inputReadSeconds = now() - readStarted
    guard rawInput.count % (inputWidth * MemoryLayout<Int64>.size) == 0 else {
        throw BackendError("input byte length must be a multiple of 144 (18 little-endian Int64 fields)")
    }
    let count = rawInput.count / (inputWidth * MemoryLayout<Int64>.size)
    // Kernel row offsets are uint32. Both input/output products must fit.
    guard count <= Int(UInt32.max) / inputWidth else {
        throw BackendError("too many rows for uint32 Metal row offsets")
    }

    let packStarted = now()
    var inputs = [UInt32](repeating: 0, count: count * inputWidth)
    try rawInput.withUnsafeBytes { bytes in
        for field in inputs.indices {
            let value = Int64(littleEndian: bytes.loadUnaligned(
                fromByteOffset: field * MemoryLayout<Int64>.size, as: Int64.self))
            guard value >= 0, value <= maxInput else {
                throw BackendError("row \(field / inputWidth), field \(field % inputWidth): value \(value) is outside 0...2^30")
            }
            inputs[field] = UInt32(value)
        }
    }
    for row in 0..<count {
        let offset = row * inputWidth
        guard inputs[offset + 1] == inputs[offset + 2] else {
            throw BackendError("row \(row): repeated-operand shape requires s1 == s0")
        }
    }
    var outputs = [UInt32](repeating: 0, count: count * outputWidth)
    var packSeconds = now() - packStarted
    var setupSeconds = 0.0
    var sourceReadSeconds = 0.0
    var compileSeconds = 0.0
    var deviceName: String? = nil
    var gpuExecutionSeconds: Double? = nil
    var computeSeconds = 0.0
    var unpackSeconds = 0.0

    if backend == "cpu" {
        let computeStarted = now()
        // CELL_BODY_BEGIN
        throw BackendError("CPU body not generated; run the Cell compiler with --build")
        // CELL_BODY_END
        computeSeconds = now() - computeStarted
    } else {
        let setupStarted = now()
        guard let device = MTLCreateSystemDefaultDevice() else {
            throw BackendError("no Metal GPU device is available; no CPU fallback is used")
        }
        deviceName = device.name
        guard let queue = device.makeCommandQueue() else {
            throw BackendError("could not create a Metal command queue")
        }
        let sourceStarted = now()
        let source = try String(contentsOfFile: sourcePath!, encoding: .utf8)
        sourceReadSeconds = now() - sourceStarted
        let compileStarted = now()
        let library = try device.makeLibrary(source: source, options: nil)
        guard let function = library.makeFunction(name: "cell_kernel") else {
            throw BackendError("Metal source must define kernel cell_kernel")
        }
        let pipeline = try device.makeComputePipelineState(function: function)
        compileSeconds = now() - compileStarted
        setupSeconds = now() - setupStarted

        let bufferPackStarted = now()
        let inputByteCount = inputs.count * MemoryLayout<UInt32>.size
        let outputByteCount = outputs.count * MemoryLayout<UInt32>.size
        guard inputByteCount <= device.maxBufferLength,
              outputByteCount <= device.maxBufferLength else {
            throw BackendError("batch exceeds this Metal device's maximum buffer length")
        }
        guard let inputBuffer = device.makeBuffer(length: max(4, inputByteCount), options: .storageModeShared),
              let outputBuffer = device.makeBuffer(length: max(4, outputByteCount), options: .storageModeShared) else {
            throw BackendError("could not allocate Metal input/output buffers")
        }
        if inputByteCount > 0 {
            inputs.withUnsafeBytes { bytes in
                inputBuffer.contents().copyMemory(from: bytes.baseAddress!, byteCount: inputByteCount)
            }
        }
        packSeconds += now() - bufferPackStarted

        let computeStarted = now()
        if count > 0 {
            guard let command = queue.makeCommandBuffer(),
                  let encoder = command.makeComputeCommandEncoder() else {
                throw BackendError("could not create a Metal command buffer/encoder")
            }
            encoder.setComputePipelineState(pipeline)
            encoder.setBuffer(inputBuffer, offset: 0, index: 0)
            encoder.setBuffer(outputBuffer, offset: 0, index: 1)
            var rowCount = UInt32(count)
            encoder.setBytes(&rowCount, length: MemoryLayout<UInt32>.size, index: 2)
            let width = min(256, pipeline.maxTotalThreadsPerThreadgroup)
            encoder.dispatchThreads(MTLSize(width: count, height: 1, depth: 1),
                                    threadsPerThreadgroup: MTLSize(width: width, height: 1, depth: 1))
            encoder.endEncoding()
            command.commit()
            command.waitUntilCompleted()
            guard command.status == .completed else {
                throw BackendError("Metal execution failed: \(command.error?.localizedDescription ?? "command did not complete")")
            }
            if command.gpuEndTime >= command.gpuStartTime, command.gpuStartTime > 0 {
                gpuExecutionSeconds = command.gpuEndTime - command.gpuStartTime
            }
        }
        computeSeconds = now() - computeStarted
        let outputCopyStarted = now()
        if outputByteCount > 0 {
            outputs.withUnsafeMutableBytes { bytes in
                bytes.baseAddress!.copyMemory(from: outputBuffer.contents(), byteCount: outputByteCount)
            }
        }
        unpackSeconds += now() - outputCopyStarted
    }

    let unpackStarted = now()
    var rawOutput = Data(count: outputs.count * MemoryLayout<Int64>.size)
    rawOutput.withUnsafeMutableBytes { bytes in
        for field in outputs.indices {
            bytes.storeBytes(of: Int64(outputs[field]).littleEndian,
                             toByteOffset: field * MemoryLayout<Int64>.size, as: Int64.self)
        }
    }
    unpackSeconds += now() - unpackStarted
    let writeStarted = now()
    try rawOutput.write(to: URL(fileURLWithPath: outputPath), options: .atomic)
    let outputWriteSeconds = now() - writeStarted
    let metadata: [String: Any] = [
        "backend": backend,
        "rows": count,
        "input_fields": inputWidth,
        "output_fields": outputWidth,
        "format": "little-endian-int64",
        "scope": "standalone-batch-prototype",
        "cpu_operator": "generated-shared-program",
        "metal_source": sourcePath as Any? ?? NSNull(),
        "gpu_device": deviceName as Any? ?? NSNull(),
        "input_read_s": inputReadSeconds,
        "pack_s": packSeconds,
        "setup_s": setupSeconds,
        "metal_source_read_s": sourceReadSeconds,
        "metal_compile_s": compileSeconds,
        "compute_s": computeSeconds,
        "gpu_execution_s": gpuExecutionSeconds as Any? ?? NSNull(),
        "unpack_s": unpackSeconds,
        "output_write_s": outputWriteSeconds,
        // Excludes this final JSON serialization, but includes source compilation.
        "total_s": now() - started
    ]
    let json = try JSONSerialization.data(withJSONObject: metadata, options: [.sortedKeys])
    FileHandle.standardOutput.write(json)
    FileHandle.standardOutput.write(Data([10]))
}

do {
    try evaluate()
} catch {
    FileHandle.standardError.write(Data("backend: \(error)\n".utf8))
    exit(1)
}
