import Foundation

func runPhysicalNetworkContracts() {
    let wifi = PhysicalInterfaceObservation(name:"en0",index:4,type:"wifi",isUsed:true,addresses:["192.0.2.10","2001:db8::10"])
    let unusedEthernet = PhysicalInterfaceObservation(name:"en5",index:9,type:"wiredEthernet",isUsed:false,addresses:["198.51.100.10"])
    var path = PhysicalPathState()
    assert(!path.snapshot.permits(generation:0))
    assert(path.snapshot.selectedInterface == nil)
    assert(!path.update(satisfied:true,ipv4:true,ipv6:false,interfaces:[unusedEthernet,wifi]))
    let initial = path.snapshot.generation
    assert(initial > 0 && path.snapshot.selectedInterface == wifi)
    assert(path.snapshot.permits(generation:initial,interface:wifi.index))
    assert(!path.snapshot.permits(generation:initial,interface:unusedEthernet.index))
    // Collection order and VPN aggregate IPv6 changes are telemetry, not a
    // physical network change. Neither may revoke the current physical fence.
    assert(!path.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[wifi,unusedEthernet]))
    assert(path.snapshot.generation == initial && path.snapshot.ipv6)
    let reorderedWifi = PhysicalInterfaceObservation(name:"en0",index:4,type:"wifi",isUsed:true,addresses:["2001:db8::10","192.0.2.10","192.0.2.10"])
    assert(!path.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[reorderedWifi,unusedEthernet]))
    let renumberedWifi = PhysicalInterfaceObservation(name:"en0",index:4,type:"wifi",isUsed:true,addresses:["192.0.2.11","2001:db8::11"])
    assert(path.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[renumberedWifi,unusedEthernet]))
    assert(path.snapshot.generation > initial && !path.snapshot.permits(generation:initial))
    // The interfaces can stay available while the actual used type changes.
    let unusedWifi = PhysicalInterfaceObservation(name:"en0",index:4,type:"wifi",isUsed:false,addresses:["192.0.2.11"])
    let ethernet = PhysicalInterfaceObservation(name:"en5",index:9,type:"wiredEthernet",isUsed:true,addresses:["198.51.100.10"])
    assert(path.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[ethernet,unusedWifi]))
    let switched = path.snapshot
    assert(switched.generation > initial && switched.selectedInterface == ethernet)
    assert(!switched.permits(generation:initial)) // Stale allow/commit/lease/packet.
    assert(switched.permits(generation:switched.generation,interface:ethernet.index))
    assert(!switched.permits(generation:switched.generation,interface:wifi.index))
    assert(path.update(satisfied:false,ipv4:true,ipv6:true,interfaces:[ethernet,unusedWifi]))
    let lost = path.snapshot.generation
    assert(!path.snapshot.permits(generation:lost) && path.snapshot.selectedInterface == nil)
    assert(path.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[ethernet,unusedWifi]))
    assert(path.snapshot.generation > lost)
    assert(!path.snapshot.permits(generation:lost))
    // A satisfied aggregate VPN path without a used physical interface does
    // not authorize a candidate physical route.
    assert(path.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[unusedEthernet,unusedWifi]))
    assert(!path.snapshot.satisfied && path.snapshot.selectedInterface == nil)
    let tunnel = PhysicalInterfaceObservation(name:"utun9",index:80,type:"wifi",isUsed:true,addresses:["192.0.2.1"])
    var tunnelOnly = PhysicalPathState()
    _ = tunnelOnly.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[tunnel])
    assert(!tunnelOnly.snapshot.satisfied && !tunnelOnly.snapshot.permits(generation:tunnelOnly.snapshot.generation))
    var addressless = PhysicalPathState()
    _ = addressless.update(satisfied:true,ipv4:true,ipv6:true,interfaces:[PhysicalInterfaceObservation(name:"en0",index:4,type:"wifi",isUsed:true)])
    assert(!addressless.snapshot.satisfied && addressless.snapshot.selectedInterface == nil)
    print("Deterministic physical generations and stale permit rejection: passed")
}
