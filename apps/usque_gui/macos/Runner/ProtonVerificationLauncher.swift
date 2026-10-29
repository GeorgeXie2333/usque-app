import Cocoa
import Darwin
import FlutterMacOS
import Security

/// Launches only the embedded CEF verifier after checking its code identity.
/// Challenge and authentication proxy secrets never enter arguments, logs,
/// environment variables or temporary files.
final class ProtonVerificationLauncher {
  private static let helperIdentifier = "io.github.georgexie2333.usque.proton-verification"
  private final class Session {
    let identifier = UUID()
    let process = Process()
    let input = Pipe()
    let output = Pipe()
    let result: FlutterResult
    private let lock = NSLock()
    private var cancelled = false
    init(result: @escaping FlutterResult) { self.result = result }
    var isCancelled: Bool { lock.lock(); defer { lock.unlock() }; return cancelled }
    func stop() {
      lock.lock(); cancelled = true; lock.unlock()
      try? input.fileHandleForWriting.close()
      if process.isRunning {
        process.terminate()
        DispatchQueue.global().asyncAfter(deadline: .now() + 2) { [self] in
          if process.isRunning { _ = Darwin.kill(process.processIdentifier, SIGKILL) }
        }
      }
    }
  }
  private var active: Session?

  deinit { active?.stop() }

  func begin(_ handoff: Data, result: @escaping FlutterResult) {
    cancel()
    let binding: ProtonVerificationContract.Binding
    let executable: URL
    let requirement: SecRequirement
    do {
      binding = try ProtonVerificationContract.Binding(handoff)
      (executable, requirement) = try validatedHelper()
    } catch {
      result(unavailable())
      return
    }
    let session = Session(result: result)
    session.process.executableURL = executable
    session.process.arguments = []
    session.process.environment = ["PATH": "/usr/bin:/bin:/usr/sbin:/sbin"]
    session.process.standardInput = session.input
    session.process.standardOutput = session.output
    session.process.standardError = FileHandle.nullDevice
    active = session
    let identifier = session.identifier
    let remaining = max(0, min(300, (binding.expiresAt - Date().timeIntervalSince1970 * 1000) / 1000))
    DispatchQueue.main.asyncAfter(deadline: .now() + remaining) { [weak self] in
      self?.finish(identifier, value: FlutterError(code: "PROTON_VERIFICATION_EXPIRED", message: "The verification challenge expired.", details: nil))
    }
    DispatchQueue.global(qos: .userInitiated).async { [weak self] in
      var input = Data()
      var output = Data()
      defer {
        input.resetBytes(in: 0..<input.count)
        output.resetBytes(in: 0..<output.count)
      }
      do {
        guard !session.isCancelled else { return }
        try session.process.run()
        guard !session.isCancelled, Self.validRunningHelper(session.process, requirement: requirement) else {
          throw ProtonVerificationBridgeError.invalidResult
        }
        input = try ProtonVerificationContract.frame(handoff, maximum: ProtonVerificationContract.maximumHandoff)
        try session.input.fileHandleForWriting.write(contentsOf: input)
        input.resetBytes(in: 0..<input.count)
        // Keep stdin open: EOF is the helper's parent-loss cancellation signal.
        while let chunk = try session.output.fileHandleForReading.read(upToCount: 16_384), !chunk.isEmpty {
          guard output.count + chunk.count <= ProtonVerificationContract.maximumResult + 4 else {
            throw ProtonVerificationBridgeError.invalidFrame
          }
          output.append(chunk)
          if output.count >= 4 {
            let size = output.prefix(4).reduce(0) { ($0 << 8) | Int($1) }
            guard size > 0, size <= ProtonVerificationContract.maximumResult else {
              throw ProtonVerificationBridgeError.invalidFrame
            }
          }
        }
        session.process.waitUntilExit()
        guard session.process.terminationStatus == 0 else { throw ProtonVerificationBridgeError.invalidResult }
        let payload = try ProtonVerificationContract.payload(output, maximum: ProtonVerificationContract.maximumResult)
        let answer = try ProtonVerificationContract.result(payload, binding: binding)
        DispatchQueue.main.async { [weak self] in
          switch answer {
          case .success(let bytes): self?.finish(session.identifier, value: FlutterStandardTypedData(bytes: bytes))
          case .cancelled: self?.finish(session.identifier, value: nil)
          case .failure(let code): self?.finish(session.identifier, value: FlutterError(code: code, message: "The official verification window could not complete the challenge.", details: nil))
          }
        }
      } catch {
        session.stop()
        DispatchQueue.main.async { [weak self] in self?.finish(session.identifier, value: self?.unavailable()) }
      }
    }
  }

  func cancel() {
    guard let active else { return }
    finish(active.identifier, value: nil)
  }

  private func finish(_ identifier: UUID, value: Any?) {
    guard let session = active, session.identifier == identifier else { return }
    active = nil
    session.stop()
    session.result(value)
  }

  private func unavailable() -> FlutterError {
    FlutterError(code: "PROTON_VERIFICATION_UNAVAILABLE", message: "The bundled official verification component is unavailable or could not be verified.", details: nil)
  }

  private func validatedHelper() throws -> (URL, SecRequirement) {
    let contents = Bundle.main.bundleURL.appendingPathComponent("Contents", isDirectory: true).resolvingSymlinksInPath()
    let helperURL = contents.appendingPathComponent("Helpers/UsqueProtonVerification.app", isDirectory: true)
    guard helperURL.resolvingSymlinksInPath() == helperURL,
      let bundle = Bundle(url: helperURL), bundle.bundleIdentifier == Self.helperIdentifier,
      let executable = bundle.executableURL,
      executable.resolvingSymlinksInPath() == executable,
      executable.deletingLastPathComponent().path == helperURL.appendingPathComponent("Contents/MacOS").path
    else { throw ProtonVerificationBridgeError.invalidResult }
    var own: SecCode?
    guard SecCodeCopySelf(SecCSFlags(), &own) == errSecSuccess, let own else {
      throw ProtonVerificationBridgeError.invalidResult
    }
    var information: CFDictionary?
    var ownStatic: SecStaticCode?
    guard SecCodeCopyStaticCode(own, SecCSFlags(), &ownStatic) == errSecSuccess, let ownStatic,
      SecCodeCopySigningInformation(ownStatic, SecCSFlags(rawValue: kSecCSSigningInformation), &information) == errSecSuccess,
      let fields = information as? [String: Any] else { throw ProtonVerificationBridgeError.invalidResult }
    let team = fields[kSecCodeInfoTeamIdentifier as String] as? String
    let identity: String
    if let team, team.utf8.allSatisfy({ (48...57).contains($0) || (65...90).contains($0) }), !team.isEmpty {
      identity = "anchor apple generic and identifier \"\(Self.helperIdentifier)\" and certificate leaf[subject.OU] = \"\(team)\""
    } else {
      #if DEBUG
      // Ad-hoc acceptance exists only in an explicitly compiled debug Runner.
      // Release bundles always require an Apple-issued identity and same Team.
      identity = "identifier \"\(Self.helperIdentifier)\""
      #else
      throw ProtonVerificationBridgeError.invalidResult
      #endif
    }
    var requirement: SecRequirement?
    guard SecRequirementCreateWithString(identity as CFString, SecCSFlags(), &requirement) == errSecSuccess,
      let requirement else { throw ProtonVerificationBridgeError.invalidResult }
    var code: SecStaticCode?
    guard SecStaticCodeCreateWithPath(helperURL as CFURL, SecCSFlags(), &code) == errSecSuccess, let code,
      SecStaticCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate | kSecCSCheckNestedCode | kSecCSCheckAllArchitectures), requirement) == errSecSuccess
    else { throw ProtonVerificationBridgeError.invalidResult }
    return (executable, requirement)
  }

  private static func validRunningHelper(_ process: Process, requirement: SecRequirement) -> Bool {
    guard process.isRunning else { return false }
    var code: SecCode?
    let attributes = [kSecGuestAttributePid as String: NSNumber(value: process.processIdentifier)] as CFDictionary
    guard SecCodeCopyGuestWithAttributes(nil, attributes, SecCSFlags(), &code) == errSecSuccess, let code else { return false }
    return SecCodeCheckValidity(code, SecCSFlags(rawValue: kSecCSStrictValidate), requirement) == errSecSuccess
  }
}
