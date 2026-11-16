import Foundation

final class ControlChannel {
    private let role: ProviderRole
    private var listener: UnsafeMutableRawPointer?
    var handler: ((Data, String, String) -> Data)?
    init(_ role: ProviderRole) { self.role = role }
    func start() throws {
        try ProviderIdentity.gate()
        let identifiers = [ProviderIdentity.control, ProviderIdentity.prefix,
                           "io.github.georgexie2333.usque.engine"] + ProviderRole.allCases.map(\.identifier)
        listener = try ProviderIdentity.service(role).withCString { service in
            try ProviderIdentity.requirement(identifiers).withCString { requirement in
                usque_native_xpc_listen(service, requirement, { context, identity, owner, bytes, length in
                    guard let context, let bytes else { return nil }
                    let channel = Unmanaged<ControlChannel>.fromOpaque(context).takeUnretainedValue()
                    let peer = identity.flatMap { try? JSONSerialization.jsonObject(with: Data(String(cString: $0).utf8)) } as? [String: String]
                    let reply = channel.handler?(Data(bytes: bytes, count: length), peer?["identifier"] ?? "", owner.map(String.init(cString:)) ?? "") ?? encodeJSON(["error_code":"provider_unavailable"])
                    return String(data: reply, encoding: .utf8).flatMap { strdup($0) }
                }, Unmanaged.passUnretained(self).toOpaque())
            }
        }
        guard listener != nil else { throw NativeFailure.providerUnavailable }
    }
}

final class ProviderClient {
    private let lock = NSLock()
    private let role: ProviderRole
    private var connection: UnsafeMutableRawPointer?
    init(_ role: ProviderRole) { self.role = role }
    deinit { if let connection { usque_native_xpc_close(connection) } }
    func request(_ data: Data, timeout: UInt32 = 10_000) throws -> Data {
        lock.lock(); defer { lock.unlock() }
        if connection == nil {
            connection = try ProviderIdentity.service(role).withCString { service in
                try ProviderIdentity.requirement([role.identifier]).withCString { requirement in
                    usque_native_xpc_open(service, requirement)
                }
            }
        }
        let pointer = data.withUnsafeBytes { buffer in
            usque_native_xpc_request(connection, buffer.bindMemory(to: UInt8.self).baseAddress, data.count, timeout)
        }
        guard let pointer else {
            if let connection { usque_native_xpc_close(connection) }; connection = nil
            throw NativeFailure.providerUnavailable
        }
        defer { usque_native_free(pointer) }
        return Data(String(cString: pointer).utf8)
    }
    func native(_ type: String, _ payload: [String: Any] = [:]) throws -> [String: Any] {
        var object = payload; object["native_action"] = type; object["version"] = 1
        guard let result = try JSONSerialization.jsonObject(with: request(encodeJSON(object))) as? [String: Any] else { throw NativeFailure.invalidEnvelope }
        if let error = result["error_code"] as? String { throw NSError(domain: "UsqueNative", code: 1, userInfo: [NSLocalizedDescriptionKey: error]) }
        return result
    }
}

final class ProviderServices {
    static let shared = ProviderServices()
    let filter = ProviderClient(.filter)
    let packet = ProviderClient(.packet)
    let dns = ProviderClient(.dns)
    let transparent = ProviderClient(.transparent)
    func client(_ role: ProviderRole) -> ProviderClient {
        switch role { case .packet: return packet; case .filter: return filter; case .dns: return dns; case .transparent: return transparent }
    }
}
