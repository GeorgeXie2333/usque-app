import Foundation
import Network
import NetworkExtension
import Darwin

/// Lives in the independent filter process. Its startup default is blocked even
/// after a restart with no credentials; controller disconnection never disarms it.
final class GuardStore {
    static let shared = GuardStore()
    private let lock = NSRecursiveLock()
    let state = NativeState()
    private var channel: ControlChannel?
    private var armed = true
    private var tunnel: String?
    private var leases: [[String: Any]] = []
    private var intents: [[String: Any]] = []
    private var flows: [UUID: [String: Any]] = [:]
    func start() throws {
        lock.lock(); defer { lock.unlock() }
        if channel != nil { return }
        let channel = ControlChannel(.filter)
        channel.handler = { [weak self] data, peer, owner in self?.request(data, peer: peer, owner: owner) ?? Data() }
        try channel.start(); self.channel = channel
        PhysicalNetwork.shared.didChange = { [weak self] in self?.block("physical_network_changed") }
    }
    func block(_ error: String? = nil) {
        lock.lock(); defer { lock.unlock() }
        armed = true; leases.removeAll(); intents.removeAll(); tunnel = nil
        state.transition("protected", error: error)
    }
    func request(_ data: Data, peer: String, owner: String) -> Data {
        lock.lock(); defer { lock.unlock() }
        do {
            guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else { throw NativeFailure.invalidEnvelope }
            if object["native_event"] as? String == "peer_disconnected" {
                leases.removeAll { $0["owner"] as? String == owner }; return encodeJSON(["accepted":true])
            }
            let isController = [ProviderIdentity.control, ProviderIdentity.prefix, "io.github.georgexie2333.usque.engine"].contains(peer)
            if let action = object["native_action"] as? String {
                guard object["version"] as? Int == 1 else { throw NativeFailure.unsupportedVersion }
                switch action {
                case "renew":
                    guard [ProviderRole.packet,.transparent,.dns].map(\.identifier).contains(peer),
                          let key = sessionObject(object["session"]), key == state.key,
                          PhysicalNetwork.shared.current().permits(generation:key.network_generation) else { throw NativeFailure.staleSession }
                    prune()
                    for index in leases.indices where leases[index]["owner"] as? String == owner {
                        leases[index]["expires"] = Date().addingTimeInterval(30)
                    }
                    return encodeJSON(["accepted":true])
                case "record_flow":
                    guard peer == ProviderRole.filter.identifier, Self.validTuple(object),
                          let id = (object["flow_id"] as? String).flatMap(UUID.init(uuidString:)) else { throw NativeFailure.permissionDenied }
                    prune(); guard flows.count < 4096 else { throw NativeFailure.providerUnavailable }
                    var flow = object; flow["expires"] = Date().addingTimeInterval(30)
                    if let encoded = object["source_audit"] as? String, let audit = Data(base64Encoded:encoded),
                       let requirement = try? ProviderIdentity.requirement(["io.github.georgexie2333.usque.engine"]) {
                        let result = audit.withUnsafeBytes { bytes in requirement.withCString { usque_native_audit_identity(bytes.bindMemory(to: UInt8.self).baseAddress, audit.count, $0) } }
                        if let result { flow["engine_authenticated"] = true; usque_native_free(result) }
                    }
                    flows[id] = flow; return encodeJSON(["accepted":true])
                case "lease":
                    guard [ProviderRole.packet, .transparent, .dns].map(\.identifier).contains(peer),
                          let key = sessionObject(object["session"]), key == state.key,
                          PhysicalNetwork.shared.current().permits(generation:key.network_generation),
                          let local = object["local_ip"] as? String, Self.numericIP(local),
                          let port = object["local_port"] as? Int, (1...65535).contains(port),
                          let remote = object["remote_ip"] as? String, Self.numericIP(remote),
                          let remotePort = object["remote_port"] as? Int, (1...65535).contains(remotePort),
                          let proto = object["protocol"] as? Int, [6,17].contains(proto),
                          let interface = object["interface_index"] as? Int,
                          PhysicalNetwork.shared.current().permits(generation:key.network_generation,interface:interface)
                    else { throw NativeFailure.staleSession }
                    prune(); guard leases.count < 1024 else { throw NativeFailure.providerUnavailable }
                    var lease = object; lease["owner"] = owner; lease["expires"] = Date().addingTimeInterval(30)
                    leases.append(lease); return encodeJSON(["accepted":true,"physical_egress_verified":false])
                case "commit_tunnel":
                    guard peer == ProviderRole.packet.identifier,
                          let key = sessionObject(object["session"]), key == state.key,
                          PhysicalNetwork.shared.current().permits(generation:key.network_generation),
                          let name = object["interface"] as? String, name.range(of: "^utun[0-9]+$", options: .regularExpression) != nil else { throw NativeFailure.staleSession }
                    tunnel = name; return encodeJSON(["accepted":true])
                case "arm":
                    guard [ProviderRole.packet, .dns, .transparent].map(\.identifier).contains(peer) || isController else { throw NativeFailure.permissionDenied }
                    block(); return encodeJSON(["accepted":true])
                case "allow":
                    guard peer == ProviderRole.packet.identifier, let key = sessionObject(object["session"]), key == state.key,
                          PhysicalNetwork.shared.current().permits(generation:key.network_generation), tunnel != nil else { throw NativeFailure.staleSession }
                    state.transition("connected", key: state.key); return encodeJSON(["accepted":true])
                case "flow_intent":
                    guard isController, let key = sessionObject(object["session"]), key == state.key,
                          PhysicalNetwork.shared.current().permits(generation:key.network_generation),
                          Self.validTuple(object) else { throw NativeFailure.invalidEnvelope }
                    if let hostname = object["hostname"], !(hostname is NSNull) {
                        guard let hostname = hostname as? String, !hostname.isEmpty, hostname.utf8.count <= 253 else { throw NativeFailure.invalidEnvelope }
                    }
                    prune(); guard intents.count < 4096 else { throw NativeFailure.providerUnavailable }
                    var intent = object; intent["expires"] = Date().addingTimeInterval(30); intents.append(intent)
                    return encodeJSON(["accepted":true])
                case "resolve_intent":
                    guard peer == ProviderRole.transparent.identifier || peer == ProviderRole.dns.identifier,
                          let id = (object["flow_id"] as? String).flatMap(UUID.init(uuidString:)),
                          let flow = flows.removeValue(forKey: id), flow["engine_authenticated"] as? Bool == true else { throw NativeFailure.permissionDenied }
                    prune()
                    guard let index = intents.firstIndex(where: { Self.tupleMatches($0, flow) }) else { throw NativeFailure.invalidEnvelope }
                    return encodeJSON(["accepted":true,"hostname":intents.remove(at: index)["hostname"] ?? NSNull()])
                case "snapshot": return encodeJSON(["accepted":true,"snapshot":state.snapshot(),"physical_network":PhysicalNetwork.shared.snapshot(),"tunnel_interface":tunnel ?? "","lease_count":leases.count])
                default: throw NativeFailure.invalidEnvelope
                }
            }
            guard isController else { throw NativeFailure.permissionDenied }
            let request = try ControlEnvelope(data)
            return try state.transact(request) {
                switch request.type {
                case "protect", "start", "replace":
                    block(); state.transition("protected", key: request.session)
                case "stop":
                    // Only an explicit, fenced controller Stop removes protection.
                    leases.removeAll(); intents.removeAll(); tunnel = nil; armed = false
                    state.transition("stopped", key: request.session, protection: false)
                case "snapshot": break
                case "cancel": block("cancelled")
                default: throw NativeFailure.invalidEnvelope
                }
            }
        } catch { return failureReply(data, error) }
    }
    func record(_ flow: NEFilterSocketFlow) {
        guard let local = flow.localEndpoint as? NWHostEndpoint,
              let remote = flow.remoteEndpoint as? NWHostEndpoint else { return }
        var object: [String: Any] = ["local_ip":local.hostname,"local_port":Int(local.port) ?? 0,
                                     "remote_ip":remote.hostname,"remote_port":Int(remote.port) ?? 0,
                                     "protocol":Int(flow.socketProtocol),"flow_id":flow.identifier.uuidString]
        if let audit = flow.sourceAppAuditToken { object["source_audit"] = audit.base64EncodedString() }
        _ = try? ProviderServices.shared.filter.native("record_flow",object)
    }
    func allows(interface: nw_interface_t, direction: NETrafficDirection, bytes: UnsafeRawPointer, count: Int) -> Bool {
        lock.lock(); defer { lock.unlock() }
        if !armed || direction == .inbound { return true }
        let name = String(cString: nw_interface_get_name(interface))
        if name == "lo0" { return true }
        let data = Data(bytes: bytes, count: count)
        if GuardPacket.isRequiredControlPlane(data) { return true }
        // A path update publishes its new generation before its asynchronous
        // didChange callback can take this lock. Re-check at packet time so an
        // old lease/tunnel is already denied in that callback gap.
        let path = PhysicalNetwork.shared.current()
        guard let key = state.key, path.permits(generation:key.network_generation) else { return false }
        if name == tunnel { return true }
        guard path.permits(generation:key.network_generation,interface:Int(nw_interface_get_index(interface))) else { return false }
        guard let packet = Self.packetTuple(data) else { return false }
        prune()
        return leases.contains {
            sessionObject($0["session"]) == key &&
            ($0["interface_index"] as? Int) == Int(nw_interface_get_index(interface)) &&
            ($0["local_ip"] as? String) == packet.local &&
            ($0["local_port"] as? Int) == packet.port && ($0["remote_ip"] as? String) == packet.remote &&
            ($0["remote_port"] as? Int) == packet.remotePort && ($0["protocol"] as? Int) == packet.proto
        }
    }
    private func prune() {
        let now = Date()
        leases.removeAll { ($0["expires"] as? Date ?? .distantPast) < now }
        intents.removeAll { ($0["expires"] as? Date ?? .distantPast) < now }
        flows = flows.filter { ($0.value["expires"] as? Date ?? .distantPast) >= now }
    }
    private static func numericIP(_ text: String) -> Bool {
        var v4 = in_addr(), v6 = in6_addr()
        return inet_pton(AF_INET, text, &v4) == 1 || inet_pton(AF_INET6, text, &v6) == 1
    }
    private static func validTuple(_ value: [String: Any]) -> Bool {
        guard let local = value["local_ip"] as? String, numericIP(local), let remote = value["remote_ip"] as? String, numericIP(remote),
              let lport = value["local_port"] as? Int, (1...65535).contains(lport),
              let rport = value["remote_port"] as? Int, (1...65535).contains(rport),
              let proto = value["protocol"] as? Int, [6,17].contains(proto) else { return false }
        return true
    }
    private static func tupleMatches(_ a: [String: Any], _ b: [String: Any]) -> Bool {
        ["local_ip","remote_ip"].allSatisfy { a[$0] as? String == b[$0] as? String } &&
        ["local_port","remote_port","protocol"].allSatisfy { a[$0] as? Int == b[$0] as? Int }
    }
    private static func packetTuple(_ data: Data) -> (local: String, port: Int, remote: String, remotePort: Int, proto: Int)? {
        guard data.count >= 20 else { return nil }
        let version = data[0] >> 4, offset: Int, proto: Int, address: Data
        if version == 4 {
            offset = Int(data[0] & 15) * 4
            guard offset >= 20, data.count >= offset + 4, data[6] & 0x3f == 0, data[7] == 0 else { return nil }
            proto = Int(data[9]); address = data.subdata(in: 16..<20)
        } else if version == 6 {
            // IPv6 extension/fragment headers are deliberately blocked until a
            // bounded parser and their signed egress fixtures are accepted.
            offset = 40; guard data.count >= 44 else { return nil }
            proto = Int(data[6]); address = data.subdata(in: 24..<40)
        } else { return nil }
        guard [6,17].contains(proto) else { return nil }
        var output = [CChar](repeating: 0, count: Int(INET6_ADDRSTRLEN))
        let converted = address.withUnsafeBytes { inet_ntop(version == 4 ? AF_INET : AF_INET6, $0.baseAddress, &output, socklen_t(output.count)) }
        guard converted != nil else { return nil }
        let remote = String(cString:output)
        let source = version == 4 ? data.subdata(in:12..<16) : data.subdata(in:8..<24)
        guard source.withUnsafeBytes({ inet_ntop(version == 4 ? AF_INET : AF_INET6, $0.baseAddress, &output, socklen_t(output.count)) }) != nil else { return nil }
        return (String(cString:output), Int(data[offset]) * 256 + Int(data[offset+1]), remote, Int(data[offset+2]) * 256 + Int(data[offset+3]), proto)
    }
}
