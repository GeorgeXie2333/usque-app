import Foundation

/// A value-only observation keeps path identity independent of dictionary order
/// and of the aggregate IP support that can change when a VPN is installed.
struct PhysicalInterfaceObservation: Equatable {
    let name: String
    let index: Int
    let type: String
    let isUsed: Bool
    let addresses: [String]
    init(name:String,index:Int,type:String,isUsed:Bool,addresses:[String] = []) {
        self.name = name; self.index = index; self.type = type; self.isUsed = isUsed
        self.addresses = Array(Set(addresses)).sorted()
    }
    var usable: Bool { isUsed && !addresses.isEmpty }
    var object: [String: Any] { ["name":name,"index":index,"type":type,"is_used":isUsed,"addresses":addresses] }
}

struct PhysicalPathSnapshot {
    let generation: UInt64
    let satisfied: Bool
    let ipv4: Bool
    let ipv6: Bool
    let interfaces: [PhysicalInterfaceObservation]
    var selectedInterface: PhysicalInterfaceObservation? {
        satisfied ? interfaces.first(where: \.usable) : nil
    }
    /// Every permit must check the live filter-owned generation. Checking only
    /// when a lease is created leaves old leases usable before change callbacks.
    func permits(generation requested: UInt64, interface: Int? = nil) -> Bool {
        guard satisfied, generation > 0, requested == generation else { return false }
        guard let interface else { return true }
        return interfaces.contains { $0.usable && $0.index == interface }
    }
    var object: [String: Any] {
        ["generation":generation,"satisfied":satisfied,"ipv4":ipv4,"ipv6":ipv6,
         "interfaces":interfaces.map(\.object),"selected_interface":selectedInterface?.object ?? NSNull(),
         "physical_egress_verified":false]
    }
}

struct PhysicalPathState {
    private struct Identity: Equatable {
        let satisfied: Bool
        let interfaces: [PhysicalInterfaceObservation]
    }
    private var identity: Identity?
    private var exhausted = false
    private(set) var snapshot = PhysicalPathSnapshot(generation:0,satisfied:false,ipv4:false,ipv6:false,interfaces:[])
    /// The first observation initializes the authority, without reporting an
    /// existing session change. Subsequent physical changes advance its fence.
    mutating func update(satisfied: Bool, ipv4: Bool, ipv6: Bool,
                         interfaces: [PhysicalInterfaceObservation]) -> Bool {
        let ordered = interfaces.filter { $0.index > 0 && !$0.name.isEmpty && !$0.name.hasPrefix("utun") }
            .sorted { ($0.index, $0.name, $0.type, $0.isUsed ? 1 : 0) < ($1.index, $1.name, $1.type, $1.isUsed ? 1 : 0) }
        let available = satisfied && ordered.contains(where: \.usable)
        let next = Identity(satisfied:available,interfaces:ordered)
        let changed = identity != next, hadPath = identity != nil
        var generation = snapshot.generation
        if changed && !exhausted {
            if generation == .max { exhausted = true } else { generation += 1 }
        }
        // Never wrap a fence and accidentally accept a previous generation.
        snapshot = PhysicalPathSnapshot(generation:generation,
                                        satisfied:available && !exhausted,
                                        ipv4:ipv4,ipv6:ipv6,interfaces:ordered)
        identity = next
        return changed && hadPath
    }
}
