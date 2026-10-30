import Foundation

@main struct ContractTests {
    static func envelope(_ key:SessionKey, type:String = "start", operation:UUID = UUID()) throws -> ControlEnvelope {
        try ControlEnvelope(encodeJSON(["version":1,"operation_id":operation.uuidString,"session":key.object,"command":["type":type]]))
    }
    static func expectFailure(_ operation:() throws -> Void) {
        do { try operation(); fatalError("Expected rejection") } catch {}
    }
    static func main() throws {
        let key = SessionKey(session_id:UUID(),config_revision:1,network_generation:7)
        var batch = PacketBatch()
        let packet = Data([0x45, 0, 0, 20])
        try batch.append([packet], captured: key, current: key)
        assert(batch.first == packet && batch.session == key)
        let replaced = SessionKey(session_id:key.session_id,config_revision:2,network_generation:7)
        batch.discardUnlessCurrent(replaced)
        assert(batch.isEmpty)
        // A delayed read callback must not attach old packets to a replacement.
        try batch.append([packet], captured:key, current:replaced)
        assert(batch.isEmpty)
        try batch.append([packet], captured:replaced, current:replaced)
        batch.discardUnlessCurrent(SessionKey(session_id:key.session_id,config_revision:2,network_generation:8))
        assert(batch.isEmpty)
        try batch.append([packet], captured:nil, current:key)
        assert(batch.isEmpty)
        expectFailure { try batch.append(Array(repeating:packet,count:257),captured:key,current:key) }
        expectFailure { try batch.append([Data()],captured:key,current:key) }
        expectFailure { try batch.append([Data(repeating:0,count:65_536)],captured:key,current:key) }
        expectFailure { try batch.append(Array(repeating:Data(repeating:0,count:65_535),count:65),captured:key,current:key) }
        assert(batch.isEmpty)
        try batch.append([packet],captured:key,current:key)
        batch.discardUnlessCurrent(nil)
        assert(batch.isEmpty)
        print("Packet backpressure session/revision/path fencing and memory bounds: passed")
        let state = NativeState()
        assert(state.snapshot()["protection_active"] as? Bool == true)
        assert(state.snapshot()["error_code"] as? String == "missing_credentials")
        assert(state.flowSession() == nil && !state.explicitlyStopped)
        let command = try envelope(key)
        let first = try state.validate(command); assert(first); state.transition("starting",key:key)
        assert(state.flowSession() == key && !state.explicitlyStopped)
        let duplicate = try state.validate(command); assert(!duplicate)
        expectFailure { _ = try state.validate(envelope(key,type:"stop",operation:command.operation)) }
        let old = SessionKey(session_id:key.session_id,config_revision:1,network_generation:6)
        expectFailure { _ = try state.validate(envelope(old,type:"stop")) }
        expectFailure { _ = try ControlEnvelope(encodeJSON(["version":99,"operation_id":UUID().uuidString,"session":key.object,"command":["type":"start"]])) }
        expectFailure { _ = try envelope(SessionKey(session_id:key.session_id,config_revision:0,network_generation:7)) }
        expectFailure { _ = try envelope(key,operation:UUID(uuidString:"00000000-0000-0000-0000-000000000000")!) }
        expectFailure { _ = try ControlEnvelope(encodeJSON(["version":1,"operation_id":UUID().uuidString,"session":"invalid","command":["type":"start"]])) }
        state.transition("stopped",key:key)
        assert(state.flowSession() == nil && state.explicitlyStopped)
        for type in ["protect","start","replace","cancel"] {
            expectFailure { _ = try state.validate(envelope(key,type:type)) }
        }
        let fresh = SessionKey(session_id:UUID(),config_revision:1,network_generation:0)
        _ = try state.validate(envelope(fresh,type:"protect")); state.transition("protected",key:fresh)
        state.transition("stopped",key:fresh)
        expectFailure { _ = try state.validate(envelope(key,type:"protect")) }
        print("Native lifecycle fencing: passed")

        let transactional = NativeState(), configuration = try envelope(key)
        var configurations = 0
        expectFailure {
            _ = try transactional.transact(configuration) {
                configurations += 1
                transactional.transition("protected",key:key,error:"policy_failed")
                throw NativeFailure.providerUnavailable
            }
        }
        assert(transactional.snapshot()["protection_active"] as? Bool == true)
        assert(transactional.snapshot()["error_code"] as? String == "policy_failed")
        let retried = try transactional.transact(configuration) {
            configurations += 1; transactional.transition("starting",key:key)
        }
        let replayed = try transactional.transact(configuration) { configurations += 1 }
        let retriedObject = try JSONSerialization.jsonObject(with:retried) as! [String:Any]
        let replayedObject = try JSONSerialization.jsonObject(with:replayed) as! [String:Any]
        assert(retriedObject["acceptance"] as? String == "accepted")
        assert(replayedObject["acceptance"] as? String == "duplicate")
        assert(configurations == 2)
        expectFailure {
            _ = try transactional.transact(envelope(key,type:"stop",operation:configuration.operation)) {
                configurations += 1
            }
        }
        assert(configurations == 2)
        print("Failed native configuration retries before duplicate acceptance: passed")

        // A stopped Rust runtime has not yet disabled the independent filter.
        let stopped = try packetRuntimeReply(encodeJSON(["operation_id":command.operation.uuidString,
            "acceptance":"accepted","snapshot":["phase":"stopped","protection_active":false,
            "production_capable":false,"session":key.object]]))
        let stoppedObject = try JSONSerialization.jsonObject(with:stopped) as! [String:Any]
        let stoppedSnapshot = stoppedObject["snapshot"] as! [String:Any]
        assert(stoppedSnapshot["phase"] as? String == "stopped")
        assert(stoppedSnapshot["protection_active"] as? Bool == true)
        assert(stoppedObject["operation_id"] as? String == command.operation.uuidString)
        expectFailure { _ = try packetRuntimeReply(encodeJSON(["acceptance":"accepted"])) }
        try validateStopRecovery(key,snapshot:["session":NSNull()])
        try validateStopRecovery(key,snapshot:["session":key.object])
        try validateStopRecovery(key,snapshot:["session":SessionKey(session_id:key.session_id,
            config_revision:1,network_generation:0).object])
        expectFailure { try validateStopRecovery(old,snapshot:["session":key.object]) }
        expectFailure { try validateStopRecovery(SessionKey(session_id:UUID(),config_revision:2,
            network_generation:8),snapshot:["session":key.object]) }
        expectFailure { try validateStopRecovery(key,snapshot:["session":["session_id":"invalid"]]) }
        print("Packet runtime stop retains independent protection: passed")

        // NE may have started a process before it accepts the Engine's Start,
        // or one role may still own the previous revision after partial Replace.
        let noHead = try recoveryStopTarget(key,snapshot:["session":NSNull(),"phase":"stopped"])
        assert(noHead == nil)
        let exact = try recoveryStopTarget(key,snapshot:["session":key.object])
        assert(exact == key)
        let next = SessionKey(session_id:key.session_id,config_revision:2,network_generation:8)
        let preceding = try recoveryStopTarget(next,snapshot:["session":key.object])
        assert(preceding == key)
        expectFailure { _ = try recoveryStopTarget(key,snapshot:["session":next.object]) }
        expectFailure { _ = try recoveryStopTarget(key,snapshot:["session":SessionKey(session_id:UUID(),config_revision:1,network_generation:0).object]) }
        expectFailure { _ = try recoveryStopTarget(key,snapshot:[:]) }
        expectFailure { _ = try recoveryStopTarget(key,snapshot:["session":key.session_id.uuidString]) }
        expectFailure { _ = try recoveryStopTarget(key,snapshot:["session":SessionKey(session_id:key.session_id,config_revision:0,network_generation:0).object]) }
        print("Partial provider stop recovery fencing: passed")

        runPhysicalNetworkContracts()

        let pipe = Pipe(), payload = Data("{\"contract\":true}".utf8)
        try FrameIO.write(payload,to:pipe.fileHandleForWriting); try pipe.fileHandleForWriting.close()
        let decoded = try FrameIO.read(pipe.fileHandleForReading); assert(decoded == payload)
        let end = try FrameIO.read(pipe.fileHandleForReading); assert(end == nil)
        let partial = Pipe(); try partial.fileHandleForWriting.write(contentsOf:Data([0,0,0,3,1])); try partial.fileHandleForWriting.close()
        expectFailure { _ = try FrameIO.read(partial.fileHandleForReading) }
        let oversized = Pipe(); try oversized.fileHandleForWriting.write(contentsOf:Data([0xff,0xff,0xff,0xff])); try oversized.fileHandleForWriting.close()
        expectFailure { _ = try FrameIO.read(oversized.fileHandleForReading) }
        print("Bounded framing, truncation and oversize: passed")

        var dhcp = Data(repeating:0,count:20+8+240)
        dhcp[0] = 0x45; dhcp[2] = UInt8(dhcp.count >> 8); dhcp[3] = UInt8(dhcp.count & 255); dhcp[9] = 17
        for index in 16..<20 { dhcp[index] = 255 }
        dhcp[21] = 68; dhcp[23] = 67; dhcp[24] = 0; dhcp[25] = 248
        dhcp[28] = 1; dhcp[29] = 1; dhcp[30] = 6
        dhcp.replaceSubrange(264..<268,with:[99,130,83,99])
        assert(GuardPacket.isRequiredControlPlane(dhcp))
        dhcp[23] = 53; assert(!GuardPacket.isRequiredControlPlane(dhcp))
        assert(!GuardPacket.isRequiredControlPlane(Data([0x60])))
        var nd = Data(repeating:0,count:64); nd[0] = 0x60; nd[5] = 24; nd[6] = 58; nd[7] = 255
        nd[24] = 0xff; nd[25] = 2; nd[39] = 1; nd[40] = 135
        assert(GuardPacket.isRequiredControlPlane(nd)); nd[7] = 64
        assert(!GuardPacket.isRequiredControlPlane(nd)); nd[7] = 255; nd[40] = 128
        assert(!GuardPacket.isRequiredControlPlane(nd))
        print("Control-plane allowlist and malformed packet rejection: passed")
        print("Native isolated contracts passed; no providers installed or activated.")
    }
}
