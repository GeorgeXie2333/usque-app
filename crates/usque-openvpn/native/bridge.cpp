#include "bridge.h"
#include <algorithm>
#include <atomic>
#include <cstring>
#include <deque>
#include <memory>
#include <mutex>
#include <string>
#include <vector>
#include <client/ovpncli.hpp>
#include <openvpn/transport/client/extern/config.hpp>
#include <openvpn/tun/extern/config.hpp>
#include <openvpn/tun/builder/capture.hpp>
#include <openvpn/tun/tunmtu.hpp>

namespace {
using namespace openvpn;
constexpr size_t MAX_PACKETS = 256;
constexpr size_t MAX_BYTES = 4 * 1024 * 1024;
constexpr size_t MAX_PACKET = 65535;
enum EventKind : uint32_t { DIAL = 1, TRANSPORT = 2, PACKET = 3, NETWORK = 4, STATE = 5, STOPPED = 6 };
enum InputKind : uint32_t { RECEIVE_TRANSPORT = 1, SEND_IP = 2, TRANSPORT_CONNECTED = 3, TRANSPORT_FAILED = 4 };
class Transport;
class Tun;
class Client;

struct Message {
    usque_ovpn_event event{};
    std::vector<uint8_t> bytes;
};
struct Input {
    uint32_t kind;
    uint64_t generation;
    std::vector<uint8_t> bytes;
};

// Only input/output queues, readiness and the io_context publication cross
// threads. Protocol objects are accessed exclusively on the core's ASIO thread.
struct Shared {
    std::mutex mutex;
    std::deque<Message> output;
    std::deque<Input> input;
    size_t output_bytes = 0;
    size_t input_bytes = 0;
    size_t transport_packets = 0;
    openvpn_io::io_context *io = nullptr;
    Transport *transport = nullptr;
    Tun *tun = nullptr;
    uint64_t generation = 0;
    bool ready = false;
    bool scheduled = false;
    std::atomic<bool> stopping{false};
    std::atomic<bool> finished{false};
    std::string remote;
    uint16_t port;
    usque_ovpn_notify notify;
    void *context;

    Shared(std::string remote_arg, uint16_t port_arg, usque_ovpn_notify notify_arg, void *context_arg)
        : remote(std::move(remote_arg)), port(port_arg), notify(notify_arg), context(context_arg) {}

    bool emit(Message message) {
        {
            std::lock_guard<std::mutex> lock(mutex);
            if (output.size() >= MAX_PACKETS || message.bytes.size() > MAX_BYTES - output_bytes)
                return false;
            message.event.length = static_cast<uint32_t>(message.bytes.size());
            output_bytes += message.bytes.size();
            if (message.event.kind == TRANSPORT)
                ++transport_packets;
            output.push_back(std::move(message));
        }
        notify(context);
        return true;
    }

    bool emit_bytes(uint32_t kind, uint64_t gen, const uint8_t *data, size_t size) {
        if (size > MAX_PACKET)
            return false;
        Message message;
        message.event.kind = kind;
        message.event.generation = gen;
        if (size)
            message.bytes.assign(data, data + size);
        return emit(std::move(message));
    }

    // Called with mutex held. A bounded queue has at most one pending ASIO
    // drain, so the ASIO handler queue cannot become an unbounded second queue.
    void schedule_locked() {
        if (io && !scheduled) {
            scheduled = true;
            openvpn_io::post(*io, [this]() { drain(); });
        }
    }
    void drain();
};

void copy_address(char (&destination)[48], const std::string &source) {
    if (source.size() >= sizeof(destination))
        throw std::runtime_error("invalid network address");
    std::memcpy(destination, source.c_str(), source.size() + 1);
}

class Transport final : public TransportClient {
    Shared &shared;
    ExternalTransport::Config config;
    TransportClientParent *parent;
    openvpn_io::io_context &io;
    uint64_t generation = 0;
    bool halted = true;

  public:
    Transport(Shared &state, const ExternalTransport::Config &conf,
              openvpn_io::io_context &context, TransportClientParent *owner)
        : shared(state), config(conf), parent(owner), io(context) {}
    ~Transport() override { stop(); }

    void transport_start() override {
        if (!config.protocol.is_tcp() && !config.protocol.is_udp())
            throw std::runtime_error("unsupported OpenVPN transport");
        if (shared.stopping.load()) {
            parent->transport_error(Error::TCP_CONNECT_ERROR, "cancelled");
            return;
        }
        {
            std::lock_guard<std::mutex> lock(shared.mutex);
            generation = ++shared.generation;
            shared.io = &io;
            shared.transport = this;
            shared.ready = false;
            halted = false;
        }
        parent->transport_wait();
        if (!shared.emit_bytes(DIAL, generation, nullptr, 0))
            fail();
    }
    void connected() {
        if (!halted)
            parent->transport_connecting();
    }
    void fail() {
        if (!halted)
            parent->transport_error(Error::TCP_CONNECT_ERROR, "WARP transport unavailable");
    }
    void receive(const std::vector<uint8_t> &data) {
        if (halted)
            return;
        try {
            BufferAllocated buffer;
            config.frame->prepare(config.protocol.is_tcp() ? Frame::READ_LINK_TCP : Frame::READ_LINK_UDP, buffer);
            buffer.write(data.data(), data.size());
            config.stats->inc_stat(SessionStats::BYTES_IN, data.size());
            config.stats->inc_stat(SessionStats::PACKETS_IN, 1);
            parent->transport_recv(buffer);
        } catch (const std::exception &) {
            fail();
        }
    }
    void writable() {
        if (!halted)
            parent->transport_needs_send();
    }
    bool transport_send_const(const Buffer &buffer) override {
        if (halted)
            return false;
        const bool sent = shared.emit_bytes(TRANSPORT, generation, buffer.c_data(), buffer.size());
        if (sent) {
            config.stats->inc_stat(SessionStats::BYTES_OUT, buffer.size());
            config.stats->inc_stat(SessionStats::PACKETS_OUT, 1);
        }
        return sent;
    }
    bool transport_send(BufferAllocated &buffer) override { return transport_send_const(buffer); }
    bool transport_has_send_queue() override { return true; }
    size_t transport_send_queue_size() override {
        std::lock_guard<std::mutex> lock(shared.mutex);
        return shared.transport_packets;
    }
    bool transport_send_queue_empty() override { return transport_send_queue_size() == 0; }
    void transport_stop_requeueing() override {}
    void reset_align_adjust(size_t) override {}
    IP::Addr server_endpoint_addr() const override { return IP::Addr(shared.remote); }
    unsigned short server_endpoint_port() const override { return shared.port; }
    void server_endpoint_info(std::string &host, std::string &port,
                              std::string &proto, std::string &ip) const override {
        host = shared.remote;
        port = std::to_string(shared.port);
        proto = config.protocol.str();
        ip = shared.remote;
    }
    Protocol transport_protocol() const override { return config.protocol; }
    void transport_reparent(TransportClientParent *owner) override { parent = owner; }
    void stop() override {
        halted = true;
        std::lock_guard<std::mutex> lock(shared.mutex);
        if (shared.transport == this) {
            shared.transport = nullptr;
            shared.ready = false;
            shared.io = nullptr;
            shared.scheduled = false;
        }
    }
};

class TransportFactory final : public TransportClientFactory {
    Shared &shared;
    ExternalTransport::Config config;
  public:
    TransportFactory(Shared &state, const ExternalTransport::Config &conf)
        : shared(state), config(conf) {}
    TransportClient::Ptr new_transport_client_obj(openvpn_io::io_context &io,
                                                  TransportClientParent *parent) override {
        return new Transport(shared, config, io, parent);
    }
    // Remote changes in PUSH_REPLY are deliberately not applied. A new remote
    // can only be selected through the application's pinned node selection.
};

class Tun final : public TunClient {
    Shared &shared;
    ExternalTun::Config config;
    TunClientParent &parent;
    TunProp::State properties;
    bool halted = true;
  public:
    Tun(Shared &state, const ExternalTun::Config &conf, TunClientParent &owner)
        : shared(state), config(conf), parent(owner) {
        // Native TUN backends supply the default when neither side sets MTU.
        // The memory backend must make the same choice explicitly.
        if (config.tun_prop.mtu == 0)
            config.tun_prop.mtu = TUN_MTU_DEFAULT;
    }
    ~Tun() override { stop(); }
    void tun_start(const OptionList &options, TransportClient &transport, CryptoDCSettings &) override {
        // Capture only: this builder has no route, DNS or interface side effects.
        TunBuilderCapture capture;
        TunProp::configure_builder(&capture, &properties, config.stats.get(),
                                   transport.server_endpoint_addr(), config.tun_prop,
                                   options, nullptr, true);
        // TunProp::State only records MTU for a pushed tun-mtu option. Capture
        // contains the effective local/default/pushed value in all cases.
        properties.mtu = capture.mtu;
        Message message;
        message.event.kind = NETWORK;
        if (properties.vpn_ip4_addr.defined())
            copy_address(message.event.ipv4, properties.vpn_ip4_addr.to_string());
        if (properties.vpn_ip6_addr.defined())
            copy_address(message.event.ipv6, properties.vpn_ip6_addr.to_string());
        message.event.mtu = static_cast<uint32_t>(properties.mtu);
        size_t dns_index = 0;
        for (const auto &entry : capture.dns_options.servers) {
            for (const auto &address : entry.second.addresses) {
                if (dns_index < 8)
                    copy_address(message.event.dns[dns_index++], address.address);
            }
        }
        {
            std::lock_guard<std::mutex> lock(shared.mutex);
            halted = false;
            shared.tun = this;
            message.event.generation = shared.generation;
        }
        if (!shared.emit(std::move(message)))
            throw std::runtime_error("network event queue full");
        parent.tun_connected();
    }
    bool tun_send(BufferAllocated &buffer) override {
        if (halted)
            return false;
        uint64_t gen;
        {
            std::lock_guard<std::mutex> lock(shared.mutex);
            gen = shared.generation;
        }
        const bool sent = shared.emit_bytes(PACKET, gen, buffer.c_data(), buffer.size());
        if (sent) {
            config.stats->inc_stat(SessionStats::TUN_BYTES_OUT, buffer.size());
            config.stats->inc_stat(SessionStats::TUN_PACKETS_OUT, 1);
        }
        return sent;
    }
    void receive(const std::vector<uint8_t> &data) {
        if (halted)
            return;
        try {
            BufferAllocated buffer;
            config.frame->prepare(Frame::READ_TUN, buffer);
            buffer.write(data.data(), data.size());
            config.stats->inc_stat(SessionStats::TUN_BYTES_IN, data.size());
            config.stats->inc_stat(SessionStats::TUN_PACKETS_IN, 1);
            parent.tun_recv(buffer);
        } catch (const std::exception &) {
            parent.tun_error(Error::TUN_READ_ERROR, "invalid IP packet");
        }
    }
    std::string tun_name() const override { return "Usque memory TUN"; }
    std::string vpn_ip4() const override {
        return properties.vpn_ip4_addr.defined() ? properties.vpn_ip4_addr.to_string() : "";
    }
    std::string vpn_ip6() const override {
        return properties.vpn_ip6_addr.defined() ? properties.vpn_ip6_addr.to_string() : "";
    }
    int vpn_mtu() const override { return properties.mtu; }
    void set_disconnect() override { stop(); }
    void stop() override {
        halted = true;
        std::lock_guard<std::mutex> lock(shared.mutex);
        if (shared.tun == this) {
            shared.tun = nullptr;
            shared.ready = false;
        }
    }
};

class TunFactory final : public TunClientFactory {
    Shared &shared;
    ExternalTun::Config config;
  public:
    TunFactory(Shared &state, const ExternalTun::Config &conf) : shared(state), config(conf) {}
    TunClient::Ptr new_tun_client_obj(openvpn_io::io_context &, TunClientParent &parent,
                                      TransportClient *) override { return new Tun(shared, config, parent); }
    bool supports_epoch_data() override { return true; }
};

void Shared::drain() {
    for (;;) {
        Input next;
        // Parent callbacks can replace a protocol object synchronously. Keep
        // its intrusive reference until this dispatch returns on the core thread.
        RCPtr<Transport> current_transport;
        RCPtr<Tun> current_tun;
        bool can_send_ip;
        bool empty;
        {
            std::lock_guard<std::mutex> lock(mutex);
            current_transport = transport;
            current_tun = tun;
            can_send_ip = ready;
            empty = input.empty();
            if (empty) {
                scheduled = false;
            } else {
                next = std::move(input.front());
                input.pop_front();
                input_bytes -= next.bytes.size();
                if (next.generation != generation)
                    continue;
            }
        }
        if (empty) {
            if (current_transport)
                current_transport->writable();
            notify(context);
            return;
        }
        if (stopping.load())
            continue;
        switch (next.kind) {
        case RECEIVE_TRANSPORT:
            if (current_transport) current_transport->receive(next.bytes);
            break;
        case SEND_IP:
            if (current_tun && can_send_ip) current_tun->receive(next.bytes);
            break;
        case TRANSPORT_CONNECTED:
            if (current_transport) current_transport->connected();
            break;
        case TRANSPORT_FAILED:
            if (current_transport) current_transport->fail();
            break;
        }
    }
}

class Client final : public ClientAPI::OpenVPNClient {
    Shared &shared;
  public:
    explicit Client(Shared &state) : shared(state) {}
    TransportClientFactory *new_transport_factory(const ExternalTransport::Config &conf) override {
        if (!conf.protocol.is_tcp() && !conf.protocol.is_udp())
            throw std::runtime_error("unsupported transport");
        return new TransportFactory(shared, conf);
    }
    TunClientFactory *new_tun_factory(const ExternalTun::Config &conf, const OptionList &) override {
        return new TunFactory(shared, conf);
    }
    bool socket_protect(openvpn_io::detail::socket_type, std::string, bool) override { return false; }
    bool pause_on_connection_timeout() override { return false; }
    void log(const ClientAPI::LogInfo &) override {}
    void acc_event(const ClientAPI::AppCustomControlMessageEvent &) override {}
    void external_pki_cert_request(ClientAPI::ExternalPKICertRequest &request) override { request.error = true; }
    void external_pki_sign_request(ClientAPI::ExternalPKISignRequest &request) override { request.error = true; }
    void clock_tick() override {
        if (shared.stopping.load())
            stop();
    }
    void event(const ClientAPI::Event &event) override {
        Message message;
        message.event.kind = STATE;
        message.event.code = event.fatal ? 2 : event.error ? 1 : 0;
        // Names are library enum labels, not peer-supplied info/log strings.
        const auto name = event.name.substr(0, 96);
        message.bytes.assign(name.begin(), name.end());
        {
            std::lock_guard<std::mutex> lock(shared.mutex);
            if (event.name == "CONNECTED") shared.ready = true;
            else if (event.name == "RECONNECTING" || event.name == "DISCONNECTED" || event.error)
                shared.ready = false;
            message.event.generation = shared.generation;
        }
        if (!shared.emit(std::move(message))) {
            shared.stopping.store(true);
            stop();
        }
    }
};
} // namespace

struct usque_ovpn_session {
    Shared shared;
    Client client;
    usque_ovpn_session(std::string remote, uint16_t port, usque_ovpn_notify notify, void *context)
        : shared(std::move(remote), port, notify, context), client(shared) {}
};

extern "C" usque_ovpn_session *usque_ovpn_create(const uint8_t *config, size_t length,
                                                 const char *remote, uint16_t port,
                                                 const char *username, const char *password, const char *key_password,
                                                 usque_ovpn_notify notify, void *context) {
    try {
        if (!config || !length || length > 128 * 1024 || !remote || !port || !notify)
            return nullptr;
        auto session = std::make_unique<usque_ovpn_session>(remote, port, notify, context);
        ClientAPI::Config settings;
        settings.content.assign(reinterpret_cast<const char *>(config), length);
        settings.serverOverride = remote;
        settings.portOverride = std::to_string(port);
        settings.connTimeout = 30;
        settings.tunPersist = false;
        settings.compressionMode = "no";
        // The allowlisted profile always sets a minimum of TLS 1.2 or 1.3.
        // Preserve a stronger profile minimum instead of overriding it.
        settings.enableNonPreferredDCAlgorithms = true;
        settings.enableLegacyAlgorithms = false;
        settings.retryOnAuthFailed = false;
        settings.clockTickMS = 100;
        settings.privateKeyPassword = key_password ? key_password : "";
        auto evaluated = session->client.eval_config(settings);
        std::fill(settings.content.begin(), settings.content.end(), '\0');
        std::fill(settings.privateKeyPassword.begin(), settings.privateKeyPassword.end(), '\0');
        if (evaluated.error)
            return nullptr;
        if (username && *username) {
            ClientAPI::ProvideCreds credentials;
            credentials.username = username;
            credentials.password = password ? password : "";
            const auto result = session->client.provide_creds(credentials);
            std::fill(credentials.username.begin(), credentials.username.end(), '\0');
            std::fill(credentials.password.begin(), credentials.password.end(), '\0');
            if (result.error) return nullptr;
        }
        return session.release();
    } catch (...) { return nullptr; }
}

extern "C" int usque_ovpn_run(usque_ovpn_session *session) {
    if (!session) return -1;
    int result = 0;
    try {
        if (!session->shared.stopping.load())
            result = session->client.connect().error ? -1 : 0;
    } catch (...) { result = -1; }
    {
        std::lock_guard<std::mutex> lock(session->shared.mutex);
        session->shared.io = nullptr;
        session->shared.transport = nullptr;
        session->shared.tun = nullptr;
        session->shared.ready = false;
        session->shared.finished.store(true);
    }
    session->shared.notify(session->shared.context);
    return result;
}

extern "C" void usque_ovpn_stop(usque_ovpn_session *session) {
    if (!session) return;
    session->shared.stopping.store(true);
    try { session->client.stop(); } catch (...) {}
}
extern "C" void usque_ovpn_destroy(usque_ovpn_session *session) { delete session; }

extern "C" int usque_ovpn_push(usque_ovpn_session *session, uint32_t kind, uint64_t generation,
                               const uint8_t *data, size_t length) {
    try {
        if (!session || kind < RECEIVE_TRANSPORT || kind > TRANSPORT_FAILED || length > MAX_PACKET
            || (length && !data)) return -1;
        auto &shared = session->shared;
        std::lock_guard<std::mutex> lock(shared.mutex);
        if (shared.stopping.load() || !shared.io || generation != shared.generation
            || (kind == SEND_IP && !shared.ready)) return -1;
        if (shared.input.size() >= MAX_PACKETS || length > MAX_BYTES - shared.input_bytes) return 0;
        Input input{kind, generation, {}};
        if (length) input.bytes.assign(data, data + length);
        shared.input_bytes += length;
        shared.input.push_back(std::move(input));
        shared.schedule_locked();
        return 1;
    } catch (...) { return -1; }
}

extern "C" int usque_ovpn_pop(usque_ovpn_session *session, usque_ovpn_event *event,
                              uint8_t *data, size_t capacity) {
    try {
        if (!session || !event || !data) return -1;
        auto &shared = session->shared;
        std::lock_guard<std::mutex> lock(shared.mutex);
        if (shared.output.empty()) {
            if (!shared.finished.load()) return 0;
            *event = {};
            event->kind = STOPPED;
            return 1;
        }
        const auto &message = shared.output.front();
        if (message.bytes.size() > capacity) return -1;
        *event = message.event;
        if (!message.bytes.empty()) std::memcpy(data, message.bytes.data(), message.bytes.size());
        shared.output_bytes -= message.bytes.size();
        if (message.event.kind == TRANSPORT) --shared.transport_packets;
        shared.output.pop_front();
        shared.schedule_locked();
        return 1;
    } catch (...) { return -1; }
}
extern "C" size_t usque_ovpn_event_size(void) { return sizeof(usque_ovpn_event); }
