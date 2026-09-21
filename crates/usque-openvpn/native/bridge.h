#ifndef USQUE_OPENVPN_BRIDGE_H
#define USQUE_OPENVPN_BRIDGE_H
#include <stddef.h>
#include <stdint.h>
#ifdef __cplusplus
extern "C" {
#endif
typedef struct usque_ovpn_session usque_ovpn_session;
typedef void (*usque_ovpn_notify)(void *context);
typedef struct {
    uint32_t kind;
    uint32_t code;
    uint64_t generation;
    uint32_t length;
    uint32_t mtu;
    char ipv4[48];
    char ipv6[48];
    char dns[8][48];
} usque_ovpn_event;

/* Pointers remain owned by the caller; input buffers are copied before return.
 * run has one caller. push/pop/stop may run concurrently with it. destroy must
 * follow run's return and the termination of all other calls. No exception
 * crosses this ABI. No API opens a socket or creates a system TUN. */
usque_ovpn_session *usque_ovpn_create(const uint8_t *config, size_t length,
                                    const char *remote, uint16_t port,
                                    const char *username, const char *password, const char *key_password,
                                    usque_ovpn_notify notify, void *context);
int usque_ovpn_run(usque_ovpn_session *session);
void usque_ovpn_stop(usque_ovpn_session *session);
void usque_ovpn_destroy(usque_ovpn_session *session);
int usque_ovpn_push(usque_ovpn_session *session, uint32_t kind, uint64_t generation,
                    const uint8_t *data, size_t length);
int usque_ovpn_pop(usque_ovpn_session *session, usque_ovpn_event *event,
                   uint8_t *data, size_t capacity);
size_t usque_ovpn_event_size(void);
#ifdef __cplusplus
}
#endif
#endif
