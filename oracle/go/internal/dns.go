package internal

import (
	"context"
	"fmt"
	"net"
	"net/netip"
	"time"

	"golang.zx2c4.com/wireguard/tun/netstack"
)

// TunnelDNSResolver implements a DNS resolver that uses the provided DNS servers
// either inside a MASQUE tunnel (if TunNet is set) or over the system network (if TunNet is nil).
type TunnelDNSResolver struct {
	// TunNet is the network stack for the tunnel you want to use for DNS resolution.
	// If nil, DNS queries are sent over the system network.
	TunNet *netstack.Net

	// DNSAddrs is the list of DNS servers to use for resolution.
	DNSAddrs []netip.Addr

	// Timeout is the timeout for DNS queries on a specific server before trying the next one.
	Timeout time.Duration

	// UseOSResolver, when true, uses net.DefaultResolver for Resolve instead of DNSAddrs.
	// Set when -l and --system-dns; otherwise with -l, DNSAddrs are queried over the host.
	UseOSResolver bool
}

// Resolve performs a DNS lookup using the provided DNS resolvers.
// It tries each resolver in order until one succeeds, sending queries either through the tunnel
// or over the system network depending on TunNet.
func (r TunnelDNSResolver) Resolve(ctx context.Context, name string) (net.IP, error) {
	if r.UseOSResolver {
		queryCtx := ctx
		var cancel context.CancelFunc
		if r.Timeout > 0 {
			queryCtx, cancel = context.WithTimeout(ctx, r.Timeout)
			defer cancel()
		}
		ips, err := net.DefaultResolver.LookupIP(queryCtx, "ip", name)
		if err != nil {
			return nil, err
		}
		if len(ips) == 0 {
			return nil, fmt.Errorf("no IP address for %q", name)
		}
		return ips[0], nil
	}

	if len(r.DNSAddrs) == 0 {
		return nil, fmt.Errorf("no DNS servers configured")
	}

	queryCtx := ctx
	var cancel context.CancelFunc
	if r.Timeout > 0 {
		queryCtx, cancel = context.WithTimeout(ctx, r.Timeout)
		defer cancel()
	}

	type result struct {
		ip  net.IP
		err error
	}
	results := make(chan result, len(r.DNSAddrs))

	for _, dnsAddr := range r.DNSAddrs {
		dnsHost := net.JoinHostPort(dnsAddr.String(), "53")

		go func(dnsHost string) {
			var dialFunc func(context.Context, string, string) (net.Conn, error)
			if r.TunNet != nil {
				dialFunc = func(ctx context.Context, network, address string) (net.Conn, error) {
					return r.TunNet.DialContext(ctx, "udp", dnsHost)
				}
			} else {
				dialFunc = func(ctx context.Context, network, address string) (net.Conn, error) {
					return net.Dial("udp", dnsHost)
				}
			}

			resolver := &net.Resolver{
				PreferGo: true,
				Dial:     dialFunc,
			}
			ips, err := resolver.LookupIP(queryCtx, "ip", name)
			if err == nil && len(ips) > 0 {
				results <- result{ip: ips[0], err: nil}
			} else {
				results <- result{ip: nil, err: err}
			}
		}(dnsHost)
	}

	var lastErr error
	for i := 0; i < len(r.DNSAddrs); i++ {
		res := <-results
		if res.err == nil && res.ip != nil {
			if cancel != nil {
				cancel()
			}
			return res.ip, nil
		}
		lastErr = res.err
	}

	return nil, fmt.Errorf("all DNS servers failed: %v", lastErr)
}

// NewNetstackResolver returns a *net.Resolver that uses the tunnel network stack
// and provided DNS servers for DNS queries.
//
// Parameters:
//   - tunNet: *netstack.Net - The tunnel network stack.
//   - dnsAddrs: []netip.Addr - DNS server addresses.
//
// Returns:
//   - *net.Resolver - A resolver that routes queries through the tunnel.
func NewNetstackResolver(tunNet *netstack.Net, dnsAddrs []netip.Addr) *net.Resolver {
	return &net.Resolver{
		PreferGo: true,
		Dial: func(ctx context.Context, network, address string) (net.Conn, error) {
			if len(dnsAddrs) == 0 {
				return nil, fmt.Errorf("no DNS servers configured")
			}
			dnsHost := net.JoinHostPort(dnsAddrs[0].String(), "53")
			return tunNet.DialContext(ctx, "udp", dnsHost)
		},
	}
}

// NewStaticResolver returns a *net.Resolver that sends DNS to dnsAddrs over the system network.
func NewStaticResolver(dnsAddrs []netip.Addr) *net.Resolver {
	return &net.Resolver{
		PreferGo: true,
		Dial: func(ctx context.Context, network, address string) (net.Conn, error) {
			if len(dnsAddrs) == 0 {
				return nil, fmt.Errorf("no DNS servers configured")
			}
			dnsHost := net.JoinHostPort(dnsAddrs[0].String(), "53")
			return net.Dial("udp", dnsHost)
		},
	}
}

// GetProxyResolver returns the appropriate *net.Resolver for HTTP proxy CONNECT handling.
//
//   - localDNS: do not use the tunnel for DNS; use dnsAddrs on the host, or OS if systemDNS.
//   - systemDNS: with localDNS, use net.DefaultResolver (ignores dnsAddrs for lookups).
func GetProxyResolver(localDNS, systemDNS bool, tunNet *netstack.Net, dnsAddrs []netip.Addr, timeout time.Duration) *net.Resolver {
	if localDNS {
		if systemDNS {
			return net.DefaultResolver
		}
		return NewStaticResolver(dnsAddrs)
	}
	return NewNetstackResolver(tunNet, dnsAddrs)
}
