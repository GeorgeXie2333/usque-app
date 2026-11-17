import Foundation
import Network
import Darwin

final class PhysicalNetwork {
    static let shared = PhysicalNetwork()
    private let lock = NSLock()
    private let monitor = NWPathMonitor()
    private var state = PhysicalPathState()
    private var candidate: NWInterface?
    var didChange: (() -> Void)?
    init() {
        monitor.pathUpdateHandler = { [weak self] path in self?.update(path) }
        monitor.start(queue: DispatchQueue(label: "usque.physical-path"))
    }
    private func update(_ path: NWPath) {
        // A candidate interface is an observation, never proof that sockets bypass
        // enforceRoutes. The signed PoC separately records actual egress packets.
        let observed = path.availableInterfaces.filter {
            [.wifi, .wiredEthernet, .cellular].contains($0.type) && !$0.name.hasPrefix("utun")
        }
        let addresses = Self.addresses(for:Set(observed.map(\.name)))
        let interfaces = observed.map {
            PhysicalInterfaceObservation(name:$0.name,index:$0.index,type:String(describing:$0.type),
                                         isUsed:path.usesInterfaceType($0.type),addresses:addresses?[$0.name] ?? [])
        }
        lock.lock()
        let changed = state.update(satisfied:path.status == .satisfied && addresses != nil,ipv4:path.supportsIPv4,
                                   ipv6:path.supportsIPv6,interfaces:interfaces)
        let selected = state.snapshot.selectedInterface
        candidate = path.availableInterfaces.first { $0.index == selected?.index && $0.name == selected?.name }
        lock.unlock()
        if changed { didChange?() }
    }
    func current() -> PhysicalPathSnapshot { lock.lock(); defer { lock.unlock() }; return state.snapshot }
    func snapshot() -> [String: Any] { current().object }
    var candidateInterface: UInt32? {
        current().selectedInterface.flatMap { UInt32(exactly:$0.index) }
    }
    var candidateNetworkInterface: NWInterface? { lock.lock(); defer { lock.unlock() }; return candidate }
    private static func addresses(for names:Set<String>) -> [String:[String]]? {
        var list:UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&list) == 0 else { return nil }
        defer { freeifaddrs(list) }
        var result:[String:[String]] = [:], current = list
        while let entry = current?.pointee {
            defer { current = entry.ifa_next }
            let name = String(cString:entry.ifa_name)
            guard names.contains(name), let address = entry.ifa_addr,
                  [UInt8(AF_INET),UInt8(AF_INET6)].contains(address.pointee.sa_family) else { continue }
            var host = [CChar](repeating:0,count:Int(NI_MAXHOST))
            guard getnameinfo(address,socklen_t(address.pointee.sa_len),&host,socklen_t(host.count),nil,0,NI_NUMERICHOST) == 0 else { return nil }
            result[name,default:[]].append(String(cString:host))
        }
        return result
    }
}
