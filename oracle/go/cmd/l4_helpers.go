package cmd

import (
	"fmt"
	"log"
	"net"
	"net/netip"
	"time"

	"github.com/Diniboy1123/usque/api"
	"github.com/Diniboy1123/usque/config"
	"github.com/Diniboy1123/usque/internal"
	quic "github.com/quic-go/quic-go"
	"github.com/spf13/cobra"
)

type l4ProxyOptions struct {
	bind              string
	port              string
	username          string
	password          string
	connectPort       int
	dnsServers        []string
	dnsTimeout        time.Duration
	useIPv6           bool
	keepalivePeriod   time.Duration
	initialPacketSize uint16
	insecure          bool
	localDNS          bool
	systemDNS         bool
	onConnect         string
	onDisconnect      string
}

func buildL4Proxy(cmd *cobra.Command, mode string) (l4ProxyOptions, *api.L4Proxy, error) {
	var opts l4ProxyOptions
	var err error

	if !config.ConfigLoaded {
		return opts, nil, fmt.Errorf("config not loaded: please register first")
	}

	if opts.bind, err = cmd.Flags().GetString("bind"); err != nil {
		return opts, nil, fmt.Errorf("failed to get bind address: %v", err)
	}
	if opts.port, err = cmd.Flags().GetString("port"); err != nil {
		return opts, nil, fmt.Errorf("failed to get port: %v", err)
	}
	if opts.username, err = cmd.Flags().GetString("username"); err != nil {
		return opts, nil, fmt.Errorf("failed to get username: %v", err)
	}
	if opts.password, err = cmd.Flags().GetString("password"); err != nil {
		return opts, nil, fmt.Errorf("failed to get password: %v", err)
	}
	if opts.connectPort, err = cmd.Flags().GetInt("connect-port"); err != nil {
		return opts, nil, fmt.Errorf("failed to get connect port: %v", err)
	}
	if opts.dnsServers, err = cmd.Flags().GetStringArray("dns"); err != nil {
		return opts, nil, fmt.Errorf("failed to get DNS servers: %v", err)
	}
	if opts.dnsTimeout, err = cmd.Flags().GetDuration("dns-timeout"); err != nil {
		return opts, nil, fmt.Errorf("failed to get DNS timeout: %v", err)
	}
	if opts.useIPv6, err = cmd.Flags().GetBool("ipv6"); err != nil {
		return opts, nil, fmt.Errorf("failed to get ipv6 flag: %v", err)
	}
	if opts.keepalivePeriod, err = cmd.Flags().GetDuration("keepalive-period"); err != nil {
		return opts, nil, fmt.Errorf("failed to get keepalive period: %v", err)
	}
	if opts.initialPacketSize, err = cmd.Flags().GetUint16("initial-packet-size"); err != nil {
		return opts, nil, fmt.Errorf("failed to get initial packet size: %v", err)
	}
	if opts.insecure, err = cmd.Flags().GetBool("insecure"); err != nil {
		return opts, nil, fmt.Errorf("failed to get insecure flag: %v", err)
	}
	if opts.localDNS, err = cmd.Flags().GetBool("local-dns"); err != nil {
		return opts, nil, fmt.Errorf("failed to get local-dns flag: %v", err)
	}
	if opts.systemDNS, err = cmd.Flags().GetBool("system-dns"); err != nil {
		return opts, nil, fmt.Errorf("failed to get system-dns flag: %v", err)
	}
	if opts.onConnect, err = cmd.Flags().GetString("on-connect"); err != nil {
		return opts, nil, fmt.Errorf("failed to get on-connect flag: %v", err)
	}
	if opts.onDisconnect, err = cmd.Flags().GetString("on-disconnect"); err != nil {
		return opts, nil, fmt.Errorf("failed to get on-disconnect flag: %v", err)
	}
	if opts.systemDNS && !opts.localDNS {
		log.Println("Warning: --system-dns only applies with -l; ignoring")
		opts.systemDNS = false
	}

	privKey, err := config.AppConfig.GetEcPrivateKey()
	if err != nil {
		return opts, nil, fmt.Errorf("failed to get private key: %v", err)
	}
	peerPubKey, err := config.AppConfig.GetEcEndpointPublicKey()
	if err != nil {
		return opts, nil, fmt.Errorf("failed to get public key: %v", err)
	}
	cert, err := internal.GenerateCert(privKey, &privKey.PublicKey)
	if err != nil {
		return opts, nil, fmt.Errorf("failed to generate cert: %v", err)
	}
	tlsConfig, err := api.PrepareTlsConfig(privKey, peerPubKey, cert, internal.L4ConnectSNI, opts.insecure)
	if err != nil {
		return opts, nil, fmt.Errorf("failed to prepare TLS config: %v", err)
	}
	if opts.insecure {
		config.WarnInsecure()
	}

	endpointAddr, err := config.SelectEndpointFromConfig(false, opts.useIPv6, opts.connectPort)
	if err != nil {
		return opts, nil, fmt.Errorf("failed to select endpoint: %v", err)
	}
	endpoint, ok := endpointAddr.(*net.UDPAddr)
	if !ok {
		return opts, nil, fmt.Errorf("l4 proxy requires an HTTP/3 UDP endpoint")
	}

	dnsAddrs, err := parseDNSAddrs(opts.dnsServers)
	if err != nil {
		return opts, nil, fmt.Errorf("failed to parse DNS server: %v", err)
	}

	hookEnv := map[string]string{
		"USQUE_MODE": mode,
		"USQUE_IPV4": config.AppConfig.IPv4,
		"USQUE_IPV6": config.AppConfig.IPv6,
	}

	resolver := &internal.TunnelDNSResolver{
		DNSAddrs:      dnsAddrs,
		Timeout:       opts.dnsTimeout,
		UseOSResolver: opts.localDNS && opts.systemDNS,
	}

	proxy, err := api.NewL4Proxy(api.L4ProxyConfig{
		TLSConfig:      tlsConfig,
		QUICConfig:     l4QUICConfig(opts.keepalivePeriod, opts.initialPacketSize),
		Endpoint:       endpoint,
		DNSResolver:    resolver,
		ResolveLocally: opts.localDNS,
		OnConnect: func(target string) {
			env := cloneHookEnv(hookEnv)
			env["USQUE_EVENT"] = "connect"
			env["USQUE_TARGET"] = target
			api.RunHook(opts.onConnect, env)
		},
		OnDisconnect: func(target string) {
			env := cloneHookEnv(hookEnv)
			env["USQUE_EVENT"] = "disconnect"
			env["USQUE_TARGET"] = target
			api.RunHook(opts.onDisconnect, env)
		},
	})
	if err != nil {
		return opts, nil, fmt.Errorf("failed to create l4 proxy: %v", err)
	}

	return opts, proxy, nil
}

func l4QUICConfig(keepalivePeriod time.Duration, initialPacketSize uint16) *quic.Config {
	cfg := &quic.Config{
		EnableDatagrams:                false,
		KeepAlivePeriod:                keepalivePeriod,
		InitialConnectionReceiveWindow: 10_000_000,
		MaxConnectionReceiveWindow:     10_000_000,
		InitialStreamReceiveWindow:     1_000_000,
		MaxStreamReceiveWindow:         1_000_000,
		MaxIncomingStreams:             100,
		MaxIncomingUniStreams:          100,
	}
	if initialPacketSize > 0 {
		cfg.InitialPacketSize = initialPacketSize
		cfg.DisablePathMTUDiscovery = true
	}
	return cfg
}

func cloneHookEnv(in map[string]string) map[string]string {
	out := make(map[string]string, len(in)+2)
	for k, v := range in {
		out[k] = v
	}
	return out
}

func parseDNSAddrs(servers []string) ([]netip.Addr, error) {
	addrs := make([]netip.Addr, 0, len(servers))
	for _, server := range servers {
		addr, err := netip.ParseAddr(server)
		if err != nil {
			return nil, err
		}
		addrs = append(addrs, addr)
	}
	return addrs, nil
}

func addL4ProxyFlags(cmd *cobra.Command, defaultPort, proxyName string) {
	cmd.Flags().StringP("bind", "b", "0.0.0.0", "Address to bind the "+proxyName+" proxy to")
	cmd.Flags().StringP("port", "p", defaultPort, "Port to listen on for "+proxyName+" proxy")
	cmd.Flags().StringP("username", "u", "", "Username for proxy authentication (specify both username and password to enable)")
	cmd.Flags().StringP("password", "w", "", "Password for proxy authentication (specify both username and password to enable)")
	cmd.Flags().IntP("connect-port", "P", 443, "Used port for MASQUE connection")
	cmd.Flags().StringArrayP("dns", "d", []string{"9.9.9.9", "149.112.112.112", "2620:fe::fe", "2620:fe::9"}, "DNS servers for local proxy name lookups with -l (unless --system-dns)")
	cmd.Flags().DurationP("dns-timeout", "t", 2*time.Second, "Timeout for DNS queries")
	cmd.Flags().BoolP("ipv6", "6", false, "Use IPv6 for MASQUE connection")
	cmd.Flags().DurationP("keepalive-period", "k", 30*time.Second, "Keepalive period for MASQUE connection")
	cmd.Flags().Uint16P("initial-packet-size", "i", 0, "Custom initial packet size for MASQUE connection (default: auto with PMTU discovery)")
	cmd.Flags().Bool("insecure", false, "Disable endpoint certificate pinning and trust any certificate")
	cmd.Flags().BoolP("local-dns", "l", true, "Resolve proxy target names locally before opening L4 CONNECT streams (required for hostname targets)")
	cmd.Flags().Bool("system-dns", false, "Resolve names via the OS (e.g. /etc/resolv.conf) instead of -d")
	cmd.Flags().String("on-connect", "", "Path to an executable to run after each successful L4 CONNECT stream (no args; context via USQUE_* env vars)")
	cmd.Flags().String("on-disconnect", "", "Path to an executable to run after each L4 CONNECT stream closes (no args; context via USQUE_* env vars)")
}
