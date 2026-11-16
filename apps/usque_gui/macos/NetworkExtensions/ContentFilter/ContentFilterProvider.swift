import NetworkExtension

@objc(ContentFilterProvider)
final class ContentFilterProvider: NEFilterPacketProvider {
    override func startFilter(completionHandler: @escaping (Error?) -> Void) {
        do {
            try GuardStore.shared.start()
            packetHandler = { _, interface, direction, bytes, count in
                GuardStore.shared.allows(interface: interface, direction: direction, bytes: bytes, count: count) ? .allow : .drop
            }
            completionHandler(nil)
        } catch { completionHandler(error) }
    }
    override func stopFilter(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        GuardStore.shared.block("filter_stopped"); completionHandler()
    }
}

@objc(FlowMetadataProvider)
final class FlowMetadataProvider: NEFilterDataProvider {
    override func startFilter(completionHandler: @escaping (Error?) -> Void) {
        // Apple may host the two filter classes in separate processes. Only
        // the packet filter owns the guard listener; metadata crosses XPC.
        do { try ProviderIdentity.gate(); completionHandler(nil) } catch { completionHandler(error) }
    }
    override func handleNewFlow(_ flow: NEFilterFlow) -> NEFilterNewFlowVerdict {
        if let socket = flow as? NEFilterSocketFlow { GuardStore.shared.record(socket) }
        // Packet filtering is the independent protection boundary. This data
        // provider only records exact source/remote metadata for transparent flows.
        return .allow()
    }
    override func stopFilter(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        _ = try? ProviderServices.shared.filter.native("arm"); completionHandler()
    }
}
