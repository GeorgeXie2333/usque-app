#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <xpc/xpc.h>
#import <netinet/in.h>
#import <arpa/inet.h>
#import <sys/socket.h>
#import <ifaddrs.h>
#import <net/if.h>
#import "../include/usque_native.h"

static const size_t maximumControlBytes = 2 * 1024 * 1024;
static xpc_connection_t controllerConnection;
static NSLock *controllerLock;
static UsqueControlHandler clientCallback;
static void *clientCallbackContext;

void usque_native_xpc_set_callback(UsqueControlHandler callback, void *context) {
    clientCallback = callback; clientCallbackContext = context;
}

static char *identity(SecCodeRef code) {
    CFDictionaryRef info = NULL;
    if (SecCodeCopySigningInformation(code, kSecCSSigningInformation, &info) != errSecSuccess) return NULL;
    NSDictionary *dict = CFBridgingRelease(info);
    NSString *identifier = dict[(__bridge NSString *)kSecCodeInfoIdentifier];
    NSString *team = dict[(__bridge NSString *)kSecCodeInfoTeamIdentifier];
    if (!identifier || !team) return NULL; // Ad-hoc signatures have no trusted Team ID.
    NSData *data = [NSJSONSerialization dataWithJSONObject:@{@"identifier":identifier,@"team":team} options:0 error:NULL];
    return strndup(data.bytes, data.length);
}

char *usque_native_self_identity(void) {
    SecCodeRef code = NULL;
    SecRequirementRef requirement = NULL;
    if (SecCodeCopySelf(kSecCSDefaultFlags, &code) != errSecSuccess) return NULL;
    OSStatus status = SecRequirementCreateWithString(CFSTR("anchor apple generic"), kSecCSDefaultFlags, &requirement);
    if (status == errSecSuccess) status = SecCodeCheckValidity(code, kSecCSStrictValidate, requirement);
    char *result = status == errSecSuccess ? identity(code) : NULL;
    if (requirement) CFRelease(requirement);
    CFRelease(code);
    return result;
}

void *usque_native_xpc_listen(const char *service, const char *requirementText, UsqueControlHandler handler, void *context) {
    NSString *requiredString = [NSString stringWithUTF8String:requirementText];
    SecRequirementRef requirement = NULL;
    if (SecRequirementCreateWithString((__bridge CFStringRef)[NSString stringWithUTF8String:requirementText], 0, &requirement) != errSecSuccess) return NULL;
    xpc_connection_t listener = xpc_connection_create_mach_service(service, NULL, XPC_CONNECTION_MACH_SERVICE_LISTENER);
    if (xpc_connection_set_peer_code_signing_requirement(listener, requirementText) != 0) { CFRelease(requirement); return NULL; }
    xpc_connection_set_event_handler(listener, ^(xpc_object_t peer) {
        if (xpc_get_type(peer) != XPC_TYPE_CONNECTION) return;
        if (xpc_connection_set_peer_code_signing_requirement(peer, requiredString.UTF8String) != 0) { xpc_connection_cancel(peer); return; }
        NSString *owner = NSUUID.UUID.UUIDString;
        xpc_connection_set_event_handler(peer, ^(xpc_object_t message) {
            if (xpc_get_type(message) == XPC_TYPE_ERROR) {
                [controllerLock lock];
                if (controllerConnection == peer) controllerConnection = nil;
                [controllerLock unlock];
                const char *event = "{\"native_event\":\"peer_disconnected\"}";
                char *ignored = handler(context, "", owner.UTF8String, (const uint8_t *)event, strlen(event));
                usque_native_free(ignored);
                return;
            }
            if (xpc_get_type(message) != XPC_TYPE_DICTIONARY) return;
            // Security.framework constructs SecCode from this message's kernel-supplied
            // audit token. This cannot race PID reuse or trust an identifier in JSON.
            SecCodeRef code = NULL;
            if (SecCodeCreateWithXPCMessage(message, kSecCSDefaultFlags, &code) != errSecSuccess) return;
            bool valid = SecCodeCheckValidity(code, kSecCSStrictValidate, requirement) == errSecSuccess;
            char *peerIdentity = valid ? identity(code) : NULL;
            CFRelease(code);
            if (!peerIdentity) return;
            NSDictionary *peerObject = [NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:peerIdentity] dataUsingEncoding:NSUTF8StringEncoding] options:0 error:NULL];
            if ([peerObject[@"identifier"] hasSuffix:@".macos-control"]) {
                [controllerLock lock]; controllerConnection = peer; [controllerLock unlock];
            }
            size_t length = 0;
            const uint8_t *data = xpc_dictionary_get_data(message, "payload", &length);
            xpc_object_t reply = xpc_dictionary_create_reply(message);
            if (reply && data && length > 0 && length <= maximumControlBytes) {
                char *result = handler(context, peerIdentity, owner.UTF8String, data, length);
                if (result) { xpc_dictionary_set_data(reply, "payload", result, strlen(result)); usque_native_free(result); }
                xpc_connection_send_message(peer, reply);
            }
            usque_native_free(peerIdentity);
        });
        xpc_connection_activate(peer);
    });
    // Retained for process lifetime, including the immutable SecRequirement captured
    // above. Controller loss never implicitly disarms the independent filter.
    if (!controllerLock) controllerLock = [NSLock new];
    xpc_connection_activate(listener);
    return (__bridge_retained void *)listener;
}

void *usque_native_xpc_open(const char *service, const char *requirement) {
    xpc_connection_t connection = xpc_connection_create_mach_service(service, NULL, XPC_CONNECTION_MACH_SERVICE_PRIVILEGED);
    if (xpc_connection_set_peer_code_signing_requirement(connection, requirement) != 0) return NULL;
    xpc_connection_set_event_handler(connection, ^(xpc_object_t event) {
        if (xpc_get_type(event) != XPC_TYPE_DICTIONARY || !clientCallback) return;
        size_t length = 0; const uint8_t *data = xpc_dictionary_get_data(event, "payload", &length);
        xpc_object_t reply = xpc_dictionary_create_reply(event);
        if (!reply || !data || !length || length > maximumControlBytes) return;
        char *result = clientCallback(clientCallbackContext, "", "", data, length);
        if (result) { xpc_dictionary_set_data(reply, "payload", result, strlen(result)); usque_native_free(result); }
        xpc_connection_send_message(connection, reply);
    });
    xpc_connection_activate(connection);
    return (__bridge_retained void *)connection;
}

char *usque_native_xpc_request(void *handle, const uint8_t *bytes, size_t length, uint32_t timeoutMs) {
    if (!handle || length == 0 || length > maximumControlBytes) return NULL;
    xpc_connection_t connection = (__bridge xpc_connection_t)handle;
    xpc_object_t message = xpc_dictionary_create(NULL, NULL, 0);
    xpc_dictionary_set_data(message, "payload", bytes, length);
    NSLock *lock = [NSLock new];
    __block BOOL expired = NO;
    __block NSData *result = nil;
    dispatch_semaphore_t completion = dispatch_semaphore_create(0);
    xpc_connection_send_message_with_reply(connection, message, dispatch_get_global_queue(QOS_CLASS_USER_INITIATED, 0), ^(xpc_object_t reply) {
        size_t count = 0;
        const void *value = xpc_get_type(reply) == XPC_TYPE_DICTIONARY ? xpc_dictionary_get_data(reply, "payload", &count) : NULL;
        [lock lock];
        if (!expired && value && count <= maximumControlBytes) result = [NSData dataWithBytes:value length:count];
        [lock unlock];
        dispatch_semaphore_signal(completion);
    });
    dispatch_semaphore_wait(completion, dispatch_time(DISPATCH_TIME_NOW, timeoutMs * (int64_t)NSEC_PER_MSEC));
    [lock lock];
    expired = YES;
    char *output = result ? strndup(result.bytes, result.length) : NULL;
    [lock unlock];
    return output;
}

void usque_native_xpc_close(void *handle) {
    if (!handle) return;
    xpc_connection_t connection = (__bridge_transfer xpc_connection_t)handle;
    xpc_connection_cancel(connection);
}

char *usque_native_xpc_call(const char *service, const char *requirement, const uint8_t *bytes, size_t length, uint32_t timeoutMs) {
    void *handle = usque_native_xpc_open(service, requirement);
    char *result = usque_native_xpc_request(handle, bytes, length, timeoutMs);
    usque_native_xpc_close(handle);
    return result;
}

char *usque_native_xpc_callback(const uint8_t *bytes, size_t length, uint32_t timeout) {
    [controllerLock lock]; xpc_connection_t connection = controllerConnection; [controllerLock unlock];
    if (!connection) return NULL;
    return usque_native_xpc_request((__bridge void *)connection, bytes, length, timeout);
}

void usque_native_free(void *value) {
    if (value) { size_t length = strlen(value); volatile uint8_t *bytes = value; while (length--) *bytes++ = 0; }
    free(value);
}

char *usque_native_audit_identity(const uint8_t *bytes, size_t length, const char *text) {
    if (length != sizeof(audit_token_t)) return NULL;
    NSData *token = [NSData dataWithBytes:bytes length:length];
    NSDictionary *attributes = @{(__bridge NSString *)kSecGuestAttributeAudit:token};
    SecCodeRef code = NULL; SecRequirementRef requirement = NULL;
    if (SecCodeCopyGuestWithAttributes(NULL, (__bridge CFDictionaryRef)attributes, 0, &code) != errSecSuccess) return NULL;
    OSStatus status = SecRequirementCreateWithString((__bridge CFStringRef)[NSString stringWithUTF8String:text], 0, &requirement);
    if (status == errSecSuccess) status = SecCodeCheckValidity(code, kSecCSStrictValidate, requirement);
    char *result = status == errSecSuccess ? identity(code) : NULL;
    if (requirement) CFRelease(requirement); CFRelease(code);
    return result;
}

int32_t usque_native_bind_probe_socket(int32_t socket, uint32_t index, char **localJSON) {
    // This is only an explicitly labelled signed PoC experiment. enforceRoutes may
    // override IP[_V6]_BOUND_IF; correctness requires external packet observation.
    struct sockaddr_storage address = {0}; socklen_t size = sizeof(address);
    if (getsockname(socket, (struct sockaddr *)&address, &size) != 0) return -1;
    int family = address.ss_family;
    int level = family == AF_INET6 ? IPPROTO_IPV6 : IPPROTO_IP;
    int option = family == AF_INET6 ? IPV6_BOUND_IF : IP_BOUND_IF;
    if (setsockopt(socket, level, option, &index, sizeof(index)) != 0) return -1;
    // Pin an actual source address as well as a port. A wildcard source lease
    // could accidentally authorize another socket on the same interface.
    struct ifaddrs *interfaces = NULL;
    if (getifaddrs(&interfaces) != 0) return -1;
    bool selected = false;
    for (struct ifaddrs *entry = interfaces; entry; entry = entry->ifa_next) {
        if (!entry->ifa_addr || if_nametoindex(entry->ifa_name) != index || entry->ifa_addr->sa_family != family) continue;
        if (family == AF_INET) {
            struct sockaddr_in *candidate = (struct sockaddr_in *)entry->ifa_addr;
            ((struct sockaddr_in *)&address)->sin_addr = candidate->sin_addr;
            selected = true; break;
        } else if (family == AF_INET6) {
            struct sockaddr_in6 *candidate = (struct sockaddr_in6 *)entry->ifa_addr;
            if (IN6_IS_ADDR_LINKLOCAL(&candidate->sin6_addr)) continue;
            ((struct sockaddr_in6 *)&address)->sin6_addr = candidate->sin6_addr;
            ((struct sockaddr_in6 *)&address)->sin6_scope_id = candidate->sin6_scope_id;
            selected = true; break;
        }
    }
    freeifaddrs(interfaces);
    if (!selected) return -1;
    if (family == AF_INET) {
        struct sockaddr_in *v4 = (struct sockaddr_in *)&address;
        if (v4->sin_port == 0 && bind(socket, (struct sockaddr *)v4, sizeof(*v4)) != 0) return -1;
    } else if (family == AF_INET6) {
        struct sockaddr_in6 *v6 = (struct sockaddr_in6 *)&address;
        if (v6->sin6_port == 0 && bind(socket, (struct sockaddr *)v6, sizeof(*v6)) != 0) return -1;
    } else return -1;
    size = sizeof(address);
    if (getsockname(socket, (struct sockaddr *)&address, &size) != 0) return -1;
    uint16_t port = family == AF_INET6 ? ntohs(((struct sockaddr_in6 *)&address)->sin6_port) : ntohs(((struct sockaddr_in *)&address)->sin_port);
    if (!port) return -1;
    char source[INET6_ADDRSTRLEN] = {0};
    void *sourceBytes = family == AF_INET6 ? (void *)&((struct sockaddr_in6 *)&address)->sin6_addr : (void *)&((struct sockaddr_in *)&address)->sin_addr;
    if (!inet_ntop(family, sourceBytes, source, sizeof(source))) return -1;
    NSDictionary *object = @{@"local_ip":@(source), @"local_port":@(port), @"interface_index":@(index)};
    NSData *encoded = [NSJSONSerialization dataWithJSONObject:object options:0 error:NULL];
    *localJSON = strndup(encoded.bytes, encoded.length);
    return 0;
}
