#include "engine_bridge.h"
#include "unix_engine_socket.h"

#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <cstring>
#include <string>
#include <vector>

namespace {
constexpr size_t kMaximumFrame = 4 * 1024 * 1024;

struct Request {
  std::string path;
  pid_t pid;
  bool wait;
  int timeout_ms;
  std::vector<uint8_t> frame;
  ~Request() {
    volatile uint8_t* sensitive = frame.data();
    for (size_t i = 0; i < frame.size(); ++i) sensitive[i] = 0;
  }
};

bool parse_peer(FlValue* args, std::string* path, pid_t* pid) {
  if (args == nullptr || fl_value_get_type(args) != FL_VALUE_TYPE_MAP) return false;
  FlValue* name = fl_value_lookup_string(args, "pipe_name");
  FlValue* process = fl_value_lookup_string(args, "engine_pid");
  if (name == nullptr || fl_value_get_type(name) != FL_VALUE_TYPE_STRING ||
      process == nullptr || fl_value_get_type(process) != FL_VALUE_TYPE_INT ||
      fl_value_get_int(process) <= 0 || fl_value_get_int(process) > G_MAXINT) {
    return false;
  }
  *path = fl_value_get_string(name);
  *pid = static_cast<pid_t>(fl_value_get_int(process));
  const char* runtime = g_getenv("XDG_RUNTIME_DIR");
  if (runtime == nullptr || runtime[0] != '/') return false;
  const std::string base = std::string(runtime) + "/usque/engine-v1.sock";
  return (*path == base || *path == base + ".events") &&
         path->size() < sizeof(sockaddr_un::sun_path);
}


void perform_request(GTask* task, gpointer, gpointer data, GCancellable*) {
  auto* request = static_cast<Request*>(data);
  int fd = -1;
  const int64_t deadline = usque_socket_deadline(request->timeout_ms);
  do {
    fd = usque_connect_engine_socket(request->path, request->pid,
        static_cast<int>(std::max<int64_t>(1, std::min<int64_t>(1000, deadline - usque_socket_deadline(0)))));
    if (fd >= 0 || !request->wait) break;
    g_usleep(20000);
  } while (usque_socket_deadline(0) < deadline);
  if (fd < 0) {
    g_task_return_new_error(task, G_IO_ERROR, G_IO_ERROR_FAILED,
                           "The authenticated local Engine socket is unavailable.");
    return;
  }
  if (request->wait) {
    close(fd);
    g_task_return_pointer(task, g_bytes_new(nullptr, 0),
                          reinterpret_cast<GDestroyNotify>(g_bytes_unref));
    return;
  }
  uint8_t header[4];
  errno = 0;
  bool ok = usque_transfer_engine_bytes(fd, request->frame.data(), request->frame.size(), true, deadline) &&
            usque_transfer_engine_bytes(fd, header, sizeof(header), false, deadline);
  const size_t length = ok ? usque_frame_length(header) : 0;
  ok = ok && length > 0 && length <= kMaximumFrame;
  std::vector<uint8_t> response;
  if (ok) {
    response.assign(header, header + sizeof(header));
    response.resize(length + sizeof(header));
    ok = usque_transfer_engine_bytes(fd, response.data() + sizeof(header), length, false, deadline);
  }
  const bool timed_out = !ok && errno == ETIMEDOUT;
  close(fd);
  // Avoid retaining request bytes (which can include credentials) after use.
  volatile uint8_t* sensitive = request->frame.data();
  for (size_t i = 0; i < request->frame.size(); ++i) sensitive[i] = 0;
  if (!ok) {
    volatile uint8_t* partial = response.data();
    for (size_t i = 0; i < response.size(); ++i) partial[i] = 0;
    g_task_return_new_error(task, G_IO_ERROR, timed_out ? G_IO_ERROR_TIMED_OUT : G_IO_ERROR_INVALID_DATA,
                           "The local Engine response is incomplete or invalid.");
    return;
  }
  auto* owned_response = new std::vector<uint8_t>(std::move(response));
  GBytes* result = g_bytes_new_with_free_func(owned_response->data(), owned_response->size(),
      [](gpointer data) {
        auto* bytes = static_cast<std::vector<uint8_t>*>(data);
        volatile uint8_t* secret = bytes->data();
        for (size_t i = 0; i < bytes->size(); ++i) secret[i] = 0;
        delete bytes;
      }, owned_response);
  g_task_return_pointer(task, result, reinterpret_cast<GDestroyNotify>(g_bytes_unref));
}

void request_finished(GObject*, GAsyncResult* result, gpointer data) {
  auto* call = FL_METHOD_CALL(data);
  g_autoptr(GError) error = nullptr;
  g_autoptr(GBytes) bytes = static_cast<GBytes*>(
      g_task_propagate_pointer(G_TASK(result), &error));
  if (error != nullptr) {
    fl_method_call_respond_error(call,
        g_error_matches(error, G_IO_ERROR, G_IO_ERROR_TIMED_OUT)
            ? "ENGINE_REQUEST_TIMEOUT" : "ENGINE_IPC_UNAVAILABLE", error->message,
                                 nullptr, nullptr);
  } else {
    gsize length = 0;
    const auto* value = static_cast<const uint8_t*>(g_bytes_get_data(bytes, &length));
    g_autoptr(FlValue) response = length == 0 ? fl_value_new_null()
                                            : fl_value_new_uint8_list(value, length);
    fl_method_call_respond_success(call, response, nullptr);
  }
  g_object_unref(call);
}

struct EventBridge {
  FlEventChannel* channel = nullptr;
  GSocket* socket = nullptr;
  GSource* source = nullptr;
  std::vector<uint8_t> pending;

  void stop() {
    if (source != nullptr) {
      g_source_destroy(source);
      g_source_unref(source);
      source = nullptr;
    }
    g_clear_object(&socket);
    pending.clear();
  }
  ~EventBridge() { stop(); }
};

gboolean event_ready(GSocket* socket, GIOCondition condition, gpointer data) {
  auto* bridge = static_cast<EventBridge*>(data);
  uint8_t bytes[16384];
  g_autoptr(GError) error = nullptr;
  const gssize count = g_socket_receive(socket, reinterpret_cast<char*>(bytes),
                                       sizeof(bytes), nullptr, &error);
  if (count < 0 && g_error_matches(error, G_IO_ERROR, G_IO_ERROR_WOULD_BLOCK) &&
      !(condition & (G_IO_HUP | G_IO_ERR))) return G_SOURCE_CONTINUE;
  if (count <= 0) {
    fl_event_channel_send_error(bridge->channel, "ENGINE_EVENT_UNAVAILABLE",
                                 "The local Engine event stream closed.",
                                 nullptr, nullptr, nullptr);
    fl_event_channel_send_end_of_stream(bridge->channel, nullptr, nullptr);
    bridge->stop();
    return G_SOURCE_REMOVE;
  }
  bridge->pending.insert(bridge->pending.end(), bytes, bytes + count);
  while (bridge->pending.size() >= 4) {
    const size_t size = usque_frame_length(bridge->pending.data());
    if (size == 0 || size > kMaximumFrame) {
      fl_event_channel_send_error(bridge->channel, "ENGINE_IPC_FRAME_TOO_LARGE",
                                   "The local Engine event frame is invalid.",
                                   nullptr, nullptr, nullptr);
      fl_event_channel_send_end_of_stream(bridge->channel, nullptr, nullptr);
      bridge->stop();
      return G_SOURCE_REMOVE;
    }
    if (bridge->pending.size() < size + 4) break;
    g_autoptr(FlValue) frame = fl_value_new_uint8_list(bridge->pending.data(), size + 4);
    fl_event_channel_send(bridge->channel, frame, nullptr, nullptr);
    bridge->pending.erase(bridge->pending.begin(), bridge->pending.begin() + size + 4);
  }
  return G_SOURCE_CONTINUE;
}

FlMethodErrorResponse* listen_events(FlEventChannel* channel, FlValue* args,
                                      gpointer data) {
  auto* bridge = static_cast<EventBridge*>(data);
  bridge->stop();
  Request request {};
  if (!parse_peer(args, &request.path, &request.pid)) {
    return fl_method_error_response_new("ENGINE_IPC_INVALID_PEER",
                                         "Invalid Engine event peer.", nullptr);
  }
  const int fd = usque_connect_engine_socket(request.path, request.pid);
  if (fd < 0) {
    return fl_method_error_response_new("ENGINE_EVENT_UNAVAILABLE",
                                         "Engine events are unavailable.", nullptr);
  }
  bridge->channel = channel;
  bridge->socket = g_socket_new_from_fd(fd, nullptr);
  if (bridge->socket == nullptr) {
    close(fd);
    return fl_method_error_response_new("ENGINE_EVENT_UNAVAILABLE",
                                         "Engine events are unavailable.", nullptr);
  }
  g_socket_set_blocking(bridge->socket, FALSE);
  bridge->source = g_socket_create_source(
      bridge->socket, static_cast<GIOCondition>(G_IO_IN | G_IO_HUP | G_IO_ERR), nullptr);
  g_source_set_callback(bridge->source, G_SOURCE_FUNC(event_ready), bridge, nullptr);
  g_source_attach(bridge->source, nullptr);
  return nullptr;
}

FlMethodErrorResponse* cancel_events(FlEventChannel*, FlValue*, gpointer data) {
  static_cast<EventBridge*>(data)->stop();
  return nullptr;
}
}  // namespace

bool usque_engine_method(FlMethodCall* call) {
  const char* method = fl_method_call_get_name(call);
  const bool wait = strcmp(method, "waitForEnginePipe") == 0;
  if (!wait && strcmp(method, "exchangeFrame") != 0) return false;
  auto* request = new Request {};
  request->wait = wait;
  request->timeout_ms = 30000;
  FlValue* args = fl_method_call_get_args(call);
  bool valid = parse_peer(args, &request->path, &request->pid);
  if (valid) {
    FlValue* timeout = fl_value_lookup_string(args, "timeout_ms");
    valid = timeout != nullptr && fl_value_get_type(timeout) == FL_VALUE_TYPE_INT &&
        fl_value_get_int(timeout) > 0 && fl_value_get_int(timeout) <= (wait ? 30000 : 1800000);
    if (valid) request->timeout_ms = static_cast<int>(fl_value_get_int(timeout));
  }
  if (!wait && valid) {
    FlValue* frame = fl_value_lookup_string(args, "request");
    valid = frame != nullptr && fl_value_get_type(frame) == FL_VALUE_TYPE_UINT8_LIST &&
            fl_value_get_length(frame) >= 4 &&
            fl_value_get_length(frame) <= kMaximumFrame + 4;
    if (valid) {
      const uint8_t* bytes = fl_value_get_uint8_list(frame);
      valid = usque_frame_length(bytes) == fl_value_get_length(frame) - 4;
      if (valid) request->frame.assign(bytes, bytes + fl_value_get_length(frame));
    }
  }
  if (!valid) {
    delete request;
    fl_method_call_respond_error(call, "ENGINE_IPC_INVALID_REQUEST",
                                 "Invalid Engine endpoint or request frame.", nullptr, nullptr);
    return true;
  }
  g_autoptr(GTask) task = g_task_new(nullptr, nullptr, request_finished, g_object_ref(call));
  g_task_set_task_data(task, request, [](gpointer value) { delete static_cast<Request*>(value); });
  g_task_run_in_thread(task, perform_request);
  return true;
}

void usque_register_engine_events(FlBinaryMessenger* messenger) {
  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  g_autoptr(FlEventChannel) channel = fl_event_channel_new(
      messenger, "io.github.georgexie2333.usque/engine_events", FL_METHOD_CODEC(codec));
  fl_event_channel_set_stream_handlers(channel, listen_events, cancel_events,
      new EventBridge {}, [](gpointer value) { delete static_cast<EventBridge*>(value); });
}
