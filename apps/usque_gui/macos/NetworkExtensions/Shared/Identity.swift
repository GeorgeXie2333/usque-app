import Foundation

enum ProviderRole: String, CaseIterable {
    case packet, transparent, dns, filter
    var suffix: String {
        switch self {
        case .packet: return "packet-tunnel"
        case .transparent: return "transparent-proxy"
        case .dns: return "dns-proxy"
        case .filter: return "content-filter"
        }
    }
    var identifier: String { ProviderIdentity.prefix + "." + suffix }
}

enum ProviderIdentity {
    #if SIGNED_POC
    static let prefix = "io.github.georgexie2333.usque.poc"
    static let signedPrototype = true
    #else
    static let prefix = "io.github.georgexie2333.usque"
    static let signedPrototype = false
    #endif
    static let control = prefix + ".macos-control"
    static var identity: [String: String]? {
        guard let pointer = usque_native_self_identity() else { return nil }
        defer { usque_native_free(pointer) }
        return (try? JSONSerialization.jsonObject(with: Data(String(cString: pointer).utf8))) as? [String: String]
    }
    static func team() throws -> String {
        guard let team = identity?["team"], team.range(of: "^[A-Z0-9]{10}$", options: .regularExpression) != nil else { throw NativeFailure.signatureRequired }
        return team
    }
    static func requirement(_ identifiers: [String]) throws -> String {
        let team = try team()
        return "anchor apple generic and certificate leaf[subject.OU] = \"\(team)\" and (" +
            identifiers.map { "identifier \"\($0)\"" }.joined(separator: " or ") + ")"
    }
    static func service(_ role: ProviderRole) throws -> String { try team() + "." + prefix + ".network." + role.rawValue }
    static func gate() throws {
        _ = try team()
        // Production builds cannot opt in with an env var or editable acceptance
        // file. A distinct signed PoC bundle may run to collect real evidence.
        guard signedPrototype else { throw NativeFailure.productionEvidenceRequired }
    }
}
