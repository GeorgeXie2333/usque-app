#pragma once
#include <string>
#include <openvpn/addr/ip.hpp>
namespace openvpn {
// The memory transport has no physical adapter. Do not enumerate adapters or
// send a hardware identifier to public VPN servers. This also removes the
// upstream Windows TAP-header dependency from the protocol-only build.
inline std::string get_hwaddr([[maybe_unused]] IP::Addr server_addr) { return {}; }
}
