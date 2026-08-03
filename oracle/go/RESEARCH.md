# Initial research on Cloudflare's MASQUE implementation

It took me months to get this all figured out *(though there is a lot of room for me to improve)*, so I figured I will write down my findings and hopefully give pointers to interested future contributors. This is going to sound like a blog post, but I hope it will be useful for someone. In the future I might will make a more concise version of this with just the protocol details, but for now this contains the whole journey's important bits.

## Table of contents

- [Initial research on Cloudflare's MASQUE implementation](#initial-research-on-cloudflares-masque-implementation)
  - [Table of contents](#table-of-contents)
  - [Initial poking](#initial-poking)
  - [API](#api)
  - [Poking on QUIC](#poking-on-quic)
  - [TLS Authentication](#tls-authentication)
  - [QUIC](#quic)
  - [Connect-IP](#connect-ip)
  - [libwarp\_mobile.so](#libwarp_mobileso)
  - [TL;DR](#tldr)
  - [WireShark inspection of the decrypted traffic](#wireshark-inspection-of-the-decrypted-traffic)
  - [Final thoughts](#final-thoughts)
  - [Extras](#extras)
  - [Interesting other CONNECT-IP related projects](#interesting-other-connect-ip-related-projects)
  - [Interesting other QUIC related projects](#interesting-other-quic-related-projects)

## Initial poking

First I took a look at the Android app. Frida support is great there and with the help of [this](https://github.com/httptoolkit/frida-interception-and-unpinning/tree/4d477da) and mitmproxy, it was trivial to see how the app registers a new user and enrolls the new device, switches keys, etc.

## API

I am not going to go into details about the API, as it is pretty well studied already by others. I did not, but you should check out [wgcf](https://github.com/ViRb3/wgcf) as they went into great lengths to reverse engineer the API.

One call stood out though:

```shell
curl -H 'Authorization: Bearer 00000000-0000-0000-0000-000000000000' -H 'User-Agent: WARP for Android' -H 'CF-Client-Version: a-6.81-4616' -H 'Content-Type: application/json; charset=UTF-8' -H 'Connection: Keep-Alive' --compressed -X PATCH https://api.cloudflareclient.com/v0a4616/reg/00000000-0000-0000-0000-000000000000 -d '{"key":"M...redacted...==","key_type":"secp256r1","tunnel_type":"masque"}'
```

The app sends this once you switch over to the MASQUE protocol. I wanted to know more about the seemingly Base64 encoded key, so I copied it over to my favorite [ASN.1 decoder](https://pkitools.net/pages/ca/asn1.html).

Looked like an `ecPublicKey` with `prime256v1`, so the `secp256r1` key type made sense. Based on earlier findings from other people regarding WireGuard, I assumed that this is the way how the client tells the server about our public key. And I assumed that there is TLS authentication later instead of other credentials.

## Poking on QUIC

Then I installed the Linux client for ease of network debugging and opened WireShark. It's important that you have an up to date WireShark as QUIC support isn't present in some distribution shipped versions.

The following filter worked for me:

```
ip.dst == 162.159.198.1 || ip.src == 162.159.198.1
```

`162.159.198.1` seems to be the anycast IP all Warp clients use for the actual tunnel.

The initial QUIC packet was a great starting point:

![QUIC Initial ClientHello](_docs/research/quic_initial_clienthello.png)

It contains a `ClientHello` with many interesting fields. The `SNI` field is set to `consumer-masque.cloudflareclient.com` that made me more certain that I am on the right track.

I didn't understand much and went down on the rabbit hole to jump into MASQUE and Cloudflare Blogs when in reality I should have focused on getting QUIC to connect. You know, first things first. But I will spare the details of these failures.

## TLS Authentication

Before anything, I tried to generate the EC keypair we saw earlier and generate a self-signed certificate too as my instinct told me that this is what the server could use for user authentication. I have some dirty code [here](_docs/research/certgen/certgen.go). It will generate a keypair and a self-signed certificate. All will be saved to the current working directory. The public key will be saved even as base64 ASN.1 DER, as this is what we saw in the API call.

Then I sent the public key to the server in the same format as the app did. I got a on OK HTTP status code, so I assumed that the server accepted my key.

## QUIC

I noticed that Go has a great QUIC library, so I decided on using Go for this project and took a look at establishing a simple QUIC connection to the server.

Came up with the following:

```go
cert, err := tls.LoadX509KeyPair(certFile, keyFile)
if err != nil {
	log.Fatalf("failed to load certificate: %v", err)
}
tlsConf := &tls.Config{
	Certificates: []tls.Certificate{cert},
	ServerName:   "consumer-masque.cloudflareclient.com",
	// apparently useless, because golang cannot set the cipher suite for tls 1.3
	//CipherSuites: []uint16{tls.TLS_AES_256_GCM_SHA384},
	NextProtos:         []string{http3.NextProtoH3},
	KeyLogWriter:       keyLogger,
	InsecureSkipVerify: true,
}

udpConn, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4zero, Port: 0})
if err != nil {
    log.Fatalf("failed to listen udp: %v", err)
}

conn, err := quic.Dial(
    ctx,
    udpConn,
    &net.UDPAddr{IP: net.ParseIP("162.159.198.1"), Port: 443},
    tlsConf,
    &quic.Config{
        EnableDatagrams:   true,
        InitialPacketSize: 1242,
    },
)
if err != nil {
    log.Fatalf("failed to dial: %v", err)
}
```

In case you are wondering, `InitialPacketSize`: 1242 came from WireShark. That was the size of the Initial packet. `KeyLogWriter` is not necessary, but with that and importing secrets you can decrypt the QUIC traffic in Wireshark. VERY useful for debugging. And the UDP address is obviously the anycast IP we saw earlier.

Then I assumed [masque-go](https://github.com/quic-go/masque-go) is what I need as even Cloudflare themselves mention `connect-udp` [in a blogpost](https://blog.cloudflare.com/unlocking-quic-proxying-potential/). It's kinda confusing, because they don't use that and the remote endpoint doesn't seem to support it either. Rabbit hole, don't go there.

But at least the connection was established and I could see some minimal traffic.

But here I got stuck since the rest of the traffic was clearly encrypted. I figured I have 3 options:
- Static analysis
- Dynamic analysis
- Log inspection from official clients

I went with static analysis first as I prefer that the most usually. However I noticed that the Android app calls natives for the tunnel establishment, which makes sense. You wouldn't want to implement this in Java, if you want to be fast and efficient. Great job there Cloudflare!

However I first thought that `libnativetunnel.so` is the library I am looking for. That one alone was ~280 MiB and yes, it's still packed to this day into the Android app. I tried to load it to IDA Free and saw my computer crash. *sigh*

Dynamic analysis was also ruled out this way, as I couldn't figure out what to hook in the first place.

Out of desperation, I played a bit with the linux client:

```shell
warp-cli debug qlog enable && warp-cli connect
```

It has an option to start qlogs, which are seemingly cute JSON like logs for QUIC debugging. Frankly I don't know much about them to this day, because quic-go does a so great job, I never had to bother with QUIC debugging yet. But anyway, qlogs are dropped to this directory: `/var/lib/cloudflare-warp/qlogs/`.

While looking at those confusing lines, one stood out:

```json
{"time":32.20791,"name":"http:frame_created","data":{"stream_id":0,"length":40,"frame":{"frame_type":"headers","headers":[{"name":":method","value":"CONNECT"},{"name":":protocol","value":"cf-connect-ip"},{"name":":scheme","value":"https"},{"name":":authority","value":"cloudflareaccess.com"},{"name":":path","value":"/"}]}}}
```

Since I spent many days on getting masque-go to work, which uses `connect-udp` as its protocol field when connecting, I had the intuition that `cf-connect-ip` kind of sounds similar to `connect-udp`. So I did some Googling. Bumped into [this issue](https://github.com/quic-go/masque-go/issues/63).

## Connect-IP

[connect-go](https://github.com/quic-go/connect-ip-go) looked exactly like what I would need.

I put together a quick example that used its [Dial](https://github.com/quic-go/connect-ip-go/blob/321f13c3d203c7afce80ee6cc33762a44b13b1aa/client.go#L15) function:

```go
proxyURITemplate := "https://cloudflareaccess.com/"
udpConn, err := net.ListenUDP("udp", &net.UDPAddr{IP: net.IPv4zero, Port: 0})
if err != nil {
	log.Fatalf("failed to listen udp: %v", err)
}

ccon, err := quic.Dial(ctx, udpConn, &net.UDPAddr{IP: net.ParseIP("162.159.198.1"), Port: 443}, tlsConf, &quic.Config{EnableDatagrams: true})
if err != nil {
	log.Fatalf("failed to dial: %v", err)
}

tr := &http3.Transport{EnableDatagrams: true}
defer tr.Close()
client, rsp, err := connectip.Dial(ctx, tr.NewClientConn(ccon), uritemplate.MustNew(proxyURITemplate))
if err != nil {
	log.Fatalf("failed to dial: %v", err)
}
defer client.Close()

log.Printf("HTTP status: %v", rsp)

req, err := connectip.ParseRequest(newRequest(proxyURITemplate), uritemplate.MustNew(proxyURITemplate))
if err != nil {
	log.Fatalf("failed to parse request: %v", err)
}

log.Printf("Request: %v", req)
```

I know, ugly and mostly copy-pasted as I had no clue how this all works at this point. But it seemed like something some tests/integrations were doing in the project, so I went with it.

First it didn't like the fact that ExtendedConnect isn't enabled on the server side. Code that returns an error can be [found here](https://github.com/quic-go/connect-ip-go/blob/321f13c3d203c7afce80ee6cc33762a44b13b1aa/client.go#L34). I just monkey patched that return out...

Then the server got a `400` back to my request, but at least I had quic and I had an endpoint I could hit. So then I could compare the request I saw from the Linux qlog to the one I sent. It seemed like the requestProtocol was wrong. [Here is where its sent](https://github.com/quic-go/connect-ip-go/blob/321f13c3d203c7afce80ee6cc33762a44b13b1aa/client.go#L46). And [here is where its set](https://github.com/quic-go/connect-ip-go/blob/321f13c3d203c7afce80ee6cc33762a44b13b1aa/request.go#L15) to a hardcoded `connect-ip` value. We need `cf-connect-ip` instead.

So I monkey patched that too. And to my surprise I got a `200` and even some team ID which definitely looks Cloudflare like:

```
&{200 OK 200 HTTP/3.0 3 0 map[Cf-Team:[2...redacted...1]] 0xc000114450 0 [] false false map[] <nil> <nil>}
```

*Fun fact: the same, undocumented (?) header can be seen if you have Proxy turned on for ZeroTrust and you request a page over HTTP. The same header will be injected into the response.*

Then I saw that `connect-ip-go` integrations request [routes](https://github.com/quic-go/connect-ip-go/blob/321f13c3d203c7afce80ee6cc33762a44b13b1aa/conn.go#L165). So I tried that, but that call just hung forever. I never received any routes.

So yeah, I could connect, I could request that stuff, but it didn't even have any request body. I was stuck again. Rest of the traffic was still encrypted and not even qlogs had more juicy details.

Though I figured that if the client certificate I use isn't signed by the private key whose public key I previously sent to the server, this last HTTP call would fail with a tls error. So that's a good sign, we most likely figured out the authentication part.

As a last resort, I read the [RFC 9484](https://datatracker.ietf.org/doc/html/rfc9484) this whole thing is supposedly about that connect-ip-go also implements.

Saw this section:

> Examples are shown below:
> 
> https://example.org/.well-known/masque/ip/{target}/{ipproto}/
> https://proxy.example.org:4443/masque/ip?t={target}&i={ipproto}
> https://proxy.example.org:4443/masque/ip{?target,ipproto}
> https://masque.example.org/?user=bob

Which put me down into another rabbit hole as I tried all of these combinations, maybe I just got the URL wrong and there is a second call to some mysterious endpoint where if I send the right path with my desired target and IP protocol, I can get raw IP traffic back and forth. I know it sounds silly looking back, but I was desperate.

Anyway as none of these worked, a thought struck in my head. If I could decrypt the traffic from the official clients, I would know the missing endpoint path.

I went to great lengths here as I took a quick look at the linux client in IDA, but working with rust structs is painful and normally if you define `SSLKEYLOGFILE` as an environment variable, the client will drop a file for you similar to how my Go code from earlier did and you can use that to decrypt the traffic in Wireshark. I knew how well this works from just the last few days of debugging my own client attempts. But the official client was built with the code path for `SSLKEYLOGFILE` generation disabled. In fact it logs a warning that it's not built into this release. The code is not there at all.

Since the binary had thousands of methods, I wasn't sure which one I could hook to manually extract all secrets I need.

Then I bumped into [friTap](https://github.com/fkie-cad/friTap) seemingly advertising exactly what I need. It seemed to be great at dumping keylogfiles from running processes for example from Android. It didn't quite work for Warp so I [opened an issue](https://github.com/fkie-cad/friTap/issues/38) *(spoilers ahead there)*. I was really desperate at this point.

Thankfully did @monkeywave come to the rescue, one of the main authors of this amazing project. I shared my findings as you can see in the issue, but completely went with the idea that `libnativetunnel.so` is the target of interest.

Unfortuantely I brang them with me on this, but let's move on. After a quick discussion, [they responsed with an exceptionally helpful research](https://github.com/fkie-cad/friTap/issues/38#issuecomment-2577761089). In just a few hours I am assuming, they figured more than I could *(but I guess their IDA didn't crash like mine :D)*...

So I learnt that the library uses [quiche](https://github.com/cloudflare/quiche) for tunnel establishment. We can almost certainly treat that as a fact as this is a Cloudflare product and why would they implement the same thing twice...

Then they also figure that quiche uses BoringSSL. I even got a [warp_hook.js](https://github.com/monkeywave/frida-scripts-collection/blob/main/warp_hook.js) frida script with their findings so far. But eventually the end conclusion was: None of the functions get called that we hook OR the native is loaded in some way that frida cannot hook into properly.

While reading all this, I was heated by dopamine that someone so knowledgeable took their time to help me out. I also got many interesting new pointers I could confirm, so I quickly jumped into trying their script out. I had the same experience. None of the hooks worked.

At this point I gave up. It's that moment, when you feel like you are so close, but hit a wall... That can absolutely destroy everything. For me, that was the moment.

I sat on it for more than a month and accepted that I am too dumb for this.

Then just 5 days ago (of the first version of this document) I decided to buy more RAM and go back to that static analysis.

I went back to the APK this time to look at the Java code and put the logic together in my head. Seemed like that is the right approach, because after spending many hours on the native library, hooking into many different things, I figured that only three functions are called from `libnativetunnel.so`. And those are actually JNI functions that are exposed to Java code. Even though that file has symbols and code for establishing tunnel connections hell it even has DWARF debugging information (it's almost like Cloudflare wants you to reverse engineer it :D), it's not used at all for the tunnelling process.

Means I was just looking at the wrong place all along and I brought someone with me on this journey. I am sorry for that monkeywave.

## libwarp_mobile.so

Then I went down more on the Java path. I found a library called `libwarp_mobile.so` that is loaded by the app. It doesn't have too much JNI exports interfaced, but it's also significantly smaller than `libnativetunnel.so`. Unfortunately for us it doesn't come with DWARF, but still has some symbols.

You can find `ssl_log_secret()` in there really easily. If you check the [source for BoringSSL](https://github.com/google/boringssl/blob/e056f59c7dfdcf891af03bc7900c946ac485c78f/ssl/ssl_lib.cc#L173), you can see what it does. By looking at xrefs in IDA, I could quickly trace back that it's indeed called in a lot of different places leading to the logging of client secrets. Which means if I can hook into this function, I probably hit the jackpot.

Then I modified the earlier warp_hook.js with the newly learnt information, you can [find it here](https://gist.github.com/Diniboy1123/595202e17214b4ff3ade5aaa8da6b6b2). It even dumped some secrets that looked perfect for me.

With that I could have manually assembled a keylogfile for Wireshark to consume. I found a decent draft that explains how it's done [here](https://www.ietf.org/archive/id/draft-thomson-tls-keylogfile-00.html#name-the-sslkeylogfile-format). I had everything, except the `client_random`.

[Seemingly](https://github.com/google/boringssl/blob/e056f59c7dfdcf891af03bc7900c946ac485c78f/ssl/ssl_lib.cc#L186) `ssl_log_secret()` has access to that as well, but it's in the `SSL` OpenSSL struct, which contains more structs and many fields. I spent a great while calculating offsets here until I got to the `client_random` field, but it was always wrong for some reason. BTW if you know a better way to do this, than calculating offsets manually, please let me know. I even dumped a larger memory region and tried to spot the `client_random` in there, but I couldn't find it.

Meanwhile it turned out that friTap has the ability to dump BoringSSL secrets normally, but it didn't work for Warp. Meanwhile that was slowly fixed as I raised issues. However friTap also struggled at the same point as me with the `client_random`. The dumped client random was always wrong. In fact it still is. I plan to look into this more in the future once I have more time.

But I had the secrets and if we look back at the ClientHello picture from the beginning of the research, WireShark seems to display the client random we probably need:

![QUIC Initial ClientHello](_docs/research/quic_initial_clienthello.png)

Was just an idea, but I assembled the keylogfile with the secrets I had and the client random from the ClientHello.

That finally worked.

## TL;DR

To intercept app traffic:

- Run friTap on the 1.1.1.1 app
- Run the app
- Launch the tunnel
- Edit the created keylogfile with the client random from the QUIC ClientHello from WireShark
- Load the keylogfile in Wireshark
- Decrypt QUIC traffic

## WireShark inspection of the decrypted traffic

I was excited to see what could be the trick that I am missing. Turns out there is no mysterious endpoint and I somehow managed to figure out almost everything. The only thing missing was the fact that Cloudflare doesn't seem to follow the RFC and doesn't seem to advertise any routes. You can just start sending packets. And. It. Will. Just. Work.

From this moment on I figured I had the whole picture. Everything went smooth with little drawbacks only.

I took inspiration from [this excellent integration](https://github.com/quic-go/connect-ip-go/blob/321f13c3d203c7afce80ee6cc33762a44b13b1aa/integration/client/client.go#L118) and I had a working client that brang up a tun adapter and it just worked. It is basically the same code as [cmd/nativetun_linux.go](cmd/nativetun_linux.go) today.

Essentially the rest of the traffic is standard unreliable datagrams, exactly how they are defined by [RFC 9221](https://datatracker.ietf.org/doc/html/rfc9221). For simplicity's sake, you take your raw IP packet and send it as datagrams. Thankfully `connect-ip-go` has excellent support for this.

## Final thoughts

After all this, I get why Cloudflare went with the non-standard `cf-connect-ip` protocol identifier. It's probably because they clearly don't follow the RFC at a few points. But overall I didn't find anything in this custom implementation that couldn't be done with following the RFC. Technically if they stick with `connect-ip`, advertise `ExtendedConnect` and send out a wildcard route, they would be RFC compliant. I am not sure if we will ever get to know their reasons for this, but I would love to hear some ideas.

Anyway this was a great learning experience for me both technically as I learnt a lot about QUIC, TLS, UDP, congestion algorithms, Go, Tun etc. and also personally as I learnt a lot about myself, most importantly to not to give up and to ask for help when needed. If you find the right person to ask, it's not a shame.

## Extras

- [cryptobufferhook.js](_docs/research/cryptobufferhook.js) - A frida script that hooks into the `CRYPTO_BUFFER_new` functions in BoringSSL to dump the buffers. I used this to dump the client certificate used by the client for connection to the remote endpoint so I could compare it with my code and figure that they generate a new certificate for every connection and each certificate is valid for 24 hours. I am not sure why are they doing that though... If one could get their hands on the cert, that wouldn't be enough to authenticate, because they need the private key as well. But once you have the private key, you can generate new certs for yourself. So yeah, I am not sure what's the point of this.

## Interesting other CONNECT-IP related projects

- [connect-ip-go](https://github.com/quic-go/connect-ip-go) - A Go implementation of RFC 9484 that uses [quic-go](https://github.com/quic-go/quic-go) for the QUIC part. I found it very easy to read and understand, but it lacks IP Forwarding support as of now.
- [Google quiche's masque implementation](https://github.com/google/quiche/tree/main/quiche/quic/masque) - Very cool prototype MASQUE implementation with support for `CONNECT-UDP`, `CONNECT-IP` and even `CONNECT-ETHERNET` modes as it was [brought to my attention](https://github.com/Diniboy1123/usque/issues/6). Seems to be a small, yet feature packed codebase with a strong focus on RFC compliance.
- [masquerade](https://github.com/ErikBcd/masquerade) - Awesome stuff on top of the original [project](https://github.com/jromwu/masquerade) extending it with `CONNECT-IP` support. Comes with both a server and a client both written in Rust. Great practical example that relies on Cloudflare's quiche for the QUIC part.

## Interesting other QUIC related projects

- [aioquic](https://github.com/aiortc/aioquic) - A Python implementation of QUIC and HTTP/3.
- [Cloudflare quiche](https://github.com/cloudflare/quiche) - Cloudflare's take on QUIC and HTTP/3 written in rust. Unfortunately lacks out of the box support for MASQUE as of now, but provides a really low level API to work with QUIC, therefore making it a great choice for custom implementations. [tokio-quiche](https://github.com/cloudflare/quiche/tree/master/tokio-quiche) was recently open-sourced by them, which makes handling many QUIC connections a piece of cake.
- [Google quiche](https://github.com/google/quiche) - Google's implementation of QUIC, HTTP/2, HTTP/3, **MASQUE** and more in C++. Mature and seemingly used by Chromium.
- [msquic](https://github.com/microsoft/msquic) - Microsoft's performant QUIC implementation in C. As far as I can tell, lacks HTTP/3 support out of the box. Comes with bindings for C# and Rust.
- [quic-go](https://github.com/quic-go/quic-go) - A Go implementation of QUIC and HTTP/3. Great library with large test coverage.

This list is not exhaustive—there are many ongoing efforts to implement QUIC and MASQUE. These are the ones I encountered, used in my research, or found interesting for future reference. For a more comprehensive list of QUIC implementations, check out [this resource](https://github.com/quicwg/base-drafts/wiki/Implementations). If you know of another interesting project that should be added, let me know!