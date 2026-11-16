// A pinned, ephemeral H2 CONNECT-IP peer for the macOS acceptance fixture.
// It echoes one run-specific UDP canary. It never forwards any traffic.
package main

import (
	"bufio"
	"bytes"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/tls"
	"crypto/x509"
	"encoding/binary"
	"fmt"
	"io"
	"log"
	"math/big"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"time"
)

func readVarint(r *bufio.Reader) (uint64, error) {
	first, err := r.ReadByte()
	if err != nil {
		return 0, err
	}
	value := uint64(first & 63)
	for i := 1; i < 1<<(first>>6); i++ {
		b, err := r.ReadByte()
		if err != nil {
			return 0, err
		}
		value = value<<8 | uint64(b)
	}
	return value, nil
}

func echoCanary(ip []byte, nonce string) []byte {
	// H2 type-zero capsules carry the raw IP packet starting at byte zero.
	if len(ip) < 28 {
		return nil
	}
	header := int(ip[0]&15) * 4
	if ip[0]>>4 != 4 || header < 20 || len(ip) < header+8 ||
		int(binary.BigEndian.Uint16(ip[2:4])) != len(ip) || ip[9] != 17 ||
		binary.BigEndian.Uint16(ip[6:8])&0x3fff != 0 ||
		!bytes.Equal(ip[16:20], []byte{198, 51, 100, 42}) {
		return nil
	}
	udp := ip[header:]
	if binary.BigEndian.Uint16(udp[2:4]) != 39017 ||
		int(binary.BigEndian.Uint16(udp[4:6])) != len(udp) || string(udp[8:]) != nonce {
		return nil
	}
	var source [4]byte
	copy(source[:], ip[12:16])
	copy(ip[12:16], ip[16:20])
	copy(ip[16:20], source[:])
	src, dst := binary.BigEndian.Uint16(udp[:2]), binary.BigEndian.Uint16(udp[2:4])
	binary.BigEndian.PutUint16(udp[:2], dst)
	binary.BigEndian.PutUint16(udp[2:4], src)
	// Swapping address and port pairs preserves their one's-complement
	// checksums; all other header and payload bytes are unchanged.
	return ip
}

func serve(directory, host, nonce string) error {
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		return err
	}
	cert := x509.Certificate{SerialNumber: big.NewInt(1), NotBefore: time.Now().Add(-time.Hour),
		NotAfter: time.Now().Add(time.Hour), KeyUsage: x509.KeyUsageDigitalSignature}
	der, err := x509.CreateCertificate(rand.Reader, &cert, &cert, &key.PublicKey, key)
	if err != nil {
		return err
	}
	pin, err := x509.MarshalPKIXPublicKey(&key.PublicKey)
	if err != nil {
		return err
	}
	if err = os.WriteFile(filepath.Join(directory, "pin.der"), pin, 0600); err != nil {
		return err
	}
	listener, err := net.ListenTCP("tcp4", &net.TCPAddr{IP: net.ParseIP(host)})
	if err != nil {
		return err
	}
	defer listener.Close()
	server := http.Server{TLSConfig: &tls.Config{
		Certificates: []tls.Certificate{{Certificate: [][]byte{der}, PrivateKey: key}},
		MinVersion:   tls.VersionTLS13, ClientAuth: tls.RequireAnyClientCert,
	}, ReadHeaderTimeout: 5 * time.Second}
	server.Handler = http.HandlerFunc(func(w http.ResponseWriter, req *http.Request) {
		if req.ProtoMajor != 2 || req.Method != "CONNECT" ||
			req.Header.Get("cf-connect-proto") != "cf-connect-ip" {
			http.Error(w, "expected CONNECT-IP", http.StatusBadRequest)
			return
		}
		w.WriteHeader(http.StatusOK)
		flush := http.NewResponseController(w)
		if flush.Flush() != nil {
			return
		}
		log.Print("accepted pinned HTTP/2 CONNECT-IP connection")
		r := bufio.NewReader(req.Body)
		for {
			kind, err := readVarint(r)
			if err != nil {
				return
			}
			size, err := readVarint(r)
			if err != nil || size > 65536 {
				return
			}
			data := make([]byte, size)
			if _, err := io.ReadFull(r, data); err != nil {
				return
			}
			if kind != 0 {
				continue
			}
			reply := echoCanary(data, nonce)
			if reply == nil {
				continue
			}
			// The canary is at most one small UDP datagram. Encode a two-byte
			// QUIC length; longer-than-minimal integer encodings are valid.
			if len(reply) >= 16384 {
				return
			}
			frame := []byte{0, 0x40 | byte(len(reply)>>8), byte(len(reply))}
			if _, err = w.Write(append(frame, reply...)); err != nil || flush.Flush() != nil {
				return
			}
			log.Print("received and returned run-specific UDP canary through CONNECT-IP")
		}
	})
	port := strconv.Itoa(listener.Addr().(*net.TCPAddr).Port)
	if err := os.WriteFile(filepath.Join(directory, "peer-port"), []byte(port), 0600); err != nil {
		return err
	}
	return server.ServeTLS(listener, "", "")
}

func main() {
	if len(os.Args) != 4 {
		log.Fatal("expected fixture-directory, bind-IP and nonce")
	}
	if err := serve(os.Args[1], os.Args[2], os.Args[3]); err != nil {
		log.Fatal(fmt.Errorf("fixture peer: %w", err))
	}
}
