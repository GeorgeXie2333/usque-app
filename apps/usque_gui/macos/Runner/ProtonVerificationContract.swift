import Foundation

enum ProtonVerificationBridgeError: Error {
  case invalidFrame
  case invalidResult
  case invalidHandoff
}

/// The helper is a separate sandboxed CEF application. Its only transport is
/// one bounded input frame and one bounded output frame on inherited pipes.
enum ProtonVerificationContract {
  static let maximumHandoff = 65_536
  static let maximumResult = 131_072

  struct Binding {
    let expiresAt: Double
    let routeEpoch: Int?
    let methods: Set<String>

    init(_ handoff: Data, now: Double = Date().timeIntervalSince1970 * 1000) throws {
      guard !handoff.isEmpty, handoff.count <= maximumHandoff,
        let fields = try JSONSerialization.jsonObject(with: handoff) as? [String: Any],
        let expiry = fields["expires_at"] as? NSNumber,
        let methods = fields["methods"] as? [String], !methods.isEmpty,
        Set(methods).count == methods.count,
        Set(methods).isSubset(of: ["captcha", "email", "sms"]),
        expiry.doubleValue > now, expiry.doubleValue - now <= 300_000
      else { throw ProtonVerificationBridgeError.invalidHandoff }
      expiresAt = expiry.doubleValue
      self.methods = Set(methods)
      if let version = fields["version"] as? Int {
        guard version == 2, let epoch = fields["route_epoch"] as? Int, (0...1).contains(epoch)
        else { throw ProtonVerificationBridgeError.invalidHandoff }
        routeEpoch = epoch
      } else { routeEpoch = nil }
    }
  }

  enum Result {
    case success(Data)
    case cancelled
    case failure(String)
  }

  static func frame(_ payload: Data, maximum: Int) throws -> Data {
    guard !payload.isEmpty, payload.count <= maximum else { throw ProtonVerificationBridgeError.invalidFrame }
    let length = UInt32(payload.count)
    var bytes = Data([UInt8(length >> 24), UInt8((length >> 16) & 255), UInt8((length >> 8) & 255), UInt8(length & 255)])
    bytes.append(payload)
    return bytes
  }

  static func payload(_ frame: Data, maximum: Int) throws -> Data {
    guard frame.count >= 4, frame.count <= maximum + 4 else { throw ProtonVerificationBridgeError.invalidFrame }
    let size = frame.prefix(4).reduce(0) { ($0 << 8) | Int($1) }
    guard size > 0, size <= maximum, frame.count == size + 4 else { throw ProtonVerificationBridgeError.invalidFrame }
    return Data(frame.dropFirst(4))
  }

  /// The CEF helper first validates with native/proton_verification.h. This
  /// final check binds its response to this launch's route, expiry and methods.
  static func result(_ payload: Data, binding: Binding, now: Double = Date().timeIntervalSince1970 * 1000) throws -> Result {
    guard !payload.isEmpty, payload.count <= maximumResult,
      let fields = try JSONSerialization.jsonObject(with: payload) as? [String: Any],
      let status = fields["status"] as? String else { throw ProtonVerificationBridgeError.invalidResult }
    switch status {
    case "cancelled":
      guard fields.count == 1 else { throw ProtonVerificationBridgeError.invalidResult }
      return .cancelled
    case "error":
      guard fields.count == 2, let code = fields["code"] as? String,
        code.hasPrefix("PROTON_VERIFICATION_"), code.utf8.count <= 128,
        code.utf8.allSatisfy({ (65...90).contains($0) || $0 == 95 || (48...57).contains($0) })
      else { throw ProtonVerificationBridgeError.invalidResult }
      return .failure(code)
    case "success":
      guard fields.count == 2, binding.expiresAt > now,
        let message = fields["message"] as? String, let bytes = message.data(using: .utf8),
        !bytes.isEmpty, bytes.count <= 32_768,
        let answer = try JSONSerialization.jsonObject(with: bytes) as? [String: Any], answer.count == 2,
        let type = answer["type"] as? String else { throw ProtonVerificationBridgeError.invalidResult }
      if type == "USQUE_VERIFICATION_NETWORK_FAILED" {
        guard let epoch = answer["route_epoch"] as? Int, binding.routeEpoch == epoch else {
          throw ProtonVerificationBridgeError.invalidResult
        }
      } else {
        guard type == "HUMAN_VERIFICATION_SUCCESS", let data = answer["payload"] as? [String: Any], data.count == 2,
          let method = data["type"] as? String, binding.methods.contains(method),
          let token = data["token"] as? String, !token.isEmpty, token.utf8.count <= 8192,
          token.utf8.allSatisfy({ (0x21...0x7e).contains($0) })
        else { throw ProtonVerificationBridgeError.invalidResult }
      }
      return .success(bytes)
    default: throw ProtonVerificationBridgeError.invalidResult
    }
  }
}
