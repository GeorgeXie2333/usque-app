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
        let state = NativeState()
        assert(state.snapshot()["protection_active"] as? Bool == true)
        assert(state.snapshot()["error_code"] as? String == "missing_credentials")
        let command = try envelope(key)
        let first = try state.validate(command); assert(first); state.transition("starting",key:key)
        let duplicate = try state.validate(command); assert(!duplicate)
        expectFailure { _ = try state.validate(envelope(key,type:"stop",operation:command.operation)) }
        let old = SessionKey(session_id:key.session_id,config_revision:1,network_generation:6)
        expectFailure { _ = try state.validate(envelope(old,type:"stop")) }
        expectFailure { _ = try ControlEnvelope(encodeJSON(["version":99,"operation_id":UUID().uuidString,"session":key.object,"command":["type":"start"]])) }
        expectFailure { _ = try envelope(SessionKey(session_id:key.session_id,config_revision:0,network_generation:7)) }
        expectFailure { _ = try envelope(key,operation:UUID(uuidString:"00000000-0000-0000-0000-000000000000")!) }
        print("Native lifecycle fencing: passed")

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
