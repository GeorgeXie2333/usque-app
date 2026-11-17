package main

import (
	"bufio"
	"bytes"
	"encoding/hex"
	"io"
	"testing"
)

// Mirrors h2.rs::encode_datagram_capsule_into: type zero, varint length,
// then the IPv4 packet, beginning immediately with its version/IHL.
const h2CanaryFrame = "0020450000200001400040114e67c0000207c633642ac3509869000c000074657374"

func TestH2CanaryWire(t *testing.T) {
	wire, err := hex.DecodeString(h2CanaryFrame)
	if err != nil {
		t.Fatal(err)
	}
	reader := bufio.NewReader(bytes.NewReader(wire))
	kind, err := readVarint(reader)
	if err != nil || kind != 0 {
		t.Fatalf("capsule type %v: %v", kind, err)
	}
	length, err := readVarint(reader)
	if err != nil || length != 32 {
		t.Fatalf("capsule length %v: %v", length, err)
	}
	packet := make([]byte, length)
	if _, err := io.ReadFull(reader, packet); err != nil {
		t.Fatal(err)
	}
	for end := 0; end < len(packet); end++ {
		if echoCanary(bytes.Clone(packet[:end]), "test") != nil {
			t.Fatalf("accepted truncated packet of length %d", end)
		}
	}
	overlong := bytes.Clone(packet)
	overlong[0] = 0x4f
	if echoCanary(overlong, "test") != nil {
		t.Fatal("accepted truncated IPv4 options")
	}
	if echoCanary(append([]byte{0}, packet...), "test") != nil {
		t.Fatal("accepted H3 context id in an H2 capsule")
	}
	reply := echoCanary(packet, "test")
	want, err := hex.DecodeString("450000200001400040114e67c633642ac00002079869c350000c000074657374")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(reply, want) {
		t.Fatalf("echo %x, want %x", reply, want)
	}
}
