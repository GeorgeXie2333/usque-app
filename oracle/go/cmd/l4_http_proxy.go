package cmd

import (
	"context"
	"fmt"
	"io"
	"log"
	"net"
	"net/http"
	"strings"

	"github.com/Diniboy1123/usque/api"
	"github.com/Diniboy1123/usque/internal"
	"github.com/spf13/cobra"
)

var l4HTTPProxyCmd = &cobra.Command{
	Use:   "l4-http-proxy",
	Short: "Expose Warp as an L4 TCP-only HTTP proxy with CONNECT support",
	Long:  "TCP-only HTTP proxy using direct HTTP/3 CONNECT streams. Doesn't require elevated privileges.",
	Run: func(cmd *cobra.Command, args []string) {
		opts, proxy, err := buildL4Proxy(cmd, "l4-http-proxy")
		if err != nil {
			cmd.Println(err)
			return
		}

		var authHeader string
		if opts.username != "" && opts.password != "" {
			authHeader = "Basic " + internal.LoginToBase64(opts.username, opts.password)
		}

		server := &http.Server{
			Addr: net.JoinHostPort(opts.bind, opts.port),
			Handler: http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
				if authHeader != "" && r.Header.Get("Proxy-Authorization") != authHeader {
					w.Header().Set("Proxy-Authenticate", `Basic realm="Proxy"`)
					http.Error(w, "Proxy authentication required", http.StatusProxyAuthRequired)
					return
				}
				if r.Method == http.MethodConnect {
					handleL4HTTPConnect(w, r, proxy)
					return
				}
				handleL4HTTPForward(w, r, proxy)
			}),
		}

		log.Printf("L4 HTTP proxy listening on %s", server.Addr)
		if err := server.ListenAndServe(); err != nil {
			cmd.Printf("Failed to start HTTP proxy: %v\n", err)
		}
	},
}

func handleL4HTTPConnect(w http.ResponseWriter, r *http.Request, proxy *api.L4Proxy) {
	target, err := authorityWithPort(r.Host, "443")
	if err != nil {
		http.Error(w, "Invalid host", http.StatusBadRequest)
		return
	}

	destConn, err := proxy.DialContext(r.Context(), target)
	if err != nil {
		log.Printf("l4 http proxy: connect %s failed: %v", target, err)
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
		_ = destConn.Close()
		return
	}
	if _, err := clientConn.Write([]byte("HTTP/1.1 200 Connection Established\r\n\r\n")); err != nil {
		_ = clientConn.Close()
		_ = destConn.Close()
		return
	}

	api.RelayTCP(clientConn, destConn)
}

func handleL4HTTPForward(w http.ResponseWriter, r *http.Request, proxy *api.L4Proxy) {
	transport := &http.Transport{
		Proxy: nil,
		DialContext: func(ctx context.Context, network, addr string) (net.Conn, error) {
			if network != "tcp" && network != "tcp4" && network != "tcp6" {
				return nil, fmt.Errorf("unsupported network %s", network)
			}
			return proxy.DialContext(ctx, addr)
		},
	}
	defer transport.CloseIdleConnections()

	req := r.Clone(r.Context())
	req.RequestURI = ""
	req.URL.Scheme = normalizedScheme(req.URL.Scheme)
	if req.URL.Host == "" {
		req.URL.Host = req.Host
	}
	req.Header = r.Header.Clone()
	req.Header.Del("Proxy-Authorization")
	req.Header.Del("Proxy-Connection")

	resp, err := transport.RoundTrip(req)
	if err != nil {
		log.Printf("l4 http proxy: request to %s failed: %v", req.URL, err)
		http.Error(w, "Failed to reach destination", http.StatusServiceUnavailable)
		return
	}
	defer func() { _ = resp.Body.Close() }()

	copyHeader(w.Header(), resp.Header)
	w.WriteHeader(resp.StatusCode)
	_, _ = io.Copy(w, resp.Body)
}

func normalizedScheme(s string) string {
	if s == "" {
		return "http"
	}
	return s
}

func authorityWithPort(authority, defaultPort string) (string, error) {
	if authority == "" {
		return "", fmt.Errorf("empty authority")
	}
	if strings.HasPrefix(authority, "[") || strings.Count(authority, ":") == 1 {
		host, port, err := net.SplitHostPort(authority)
		if err == nil {
			return net.JoinHostPort(host, port), nil
		}
	}
	host := authority
	if strings.HasPrefix(authority, "[") && strings.HasSuffix(authority, "]") {
		host = strings.TrimPrefix(strings.TrimSuffix(authority, "]"), "[")
	}
	return net.JoinHostPort(host, defaultPort), nil
}

func init() {
	addL4ProxyFlags(l4HTTPProxyCmd, "8000", "HTTP")
	rootCmd.AddCommand(l4HTTPProxyCmd)
}
