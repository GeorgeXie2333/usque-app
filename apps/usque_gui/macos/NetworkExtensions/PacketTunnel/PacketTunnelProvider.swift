import Foundation
import NetworkExtension
import Darwin

@objc(PacketTunnelProvider)
final class PacketTunnelProvider: NEPacketTunnelProvider {
    private let state = NativeState()
    private let channel = ControlChannel(.packet)
    private var runtime: UnsafeMutableRawPointer?
    private let runtimeLock = NSLock()
    private let queue = DispatchQueue(label: "usque.packet.io")
    private var running = false
    private var pending = PacketBatch()
    private var readInFlight = false
    private var timer: DispatchSourceTimer?
    private var tunnelInterface: String?
    private var renewedAt = Date.distantPast
    override func startTunnel(options: [String: NSObject]?, completionHandler: @escaping (Error?) -> Void) {
        do {
            try ProviderIdentity.gate()
            runtime = usque_macos_runtime_create_with_broker({ context, bytes, count in
                guard let context, let bytes else { return -1 }
                let provider = Unmanaged<PacketTunnelProvider>.fromOpaque(context).takeUnretainedValue()
                return provider.platform(Data(bytes: bytes, count: count))
            }, { _,bytes,count,response,capacity,length in
                guard let bytes,let response,let length,
                      let result = usque_native_xpc_callback(bytes,count,15_000) else { return -1 }
                defer { usque_native_free(result) }
                let size = strlen(result)
                guard size <= capacity else { return -1 }
                memcpy(response,result,size); length.pointee = size; return 0
            }, Unmanaged.passUnretained(self).toOpaque())
            guard runtime != nil else { throw NativeFailure.providerUnavailable }
            channel.handler = { [weak self] data, peer, _ in self?.request(data, peer: peer) ?? Data() }
            try channel.start()
            PhysicalNetwork.shared.didChange = { [weak self] in
                self?.state.transition("protected", error: "physical_network_changed")
                _ = try? ProviderServices.shared.filter.native("arm")
            }
            // No credentials in options/providerConfiguration. The signed,
            // authenticated controller must hand off the session over XPC.
            state.transition("protected", error: NativeFailure.missingCredentials.rawValue)
            running = true
            queue.async { [weak self] in self?.readPackets() }
            let timer = DispatchSource.makeTimerSource(queue: queue)
            timer.schedule(deadline: .now(), repeating: .milliseconds(5))
            timer.setEventHandler { [weak self] in self?.drainPackets() }
            timer.resume(); self.timer = timer
            completionHandler(nil)
        } catch { completionHandler(error) }
    }
    private func request(_ data: Data, peer: String) -> Data {
        runtimeLock.lock(); defer { runtimeLock.unlock() }
        do {
            guard [ProviderIdentity.control, ProviderIdentity.prefix, "io.github.georgexie2333.usque.engine"].contains(peer), let runtime else { throw NativeFailure.permissionDenied }
            if let object = try JSONSerialization.jsonObject(with:data) as? [String:Any], object["native_action"] as? String == "snapshot" {
                return encodeJSON(["accepted":true,"snapshot":state.snapshot()])
            }
            if let object = try JSONSerialization.jsonObject(with:data) as? [String:Any], object["native_action"] as? String == "runtime_snapshot" {
                guard object["version"] as? Int == 1 else { throw NativeFailure.unsupportedVersion }
                var output = UsqueBuffer(data:nil,len:0)
                let code = usque_macos_runtime_snapshot(runtime,&output)
                defer { usque_macos_buffer_free(output) }
                guard code == 0, let bytes = output.data,
                      let snapshot = try JSONSerialization.jsonObject(with:Data(bytes:bytes,count:output.len)) as? [String:Any] else { throw NativeFailure.providerUnavailable }
                return try packetRuntimeReply(encodeJSON(["accepted":true,"snapshot":snapshot]))
            }
            let envelope = try ControlEnvelope(data)
            var output = UsqueBuffer(data: nil, len: 0)
            let code = data.withUnsafeBytes { usque_macos_runtime_command(runtime, $0.bindMemory(to: UInt8.self).baseAddress, data.count, &output) }
            defer { usque_macos_buffer_free(output) }
            guard code == 0, let bytes = output.data else { throw NativeFailure.invalidEnvelope }
            if envelope.type == "start" || envelope.type == "replace" { state.transition("starting", key: envelope.session) }
            return try packetRuntimeReply(Data(bytes: bytes, count: output.len))
        } catch { return failureReply(data, error) }
    }
    private func platform(_ data: Data) -> Int32 {
        do {
            guard let request = try JSONSerialization.jsonObject(with: data) as? [String: Any],
                  let action = request["type"] as? String, let key = sessionObject(request["session"]) else { return -1 }
            switch action {
            case "protect", "block":
                _ = try ProviderServices.shared.filter.native("arm", ["session":key.object])
                state.transition("protected", key: key)
            case "configure_dns_routes":
                guard let settings = request["settings"] as? [String: Any], settings["enforce_routes"] as? Bool == true,
                      let v4 = settings["assigned_ipv4"] as? String else { return -1 }
                let network = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "127.0.0.1")
                let ipv4 = NEIPv4Settings(addresses:[v4], subnetMasks:["255.255.255.255"])
                ipv4.includedRoutes = [NEIPv4Route.default()]; network.ipv4Settings = ipv4
                if let v6 = settings["assigned_ipv6"] as? String {
                    let ipv6 = NEIPv6Settings(addresses:[v6], networkPrefixLengths:[128])
                    ipv6.includedRoutes = [NEIPv6Route.default()]; network.ipv6Settings = ipv6
                } else { return -1 } // Never leave IPv6 physically unprotected.
                let dns = NEDNSSettings(servers: settings["dns_servers"] as? [String] ?? [])
                dns.matchDomains = [""]; network.dnsSettings = dns
                network.mtu = (settings["mtu"] as? Int).map(NSNumber.init(value:))
                try apply(network)
                guard let name = Self.interface(address:v4) else { throw NativeFailure.providerUnavailable }
                tunnelInterface = name
                _ = try ProviderServices.shared.filter.native("commit_tunnel", ["session":key.object,"interface":name])
            case "allow":
                guard tunnelInterface != nil else { return -1 }
                _ = try ProviderServices.shared.filter.native("allow", ["session":key.object])
                state.transition("connected", key:key)
            case "clear_dns_routes": try apply(nil); tunnelInterface = nil
            case "release":
                // The controller owns final guard Stop after DNS/proxy managers
                // have stopped. Runtime Release does not prematurely disarm it.
                state.transition("stopped", key:key, protection:true)
            case "protect_socket":
                try ProviderIdentity.gate()
                guard let socket = request["socket"] as? Int, let remoteText = request["remote"] as? String,
                      let remote = endpointParts(remoteText), let proto = request["protocol"] as? Int,
                      request["generation"] as? UInt64 == key.network_generation,
                      let interface = PhysicalNetwork.shared.candidateInterface else { return -1 }
                var pointer: UnsafeMutablePointer<CChar>?
                guard usque_native_bind_probe_socket(Int32(socket), interface, &pointer) == 0, let pointer else { return -1 }
                defer { usque_native_free(pointer) }
                guard let local = try JSONSerialization.jsonObject(with: Data(String(cString:pointer).utf8)) as? [String: Any] else { return -1 }
                _ = try ProviderServices.shared.filter.native("lease", ["session":key.object,"local_ip":local["local_ip"] ?? "","local_port":local["local_port"] ?? 0,
                    "remote_ip":remote.0,"remote_port":Int(remote.1),"protocol":proto,"interface_index":Int(interface)])
            default: return -1
            }
            return 0
        } catch { return -1 }
    }
    private func apply(_ settings: NEPacketTunnelNetworkSettings?) throws {
        let completion = DispatchSemaphore(value:0), result = ErrorBox()
        setTunnelNetworkSettings(settings) { error in result.error = error; completion.signal() }
        guard completion.wait(timeout: .now()+15) == .success else { throw NativeFailure.providerUnavailable }
        if let error = result.error { throw error }
    }
    private func readPackets() {
        guard running, !readInFlight else { return }
        readInFlight = true
        let session = state.flowSession()
        packetFlow.readPackets { [weak self] packets, _ in
            guard let self else { return }
            self.queue.async {
                self.readInFlight = false
                guard self.running else { return }
                // A full Rust queue pauses reads; the bounded pending batch is
                // retried by the timer. It never silently drops and continues.
                do {
                    try self.pending.append(packets, captured: session, current: self.state.flowSession())
                } catch {
                    self.failPacketIO()
                    return
                }
                self.flushPending()
            }
        }
    }
    private func flushPending() {
        guard let runtime else { return }
        pending.discardUnlessCurrent(state.flowSession())
        while let packet = pending.first, let session = pending.session {
            let identity = encodeJSON(session.object)
            let code = identity.withUnsafeBytes { key in
                packet.withUnsafeBytes {
                    usque_macos_runtime_send_session_packet(runtime, $0.bindMemory(to: UInt8.self).baseAddress, packet.count,
                        key.bindMemory(to: UInt8.self).baseAddress, identity.count)
                }
            }
            if code == -2 { return }
            pending.removeFirst()
            if code == -3 { pending.removeAll(); break }
            if code != 0 { failPacketIO(); return }
        }
        if running { readPackets() }
    }
    private func drainPackets() {
        guard running, let runtime else { return }
        if Date().timeIntervalSince(renewedAt) >= 5, let key = state.key {
            renewedAt = Date()
            if (try? ProviderServices.shared.filter.native("renew",["session":key.object])) == nil {
                failPacketIO(); return
            }
        }
        if !pending.isEmpty { flushPending() }
        guard running else { return }
        for _ in 0..<128 {
            var buffer = UsqueBuffer(data:nil,len:0)
            let code = usque_macos_runtime_receive_packet(runtime, &buffer)
            guard code == 1, let pointer = buffer.data else { usque_macos_buffer_free(buffer); break }
            let packet = Data(bytes:pointer,count:buffer.len); usque_macos_buffer_free(buffer)
            guard let first = packet.first, [4,6].contains(first >> 4) else { continue }
            if !packetFlow.writePackets([packet], withProtocols:[NSNumber(value:first >> 4 == 6 ? AF_INET6 : AF_INET)]) {
                failPacketIO(); return
            }
        }
    }
    private func failPacketIO() {
        running = false
        timer?.cancel()
        pending.removeAll()
        _ = try? ProviderServices.shared.filter.native("arm")
        cancelTunnelWithError(NativeFailure.providerUnavailable)
    }
    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        // Unexpected provider shutdown must not clear independent protection.
        _ = try? ProviderServices.shared.filter.native("arm")
        queue.async { [weak self] in
            guard let self else { completionHandler(); return }
            self.running = false; self.timer?.cancel(); self.pending.removeAll()
            self.runtimeLock.lock()
            if let runtime = self.runtime { usque_macos_runtime_destroy(runtime); self.runtime = nil }
            self.runtimeLock.unlock()
            completionHandler()
        }
    }
    private static func interface(address: String) -> String? {
        var list: UnsafeMutablePointer<ifaddrs>?
        guard getifaddrs(&list) == 0 else { return nil }; defer { freeifaddrs(list) }
        var current = list
        while let entry = current?.pointee {
            defer { current = entry.ifa_next }
            guard let socket = entry.ifa_addr, socket.pointee.sa_family == UInt8(AF_INET) else { continue }
            var host = [CChar](repeating:0,count:Int(NI_MAXHOST))
            if getnameinfo(socket,socklen_t(socket.pointee.sa_len),&host,socklen_t(host.count),nil,0,NI_NUMERICHOST) == 0,
               String(cString:host) == address { return String(cString:entry.ifa_name) }
        }
        return nil
    }
}

private final class ErrorBox {
    private let lock = NSLock(); private var value: Error?
    var error: Error? { get { lock.lock(); defer { lock.unlock() }; return value } set { lock.lock(); value = newValue; lock.unlock() } }
}
