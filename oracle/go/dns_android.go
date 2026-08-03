//go:build android && !cgo
// +build android,!cgo

package main

import (
	"context"
	"net"
	"sync"
	"time"
)

func init() {
	// On Android, when not using cgo, we need to manually set up the default DNS resolver.
	// This resolver will attempt Cloudflare's DNS over both IPv4 and IPv6.

	var dialer net.Dialer
	dnsServers := []string{
		"[2606:4700:4700::1111]:53", // Cloudflare IPv6
		"[2606:4700:4700::1001]:53", // Cloudflare IPv6
		"1.1.1.1:53",                // Cloudflare IPv4
		"1.0.0.1:53",                // Cloudflare IPv4
	}

	net.DefaultResolver = &net.Resolver{
		PreferGo: false,
		Dial: func(ctx context.Context, network, address string) (net.Conn, error) {
			ctx, cancel := context.WithCancel(ctx)
			defer cancel()

			var wg sync.WaitGroup
			result := make(chan net.Conn, 1)
			errChan := make(chan error, len(dnsServers))

			for _, ip := range dnsServers {
				wg.Add(1)
				go func(ip string) {
					defer wg.Done()
					conn, err := dialer.DialContext(ctx, "udp", ip)
					if err == nil {
						select {
						case result <- conn:
							cancel()
						default:
						}
					} else {
						errChan <- err
					}
				}(ip)
			}

			go func() {
				wg.Wait()
				close(result)
				close(errChan)
			}()

			select {
			case conn := <-result:
				return conn, nil
			case <-time.After(2 * time.Second):
				return nil, net.ErrClosed
			}
		},
	}
}
