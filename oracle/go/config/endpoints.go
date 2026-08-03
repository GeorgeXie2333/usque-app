package config

import (
	"fmt"
	"log"
	"net"
)

const (
	HTTP2WikiURL         = "https://github.com/Diniboy1123/usque/wiki/HTTP-2-support"
	DefaultEndpointH2V4  = "162.159.198.2"
	DefaultEndpointH2V6  = ""
	EndpointHelpSuffixH2 = " For details: https://github.com/Diniboy1123/usque/wiki/HTTP-2-support"
)

// LogHTTP2Endpoint prints the wiki reference and active endpoint when HTTP/2 mode is enabled.
func LogHTTP2Endpoint(endpoint net.Addr) {
	log.Printf("HTTP/2 mode enabled. See %s", HTTP2WikiURL)
	log.Printf("Using HTTP/2 endpoint %s", endpoint.String())
}

// WarnInsecure prints a warning when certificate pinning is disabled.
func WarnInsecure() {
	log.Println("WARNING: --insecure is set, endpoint certificate pinning is disabled. Do not use in production!")
}

// SelectEndpointFromConfig returns a protocol-appropriate remote endpoint:
// TCP for HTTP/2 mode and UDP for HTTP/3 mode.
func SelectEndpointFromConfig(useHTTP2 bool, useIPv6 bool, port int) (net.Addr, error) {
	if useHTTP2 {
		if useIPv6 {
			if AppConfig.EndpointH2V6 == "" {
				return nil, fmt.Errorf("--http2 with --ipv6 requires config endpoint_h2_v6 to be set; see %s", HTTP2WikiURL)
			}

			ip := net.ParseIP(AppConfig.EndpointH2V6)
			if ip == nil {
				return nil, fmt.Errorf("invalid endpoint_h2_v6 value %q; see %s", AppConfig.EndpointH2V6, HTTP2WikiURL)
			}

			return &net.TCPAddr{IP: ip, Port: port}, nil
		}

		v4 := AppConfig.EndpointH2V4
		if v4 == "" {
			v4 = DefaultEndpointH2V4
		}

		ip := net.ParseIP(v4)
		if ip == nil {
			return nil, fmt.Errorf("invalid endpoint_h2_v4 value %q; see %s", v4, HTTP2WikiURL)
		}

		return &net.TCPAddr{IP: ip, Port: port}, nil
	}

	if useIPv6 {
		ip := net.ParseIP(AppConfig.EndpointV6)
		if ip == nil {
			return nil, fmt.Errorf("invalid endpoint_v6 value %q", AppConfig.EndpointV6)
		}
		return &net.UDPAddr{IP: ip, Port: port}, nil
	}

	ip := net.ParseIP(AppConfig.EndpointV4)
	if ip == nil {
		return nil, fmt.Errorf("invalid endpoint_v4 value %q", AppConfig.EndpointV4)
	}
	return &net.UDPAddr{IP: ip, Port: port}, nil
}
