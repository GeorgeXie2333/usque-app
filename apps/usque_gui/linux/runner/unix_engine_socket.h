#ifndef USQUE_LINUX_UNIX_ENGINE_SOCKET_H_
#define USQUE_LINUX_UNIX_ENGINE_SOCKET_H_
#include <cstdint>
#include <string>
#include <sys/types.h>

// Authenticate both the filesystem endpoint and the actual connected process.
// The caller owns the returned close-on-exec descriptor, or -1 on failure.
int usque_connect_engine_socket(const std::string& path, pid_t expected_pid, int timeout_ms = 1000);
int64_t usque_socket_deadline(int timeout_ms);
bool usque_transfer_engine_bytes(int fd, uint8_t* bytes, size_t length,
                                bool writing, int64_t deadline_ms);
uint32_t usque_frame_length(const uint8_t* bytes);
#endif
