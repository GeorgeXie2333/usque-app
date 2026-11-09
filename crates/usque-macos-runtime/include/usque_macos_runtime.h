#ifndef USQUE_MACOS_RUNTIME_H
#define USQUE_MACOS_RUNTIME_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct UsqueMacosRuntime UsqueMacosRuntime;
typedef struct UsqueMacosPolicy UsqueMacosPolicy;
typedef struct { uint8_t *data; size_t len; } UsqueMacosBuffer;
/* May run concurrently on Rust workers. Zero only after OS completion. */
typedef int32_t (*UsqueMacosPlatformCallback)(void *, const uint8_t *, size_t);
typedef int32_t (*UsqueMacosCredentialCallback)(void *, const uint8_t *, size_t, uint8_t *, size_t, size_t *);
uint32_t usque_macos_protocol_version(void);
UsqueMacosRuntime *usque_macos_runtime_create(UsqueMacosPlatformCallback, void *);
UsqueMacosRuntime *usque_macos_runtime_create_with_broker(UsqueMacosPlatformCallback, UsqueMacosCredentialCallback, void *);
int32_t usque_macos_runtime_command(UsqueMacosRuntime *, const uint8_t *, size_t, UsqueMacosBuffer *);
int32_t usque_macos_runtime_snapshot(UsqueMacosRuntime *, UsqueMacosBuffer *);
/* 0 queued, -2 backpressure, -3 not connected. */
int32_t usque_macos_runtime_send_packet(UsqueMacosRuntime *, const uint8_t *, size_t);
/* 1 packet, 0 empty, negative error. */
int32_t usque_macos_runtime_receive_packet(UsqueMacosRuntime *, UsqueMacosBuffer *);
void usque_macos_buffer_free(UsqueMacosBuffer);
/* Explicit Stop must complete first for intentional network restoration. */
void usque_macos_runtime_destroy(UsqueMacosRuntime *);
UsqueMacosPolicy *usque_macos_policy_new_json(const uint8_t *, size_t);
/* 0 PROXY, 1 DIRECT, 2 REJECT, -1 invalid (reject). */
int32_t usque_macos_policy_route_host(const UsqueMacosPolicy *, const uint8_t *, size_t);
int32_t usque_macos_policy_route_ip(const UsqueMacosPolicy *, const uint8_t *, size_t);
int32_t usque_macos_policy_route_flow(const UsqueMacosPolicy *, const uint8_t *, size_t, const uint8_t *, size_t);
void usque_macos_policy_free(UsqueMacosPolicy *);
#ifdef __cplusplus
}
#endif
#endif
