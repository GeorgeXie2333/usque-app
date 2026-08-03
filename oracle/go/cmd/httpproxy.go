package cmd

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"net/netip"
	"time"

	"github.com/Diniboy1123/usque/api"
	"github.com/Diniboy1123/usque/config"
	"github.com/Diniboy1123/usque/internal"
	"github.com/spf13/cobra"
	"golang.zx2c4.com/wireguard/tun/netstack"
)

var httpProxyCmd = &cobra.Command{
	Use:   "http-proxy",
	Short: "Expose Warp as an HTTP proxy with CONNECT support",
	Long:  "Dual-stack HTTP proxy with CONNECT support. Doesn't require elevated privileges.",
	Run: func(cmd *cobra.Command, args []string) {
		if !config.ConfigLoaded {
			cmd.Println("Config not loaded. Please register first.")
			return
		}
		log.Println("Hint: l4-http-proxy is faster for TCP-only HTTP proxy use cases.")

		sni, err := cmd.Flags().GetString("sni-address")
		if err != nil {
			cmd.Printf("Failed to get SNI address: %v\n", err)
			return
		}

		privKey, err := config.AppConfig.GetEcPrivateKey()
		if err != nil {
			cmd.Printf("Failed to get private key: %v\n", err)
			return
		}
		peerPubKey, err := config.AppConfig.GetEcEndpointPublicKey()
		if err != nil {
			cmd.Printf("Failed to get public key: %v\n", err)
			return
		}

		cert, err := internal.GenerateCert(privKey, &privKey.PublicKey)
		if err != nil {
			cmd.Printf("Failed to generate cert: %v\n", err)
			return
		}

		insecure, err := cmd.Flags().GetBool("insecure")
		if err != nil {
			cmd.Printf("Failed to get insecure flag: %v\n", err)
			return
		}

		tlsConfig, err := api.PrepareTlsConfig(privKey, peerPubKey, cert, sni, insecure)
		if err != nil {
			cmd.Printf("Failed to prepare TLS config: %v\n", err)
			return
		}

		keepalivePeriod, err := cmd.Flags().GetDuration("keepalive-period")
		if err != nil {
			cmd.Printf("Failed to get keepalive period: %v\n", err)
			return
		}
		initialPacketSize, err := cmd.Flags().GetUint16("initial-packet-size")
		if err != nil {
			cmd.Printf("Failed to get initial packet size: %v\n", err)
			return
		}

		bindAddress, err := cmd.Flags().GetString("bind")
		if err != nil {
			cmd.Printf("Failed to get bind address: %v\n", err)
			return
		}

		port, err := cmd.Flags().GetString("port")
		if err != nil {
			cmd.Printf("Failed to get port: %v\n", err)
			return
		}

		connectPort, err := cmd.Flags().GetInt("connect-port")
		if err != nil {
			cmd.Printf("Failed to get connect port: %v\n", err)
			return
		}

		useHTTP2, err := cmd.Flags().GetBool("http2")
		if err != nil {
			cmd.Printf("Failed to get HTTP/2 flag: %v\n", err)
			return
		}

		useIPv6, err := cmd.Flags().GetBool("ipv6")
		if err != nil {
			cmd.Printf("Failed to get ipv6 flag: %v\n", err)
			return
		}

		endpoint, err := config.SelectEndpointFromConfig(useHTTP2, useIPv6, connectPort)
		if err != nil {
			cmd.Printf("Failed to select endpoint: %v\n", err)
			return
		}

		if insecure {
			config.WarnInsecure()
		}

		if useHTTP2 {
			config.LogHTTP2Endpoint(endpoint)
		}

		tunnelIPv4, err := cmd.Flags().GetBool("no-tunnel-ipv4")
		if err != nil {
			cmd.Printf("Failed to get no tunnel IPv4: %v\n", err)
			return
		}

		tunnelIPv6, err := cmd.Flags().GetBool("no-tunnel-ipv6")
		if err != nil {
			cmd.Printf("Failed to get no tunnel IPv6: %v\n", err)
			return
		}

		var localAddresses []netip.Addr
		if !tunnelIPv4 {
			v4, err := netip.ParseAddr(config.AppConfig.IPv4)
			if err != nil {
				cmd.Printf("Failed to parse IPv4 address: %v\n", err)
				return
			}
			localAddresses = append(localAddresses, v4)
		}
		if !tunnelIPv6 {
			v6, err := netip.ParseAddr(config.AppConfig.IPv6)
			if err != nil {
				cmd.Printf("Failed to parse IPv6 address: %v\n", err)
				return
			}
			localAddresses = append(localAddresses, v6)
		}

		dnsServers, err := cmd.Flags().GetStringArray("dns")
		if err != nil {
			cmd.Printf("Failed to get DNS servers: %v\n", err)
			return
		}

		var dnsAddrs []netip.Addr
		for _, dns := range dnsServers {
			addr, err := netip.ParseAddr(dns)
			if err != nil {
				cmd.Printf("Failed to parse DNS server: %v\n", err)
				return
			}
			dnsAddrs = append(dnsAddrs, addr)
		}

		dnsTimeout, err := cmd.Flags().GetDuration("dns-timeout")
		if err != nil {
			cmd.Printf("Failed to get DNS timeout: %v\n", err)
			return
		}

		localDNS, err := cmd.Flags().GetBool("local-dns")
		if err != nil {
			cmd.Printf("Failed to get local-dns flag: %v\n", err)
			return
		}

		systemDNS, err := cmd.Flags().GetBool("system-dns")
		if err != nil {
			cmd.Printf("Failed to get system-dns flag: %v\n", err)
			return
		}
		if systemDNS && !localDNS {
			log.Println("Warning: --system-dns only applies with -l; ignoring")
			systemDNS = false
		}

		mtu, err := cmd.Flags().GetInt("mtu")
		if err != nil {
			cmd.Printf("Failed to get MTU: %v\n", err)
			return
		}
		if mtu != 1280 {
			log.Println("Warning: MTU is not the default 1280. This is not supported. Packet loss and other issues may occur.")
		}

		var username string
		var password string
		if u, err := cmd.Flags().GetString("username"); err == nil && u != "" {
			username = u
		}
		if p, err := cmd.Flags().GetString("password"); err == nil && p != "" {
			password = p
		}

		reconnectDelay, err := cmd.Flags().GetDuration("reconnect-delay")
		if err != nil {
			cmd.Printf("Failed to get reconnect delay: %v\n", err)
			return
		}

		alwaysReconnect, err := cmd.Flags().GetBool("always-reconnect")
		if err != nil {
			cmd.Printf("Failed to get always-reconnect flag: %v\n", err)
			return
		}

		onConnect, err := cmd.Flags().GetString("on-connect")
		if err != nil {
			cmd.Printf("Failed to get on-connect flag: %v\n", err)
			return
		}

		onDisconnect, err := cmd.Flags().GetString("on-disconnect")
		if err != nil {
			cmd.Printf("Failed to get on-disconnect flag: %v\n", err)
			return
		}

		hookEnv := map[string]string{
			"USQUE_MODE": "http-proxy",
			"USQUE_IPV4": config.AppConfig.IPv4,
			"USQUE_IPV6": config.AppConfig.IPv6,
		}

		var authHeader string
		if username != "" && password != "" {
			authHeader = "Basic " + internal.LoginToBase64(username, password)
		}

		tunDev, tunNet, err := netstack.CreateNetTUN(localAddresses, dnsAddrs, mtu)
		if err != nil {
			cmd.Printf("Failed to create virtual TUN device: %v\n", err)
			return
		}
		defer func() { _ = tunDev.Close() }()

		resolver := internal.GetProxyResolver(localDNS, systemDNS, tunNet, dnsAddrs, dnsTimeout)

		go api.MaintainTunnel(context.Background(), api.MaintainTunnelConfig{
			TLSConfig:         tlsConfig,
			KeepalivePeriod:   keepalivePeriod,
			InitialPacketSize: initialPacketSize,
			Endpoint:          endpoint,
			Device:            api.NewNetstackAdapter(tunDev),
			MTU:               mtu,
			ReconnectDelay:    reconnectDelay,
			AlwaysReconnect:   alwaysReconnect,
			UseHTTP2:          useHTTP2,
			OnConnect:         onConnect,
			OnDisconnect:      onDisconnect,
			HookEnv:           hookEnv,
		})

		server := &http.Server{
			Addr: net.JoinHostPort(bindAddress, port),
			Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if !authenticate(r, authHeader) {
					w.Header().Set("Proxy-Authenticate", `Basic realm="Proxy"`)
					http.Error(w, "Proxy authentication required", http.StatusProxyAuthRequired)
					return
				}

				if r.Method == http.MethodConnect {
					handleHTTPSConnect(w, r, tunNet, resolver)
				} else {
					handleHTTPProxy(w, r, tunNet, resolver)
				}
			}),
		}

		log.Printf("HTTP proxy listening on %s:%s\n", bindAddress, port)
		if err := server.ListenAndServe(); err != nil {
			cmd.Printf("Failed to start HTTP proxy: %v\n", err)
		}
	},
}

// authenticate verifies the Proxy-Authorization header in an HTTP request.
//
// Parameters:
//   - r: *http.Request - The incoming HTTP request.
//   - expectedAuth: string - The expected authorization token.
//
// Returns:
//   - bool: True if the authorization header matches the expected value, otherwise false.
func authenticate(r *http.Request, expectedAuth string) bool {
	authHeader := r.Header.Get("Proxy-Authorization")
	return authHeader == expectedAuth
}

// handleHTTPSConnect establishes a tunnel to the destination using the provided resolver.
//
// Parameters:
//   - w: http.ResponseWriter - The response writer for the HTTP request.
//   - r: *http.Request - The incoming HTTP request.
//   - tunNet: *netstack.Net - The netstack network interface.
//   - resolver: *net.Resolver - The DNS resolver to use for the tunnel.
func handleHTTPSConnect(w http.ResponseWriter, r *http.Request, tunNet *netstack.Net, resolver *net.Resolver) {
	ctx := r.Context()

	host, port, err := net.SplitHostPort(r.Host)
	if err != nil {
		http.Error(w, "Invalid host", http.StatusBadRequest)
		return
	}

	var destAddr string
	if resolver != nil {
		ips, err := resolver.LookupIP(ctx, "ip", host)
		if err != nil || len(ips) == 0 {
			http.Error(w, "DNS resolution failed", http.StatusServiceUnavailable)
			return
		}
		destAddr = net.JoinHostPort(ips[0].String(), port)
	} else {
		destAddr = r.Host
	}

	destConn, err := tunNet.DialContext(ctx, "tcp", destAddr)
	if err != nil {
		http.Error(w, "Unable to connect to destination", http.StatusServiceUnavailable)
		return
	}

	hj, ok := w.(http.Hijacker)
	if !ok {
		http.Error(w, "Hijacking not supported", http.StatusInternalServerError)
		_ = destConn.Close()
		return
	}

	clientConn, _, err := hj.Hijack()
	if err != nil {
		http.Error(w, "Hijacking failed", http.StatusInternalServerError)
		_ = destConn.Close()
		return
	}

	_, err = clientConn.Write([]byte("HTTP/1.1 200 Connection Established\r\n\r\n"))
	if err != nil {
		_ = clientConn.Close()
		_ = destConn.Close()
		return
	}

	go func() {
		defer func() { _ = destConn.Close() }()
		defer func() { _ = clientConn.Close() }()
		_, _ = io.Copy(destConn, clientConn)
	}()
	_, _ = io.Copy(clientConn, destConn)
}

// handleHTTPProxy forwards HTTP proxy requests to the destination and relays responses back to the client using the provided resolver.
//
// Parameters:
//   - w: http.ResponseWriter - The response writer for the HTTP request.
//   - r: *http.Request - The incoming HTTP request.
//   - tunNet: *netstack.Net - The netstack network interface.
//   - resolver: *net.Resolver - The DNS resolver to use for the tunnel.
func handleHTTPProxy(w http.ResponseWriter, r *http.Request, tunNet *netstack.Net, resolver *net.Resolver) {
	client := &http.Client{
		Transport: &http.Transport{
			DialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
				host, port, err := net.SplitHostPort(addr)
				if err != nil {
					return nil, fmt.Errorf("invalid address: %w", err)
				}

				var dialAddr string
				if resolver != nil {
					ips, err := resolver.LookupIP(ctx, "ip", host)
					if err != nil || len(ips) == 0 {
						return nil, fmt.Errorf("DNS resolution failed for %s: %w", host, err)
					}
					dialAddr = net.JoinHostPort(ips[0].String(), port)
				} else {
					dialAddr = addr
				}

				return tunNet.DialContext(ctx, network, dialAddr)
			},
		},
	}

	req, err := http.NewRequestWithContext(r.Context(), r.Method, r.URL.String(), r.Body)
	if err != nil {
		http.Error(w, "Invalid request", http.StatusBadRequest)
		return
	}
	req.Header = r.Header.Clone()

	resp, err := client.Do(req)
	if err != nil {
		http.Error(w, "Failed to reach destination", http.StatusServiceUnavailable)
		return
	}
	defer func() { _ = resp.Body.Close() }()

	copyHeader(w.Header(), resp.Header)
	w.WriteHeader(resp.StatusCode)
	_, _ = io.Copy(w, resp.Body)
}

// copyHeader copies HTTP headers from one header map to another.
//
// Parameters:
//   - dst: http.Header - The destination header map.
//   - src: http.Header - The source header map.
func copyHeader(dst, src http.Header) {
	for k, vv := range src {
		for _, v := range vv {
			dst.Add(k, v)
		}
	}
}

func init() {
	httpProxyCmd.Flags().StringP("bind", "b", "0.0.0.0", "Address to bind the HTTP proxy to")
	httpProxyCmd.Flags().StringP("port", "p", "8000", "Port to listen on for HTTP proxy")
	httpProxyCmd.Flags().StringP("username", "u", "", "Username for proxy authentication (specify both username and password to enable)")
	httpProxyCmd.Flags().StringP("password", "w", "", "Password for proxy authentication (specify both username and password to enable)")
	httpProxyCmd.Flags().IntP("connect-port", "P", 443, "Used port for MASQUE connection")
	httpProxyCmd.Flags().StringArrayP("dns", "d", []string{"9.9.9.9", "149.112.112.112", "2620:fe::fe", "2620:fe::9"}, "DNS servers for the tunnel stack; with -l also used for proxy name lookups (unless --system-dns)")
	httpProxyCmd.Flags().DurationP("dns-timeout", "t", 2*time.Second, "Timeout for DNS queries")
	httpProxyCmd.Flags().BoolP("ipv6", "6", false, "Use IPv6 for MASQUE connection")
	httpProxyCmd.Flags().BoolP("no-tunnel-ipv4", "F", false, "Disable IPv4 inside the MASQUE tunnel")
	httpProxyCmd.Flags().BoolP("no-tunnel-ipv6", "S", false, "Disable IPv6 inside the MASQUE tunnel")
	httpProxyCmd.Flags().StringP("sni-address", "s", internal.ConnectSNI, "SNI address to use for MASQUE connection")
	httpProxyCmd.Flags().DurationP("keepalive-period", "k", 30*time.Second, "Keepalive period for MASQUE connection")
	httpProxyCmd.Flags().IntP("mtu", "m", 1280, "MTU for MASQUE connection")
	httpProxyCmd.Flags().Uint16P("initial-packet-size", "i", 0, "Custom initial packet size for MASQUE connection (default: auto with PMTU discovery)")
	httpProxyCmd.Flags().DurationP("reconnect-delay", "r", 1*time.Second, "Delay between reconnect attempts")
	httpProxyCmd.Flags().Bool("always-reconnect", false, "Always reconnect after tunnel loss, even when idle")
	httpProxyCmd.Flags().Bool("http2", false, "Use HTTP/2 over TCP+TLS instead of HTTP/3 over QUIC."+config.EndpointHelpSuffixH2)
	httpProxyCmd.Flags().Bool("insecure", false, "Disable endpoint certificate pinning and trust any certificate")
	httpProxyCmd.Flags().BoolP("local-dns", "l", false, "Do not send proxy DNS through the tunnel; use -d over the host instead. Add --system-dns to use the OS resolver instead of -d")
	httpProxyCmd.Flags().Bool("system-dns", false, "With -l, resolve names via the OS (e.g. /etc/resolv.conf) instead of -d")
	httpProxyCmd.Flags().String("on-connect", "", "Path to an executable to run after each successful tunnel connect (no args; context via USQUE_* env vars)")
	httpProxyCmd.Flags().String("on-disconnect", "", "Path to an executable to run after each tunnel disconnect (no args; context via USQUE_* env vars)")
	rootCmd.AddCommand(httpProxyCmd)
}
