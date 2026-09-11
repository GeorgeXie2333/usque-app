// Test-only memory peer. No sockets, operating-system TUN, routes, or files.
#include <cstring>
#include <deque>
#include <memory>
#include <string>
#include <vector>
#include <openvpn/ssl/sslchoose.hpp>
#include <openvpn/init/initprocess.hpp>
#include <openvpn/client/cliproto.hpp>
#include <openvpn/crypto/cryptodcsel.hpp>
#include <openvpn/mbedtls/crypto/api.hpp>
#include <openvpn/mbedtls/ssl/sslctx.hpp>
#include <openvpn/mbedtls/util/rand.hpp>

namespace {
using namespace openvpn;
class Peer final : public ProtoContextCallbackInterface {
    // Keep the process time base and crypto initialization alive across both
    // peers. A client created afterward must not reset our handshake timers.
    InitProcess::Init init_;
    Time now_ = Time::now();
    Frame::Ptr frame_{new Frame(Frame::Context(512, 16384, 512, 0, 16, 0))};
    SessionStats::Ptr stats_{new SessionStats};
    std::unique_ptr<ProtoContext> context_;
    std::deque<std::vector<uint8_t>> output_;
    bool reject_auth_;
    uint32_t data_count_ = 0;
    uint32_t handshakes_ = 0;
    uint32_t data_keys_ = 0;
    void control_net_send(const Buffer& packet) override {
        if (output_.size() >= 256 || packet.size() > 65535) throw std::runtime_error("test peer queue limit");
        output_.emplace_back(packet.c_data(), packet.c_data() + packet.size());
    }
    void control_recv(BufferPtr&& packet) override {
        const std::string request(reinterpret_cast<const char*>(packet->c_data()), packet->size());
        if (request.rfind("PUSH_REQUEST", 0) != 0) return;
        const std::string reply = reject_auth_ ? "AUTH_FAILED" :
            "PUSH_REPLY,topology subnet,ifconfig 10.8.0.2 255.255.255.0,route-gateway 10.8.0.1,dhcp-option DNS 10.8.0.1,tun-mtu 1500,cipher AES-128-CBC,auth SHA1";
        BufferAllocated message(reinterpret_cast<const unsigned char*>(reply.c_str()), reply.size() + 1, 0);
        context_->control_send(std::move(message));
    }
    bool supports_epoch_data() override { return false; }
    void active(bool) override { ++handshakes_; }
public:
    Peer(const char* ca, const char* cert, const char* key, bool reject_auth) : reject_auth_(reject_auth) {
        MbedTLSRandom::Ptr rng(new MbedTLSRandom);
        MbedTLSContext::Config::Ptr tls(new MbedTLSContext::Config);
        tls->set_mode(Mode(Mode::SERVER));
        tls->set_frame(frame_);
        tls->set_rng(rng);
        tls->load_ca(ca, true);
        tls->load_cert(cert);
        tls->load_private_key(key);
        tls->set_tls_version_min(TLSVersion::Type::V1_2);
        tls->set_tls_version_max(TLSVersion::Type::V1_2);
        ProtoContext::ProtoConfig::Ptr config(new ProtoContext::ProtoConfig);
        config->ssl_factory = tls->new_factory();
        config->frame = frame_;
        config->now = &now_;
        config->rng = rng;
        config->prng = rng;
        config->protocol = Protocol(Protocol::TCPv4);
        config->layer = Layer(Layer::OSI_LAYER_3);
        config->comp_ctx = CompressContext(CompressContext::NONE, false);
        config->dc.set_factory(new CryptoDCSelect<MbedTLSCryptoAPI>(config->ssl_factory->libctx(), frame_, stats_, rng));
        config->tlsprf_factory.reset(new CryptoTLSPRFFactory<MbedTLSCryptoAPI>);
        CryptoAlgs::allow_default_dc_algs<MbedTLSCryptoAPI>(config->ssl_factory->libctx(), true, false);
        config->dc.set_cipher(CryptoAlgs::lookup("AES-128-CBC"));
        config->dc.set_digest(CryptoAlgs::lookup("SHA1"));
        config->handshake_window = Time::Duration::seconds(10);
        config->become_primary = Time::Duration::seconds(1);
        // The shipping Core is a client. Let it initiate renegotiation just
        // as in the upstream client/server tests (server uses a later timer).
        config->renegotiate = Time::Duration::seconds(60);
        config->expire = Time::Duration::seconds(30);
        config->keepalive_ping = Time::Duration::seconds(5);
        config->keepalive_timeout = Time::Duration::seconds(30);
        config->keepalive_timeout_early = Time::Duration::seconds(10);
        context_.reset(new ProtoContext(this, config, stats_));
        context_->reset();
        context_->start();
    }
    void tick() {
        now_ = Time::now();
        if (now_ >= context_->next_housekeeping()) context_->housekeeping();
        context_->flush(true);
        if (context_->invalidated()) throw std::runtime_error(std::string("test peer invalidated: ") + Error::name(context_->invalidation_reason()));
    }
    void receive(const uint8_t* data, size_t length) {
        if (length == 0 || length > 65535) throw std::runtime_error("test packet size");
        auto packet = BufferAllocatedRc::Create();
        frame_->prepare(Frame::READ_LINK_TCP, *packet);
        packet->write(data, length);
        auto type = context_->packet_type(*packet);
        if (type.is_control()) context_->control_net_recv(type, std::move(packet));
        else if (type.is_data()) {
            context_->data_decrypt(type, *packet);
            if (packet->size()) {
                ++data_count_;
                data_keys_ |= 1u << (data[0] & 7u);
                context_->data_encrypt(*packet);
                control_net_send(*packet);
            }
        }
        tick();
    }
    int pop(uint8_t* out, size_t capacity) {
        tick();
        if (output_.empty()) return 0;
        if (output_.front().size() > capacity) return -1;
        auto packet = std::move(output_.front()); output_.pop_front();
        std::memcpy(out, packet.data(), packet.size());
        return static_cast<int>(packet.size());
    }
    uint32_t data_count() const { return data_count_; }
    uint32_t handshakes() const { return handshakes_; }
    uint32_t data_keys() const { return data_keys_; }
};
thread_local std::string peer_error;
}
extern "C" {
void* usque_test_peer_create(const char* ca, const char* cert, const char* key, int reject_auth) {
    try { return new Peer(ca, cert, key, reject_auth != 0); }
    catch (const std::exception& e) { peer_error = e.what(); return nullptr; }
}
void usque_test_peer_destroy(void* peer) { delete static_cast<Peer*>(peer); }
int usque_test_peer_receive(void* peer, const uint8_t* packet, size_t length) {
    try { static_cast<Peer*>(peer)->receive(packet, length); return 0; }
    catch (const std::exception& e) { peer_error = e.what(); return -1; }
}
int usque_test_peer_pop(void* peer, uint8_t* packet, size_t capacity) {
    try { return static_cast<Peer*>(peer)->pop(packet, capacity); }
    catch (const std::exception& e) { peer_error = e.what(); return -1; }
}
uint32_t usque_test_peer_data_count(void* peer) { return static_cast<Peer*>(peer)->data_count(); }
uint32_t usque_test_peer_handshakes(void* peer) { return static_cast<Peer*>(peer)->handshakes(); }
uint32_t usque_test_peer_data_keys(void* peer) { return static_cast<Peer*>(peer)->data_keys(); }
const char* usque_test_peer_error() { return peer_error.c_str(); }
}
