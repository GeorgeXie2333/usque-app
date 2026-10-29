import Foundation

/// Used only on the packet IO queue. A pending batch belongs to the session
/// that requested the read, including its configuration and physical path.
struct PacketBatch {
    private var packets: [Data] = []
    private(set) var session: SessionKey?
    var first: Data? { packets.first }
    var isEmpty: Bool { packets.isEmpty }

    mutating func append(_ batch: [Data], captured: SessionKey?, current: SessionKey?) throws {
        discardUnlessCurrent(current)
        guard let captured, captured == current else { return }
        guard packets.count + batch.count <= 256,
              batch.allSatisfy({ !$0.isEmpty && $0.count <= 65_535 }),
              packets.reduce(0, { $0 + $1.count }) + batch.reduce(0, { $0 + $1.count }) <= 4 * 1024 * 1024
        else { throw NativeFailure.providerUnavailable }
        session = captured
        packets.append(contentsOf: batch)
    }
    mutating func discardUnlessCurrent(_ current: SessionKey?) {
        if session != current { removeAll() }
    }
    mutating func removeFirst() { packets.removeFirst() }
    mutating func removeAll() { packets.removeAll(); session = nil }
}
