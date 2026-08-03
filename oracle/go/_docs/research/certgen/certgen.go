package main

import (
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"encoding/base64"
	"encoding/pem"
	"log"
	"math/big"
	"os"
	"time"
)

func main() {
	privKey, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		log.Fatalf("Failed to generate secp256r1 key: %v", err)
	}

	privkeyOut, err := os.Create("privkey.pem")
	if err != nil {
		log.Fatalf("Failed to open privkey.pem for writing: %v", err)
	}

	marshalledPrivKey, err := x509.MarshalECPrivateKey(privKey)
	if err != nil {
		log.Fatalf("Failed to marshal private key: %v", err)
	}

	privkeyBlock := &pem.Block{Type: "EC PRIVATE KEY", Bytes: marshalledPrivKey}
	if err := pem.Encode(privkeyOut, privkeyBlock); err != nil {
		log.Fatalf("Failed to write data to privkey.pem: %v", err)
	}
	if err := privkeyOut.Close(); err != nil {
		log.Fatalf("Error closing privkey.pem: %v", err)
	}

	pubKeyBytes, err := x509.MarshalPKIXPublicKey(&privKey.PublicKey)
	if err != nil {
		log.Fatalf("Failed to marshal public key: %v", err)
	}
	pubKeyBase64 := base64.StdEncoding.EncodeToString(pubKeyBytes)

	pubKeyOut, err := os.Create("pubKeyBase64.txt")
	if err != nil {
		log.Fatalf("Failed to open pubKeyBase64.txt for writing: %v", err)
	}
	if _, err := pubKeyOut.WriteString(pubKeyBase64); err != nil {
		log.Fatalf("Failed to write data to pubKeyBase64.txt: %v", err)
	}

	if err := pubKeyOut.Close(); err != nil {
		log.Fatalf("Error closing pubKeyBase64.txt: %v", err)
	}

	certOut, err := os.Create("cert.pem")
	if err != nil {
		log.Fatalf("Failed to open cert.pem for writing: %v", err)
	}

	certDER, err := x509.CreateCertificate(rand.Reader, &x509.Certificate{
		SerialNumber: big.NewInt(0),
		NotBefore:    time.Now(),
		NotAfter:     time.Now().Add(365 * 24 * time.Hour),
	}, &x509.Certificate{}, &privKey.PublicKey, privKey)
	if err != nil {
		log.Fatalf("Failed to create certificate: %v", err)
	}

	if err := pem.Encode(certOut, &pem.Block{Type: "CERTIFICATE", Bytes: certDER}); err != nil {
		log.Fatalf("Failed to write data to cert.pem: %v", err)
	}
	if err := certOut.Close(); err != nil {
		log.Fatalf("Error closing cert.pem: %v", err)
	}
}
