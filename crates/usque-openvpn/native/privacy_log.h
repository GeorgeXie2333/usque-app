#pragma once
#include <openvpn/log/logbase.hpp>
#undef OPENVPN_LOG
#undef OPENVPN_LOG_NTNL
#undef OPENVPN_LOG_STRING
// Core logs can contain certificates, session tokens and peer-controlled text.
// Only the typed events emitted by our bridge cross into application logging.
#define OPENVPN_LOG(args) do {} while (false)
#define OPENVPN_LOG_NTNL(args) do {} while (false)
#define OPENVPN_LOG_STRING(args) do {} while (false)
