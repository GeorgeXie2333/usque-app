// Workstation-safe private D-Bus fixture for NetworkManager's public schema.
// Shapes follow NetworkManager 1.52 introspection; no system bus is accessed.
#include <gio/gio.h>
#include <cstdio>
#include <cstring>

namespace {
const char* kManager = "<node><interface name='org.freedesktop.NetworkManager'>"
  "<method name='GetDeviceByIpIface'><arg type='s' direction='in'/><arg type='o' direction='out'/></method>"
  "</interface></node>";
const char* kDevice = "<node><interface name='org.freedesktop.NetworkManager.Device'>"
  "<property name='Ip4Config' type='o' access='read'/><property name='Ip6Config' type='o' access='read'/>"
  "</interface></node>";
const char* kV4 = "<node><interface name='org.freedesktop.NetworkManager.IP4Config'>"
  "<property name='NameserverData' type='aa{sv}' access='read'/></interface></node>";
const char* kV6 = "<node><interface name='org.freedesktop.NetworkManager.IP6Config'>"
  "<property name='Nameservers' type='aay' access='read'/></interface></node>";
void Method(GDBusConnection*, const gchar*, const gchar*, const gchar*, const gchar*,
            GVariant*, GDBusMethodInvocation* invocation, gpointer) {
  g_dbus_method_invocation_return_value(invocation, g_variant_new("(o)", "/device"));
}
GVariant* Property(GDBusConnection*, const gchar*, const gchar*, const gchar*,
                   const gchar* property, GError**, gpointer) {
  if (strcmp(property, "Ip4Config") == 0) return g_variant_new_object_path("/v4");
  if (strcmp(property, "Ip6Config") == 0) return g_variant_new_object_path("/v6");
  if (strcmp(property, "NameserverData") == 0) {
    GVariantBuilder dict, array;
    g_variant_builder_init(&dict, G_VARIANT_TYPE("a{sv}"));
    g_variant_builder_add(&dict, "{sv}", "address", g_variant_new_string("192.0.2.53"));
    g_variant_builder_init(&array, G_VARIANT_TYPE("aa{sv}"));
    g_variant_builder_add_value(&array, g_variant_builder_end(&dict));
    return g_variant_builder_end(&array);
  }
  guint8 address[16] = {0xfe, 0x80}; address[15] = 0x53;
  GVariantBuilder array;
  g_variant_builder_init(&array, G_VARIANT_TYPE("aay"));
  g_variant_builder_add_value(&array, g_variant_new_fixed_array(G_VARIANT_TYPE_BYTE, address, 16, 1));
  return g_variant_builder_end(&array);
}
}
int main(int argc, char** argv) {
  if (argc != 2) return 2;
  g_autoptr(GError) error = nullptr;
  g_autoptr(GDBusConnection) bus = g_dbus_connection_new_for_address_sync(argv[1],
      static_cast<GDBusConnectionFlags>(G_DBUS_CONNECTION_FLAGS_AUTHENTICATION_CLIENT |
                                       G_DBUS_CONNECTION_FLAGS_MESSAGE_BUS_CONNECTION),
      nullptr, nullptr, &error);
  if (!bus) return 3;
  static const GDBusInterfaceVTable table = {Method, Property, nullptr, {nullptr}};
  const char* paths[] = {"/org/freedesktop/NetworkManager", "/device", "/v4", "/v6"};
  const char* schemas[] = {kManager, kDevice, kV4, kV6};
  for (int i = 0; i < 4; ++i) {
    g_autoptr(GDBusNodeInfo) info = g_dbus_node_info_new_for_xml(schemas[i], &error);
    if (!info || !g_dbus_connection_register_object(bus, paths[i], info->interfaces[0], &table, nullptr, nullptr, &error)) return 4;
  }
  g_autoptr(GVariant) reply = g_dbus_connection_call_sync(bus, "org.freedesktop.DBus",
      "/org/freedesktop/DBus", "org.freedesktop.DBus", "RequestName",
      g_variant_new("(su)", "org.freedesktop.NetworkManager", 0u), G_VARIANT_TYPE("(u)"),
      G_DBUS_CALL_FLAGS_NONE, 5000, nullptr, &error);
  if (!reply) return 5;
  puts("READY"); fflush(stdout);
  g_autoptr(GMainLoop) loop = g_main_loop_new(nullptr, FALSE);
  g_main_loop_run(loop);
}
