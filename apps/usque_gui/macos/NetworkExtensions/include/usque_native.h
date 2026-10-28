#ifndef USQUE_NATIVE_H
#define USQUE_NATIVE_H
#include <stddef.h>
#include <stdint.h>
#include <stdbool.h>
#ifdef __OBJC__
@class NWEndpoint;
typedef NWEndpoint *UsqueLegacyEndpoint;
#endif
typedef struct { uint8_t *data; size_t len; } UsqueBuffer;
typedef int32_t (*UsquePlatformAction)(void *, const uint8_t *, size_t);
typedef int32_t (*UsqueCredentialCallback)(void *, const uint8_t *, size_t, uint8_t *, size_t, size_t *);
void *usque_macos_runtime_create(UsquePlatformAction, void *);
void *usque_macos_runtime_create_with_broker(UsquePlatformAction, UsqueCredentialCallback, void *);
int32_t usque_macos_runtime_command(void *, const uint8_t *, size_t, UsqueBuffer *);
int32_t usque_macos_runtime_snapshot(void *, UsqueBuffer *);
int32_t usque_macos_runtime_send_packet(void *, const uint8_t *, size_t);
int32_t usque_macos_runtime_receive_packet(void *, UsqueBuffer *);
void usque_macos_buffer_free(UsqueBuffer);
void usque_macos_runtime_destroy(void *);
uint32_t usque_macos_protocol_version(void);
void *usque_macos_policy_new_json(const uint8_t *, size_t);
int32_t usque_macos_policy_route_host(void *, const uint8_t *, size_t);
int32_t usque_macos_policy_route_ip(void *, const uint8_t *, size_t);
int32_t usque_macos_policy_route_flow(void *, const uint8_t *, size_t, const uint8_t *, size_t);
void usque_macos_policy_free(void *);
/* The server verifies the audit token on every XPC message, never a PID. */
typedef char *(*UsqueControlHandler)(void *, const char *, const char *, const uint8_t *, size_t);
void *usque_native_xpc_listen(const char *, const char *, UsqueControlHandler, void *);
char *usque_native_xpc_call(const char *, const char *, const uint8_t *, size_t, uint32_t);
void *usque_native_xpc_open(const char *, const char *);
char *usque_native_xpc_request(void *, const uint8_t *, size_t, uint32_t);
void usque_native_xpc_close(void *);
void usque_native_xpc_set_callback(UsqueControlHandler, void *);
char *usque_native_xpc_callback(const uint8_t *, size_t, uint32_t);
char *usque_native_self_identity(void);
char *usque_native_audit_identity(const uint8_t *, size_t, const char *);
void usque_native_free(void *);
int32_t usque_native_bind_probe_socket(int32_t, uint32_t, char **);
#endif
