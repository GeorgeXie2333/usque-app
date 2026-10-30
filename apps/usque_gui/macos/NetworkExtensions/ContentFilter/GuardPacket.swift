import Foundation

/// Narrow IP control-plane exceptions required to keep the physical network
/// configured while ordinary egress is blocked. No generic UDP/DNS exception.
enum GuardPacket {
    static func isRequiredControlPlane(_ data: Data) -> Bool {
        guard let first = data.first else { return false }
        if first >> 4 == 4 {
            guard data.count >= 20 else { return false }
            let offset = Int(first & 15) * 4
            let length = Int(data[2])*256+Int(data[3])
            guard offset >= 20, length == data.count, length >= offset+8+240,
                  data[6]&0x3f == 0, data[7] == 0, data[9] == 17,
                  data[16..<20].elementsEqual([255,255,255,255]) else { return false }
            let udp = offset, bootp = offset+8
            guard data[udp..<udp+4].elementsEqual([0,68,0,67]),
                  Int(data[udp+4])*256+Int(data[udp+5]) == length-offset,
                  data[bootp] == 1, data[bootp+1] == 1, data[bootp+2] == 6,
                  data[bootp+236..<bootp+240].elementsEqual([99,130,83,99]) else { return false }
            return true
        }
        if first >> 4 == 6 {
            guard data.count >= 40, Int(data[4])*256+Int(data[5])+40 == data.count else { return false }
            let linkLocal = data[8] == 0xfe && data[9]&0xc0 == 0x80
            let unspecified = data[8..<24].allSatisfy { $0 == 0 }
            let multicastLinkLocal = data[24] == 0xff && data[25] == 2
            if data[6] == 58 {
                guard data.count >= 48, data[7] == 255, data[41] == 0,
                      [133,135,136].contains(data[40]), multicastLinkLocal,
                      linkLocal || unspecified else { return false }
                let minimum = data[40] == 133 ? 48 : 64
                return data.count >= minimum
            }
            if data[6] == 17 {
                guard data.count >= 52, linkLocal, multicastLinkLocal,
                      data[26..<39].allSatisfy({ $0 == 0 }), data[39] == 2,
                      data[40..<44].elementsEqual([2,34,2,35]),
                      Int(data[44])*256+Int(data[45]) == data.count-40,
                      [1,3,4,5,6,8,9,11].contains(data[48]) else { return false }
                return true
            }
        }
        return false
    }
}
