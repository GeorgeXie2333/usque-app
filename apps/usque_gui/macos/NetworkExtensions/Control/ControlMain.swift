import Foundation

@main struct ControlMain {
    static func main() {
        guard CommandLine.arguments.dropFirst().elementsEqual(["--stdio"]) else {
            FileHandle.standardError.write(Data("Usage: usque-macos-control --stdio (4-byte BE framed JSON; no credentials in arguments)\n".utf8)); exit(2)
        }
        _ = PhysicalNetwork.shared
        let manager = ExtensionManager()
        let relay = BrokerRelay(); relay.install()
        let operations = DispatchQueue(label:"usque.control.operations")
        DispatchQueue.global(qos:.userInitiated).async {
            do {
                while let data = try FrameIO.read(.standardInput) {
                    if relay.consume(data) { continue }
                    operations.async {
                    var reply:Data
                    do {
                        let object = try JSONSerialization.jsonObject(with:data) as? [String:Any]
                        if let native = object?["native"] as? [String:Any], let type = native["type"] as? String {
                            guard object?["version"] as? Int == 1, let id = object?["operation_id"] as? String, UUID(uuidString:id) != nil else { throw NativeFailure.invalidEnvelope }
                            switch type {
                            case "status": reply = encodeJSON(["operation_id":id,"native":manager.status()])
                            case "activate": try manager.activate(); reply = encodeJSON(["operation_id":id,"native":["accepted":true]])
                            case "flow_intent":
                                try ProviderIdentity.gate()
                                var intent = native; intent.removeValue(forKey:"type")
                                let result = try ProviderServices.shared.filter.native("flow_intent",intent)
                                reply = encodeJSON(["operation_id":id,"native":result])
                            default: throw NativeFailure.invalidEnvelope
                            }
                        } else {
                            let envelope = try ControlEnvelope(data)
                            switch envelope.type {
                            case "protect": reply = try manager.protect(envelope)
                            case "start","replace": reply = try manager.start(envelope,original:data)
                            case "stop": reply = try manager.stop(envelope,original:data)
                            case "snapshot","cancel": reply = try ProviderServices.shared.packet.request(data)
                            default: throw NativeFailure.invalidEnvelope
                            }
                        }
                    } catch { reply = failureReply(data,error) }
                    do { try relay.emit(reply) } catch { relay.close(); exit(2) }
                    }
                }
                // Control/Engine exit intentionally leaves providers and guard.
                relay.close(); operations.sync {}; exit(0)
            } catch { relay.close(); exit(2) }
        }
        RunLoop.main.run()
    }
}
