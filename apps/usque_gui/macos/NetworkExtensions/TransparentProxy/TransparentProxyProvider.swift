import Foundation
import NetworkExtension
import Network
import Darwin

@objc(TransparentProxyProvider)
final class TransparentProxyProvider: NETransparentProxyProvider {
    private let state = NativeState()
    private let policy = RoutingPolicy()
    private let channel = ControlChannel(.transparent)
    private let lock = NSLock()
    private var relays: [UUID: DirectFlowRelay] = [:]
    override func startProxy(options: [String: Any]? = nil, completionHandler: @escaping (Error?) -> Void) {
        do {
            try ProviderIdentity.gate()
            channel.handler = { [weak self] data, peer, _ in self?.request(data, peer:peer) ?? Data() }
            try channel.start()
            let settings = NETransparentProxyNetworkSettings(tunnelRemoteAddress:"127.0.0.1")
            settings.includedNetworkRules = [NENetworkRule(remoteNetwork:nil, remotePrefix:0, localNetwork:nil, localPrefix:0, protocol:.any, direction:.outbound)]
            // DNS is managed independently. Transparent proxy DNS/proxy settings
            // are ignored by Apple and are never treated as a DNS boundary here.
            setTunnelNetworkSettings(settings, completionHandler:completionHandler)
            PhysicalNetwork.shared.didChange = { [weak self] in self?.closeAll(); _ = try? ProviderServices.shared.filter.native("arm") }
        } catch { completionHandler(error) }
    }
    private func request(_ data: Data, peer: String) -> Data {
        do {
            guard [ProviderIdentity.control,ProviderIdentity.prefix,"io.github.georgexie2333.usque.engine"].contains(peer) else { throw NativeFailure.permissionDenied }
            if let object = try JSONSerialization.jsonObject(with:data) as? [String:Any], object["native_action"] as? String == "snapshot" {
                return encodeJSON(["accepted":true,"snapshot":state.snapshot()])
            }
            let request = try ControlEnvelope(data)
            guard try state.validate(request) else { return state.reply(request,duplicate:true) }
            switch request.type {
            case "start","replace":
                guard let configuration = request.command["configuration"] as? [String:Any] else { throw NativeFailure.invalidEnvelope }
                closeAll(); try policy.replace(configuration); state.transition("starting",key:request.session)
            case "protect","cancel": closeAll(); state.transition("protected",key:request.session)
            case "stop": closeAll(); state.transition("stopped",key:request.session)
            case "snapshot": break
            default: throw NativeFailure.invalidEnvelope
            }
            return state.reply(request)
        } catch { return failureReply(data,error) }
    }
    override func handleNewFlow(_ flow: NEAppProxyFlow) -> Bool {
        // Provider-generated sockets must reach the same tunnel without being
        // recursively re-proxied. Physical egress still requires a filter lease.
        if ProviderRole.allCases.map(\.identifier).contains(flow.metaData.sourceAppSigningIdentifier) { return false }
        guard let tcp = flow as? NEAppProxyTCPFlow, let remote = tcp.remoteEndpoint as? NWHostEndpoint else {
            // UDP is delegated to handleNewUDPFlow on supported OS paths; unknown
            // metadata is closed rather than accidentally bypassed.
            return reject(flow)
        }
        return route(flow,remote:remote,proto:6)
    }
    override func handleNewUDPFlow(_ flow: NEAppProxyUDPFlow, initialRemoteEndpoint remoteEndpoint: UsqueLegacyEndpoint) -> Bool {
        if ProviderRole.allCases.map(\.identifier).contains(flow.metaData.sourceAppSigningIdentifier) { return false }
        guard let remote = remoteEndpoint as? NWHostEndpoint else { return reject(flow) }
        return route(flow,remote:remote,proto:17)
    }
    private func route(_ flow: NEAppProxyFlow, remote: NWHostEndpoint, proto: Int) -> Bool {
        var hostname = flow.remoteHostname
        if flow.metaData.sourceAppSigningIdentifier == "io.github.georgexie2333.usque.engine" {
            // Resolving a hostname before opening BSD sockets erases that intent.
            // Recover it only through exact tuple + source-audit-token metadata.
            guard let id = flow.metaData.filterFlowIdentifier,
                  let reply = try? ProviderServices.shared.filter.native("resolve_intent",["flow_id":id.uuidString]),
                  reply["accepted"] as? Bool == true else { return reject(flow) }
            hostname = reply["hostname"] as? String
        }
        switch policy.route(host:hostname,ip:remote.hostname) {
        case .proxy: return false // System networking enters the one Packet Tunnel.
        case .reject: return reject(flow)
        case .direct:
            guard ProviderIdentity.signedPrototype, let key = state.key else { return reject(flow) }
            let id = UUID()
            lock.lock(); let full = relays.count >= 128; lock.unlock()
            guard !full else { return reject(flow) }
            let relay = DirectFlowRelay(flow:flow,remote:remote,proto:proto,key:key) { [weak self] in
                self?.lock.lock(); self?.relays.removeValue(forKey:id); self?.lock.unlock()
            }
            lock.lock(); relays[id] = relay; lock.unlock(); relay.start()
            return true
        }
    }
    private func reject(_ flow: NEAppProxyFlow) -> Bool {
        let error = NativeFailure.notConnected
        flow.closeReadWithError(error); flow.closeWriteWithError(error)
        return true // Transparent `false` means bypass, so denied flows return true.
    }
    private func closeAll() {
        lock.lock(); let current = Array(relays.values); relays.removeAll(); lock.unlock()
        current.forEach { $0.close() }
    }
    override func stopProxy(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        closeAll(); _ = try? ProviderServices.shared.filter.native("arm"); completionHandler()
    }
}

/// Signed PoC only. A bounded BSD relay makes the candidate physical socket's
/// exact source port known *before* connecting. It does not claim that binding an
/// interface overrides enforceRoutes; packet capture is the acceptance gate.
private final class DirectFlowRelay {
    private let flow: NEAppProxyFlow
    private let remote: NWHostEndpoint
    private let proto: Int
    private let key: SessionKey
    private let completion: () -> Void
    private let lock = NSLock()
    private var fd: Int32 = -1
    private var ended = false
    private let writer = DispatchQueue(label:"usque.direct.write")
    private var renewal: DispatchSourceTimer?
    init(flow:NEAppProxyFlow,remote:NWHostEndpoint,proto:Int,key:SessionKey,completion:@escaping ()->Void) {
        self.flow = flow; self.remote = remote; self.proto = proto; self.key = key; self.completion = completion
    }
    func start() {
        DispatchQueue.global(qos:.userInitiated).async { [self] in
            do {
                guard let port = UInt16(remote.port), let interface = PhysicalNetwork.shared.candidateInterface else { throw NativeFailure.providerUnavailable }
                var v4 = in_addr(), v6 = in6_addr()
                let ipv6 = inet_pton(AF_INET6,remote.hostname,&v6) == 1
                guard ipv6 || inet_pton(AF_INET,remote.hostname,&v4) == 1 else { throw NativeFailure.invalidEnvelope }
                let socket = Darwin.socket(ipv6 ? AF_INET6 : AF_INET,proto == 6 ? SOCK_STREAM : SOCK_DGRAM,Int32(proto))
                guard socket >= 0 else { throw NativeFailure.providerUnavailable }
                lock.lock(); if ended { lock.unlock(); Darwin.close(socket); return }; fd = socket; lock.unlock()
                var timeout = timeval(tv_sec:10,tv_usec:0), one:Int32 = 1
                setsockopt(socket,SOL_SOCKET,SO_RCVTIMEO,&timeout,socklen_t(MemoryLayout.size(ofValue:timeout)))
                setsockopt(socket,SOL_SOCKET,SO_SNDTIMEO,&timeout,socklen_t(MemoryLayout.size(ofValue:timeout)))
                setsockopt(socket,SOL_SOCKET,SO_NOSIGPIPE,&one,socklen_t(MemoryLayout.size(ofValue:one)))
                var pointer:UnsafeMutablePointer<CChar>?
                guard usque_native_bind_probe_socket(socket,interface,&pointer) == 0, let pointer else { throw NativeFailure.providerUnavailable }
                defer { usque_native_free(pointer) }
                guard let object = try JSONSerialization.jsonObject(with:Data(String(cString:pointer).utf8)) as? [String:Any] else { throw NativeFailure.invalidEnvelope }
                _ = try ProviderServices.shared.filter.native("lease",["session":key.object,"local_ip":object["local_ip"] ?? "","local_port":object["local_port"] ?? 0,
                     "remote_ip":remote.hostname,"remote_port":Int(port),"protocol":proto,"interface_index":Int(interface)])
                var status:Int32 = -1
                if ipv6 {
                    var address = sockaddr_in6(); address.sin6_len = UInt8(MemoryLayout<sockaddr_in6>.size); address.sin6_family = sa_family_t(AF_INET6)
                    address.sin6_port = port.bigEndian; address.sin6_addr = v6
                    status = withUnsafePointer(to:&address) { $0.withMemoryRebound(to:sockaddr.self,capacity:1) { Darwin.connect(socket,$0,socklen_t(MemoryLayout<sockaddr_in6>.size)) } }
                } else {
                    var address = sockaddr_in(); address.sin_len = UInt8(MemoryLayout<sockaddr_in>.size); address.sin_family = sa_family_t(AF_INET)
                    address.sin_port = port.bigEndian; address.sin_addr = v4
                    status = withUnsafePointer(to:&address) { $0.withMemoryRebound(to:sockaddr.self,capacity:1) { Darwin.connect(socket,$0,socklen_t(MemoryLayout<sockaddr_in>.size)) } }
                }
                guard status == 0 else { throw NativeFailure.physicalEgressUnverified }
                let renewal = DispatchSource.makeTimerSource(queue:writer)
                renewal.schedule(deadline:.now()+5,repeating:5)
                renewal.setEventHandler { [weak self] in
                    guard let self else { return }
                    if (try? ProviderServices.shared.filter.native("renew",["session":self.key.object])) == nil { self.close() }
                }
                lock.lock()
                if ended { lock.unlock(); renewal.cancel(); return }
                self.renewal = renewal; renewal.resume(); lock.unlock()
                flow.open(withLocalEndpoint:nil) { [weak self] error in
                    guard let self else { return }; if error != nil { self.close(); return }
                    if let tcp = self.flow as? NEAppProxyTCPFlow { self.readTCP(tcp) }
                    else if let udp = self.flow as? NEAppProxyUDPFlow { self.readUDP(udp) }
                    DispatchQueue.global(qos:.userInitiated).async { self.receive() }
                }
            } catch { close() }
        }
    }
    private func readTCP(_ tcp: NEAppProxyTCPFlow) {
        tcp.readData { [weak self] data,error in
            guard let self else { return }
            guard error == nil, let data, !data.isEmpty, data.count <= 65536 else { self.close(); return }
            self.writer.async { if self.send(data) { self.readTCP(tcp) } else { self.close() } }
        }
    }
    private func readUDP(_ udp: NEAppProxyUDPFlow) {
        udp.readDatagrams { [weak self] packets,endpoints,error in
            guard let self else { return }
            guard error == nil, let packets, let endpoints, packets.count == endpoints.count, packets.count <= 128 else { self.close(); return }
            self.writer.async {
                for (packet,endpoint) in zip(packets,endpoints) {
                    guard let host = endpoint as? NWHostEndpoint, host.hostname == self.remote.hostname, host.port == self.remote.port,
                          packet.count <= 65507, self.send(packet) else { self.close(); return }
                }
                self.readUDP(udp)
            }
        }
    }
    private func send(_ data: Data) -> Bool {
        lock.lock(); let socket = fd, active = !ended; lock.unlock(); guard active else { return false }
        return data.withUnsafeBytes { bytes in
            var sent = 0
            while sent < data.count {
                let n = Darwin.send(socket,bytes.baseAddress!.advanced(by:sent),data.count-sent,0)
                if n < 0 && errno == EINTR { continue }; if n <= 0 { return false }; sent += n
                if proto == 17 { return sent == data.count }
            }
            return true
        }
    }
    private func receive() {
        var bytes = [UInt8](repeating:0,count:65536)
        lock.lock(); let socket = fd, active = !ended; lock.unlock(); guard active else { return }
        let count = Darwin.recv(socket,&bytes,bytes.count,0)
        guard count > 0 else { close(); return }
        let data = Data(bytes.prefix(count))
        if let tcp = flow as? NEAppProxyTCPFlow {
            tcp.write(data) { [weak self] error in if error != nil { self?.close() } else { DispatchQueue.global().async { self?.receive() } } }
        } else if let udp = flow as? NEAppProxyUDPFlow {
            udp.writeDatagrams([data],sentBy:[remote]) { [weak self] error in if error != nil { self?.close() } else { DispatchQueue.global().async { self?.receive() } } }
        }
    }
    func close() {
        lock.lock(); if ended { lock.unlock(); return }; ended = true; let socket = fd; fd = -1; let timer = renewal; renewal = nil; lock.unlock()
        timer?.cancel()
        if socket >= 0 { Darwin.shutdown(socket,SHUT_RDWR); Darwin.close(socket) }
        flow.closeReadWithError(nil); flow.closeWriteWithError(nil); completion()
    }
}
