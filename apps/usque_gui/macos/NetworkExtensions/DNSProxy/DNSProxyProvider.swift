import Foundation
import NetworkExtension
import Network
import Security
import Darwin

@objc(DNSProxyProvider)
final class DNSProxyProvider: NEDNSProxyProvider {
    private let state = NativeState()
    private let policy = RoutingPolicy()
    private let channel = ControlChannel(.dns)
    private let lock = NSLock()
    private var profile: [String:Any] = [:]
    private var relays: [UUID:DNSFlowRelay] = [:]
    override func startProxy(options: [String:Any]? = nil, completionHandler: @escaping (Error?)->Void) {
        do {
            try ProviderIdentity.gate()
            channel.handler = { [weak self] data,peer,_ in self?.request(data,peer:peer) ?? Data() }
            try channel.start()
            PhysicalNetwork.shared.didChange = { [weak self] in self?.closeAll(); _ = try? ProviderServices.shared.filter.native("arm") }
            completionHandler(nil)
        } catch { completionHandler(error) }
    }
    private func request(_ data:Data,peer:String)->Data {
        do {
            guard [ProviderIdentity.control,ProviderIdentity.prefix,"io.github.georgexie2333.usque.engine"].contains(peer) else { throw NativeFailure.permissionDenied }
            if let object = try JSONSerialization.jsonObject(with:data) as? [String:Any],object["native_action"] as? String == "snapshot" { return encodeJSON(["accepted":true,"snapshot":state.snapshot()]) }
            let envelope = try ControlEnvelope(data)
            guard try state.validate(envelope) else { return state.reply(envelope,duplicate:true) }
            switch envelope.type {
            case "start","replace":
                guard let configuration = envelope.command["configuration"] as? [String:Any],let profile = configuration["profile"] as? [String:Any] else { throw NativeFailure.invalidEnvelope }
                closeAll(); try policy.replace(configuration); lock.lock(); self.profile = profile; lock.unlock()
                state.transition("starting",key:envelope.session)
            case "protect","cancel": closeAll(); state.transition("protected",key:envelope.session)
            case "stop": closeAll(); state.transition("stopped",key:envelope.session)
            case "snapshot": break
            default: throw NativeFailure.invalidEnvelope
            }
            return state.reply(envelope)
        } catch { return failureReply(data,error) }
    }
    override func handleNewFlow(_ flow:NEAppProxyFlow)->Bool {
        guard let key = state.key else { return false }
        let id = UUID(); lock.lock()
        guard relays.count < 128 else { lock.unlock(); return false }
        let profile = self.profile
        let relay = DNSFlowRelay(flow:flow,key:key,profile:profile,policy:policy) { [weak self] in
            self?.lock.lock(); self?.relays.removeValue(forKey:id); self?.lock.unlock()
        }
        relays[id] = relay; lock.unlock(); relay.start(); return true
    }
    private func closeAll() {
        lock.lock(); let relays = Array(self.relays.values); self.relays.removeAll(); lock.unlock()
        relays.forEach { $0.close() }
    }
    override func stopProxy(with reason:NEProviderStopReason,completionHandler:@escaping ()->Void) {
        closeAll(); _ = try? ProviderServices.shared.filter.native("arm"); completionHandler()
    }
}

private final class DNSFlowRelay {
    private let flow:NEAppProxyFlow
    private let key:SessionKey
    private let profile:[String:Any]
    private let policy:RoutingPolicy
    private let completion:()->Void
    private let queue = DispatchQueue(label:"usque.dns.flow")
    private var buffer = Data()
    private var ended = false
    private var active:DNSExchange?
    init(flow:NEAppProxyFlow,key:SessionKey,profile:[String:Any],policy:RoutingPolicy,completion:@escaping ()->Void) {
        self.flow = flow; self.key = key; self.profile = profile; self.policy = policy; self.completion = completion
    }
    func start() {
        flow.open(withLocalEndpoint:nil) { [weak self] error in
            guard let self else { return }; self.queue.async {
                if error != nil { self.close(); return }
                if let tcp = self.flow as? NEAppProxyTCPFlow { self.readTCP(tcp) }
                else if let udp = self.flow as? NEAppProxyUDPFlow { self.readUDP(udp) }
                else { self.close() }
            }
        }
    }
    private func readTCP(_ tcp:NEAppProxyTCPFlow) {
        guard !ended else { return }
        tcp.readData { [weak self] data,error in
            guard let self else { return }; self.queue.async {
                guard error == nil,let data,!data.isEmpty,self.buffer.count + data.count <= 131074 else { self.close(); return }
                self.buffer.append(data); self.consumeTCP(tcp)
            }
        }
    }
    private func consumeTCP(_ tcp:NEAppProxyTCPFlow) {
        guard !ended else { return }
        guard buffer.count >= 2 else { readTCP(tcp); return }
        let size = Int(buffer[0])*256+Int(buffer[1])
        guard size >= 12 else { close(); return }
        guard buffer.count >= size+2 else { readTCP(tcp); return }
        let query = buffer.subdata(in:2..<size+2); buffer.removeFirst(size+2)
        exchange(query) { [weak self] response in
            guard let self,let response else { self?.close(); return }
            var size = UInt16(response.count).bigEndian
            let framed = withUnsafeBytes(of:&size) { Data($0) }+response
            tcp.write(framed) { error in self.queue.async {
                if error != nil { self.close() } else if self.buffer.isEmpty { self.readTCP(tcp) } else { self.consumeTCP(tcp) }
            } }
        }
    }
    private func readUDP(_ udp:NEAppProxyUDPFlow) {
        guard !ended else { return }
        udp.readDatagrams { [weak self] packets,endpoints,error in
            guard let self else { return }; self.queue.async {
                guard error == nil,let packets,let endpoints,packets.count == endpoints.count,packets.count <= 128 else { self.close(); return }
                self.consumeUDP(udp,packets:packets,endpoints:endpoints,index:0)
            }
        }
    }
    private func consumeUDP(_ udp:NEAppProxyUDPFlow,packets:[Data],endpoints:[UsqueLegacyEndpoint],index:Int) {
        guard index < packets.count else { readUDP(udp); return }
        exchange(packets[index]) { [weak self] response in
            guard let self,let response else { self?.close(); return }
            udp.writeDatagrams([response],sentBy:[endpoints[index]]) { error in self.queue.async {
                if error != nil { self.close() } else { self.consumeUDP(udp,packets:packets,endpoints:endpoints,index:index+1) }
            } }
        }
    }
    private func exchange(_ query:Data,complete:@escaping (Data?)->Void) {
        guard let hostname = DNSExchange.question(query) else { complete(nil); return }
        let route = policy.route(host:hostname,ip:nil)
        guard route != .reject else { complete(nil); return }
        let direct = route == .direct
        let settings = profile[direct ? "direct_dns" : "warp_dns"] as? [String:Any] ?? [:]
        let mode = settings["mode"] as? String ?? (direct ? "physical_system" : "plain")
        // Physical system DNS would introduce an unverified plaintext escape.
        // It is intentionally fail-closed; there is never an encrypted fallback.
        guard !direct || ["dot","doh"].contains(mode) else { complete(nil); return }
        let ips = settings["bootstrap_ips"] as? [String] ?? (direct ? [] : (profile["dns_servers"] as? [String] ?? []))
        guard let address = ips.first else { complete(nil); return }
        let exchange = DNSExchange(query:query,address:address,settings:settings,mode:mode,direct:direct,key:key,queue:queue)
        active = exchange
        exchange.run { [weak self] response in self?.active = nil; complete(response) }
    }
    func close() {
        queue.async { [self] in
            guard !ended else { return }; ended = true; active?.cancel(); active = nil
            flow.closeReadWithError(nil); flow.closeWriteWithError(nil); completion()
        }
    }
}

/// One query per bounded TLS/TCP exchange. No URLSession/system resolver,
/// cookies, redirects, HTTP cache, plaintext retry or recursive bootstrap lookup.
private final class DNSExchange {
    private let query:Data,address:String,settings:[String:Any],mode:String,direct:Bool,key:SessionKey,queue:DispatchQueue
    private var connection:NWConnection?
    private var finish:((Data?)->Void)?
    private var received = Data()
    init(query:Data,address:String,settings:[String:Any],mode:String,direct:Bool,key:SessionKey,queue:DispatchQueue) {
        self.query = query; self.address = address; self.settings = settings; self.mode = mode; self.direct = direct; self.key = key; self.queue = queue
    }
    func run(_ complete:@escaping (Data?)->Void) {
        finish = complete
        do {
            var v4 = in_addr(),v6 = in6_addr()
            let ipv6 = inet_pton(AF_INET6,address,&v6) == 1
            guard ipv6 || inet_pton(AF_INET,address,&v4) == 1 else { throw NativeFailure.invalidEnvelope }
            let encrypted = ["doh","dot"].contains(mode)
            let server = settings["server_name"] as? String ?? ""
            let port = (settings["port"] as? Int).flatMap(UInt16.init(exactly:)).flatMap { $0 == 0 ? nil : $0 } ?? (mode == "doh" ? 443 : mode == "dot" ? 853 : 53)
            let parameters:NWParameters
            if encrypted {
                guard !server.isEmpty else { throw NativeFailure.invalidEnvelope }
                let tls = NWProtocolTLS.Options()
                sec_protocol_options_set_tls_server_name(tls.securityProtocolOptions,server)
                sec_protocol_options_set_min_tls_protocol_version(tls.securityProtocolOptions,.TLSv12)
                if mode == "doh" { sec_protocol_options_add_tls_application_protocol(tls.securityProtocolOptions,"http/1.1") }
                parameters = NWParameters(tls:tls,tcp:NWProtocolTCP.Options())
            } else { parameters = .tcp }
            if direct {
                try ProviderIdentity.gate()
                guard let interface = PhysicalNetwork.shared.candidateNetworkInterface else { throw NativeFailure.providerUnavailable }
                let socket = Darwin.socket(ipv6 ? AF_INET6 : AF_INET,SOCK_STREAM,IPPROTO_TCP)
                guard socket >= 0 else { throw NativeFailure.providerUnavailable }; defer { Darwin.close(socket) }
                var pointer:UnsafeMutablePointer<CChar>?
                guard usque_native_bind_probe_socket(socket,UInt32(interface.index),&pointer) == 0,let pointer else { throw NativeFailure.providerUnavailable }
                defer { usque_native_free(pointer) }
                guard let object = try JSONSerialization.jsonObject(with:Data(String(cString:pointer).utf8)) as? [String:Any],let local = object["local_port"] as? Int,
                      let localPort = Network.NWEndpoint.Port(rawValue:UInt16(local)) else { throw NativeFailure.invalidEnvelope }
                parameters.requiredInterface = interface
                guard let localIP = object["local_ip"] as? String else { throw NativeFailure.invalidEnvelope }
                parameters.requiredLocalEndpoint = .hostPort(host:Network.NWEndpoint.Host(localIP),port:localPort)
                _ = try ProviderServices.shared.filter.native("lease",["session":key.object,"local_ip":object["local_ip"] ?? "","local_port":local,"remote_ip":address,
                    "remote_port":Int(port),"protocol":6,"interface_index":interface.index])
                // Socket closes before NWConnection binds this exact port. A
                // collision fails, never falls back to a different source tuple.
            }
            guard let nwPort = Network.NWEndpoint.Port(rawValue:port) else { throw NativeFailure.invalidEnvelope }
            let connection = NWConnection(host:Network.NWEndpoint.Host(address),port:nwPort,using:parameters)
            self.connection = connection
            connection.stateUpdateHandler = { [weak self] state in
                guard let self else { return }
                switch state { case .ready:self.send(server:server); case .failed,.cancelled:self.done(nil); default:break }
            }
            connection.start(queue:queue)
            queue.asyncAfter(deadline:.now()+10) { [weak self] in self?.done(nil) }
        } catch { done(nil) }
    }
    private func send(server:String) {
        let request:Data
        if mode == "doh" {
            let path = settings["doh_path"] as? String ?? "/dns-query"
            guard path.hasPrefix("/"), !path.contains("\r"),!path.contains("\n"),!server.contains("\r"),!server.contains("\n") else { done(nil); return }
            request = Data("POST \(path) HTTP/1.1\r\nHost: \(server)\r\nContent-Type: application/dns-message\r\nAccept: application/dns-message\r\nContent-Length: \(query.count)\r\nConnection: close\r\n\r\n".utf8)+query
        } else {
            var size = UInt16(query.count).bigEndian
            request = withUnsafeBytes(of:&size) { Data($0) }+query
        }
        connection?.send(content:request,completion:.contentProcessed { [weak self] error in
            if error != nil { self?.done(nil) } else { self?.receive() }
        })
    }
    private func receive() {
        connection?.receive(minimumIncompleteLength:1,maximumLength:65536) { [weak self] data,_,complete,error in
            guard let self,self.finish != nil else { return }
            if let data { self.received.append(data) }
            guard error == nil,self.received.count <= 73728 else { self.done(nil); return }
            if self.mode == "doh" {
                if let separator = self.received.range(of:Data("\r\n\r\n".utf8)) {
                    guard separator.lowerBound <= 8192,let headers = String(data:self.received[..<separator.lowerBound],encoding:.utf8) else { self.done(nil); return }
                    let lines = headers.components(separatedBy:"\r\n")
                    guard lines.first?.hasPrefix("HTTP/1.1 200 ") == true,
                          lines.contains(where:{$0.lowercased().hasPrefix("content-type: application/dns-message")}),
                          !lines.contains(where:{$0.lowercased().hasPrefix("transfer-encoding:")}),
                          let line = lines.first(where:{$0.lowercased().hasPrefix("content-length:")}),
                          let length = Int(line.split(separator:":",maxSplits:1)[1].trimmingCharacters(in:.whitespaces)), (12...65535).contains(length)
                    else { self.done(nil); return }
                    let body = self.received[separator.upperBound...]
                    if body.count >= length { self.validate(Data(body.prefix(length))); return }
                }
            } else if self.received.count >= 2 {
                let length = Int(self.received[0])*256+Int(self.received[1])
                guard length >= 12 else { self.done(nil); return }
                if self.received.count >= length+2 { self.validate(self.received.subdata(in:2..<length+2)); return }
            }
            if complete { self.done(nil) } else { self.receive() }
        }
    }
    private func validate(_ response:Data) {
        guard response.count >= 12,response[0] == query[0],response[1] == query[1],response[2]&0x80 != 0 else { done(nil); return }
        done(response)
    }
    private func done(_ result:Data?) {
        guard let callback = finish else { return }; finish = nil; connection?.cancel(); connection = nil; callback(result)
    }
    func cancel() { done(nil) }
    static func question(_ packet:Data)->String? {
        guard packet.count >= 17,packet.count <= 65535,packet[2]&0x80 == 0,packet[4] == 0,packet[5] == 1 else { return nil }
        var labels:[String] = [],index = 12,total = 0
        while index < packet.count {
            let length = Int(packet[index]); index += 1
            if length == 0 { return index+4 <= packet.count && !labels.isEmpty ? labels.joined(separator:".") : nil }
            guard length <= 63,index+length < packet.count,total+length+1 <= 253,
                  let label = String(data:packet.subdata(in:index..<index+length),encoding:.ascii),
                  label.range(of:"^[A-Za-z0-9_-]+$",options:.regularExpression) != nil else { return nil }
            labels.append(label.lowercased()); total += length+1; index += length
        }
        return nil
    }
}
