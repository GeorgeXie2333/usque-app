import Foundation
import CryptoKit

enum NativeFailure: String, Error {
    case invalidFrame = "invalid_frame"
    case invalidEnvelope = "invalid_envelope"
    case unsupportedVersion = "unsupported_version"
    case staleSession = "stale_session"
    case operationConflict = "operation_conflict"
    case providerUnavailable = "provider_unavailable"
    case signatureRequired = "signature_required"
    case productionEvidenceRequired = "production_evidence_required"
    case physicalEgressUnverified = "physical_egress_unverified"
    case permissionDenied = "permission_denied"
    case awaitingApproval = "awaiting_approval"
    case extensionVersionMismatch = "extension_version_mismatch"
    case activationRequiresReboot = "activation_requires_reboot"
    case missingCredentials = "missing_credentials"
    case notConnected = "not_connected"
}

func encodeJSON(_ value: Any) -> Data {
    (try? JSONSerialization.data(withJSONObject: value, options: [.sortedKeys])) ?? Data("{}".utf8)
}

struct SessionKey: Codable, Equatable {
    let session_id: UUID
    let config_revision: UInt64
    let network_generation: UInt64
    var object: [String: Any] {
        ["session_id": session_id.uuidString.lowercased(), "config_revision": config_revision,
         "network_generation": network_generation]
    }
}

struct ControlEnvelope {
    let operation: UUID
    let session: SessionKey?
    let type: String
    let command: [String: Any]
    let object: [String: Any]
    let digest: Data
    init(_ data: Data) throws {
        guard !data.isEmpty, data.count <= FrameIO.maximum,
              let object = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let version = object["version"] as? Int else { throw NativeFailure.invalidEnvelope }
        guard version == 1 else { throw NativeFailure.unsupportedVersion }
        guard let operationText = object["operation_id"] as? String, let operation = UUID(uuidString: operationText),
              operation != UUID(uuidString:"00000000-0000-0000-0000-000000000000"),
              let command = object["command"] as? [String: Any], let type = command["type"] as? String,
              ["protect", "start", "replace", "stop", "snapshot", "cancel"].contains(type)
        else { throw NativeFailure.invalidEnvelope }
        let session = try object["session"].flatMap { value -> SessionKey? in
            if value is NSNull { return nil }
            return try JSONDecoder().decode(SessionKey.self, from: encodeJSON(value))
        }
        guard let session, session.config_revision > 0,
              session.session_id != UUID(uuidString:"00000000-0000-0000-0000-000000000000") else { throw NativeFailure.invalidEnvelope }
        self.operation = operation; self.session = session; self.type = type
        self.command = command; self.object = object
        digest = Data(SHA256.hash(data: encodeJSON(object)))
    }
    func redacted() -> Data {
        var value = object, command = self.command
        command.removeValue(forKey: "credentials")
        value["command"] = command
        return encodeJSON(value)
    }
}

/// The helper reads one bounded message at a time. A partial frame terminates
/// the connection; no unbounded read-to-EOF, newline protocol or secret in argv.
enum FrameIO {
    static let maximum = 2 * 1_048_576
    static func read(_ input: FileHandle) throws -> Data? {
        guard let header = try exactly(input, count: 4, allowEOF: true) else { return nil }
        let length = header.reduce(UInt32(0)) { ($0 << 8) | UInt32($1) }
        guard length > 0, length <= maximum else { throw NativeFailure.invalidFrame }
        return try exactly(input, count: Int(length), allowEOF: false)
    }
    private static func exactly(_ input: FileHandle, count: Int, allowEOF: Bool) throws -> Data? {
        var result = Data()
        while result.count < count {
            let bytes = try input.read(upToCount: count - result.count) ?? Data()
            if bytes.isEmpty {
                if result.isEmpty && allowEOF { return nil }
                throw NativeFailure.invalidFrame
            }
            result.append(bytes)
        }
        return result
    }
    static func write(_ data: Data, to output: FileHandle) throws {
        guard !data.isEmpty, data.count <= maximum else { throw NativeFailure.invalidFrame }
        var length = UInt32(data.count).bigEndian
        try withUnsafeBytes(of: &length) { try output.write(contentsOf: Data($0)) }
        try output.write(contentsOf: data)
    }
}

/// Native roles use the same fencing and idempotence rules; only the packet
/// provider owns the Rust DataPlaneRuntime. This class never stores credentials.
final class NativeState {
    private let lock = NSRecursiveLock()
    private(set) var key: SessionKey?
    private var phase = "protected"
    private var protection = true
    private var error: String? = NativeFailure.missingCredentials.rawValue
    private var operations: [UUID: Data] = [:]
    private var order: [UUID] = []
    func validate(_ request: ControlEnvelope) throws -> Bool {
        lock.lock(); defer { lock.unlock() }
        if let existing = operations[request.operation] {
            guard existing == request.digest else { throw NativeFailure.operationConflict }
            return false
        }
        if let next = request.session, let current = key {
            guard next.session_id == current.session_id || phase == "stopped" else { throw NativeFailure.staleSession }
            guard next.network_generation >= current.network_generation,
                  next.config_revision >= current.config_revision else { throw NativeFailure.staleSession }
            if ["stop", "cancel"].contains(request.type), next != current { throw NativeFailure.staleSession }
        }
        operations[request.operation] = request.digest; order.append(request.operation)
        if order.count > 256 { operations.removeValue(forKey: order.removeFirst()) }
        return true
    }
    func transition(_ phase: String, key: SessionKey? = nil, protection: Bool = true, error: String? = nil) {
        lock.lock(); defer { lock.unlock() }
        if let key { self.key = key }
        self.phase = phase; self.protection = protection; self.error = error
    }
    func snapshot() -> [String: Any] {
        lock.lock(); defer { lock.unlock() }
        return ["phase": phase, "session": key?.object ?? NSNull(), "protection_active": protection,
                "production_capable": false, "error_code": error ?? NSNull()]
    }
    func reply(_ request: ControlEnvelope, duplicate: Bool = false) -> Data {
        encodeJSON(["operation_id": request.operation.uuidString.lowercased(),
                    "acceptance": duplicate ? "duplicate" : "accepted", "snapshot": snapshot()])
    }
}

func failureReply(_ data: Data, _ error: Error) -> Data {
    let input = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
    let osError = error as NSError
    return encodeJSON(["operation_id": input?["operation_id"] ?? NSNull(), "acceptance": "rejected",
                       "error_code": (error as? NativeFailure)?.rawValue ?? "native_operation_failed",
                       "error_domain":osError.domain,"native_error_code":osError.code])
}
