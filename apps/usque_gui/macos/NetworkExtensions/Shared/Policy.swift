import Foundation

enum Route: Int32 { case proxy = 0, direct = 1, reject = 2 }

/// All routing decisions come from usque-core's Rust matcher, including its
/// ordering and GeoSite/GeoIP behavior. Swift only adapts platform flow metadata.
final class RoutingPolicy {
    private let lock = NSLock()
    private var handle: UnsafeMutableRawPointer?
    deinit { if let handle { usque_macos_policy_free(handle) } }
    func replace(_ configuration: [String: Any]) throws {
        let data = encodeJSON(configuration)
        let next = data.withUnsafeBytes { usque_macos_policy_new_json($0.bindMemory(to: UInt8.self).baseAddress, data.count) }
        guard let next else { throw NativeFailure.invalidEnvelope }
        lock.lock(); let old = handle; handle = next; lock.unlock()
        if let old { usque_macos_policy_free(old) }
    }
    func route(host: String?, ip: String?) -> Route {
        lock.lock(); defer { lock.unlock() }
        guard let handle else { return .reject }
        let name = Data((host ?? "").utf8), address = Data((ip ?? "").utf8)
        let result = name.withUnsafeBytes { nameBytes in address.withUnsafeBytes { addressBytes in
            usque_macos_policy_route_flow(handle,nameBytes.bindMemory(to:UInt8.self).baseAddress,name.count,
                                         addressBytes.bindMemory(to:UInt8.self).baseAddress,address.count)
        } }
        return Route(rawValue: result) ?? .reject
    }
}

func sessionObject(_ object: Any?) -> SessionKey? {
    guard let object else { return nil }
    return try? JSONDecoder().decode(SessionKey.self, from: encodeJSON(object))
}

func endpointParts(_ value: String) -> (String, UInt16)? {
    guard let url = URL(string: "tcp://" + value), let host = url.host,
          let port = url.port.flatMap(UInt16.init(exactly:)) else { return nil }
    return (host.trimmingCharacters(in: CharacterSet(charactersIn: "[]")), port)
}
