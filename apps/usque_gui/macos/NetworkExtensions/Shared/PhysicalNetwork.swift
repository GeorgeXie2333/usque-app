import Foundation
import Network

final class PhysicalNetwork {
    static let shared = PhysicalNetwork()
    private let lock = NSLock()
    private let monitor = NWPathMonitor()
    private var fingerprint = ""
    private var candidate: NWInterface?
    private var value: [String: Any] = ["generation": UInt64(0), "satisfied": false, "interfaces": []]
    var didChange: (() -> Void)?
    init() {
        monitor.pathUpdateHandler = { [weak self] path in self?.update(path) }
        monitor.start(queue: DispatchQueue(label: "usque.physical-path"))
    }
    private func update(_ path: NWPath) {
        // A candidate interface is an observation, never proof that sockets bypass
        // enforceRoutes. The signed PoC separately records actual egress packets.
        let interfaces = path.availableInterfaces.filter {
            [.wifi, .wiredEthernet, .cellular].contains($0.type) && !$0.name.hasPrefix("utun")
        }.map { ["name": $0.name, "index": $0.index] as [String: Any] }
        let next = "\(path.status)-\(path.supportsIPv4)-\(path.supportsIPv6)-\(interfaces)"
        lock.lock()
        let changed = next != fingerprint, hadPath = !fingerprint.isEmpty
        if changed {
            fingerprint = next
            candidate = path.availableInterfaces.first { [.wifi,.wiredEthernet,.cellular].contains($0.type) && !$0.name.hasPrefix("utun") }
            value = ["generation": ((value["generation"] as? UInt64) ?? 0) + 1,
                     "satisfied": path.status == .satisfied, "ipv4": path.supportsIPv4,
                     "ipv6": path.supportsIPv6, "interfaces": interfaces,
                     "physical_egress_verified": false]
        }
        lock.unlock()
        if changed && hadPath { didChange?() }
    }
    func snapshot() -> [String: Any] { lock.lock(); defer { lock.unlock() }; return value }
    var candidateInterface: UInt32? {
        let interfaces = snapshot()["interfaces"] as? [[String: Any]]
        return (interfaces?.first?["index"] as? Int).flatMap(UInt32.init(exactly:))
    }
    var candidateNetworkInterface: NWInterface? { lock.lock(); defer { lock.unlock() }; return candidate }
}
