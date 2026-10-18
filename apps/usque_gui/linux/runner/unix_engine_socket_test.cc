#include "unix_engine_socket.h"

#include <fcntl.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <cerrno>
#include <vector>

namespace {
void check(bool condition, const char* message) {
  if (!condition) {
    fprintf(stderr, "FAIL: %s\n", message);
    exit(1);
  }
}
}

int main() {
  char pattern[] = "/tmp/usque-socket-test-XXXXXX";
  const char* directory = mkdtemp(pattern);
  check(directory != nullptr, "create private fixture directory");
  const std::string path = std::string(directory) + "/engine.sock";
  const int listener = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  check(listener >= 0, "create fixture listener");
  sockaddr_un address {};
  address.sun_family = AF_UNIX;
  memcpy(address.sun_path, path.c_str(), path.size() + 1);
  check(bind(listener, reinterpret_cast<sockaddr*>(&address), sizeof(address)) == 0,
        "bind fixture listener");
  check(chmod(path.c_str(), 0600) == 0 && listen(listener, 8) == 0,
        "protect fixture listener");

  int peer = usque_connect_engine_socket(path, getpid());
  check(peer >= 0, "accept exact peer PID and UID in private directory");
  check((fcntl(peer, F_GETFD) & FD_CLOEXEC) != 0, "descriptor cannot leak to a child");
  close(peer);
  check(usque_connect_engine_socket(path, getpid() + 1) == -1,
        "reject same-user wrong process identity");
  check(chmod(path.c_str(), 0660) == 0, "change fixture socket mode");
  check(usque_connect_engine_socket(path, getpid()) == -1, "reject group-accessible socket");
  check(chmod(path.c_str(), 0600) == 0 && chmod(directory, 0750) == 0,
        "change fixture directory mode");
  check(usque_connect_engine_socket(path, getpid()) == -1, "reject group-accessible runtime directory");
  check(chmod(directory, 0700) == 0, "restore private directory");
  const std::string alias = std::string(directory) + "/alias.sock";
  check(symlink(path.c_str(), alias.c_str()) == 0, "create fixture symlink");
  check(usque_connect_engine_socket(alias, getpid()) == -1, "reject socket symlink");
  const std::string directory_alias = std::string(directory) + "-alias";
  check(symlink(directory, directory_alias.c_str()) == 0, "create directory symlink");
  check(usque_connect_engine_socket(directory_alias + "/engine.sock", getpid()) == -1,
        "reject private directory symlink");
  const uint8_t maximum[] = {0xff, 0xff, 0xff, 0xff};
  const uint8_t bounded[] = {0x00, 0x40, 0x00, 0x00};
  check(usque_frame_length(maximum) == UINT32_MAX, "unsigned frame length does not overflow");
  check(usque_frame_length(bounded) == 4 * 1024 * 1024, "frame length uses big endian");

  int partial[2];
  check(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, partial) == 0,
        "create inert fragmented-response fixture");
  uint8_t fragment[] = {0x42, 0x43};
  check(write(partial[0], fragment, 1) == 1, "queue partial response");
  const int64_t deadline = usque_socket_deadline(60);
  check(!usque_transfer_engine_bytes(partial[1], fragment, 2, false, deadline) && errno == ETIMEDOUT,
        "partial reads share the total operation deadline");
  check(usque_socket_deadline(0) < deadline + 1000, "stalled peer remains bounded");
  check(!usque_transfer_engine_bytes(partial[1], fragment, 1, false, deadline) && errno == ETIMEDOUT,
        "expired deadline is not reset between header and payload");
  close(partial[0]);
  check(!usque_transfer_engine_bytes(partial[1], fragment, 1, false, usque_socket_deadline(1000)),
        "peer process or socket closure stops the transfer promptly");
  close(partial[1]);

  check(socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, partial) == 0,
        "create inert stalled-write fixture");
  const int send_buffer = 4096;
  check(setsockopt(partial[0], SOL_SOCKET, SO_SNDBUF, &send_buffer, sizeof(send_buffer)) == 0,
        "bound the fixture buffer independently of host defaults");
  std::vector<uint8_t> large_frame(4 * 1024 * 1024, 0);
  const int64_t write_deadline = usque_socket_deadline(60);
  check(!usque_transfer_engine_bytes(partial[0], large_frame.data(), large_frame.size(), true, write_deadline) && errno == ETIMEDOUT,
        "a peer that does not consume requests cannot stall a write indefinitely");
  check(usque_socket_deadline(0) < write_deadline + 1000,
        "stalled writes share the bounded operation deadline");
  close(partial[0]);
  close(partial[1]);

  close(listener);
  unlink(alias.c_str());
  unlink(directory_alias.c_str());
  unlink(path.c_str());
  rmdir(directory);
  puts("Linux authenticated Engine socket tests passed.");
  return 0;
}
