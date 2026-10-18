#include "unix_engine_socket.h"
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
#include <poll.h>
#include <chrono>
#include <algorithm>
#include <cerrno>
#include <cstring>

int usque_connect_engine_socket(const std::string& path, pid_t expected_pid, int timeout_ms) {
  if (path.empty() || path[0] != '/' || path.size() >= sizeof(sockaddr_un::sun_path) || expected_pid <= 0 || timeout_ms <= 0 || timeout_ms > 1000) {
    errno = EINVAL;
    return -1;
  }
  struct stat directory {}, socket_stat {};
  const auto parent = path.substr(0, path.rfind('/'));
  if (lstat(parent.c_str(), &directory) != 0 || !S_ISDIR(directory.st_mode) ||
      directory.st_uid != getuid() || (directory.st_mode & 0777) != 0700 ||
      lstat(path.c_str(), &socket_stat) != 0 ||
      !S_ISSOCK(socket_stat.st_mode) || socket_stat.st_uid != getuid() ||
      (socket_stat.st_mode & 0777) != 0600) {
    errno = EACCES;
    return -1;
  }
  int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
  if (fd < 0) return -1;
  timeval timeout {timeout_ms / 1000, (timeout_ms % 1000) * 1000};
  if (setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)) != 0 ||
      setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &timeout, sizeof(timeout)) != 0) {
    close(fd);
    return -1;
  }
  sockaddr_un address {};
  address.sun_family = AF_UNIX;
  memcpy(address.sun_path, path.c_str(), path.size() + 1);
  ucred peer {};
  socklen_t length = sizeof(peer);
  if (connect(fd, reinterpret_cast<sockaddr*>(&address), sizeof(address)) != 0 ||
      getsockopt(fd, SOL_SOCKET, SO_PEERCRED, &peer, &length) != 0 ||
      length != sizeof(peer) || peer.uid != getuid() || peer.pid != expected_pid) {
    close(fd);
    errno = EACCES;
    return -1;
  }
  return fd;
}

int64_t usque_socket_deadline(int timeout_ms) {
  return std::chrono::duration_cast<std::chrono::milliseconds>(
      std::chrono::steady_clock::now().time_since_epoch()).count() + timeout_ms;
}

bool usque_transfer_engine_bytes(int fd, uint8_t* bytes, size_t length,
                                bool writing, int64_t deadline_ms) {
  while (length != 0) {
    const int64_t remaining = deadline_ms - usque_socket_deadline(0);
    if (remaining <= 0) { errno = ETIMEDOUT; return false; }
    pollfd socket {fd, static_cast<short>(writing ? POLLOUT : POLLIN), 0};
    const int ready = poll(&socket, 1, static_cast<int>(std::min<int64_t>(remaining, 1800000)));
    if (ready < 0 && errno == EINTR) continue;
    if (ready == 0) { errno = ETIMEDOUT; return false; }
    if (ready < 0) return false;
    const ssize_t count = writing ? send(fd, bytes, length, MSG_NOSIGNAL | MSG_DONTWAIT)
                                  : recv(fd, bytes, length, MSG_DONTWAIT);
    if (count < 0 && (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)) continue;
    if (count <= 0) return false;
    bytes += count;
    length -= count;
  }
  return true;
}


uint32_t usque_frame_length(const uint8_t* bytes) {
  return (static_cast<uint32_t>(bytes[0]) << 24) |
         (static_cast<uint32_t>(bytes[1]) << 16) |
         (static_cast<uint32_t>(bytes[2]) << 8) | bytes[3];
}
