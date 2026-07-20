package api

import (
	"context"
	"crypto/tls"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"sync"
	"time"

	quic "github.com/quic-go/quic-go"
	"github.com/quic-go/quic-go/http3"
)

const (
	defaultL4ConnectTimeout    = 15 * time.Second
	defaultL4ConnectRetryCount = 2
)

// DNSResolver resolves a hostname for L4Proxy local DNS mode.
type DNSResolver interface {
	Resolve(ctx context.Context, name string) (net.IP, error)
}

// L4ProxyConfig configures a new L4Proxy.
type L4ProxyConfig struct {
	TLSConfig         *tls.Config
	QUICConfig        *quic.Config
	Endpoint          *net.UDPAddr
	DNSResolver       DNSResolver
	ResolveLocally    bool
	OnConnect         func(target string)
	OnDisconnect      func(target string)
	ConnectTimeout    time.Duration
	ConnectRetryCount int
}

// L4Proxy opens one HTTP/3 CONNECT stream for each proxied TCP connection.
type L4Proxy struct {
	tlsConfig         *tls.Config
	quicConfig        *quic.Config
	endpoint          *net.UDPAddr
	dnsResolver       DNSResolver
	resolveLocally    bool
	onConnect         func(target string)
	onDisconnect      func(target string)
	connectTimeout    time.Duration
	connectRetryCount int
	connMu            sync.Mutex
	client            *l4HTTP3Client
	dialFn            func(context.Context, string) (*l4TCPConn, error)
}

type l4HTTP3Client struct {
	udpConn    *net.UDPConn
	quicConn   *quic.Conn
	clientConn *http3.ClientConn
}

// NewL4Proxy creates an L4 proxy dialer from a configuration struct.
func NewL4Proxy(cfg L4ProxyConfig) (*L4Proxy, error) {
	if cfg.TLSConfig == nil {
		return nil, fmt.Errorf("missing TLS config")
	}
	if cfg.Endpoint == nil {
		return nil, fmt.Errorf("missing HTTP/3 UDP endpoint")
	}
	if cfg.ResolveLocally && cfg.DNSResolver == nil {
		return nil, fmt.Errorf("missing DNS resolver")
	}
	if cfg.ConnectTimeout <= 0 {
		cfg.ConnectTimeout = defaultL4ConnectTimeout
	}
	if cfg.ConnectRetryCount <= 0 {
		cfg.ConnectRetryCount = defaultL4ConnectRetryCount
	}

	proxy := &L4Proxy{
		tlsConfig:         cfg.TLSConfig,
		quicConfig:        cfg.QUICConfig,
		endpoint:          cfg.Endpoint,
		dnsResolver:       cfg.DNSResolver,
		resolveLocally:    cfg.ResolveLocally,
		onConnect:         cfg.OnConnect,
		onDisconnect:      cfg.OnDisconnect,
		connectTimeout:    cfg.ConnectTimeout,
		connectRetryCount: cfg.ConnectRetryCount,
	}
	proxy.dialFn = proxy.dial
	return proxy, nil
}

// DialContext connects target over an L4 MASQUE HTTP/3 CONNECT stream.
func (p *L4Proxy) DialContext(ctx context.Context, target string) (net.Conn, error) {
	if p == nil || p.tlsConfig == nil {
		return nil, fmt.Errorf("missing TLS config")
	}
	if p.endpoint == nil {
		return nil, fmt.Errorf("missing HTTP/3 UDP endpoint")
	}
	target, err := p.resolveTarget(ctx, target)
	if err != nil {
		return nil, err
	}

	timeout := p.connectTimeout
	if timeout <= 0 {
		timeout = defaultL4ConnectTimeout
	}
	attempts := p.connectRetryCount
	if attempts <= 0 {
		attempts = defaultL4ConnectRetryCount
	}
	dial := p.dialFn
	if dial == nil {
		dial = p.dial
	}

	var lastErr error
	for range attempts {
		dialCtx, cancel := context.WithTimeout(ctx, timeout)
		conn, err := dial(dialCtx, target)
		cancel()
		if err == nil {
			if p.onConnect != nil {
				p.onConnect(target)
			}
			conn.onClose = func() {
				if p.onDisconnect != nil {
					p.onDisconnect(target)
				}
			}
			return conn, nil
		}
		lastErr = err
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
	}
	return nil, lastErr
}

func (p *L4Proxy) resolveTarget(ctx context.Context, target string) (string, error) {
	if !p.resolveLocally {
		return target, nil
	}
	if p.dnsResolver == nil {
		return "", fmt.Errorf("missing DNS resolver")
	}
	host, port, err := net.SplitHostPort(target)
	if err != nil {
		return "", fmt.Errorf("invalid target %q: %w", target, err)
	}
	if net.ParseIP(host) != nil {
		return target, nil
	}
	ip, err := p.dnsResolver.Resolve(ctx, host)
	if err != nil {
		return "", fmt.Errorf("DNS resolution failed for %s: %w", host, err)
	}
	return net.JoinHostPort(ip.String(), port), nil
}

func (p *L4Proxy) dial(ctx context.Context, target string) (*l4TCPConn, error) {
	h3Client, err := p.getOrCreateClientConn(ctx)
	if err != nil {
		return nil, err
	}

	stream, err := h3Client.clientConn.OpenRequestStream(ctx)
	if err != nil {
		if !shouldReconnectOnOpenStreamError(ctx, err) {
			return nil, err
		}
		// The cached HTTP/3 connection might be stale; reconnect once and retry.
		p.closeClientConnIfCurrent(h3Client)
		h3Client, err = p.getOrCreateClientConn(ctx)
		if err != nil {
			return nil, err
		}
		stream, err = h3Client.clientConn.OpenRequestStream(ctx)
		if err != nil {
			if shouldReconnectOnOpenStreamError(ctx, err) {
				p.closeClientConnIfCurrent(h3Client)
			}
			return nil, err
		}
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodConnect, "https://"+target, nil)
	if err != nil {
		_ = stream.Close()
		return nil, err
	}
	req.Host = target
	if err := stream.SendRequestHeader(req); err != nil {
		_ = stream.Close()
		return nil, err
	}
	response, err := stream.ReadResponse()
	if err != nil {
		_ = stream.Close()
		return nil, err
	}
	if response.StatusCode < 200 || response.StatusCode > 299 {
		_ = stream.Close()
		return nil, fmt.Errorf("CONNECT rejected with status %d", response.StatusCode)
	}
	return &l4TCPConn{stream: stream, local: h3Client.udpConn.LocalAddr(), remote: l4Addr(target)}, nil
}

func shouldReconnectOnOpenStreamError(ctx context.Context, err error) bool {
	if err == nil {
		return false
	}
	if errors.Is(err, context.Canceled) || errors.Is(err, context.DeadlineExceeded) {
		return false
	}
	if ctx != nil && (errors.Is(ctx.Err(), context.Canceled) || errors.Is(ctx.Err(), context.DeadlineExceeded)) {
		return false
	}
	return true
}

func (p *L4Proxy) getOrCreateClientConn(ctx context.Context) (*l4HTTP3Client, error) {
	p.connMu.Lock()
	if p.client != nil {
		client := p.client
		p.connMu.Unlock()
		return client, nil
	}
	p.connMu.Unlock()

	udpConn, err := listenUDPForEndpoint(p.endpoint)
	if err != nil {
		return nil, err
	}
	// without ConnectionIDLength set, backend occasionally throws PROTOCOL_VIOLATION
	// and that closes our connection
	qtr := &quic.Transport{Conn: udpConn, ConnectionIDLength: 20}
	quicConn, err := qtr.Dial(ctx, p.endpoint, p.tlsConfig, p.quicConfig)
	if err != nil {
		_ = udpConn.Close()
		return nil, err
	}

	newClient := &l4HTTP3Client{
		udpConn:    udpConn,
		quicConn:   quicConn,
		clientConn: (&http3.Transport{}).NewClientConn(quicConn),
	}

	p.connMu.Lock()
	if p.client != nil {
		current := p.client
		p.connMu.Unlock()
		closeL4HTTP3(newClient.udpConn, newClient.quicConn)
		return current, nil
	}
	p.client = newClient
	p.connMu.Unlock()

	return newClient, nil
}

func (p *L4Proxy) closeClientConnIfCurrent(expected *l4HTTP3Client) {
	if expected == nil {
		return
	}

	p.connMu.Lock()
	if p.client != expected {
		p.connMu.Unlock()
		return
	}
	p.client = nil
	p.connMu.Unlock()

	closeL4HTTP3(expected.udpConn, expected.quicConn)
}

func listenUDPForEndpoint(endpoint *net.UDPAddr) (*net.UDPConn, error) {
	if endpoint.IP.To4() == nil {
		return net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv6zero})
	}
	return net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4zero})
}

func closeL4HTTP3(udpConn *net.UDPConn, quicConn *quic.Conn) {
	if quicConn != nil {
		_ = quicConn.CloseWithError(0, "")
	}
	if udpConn != nil {
		_ = udpConn.Close()
	}
}

type l4TCPConn struct {
	stream  l4Stream
	local   net.Addr
	remote  net.Addr
	once    sync.Once
	onClose func()
}

type l4Stream interface {
	io.ReadWriteCloser
	CancelRead(quic.StreamErrorCode)
	SetDeadline(time.Time) error
	SetReadDeadline(time.Time) error
	SetWriteDeadline(time.Time) error
}

func (c *l4TCPConn) Read(b []byte) (int, error) {
	if c.stream == nil {
		return 0, net.ErrClosed
	}
	return c.stream.Read(b)
}

func (c *l4TCPConn) Write(b []byte) (int, error) {
	if c.stream == nil {
		return 0, net.ErrClosed
	}
	return c.stream.Write(b)
}

func (c *l4TCPConn) CloseWrite() error {
	if c.stream == nil {
		return nil
	}
	return c.stream.Close()
}
func (c *l4TCPConn) CloseRead() error {
	if c.stream == nil {
		return nil
	}
	c.stream.CancelRead(quic.StreamErrorCode(http3.ErrCodeNoError))
	return nil
}
func (c *l4TCPConn) Close() error {
	var err error
	c.once.Do(func() {
		err = c.CloseWrite()
		_ = c.CloseRead()
		if c.onClose != nil {
			c.onClose()
		}
	})
	return err
}
func (c *l4TCPConn) LocalAddr() net.Addr {
	if c.local == nil {
		return l4Addr("l4")
	}
	return c.local
}
func (c *l4TCPConn) RemoteAddr() net.Addr { return c.remote }
func (c *l4TCPConn) SetDeadline(t time.Time) error {
	if c.stream == nil {
		return nil
	}
	return c.stream.SetDeadline(t)
}
func (c *l4TCPConn) SetReadDeadline(t time.Time) error {
	if c.stream == nil {
		return nil
	}
	return c.stream.SetReadDeadline(t)
}
func (c *l4TCPConn) SetWriteDeadline(t time.Time) error {
	if c.stream == nil {
		return nil
	}
	return c.stream.SetWriteDeadline(t)
}

type l4Addr string

func (a l4Addr) Network() string { return "masque-l4-tcp" }
func (a l4Addr) String() string  { return string(a) }

type closeWriter interface {
	CloseWrite() error
}

// RelayTCP copies bytes in both directions and preserves TCP half-close.
func RelayTCP(a, b net.Conn) {
	var wg sync.WaitGroup
	wg.Add(2)
	relay := func(dst, src net.Conn) {
		defer wg.Done()
		_, _ = io.Copy(dst, src)
		if cw, ok := dst.(closeWriter); ok {
			_ = cw.CloseWrite()
		}
	}
	go relay(a, b)
	go relay(b, a)
	wg.Wait()
	_ = a.Close()
	_ = b.Close()
}
