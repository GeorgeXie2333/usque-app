package cmd

import (
	"context"
	"log"
	"net"

	"github.com/Diniboy1123/usque/internal"
	"github.com/spf13/cobra"
)

var l4SocksCmd = &cobra.Command{
	Use:   "l4-socks",
	Short: "Expose Warp as an L4 TCP-only SOCKS5 proxy",
	Long:  "TCP-only SOCKS5 proxy using direct HTTP/3 CONNECT streams. Doesn't require elevated privileges.",
	Run: func(cmd *cobra.Command, args []string) {
		opts, proxy, err := buildL4Proxy(cmd, "l4-socks")
		if err != nil {
			cmd.Println(err)
			return
		}

		addr := net.JoinHostPort(opts.bind, opts.port)
		server, err := internal.NewSOCKS5Server(internal.SOCKS5Config{
			Addr:     addr,
			Username: opts.username,
			Password: opts.password,
			DialTCP: func(ctx context.Context, network, address string) (net.Conn, error) {
				return proxy.DialContext(ctx, address)
			},
			TCPOnly: true,
			Logger:  log.Default(),
		})
		if err != nil {
			cmd.Printf("Failed to create SOCKS proxy: %v\n", err)
			return
		}

		log.Printf("L4 SOCKS proxy listening on %s", addr)
		if err := server.Start(); err != nil {
			cmd.Printf("Failed to start SOCKS proxy: %v\n", err)
		}
	},
}

func init() {
	addL4ProxyFlags(l4SocksCmd, "1080", "SOCKS")
	rootCmd.AddCommand(l4SocksCmd)
}
