import Foundation
import NetworkExtension
import SystemExtensions

private final class ActivationRequest: NSObject, OSSystemExtensionRequestDelegate {
    let complete = DispatchSemaphore(value:0)
    var state = "querying"
    var properties: [String:Any] = [:]
    var error: Error?
    func request(_ request: OSSystemExtensionRequest, actionForReplacingExtension existing: OSSystemExtensionProperties, withExtension ext: OSSystemExtensionProperties) -> OSSystemExtensionRequest.ReplacementAction { .replace }
    func requestNeedsUserApproval(_ request: OSSystemExtensionRequest) { state = "awaiting_approval"; complete.signal() }
    func request(_ request: OSSystemExtensionRequest, didFailWithError error: Error) { self.error = error; state = "failed"; complete.signal() }
    func request(_ request: OSSystemExtensionRequest, didFinishWithResult result: OSSystemExtensionRequest.Result) {
        state = result == .completed ? "completed" : "requires_reboot"; complete.signal()
    }
    func request(_ request: OSSystemExtensionRequest, foundProperties properties: [OSSystemExtensionProperties]) {
        self.properties = ["installed":!properties.isEmpty,"enabled":properties.contains(where: \.isEnabled),
                           "awaiting_approval":properties.contains(where: \.isAwaitingUserApproval),
                           "versions":properties.map(\.bundleShortVersion)]
        state = "queried"; complete.signal()
    }
}

/// All preference writes and activation requests are explicit controller actions.
/// Merely launching the helper or asking for status does not install/start VPNs.
final class ExtensionManager {
    private var activeRequests: [(OSSystemExtensionRequest,ActivationRequest)] = []
    private var packet: NETunnelProviderManager?
    private var transparent: NETransparentProxyManager?
    private let dns = NEDNSProxyManager.shared()
    private let filter = NEFilterManager.shared()
    func status() -> [String:Any] {
        var roles: [String:Any] = [:]
        for role in ProviderRole.allCases {
            var details: [String:Any] = ["installed":false,"enabled":false]
            if ProviderIdentity.identity != nil {
                let delegate = ActivationRequest()
                let request = OSSystemExtensionRequest.propertiesRequest(forExtensionWithIdentifier:role.identifier,queue:.global())
                request.delegate = delegate; OSSystemExtensionManager.shared.submitRequest(request)
                if delegate.complete.wait(timeout:.now()+3) == .success {
                    details.merge(delegate.properties) { _,next in next }; details["activation_state"] = delegate.state
                    if let error = delegate.error as NSError? { details["error_domain"] = error.domain; details["error_code"] = error.code }
                } else { details["activation_state"] = "query_timeout" }
            } else { details["activation_state"] = "signature_required" }
            roles[role.rawValue] = details
        }
        do {
            guard ProviderIdentity.identity != nil else { throw NativeFailure.signatureRequired }
            try loadManagers()
            if var entry = roles["packet"] as? [String:Any] { entry["preference_enabled"] = packet?.isEnabled ?? false; entry["connection_status"] = packet?.connection.status.rawValue ?? 0; roles["packet"] = entry }
            if var entry = roles["transparent"] as? [String:Any] { entry["preference_enabled"] = transparent?.isEnabled ?? false; entry["connection_status"] = transparent?.connection.status.rawValue ?? 0; roles["transparent"] = entry }
            if var entry = roles["dns"] as? [String:Any] { entry["preference_enabled"] = dns.isEnabled; roles["dns"] = entry }
            if var entry = roles["filter"] as? [String:Any] { entry["preference_enabled"] = filter.isEnabled; roles["filter"] = entry }
        } catch { /* Details above still distinguish signature/activation state. */ }
        let guardNetwork = filter.isEnabled ? (try? ProviderServices.shared.filter.native("snapshot"))?["physical_network"] as? [String:Any] : nil
        return ["providers":roles,"physical_network":guardNetwork ?? PhysicalNetwork.shared.snapshot(),"physical_generation_authority":guardNetwork == nil ? "uninitialized" : "filter","signed_prototype":ProviderIdentity.signedPrototype,
                "signature_valid":ProviderIdentity.identity != nil,"production_capable":false,"real_network_acceptance":"not_run"]
    }
    func activate() throws {
        try ProviderIdentity.gate()
        for role in ProviderRole.allCases {
            let delegate = ActivationRequest()
            let request = OSSystemExtensionRequest.activationRequest(forExtensionWithIdentifier:role.identifier,queue:.global())
            request.delegate = delegate; activeRequests.append((request,delegate))
            OSSystemExtensionManager.shared.submitRequest(request)
            guard delegate.complete.wait(timeout:.now()+30) == .success else { throw NativeFailure.awaitingApproval }
            if let error = delegate.error { throw error }
            if delegate.state == "awaiting_approval" { throw NativeFailure.awaitingApproval }
            if delegate.state == "requires_reboot" { throw NativeFailure.activationRequiresReboot }
        }
    }
    private func wait(_ invoke: (@escaping (Error?)->Void)->Void) throws {
        let semaphore = DispatchSemaphore(value:0); let box = ManagerResult()
        invoke { error in box.error = error; semaphore.signal() }
        guard semaphore.wait(timeout:.now()+15) == .success else { throw NativeFailure.providerUnavailable }
        if let error = box.error { throw error }
    }
    private func loadManagers() throws {
        let completion = DispatchSemaphore(value:0); let box = ManagerResult()
        NETunnelProviderManager.loadAllFromPreferences { managers,error in
            self.packet = managers?.first { ($0.protocolConfiguration as? NETunnelProviderProtocol)?.providerBundleIdentifier == ProviderRole.packet.identifier }
            box.error = error; completion.signal()
        }
        guard completion.wait(timeout:.now()+15) == .success else { throw NativeFailure.providerUnavailable }
        if let error = box.error { throw error }
        NETransparentProxyManager.loadAllFromPreferences { managers,error in
            self.transparent = managers?.first { ($0.protocolConfiguration as? NETunnelProviderProtocol)?.providerBundleIdentifier == ProviderRole.transparent.identifier }
            box.error = error; completion.signal()
        }
        guard completion.wait(timeout:.now()+15) == .success else { throw NativeFailure.providerUnavailable }
        if let error = box.error { throw error }
        try wait { dns.loadFromPreferences(completionHandler:$0) }
        try wait { filter.loadFromPreferences(completionHandler:$0) }
    }
    private func ensureInstalled() throws {
        try ProviderIdentity.gate()
        let current = status()["providers"] as? [String:[String:Any]] ?? [:]
        guard ProviderRole.allCases.allSatisfy({ current[$0.rawValue]?["enabled"] as? Bool == true }) else {
            throw NativeFailure.awaitingApproval
        }
        guard ProviderRole.allCases.allSatisfy({ (current[$0.rawValue]?["versions"] as? [String])?.allSatisfy({ $0 == "0.1.0" }) == true }) else {
            throw NativeFailure.extensionVersionMismatch
        }
    }
    private func ready(_ role:ProviderRole) throws {
        let deadline = Date().addingTimeInterval(15)
        while Date() < deadline {
            if (try? ProviderServices.shared.client(role).native("snapshot")) != nil { return }
            // XPC provider initialization is asynchronous, not an authorization
            // acknowledgement. Retry only within this bounded startup deadline.
            Thread.sleep(forTimeInterval:0.1)
        }
        throw NativeFailure.providerUnavailable
    }
    func protect(_ envelope:ControlEnvelope) throws -> Data {
        try ensureInstalled()
        let configuration = NEFilterProviderConfiguration()
        configuration.filterPackets = true; configuration.filterSockets = true
        configuration.filterDataProviderBundleIdentifier = ProviderRole.filter.identifier
        configuration.filterPacketProviderBundleIdentifier = ProviderRole.filter.identifier
        configuration.vendorConfiguration = ["protocol_version":1]
        filter.providerConfiguration = configuration; filter.localizedDescription = "Usque Network Prototype Protection"
        filter.grade = .firewall; filter.isEnabled = true
        try wait { filter.saveToPreferences(completionHandler:$0) }; try ready(.filter)
        return try ProviderServices.shared.filter.request(envelope.redacted())
    }
    func start(_ envelope:ControlEnvelope, original:Data) throws -> Data {
        try ensureInstalled()
        guard let key = envelope.session else { throw NativeFailure.staleSession }
        // Protection exists and acknowledges the new fenced session before DNS,
        // flow classification or a packet tunnel can start opening connections.
        let protection = try protect(envelope)
        try accepted(protection)
        guard let physical = try ProviderServices.shared.filter.native("snapshot")["physical_network"] as? [String:Any],
              key.network_generation == physical["generation"] as? UInt64 else { throw NativeFailure.staleSession }
        let dnsProtocol = NEDNSProxyProviderProtocol()
        dnsProtocol.providerBundleIdentifier = ProviderRole.dns.identifier
        dnsProtocol.providerConfiguration = ["protocol_version":1]
        dns.providerProtocol = dnsProtocol; dns.localizedDescription = "Usque Network Prototype DNS"; dns.isEnabled = true
        try wait { dns.saveToPreferences(completionHandler:$0) }; try ready(.dns)
        try accepted(ProviderServices.shared.dns.request(envelope.redacted()))
        let proxy = transparent ?? NETransparentProxyManager(); transparent = proxy
        let proxyProtocol = NETunnelProviderProtocol()
        proxyProtocol.providerBundleIdentifier = ProviderRole.transparent.identifier
        proxyProtocol.serverAddress = "Usque flow policy"; proxyProtocol.providerConfiguration = ["protocol_version":1]
        proxy.protocolConfiguration = proxyProtocol; proxy.localizedDescription = "Usque Network Prototype Flows"; proxy.isEnabled = true
        try wait { proxy.saveToPreferences(completionHandler:$0) }; try wait { proxy.loadFromPreferences(completionHandler:$0) }
        if proxy.connection.status != .connected && proxy.connection.status != .connecting { try proxy.connection.startVPNTunnel() }
        try ready(.transparent); try accepted(ProviderServices.shared.transparent.request(envelope.redacted()))
        let manager = packet ?? NETunnelProviderManager(); packet = manager
        let tunnel = NETunnelProviderProtocol()
        tunnel.providerBundleIdentifier = ProviderRole.packet.identifier; tunnel.serverAddress = "Usque Packet Tunnel"
        tunnel.providerConfiguration = ["protocol_version":1]
        tunnel.enforceRoutes = true; tunnel.includeAllNetworks = false; tunnel.excludeLocalNetworks = false
        manager.protocolConfiguration = tunnel; manager.localizedDescription = "Usque Network Prototype VPN"; manager.isEnabled = true
        try wait { manager.saveToPreferences(completionHandler:$0) }; try wait { manager.loadFromPreferences(completionHandler:$0) }
        if manager.connection.status != .connected && manager.connection.status != .connecting { try manager.connection.startVPNTunnel() }
        try ready(.packet)
        return try ProviderServices.shared.packet.request(original)
    }
    func stop(_ envelope:ControlEnvelope, original:Data) throws -> Data {
        try ProviderIdentity.gate(); try loadManagers()
        let result = try ProviderServices.shared.packet.request(original); try accepted(result)
        let deadline = Date().addingTimeInterval(20)
        var stopped = false
        while Date() < deadline {
            var snapshot = envelope.object, command = envelope.command
            command["type"] = "snapshot"; snapshot["command"] = command; snapshot["operation_id"] = UUID().uuidString
            let reply = try ProviderServices.shared.packet.request(encodeJSON(snapshot))
            let object = try JSONSerialization.jsonObject(with:reply) as? [String:Any]
            if (object?["snapshot"] as? [String:Any])?["phase"] as? String == "stopped" { stopped = true; break }
            Thread.sleep(forTimeInterval:0.1)
        }
        guard stopped else { throw NativeFailure.providerUnavailable }
        packet?.connection.stopVPNTunnel()
        if let packet { packet.isEnabled = false; try wait { packet.saveToPreferences(completionHandler:$0) } }
        try accepted(ProviderServices.shared.transparent.request(envelope.redacted()))
        transparent?.connection.stopVPNTunnel()
        if let transparent { transparent.isEnabled = false; try wait { transparent.saveToPreferences(completionHandler:$0) } }
        try accepted(ProviderServices.shared.dns.request(envelope.redacted()))
        dns.isEnabled = false; try wait { dns.saveToPreferences(completionHandler:$0) }
        // Guard is the final shutdown step, after the three data providers.
        try accepted(ProviderServices.shared.filter.request(envelope.redacted()))
        filter.isEnabled = false; try wait { filter.saveToPreferences(completionHandler:$0) }
        return encodeJSON(["operation_id":envelope.operation.uuidString.lowercased(),"acceptance":"accepted",
                           "snapshot":["phase":"stopped","session":envelope.session?.object ?? [:],
                                       "protection_active":false,"production_capable":false,"error_code":NSNull(),"selected_path":NSNull()]])
    }
    private func accepted(_ data:Data) throws {
        let object = try JSONSerialization.jsonObject(with:data) as? [String:Any]
        guard ["accepted","duplicate"].contains(object?["acceptance"] as? String ?? "") else { throw NativeFailure.providerUnavailable }
    }
}

private final class ManagerResult {
    private let lock = NSLock(); private var value:Error?
    var error:Error? { get { lock.lock(); defer { lock.unlock() }; return value } set { lock.lock(); value = newValue; lock.unlock() } }
}
