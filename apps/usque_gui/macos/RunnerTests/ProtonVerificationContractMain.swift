import Foundation

/// Foundation-only executable: no Cocoa, app, browser, Keychain or networking.
@main
enum ProtonVerificationContractMain {
  static func main() throws {
    try frames()
    try integerBindings()
    try expiryAndMethods()
    try captchaResults()
    try routeResults()
    try terminalResults()
    print("6 Proton verification contract groups passed")
  }

  private static func require(_ value: @autoclosure () throws -> Bool) throws {
    let passed = try value()
    if !passed { throw NSError(domain: "ProtonContractFixture", code: 1) }
  }

  private static func binding(_ json: String) throws -> ProtonVerificationContract.Binding {
    try ProtonVerificationContract.Binding(Data(json.utf8), now: 100_000)
  }

  private static func rejectedHandoff(_ json: String) throws {
    do {
      _ = try binding(json)
      try require(false)
    } catch ProtonVerificationBridgeError.invalidHandoff {}
  }

  private static func wrapped(_ message: String) throws -> Data {
    try JSONSerialization.data(withJSONObject: ["status": "success", "message": message])
  }

  private static let valid = #"{"version":2,"route_epoch":1,"expires_at":101000,"methods":["captcha"]}"#
  private static let captcha = #"{"type":"HUMAN_VERIFICATION_SUCCESS","payload":{"type":"captcha","token":"synthetic-captcha-token"}}"#

  private static func frames() throws {
    let bytes = Data("synthetic-only".utf8)
    let frame = try ProtonVerificationContract.frame(bytes, maximum: 128)
    try require(try ProtonVerificationContract.payload(frame, maximum: 128) == bytes)
    for maximum in [-1, 0, Int.max] {
      do {
        _ = try ProtonVerificationContract.frame(bytes, maximum: maximum)
        try require(false)
      } catch ProtonVerificationBridgeError.invalidFrame {}
      do {
        _ = try ProtonVerificationContract.payload(frame, maximum: maximum)
        try require(false)
      } catch ProtonVerificationBridgeError.invalidFrame {}
    }
    for malformed in [Data(), Data([0, 0, 0, 0]), frame.dropLast(), frame + Data([0])] {
      do {
        _ = try ProtonVerificationContract.payload(Data(malformed), maximum: 128)
        try require(false)
      } catch ProtonVerificationBridgeError.invalidFrame {}
    }
  }

  private static func integerBindings() throws {
    try require(try binding(valid).routeEpoch == 1)
    for replacement in ["true", "false", "1.0", "\"1\""] {
      try rejectedHandoff(valid.replacingOccurrences(of: "\"route_epoch\":1", with: "\"route_epoch\":\(replacement)"))
    }
    for replacement in ["true", "2.0", "\"2\"", "null"] {
      try rejectedHandoff(valid.replacingOccurrences(of: "\"version\":2", with: "\"version\":\(replacement)"))
    }
    for replacement in ["true", "101000.0", "\"101000\""] {
      try rejectedHandoff(valid.replacingOccurrences(of: "101000", with: replacement))
    }
  }

  private static func expiryAndMethods() throws {
    try rejectedHandoff(valid.replacingOccurrences(of: "101000", with: "100000"))
    try rejectedHandoff(valid.replacingOccurrences(of: "101000", with: "400001"))
    for methods in ["[]", "[\"captcha\",\"captcha\"]", "[\"unknown\"]"] {
      try rejectedHandoff(valid.replacingOccurrences(of: "[\"captcha\"]", with: methods))
    }
  }

  private static func captchaResults() throws {
    let expected = try binding(valid)
    let result = try ProtonVerificationContract.result(wrapped(captcha), binding: expected, now: 100_001)
    if case .success(let bytes) = result { try require(bytes == Data(captcha.utf8)) }
    else { try require(false) }
    for bad in [
      captcha.replacingOccurrences(of: "\"captcha\"", with: "\"sms\""),
      captcha.replacingOccurrences(of: "synthetic-captcha-token", with: ""),
      captcha.replacingOccurrences(of: "synthetic-captcha-token", with: "token with spaces"),
      captcha.replacingOccurrences(of: "\"type\":\"captcha\"", with: "\"type\":\"captcha\",\"extra\":true"),
    ] {
      do {
        _ = try ProtonVerificationContract.result(wrapped(bad), binding: expected, now: 100_001)
        try require(false)
      } catch ProtonVerificationBridgeError.invalidResult {}
    }
  }

  private static func routeResults() throws {
    let expected = try binding(valid)
    let message = #"{"type":"USQUE_VERIFICATION_NETWORK_FAILED","route_epoch":1}"#
    if case .success = try ProtonVerificationContract.result(wrapped(message), binding: expected, now: 100_001) {}
    else { try require(false) }
    for epoch in ["0", "true", "false", "1.0", "\"1\""] {
      let bad = message.replacingOccurrences(of: "\"route_epoch\":1", with: "\"route_epoch\":\(epoch)")
      do {
        _ = try ProtonVerificationContract.result(wrapped(bad), binding: expected, now: 100_001)
        try require(false)
      } catch ProtonVerificationBridgeError.invalidResult {}
    }
    do {
      _ = try ProtonVerificationContract.result(wrapped(captcha), binding: expected, now: 101_000)
      try require(false)
    } catch ProtonVerificationBridgeError.invalidResult {}
  }

  private static func terminalResults() throws {
    let expected = try binding(valid)
    if case .cancelled = try ProtonVerificationContract.result(Data(#"{"status":"cancelled"}"#.utf8), binding: expected) {}
    else { try require(false) }
    if case .failure(let code) = try ProtonVerificationContract.result(
      Data(#"{"status":"error","code":"PROTON_VERIFICATION_LOAD"}"#.utf8), binding: expected) {
      try require(code == "PROTON_VERIFICATION_LOAD")
    } else { try require(false) }
    for malformed in [
      #"{"status":"cancelled","message":"unexpected"}"#,
      #"{"status":"error","code":"OTHER_ERROR"}"#,
      #"{"status":"success","message":"{}","extra":true}"#,
    ] {
      do {
        _ = try ProtonVerificationContract.result(Data(malformed.utf8), binding: expected)
        try require(false)
      } catch ProtonVerificationBridgeError.invalidResult {}
    }
  }
}
