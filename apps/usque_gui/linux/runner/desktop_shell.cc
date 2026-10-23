#include "desktop_shell.h"

#include "engine_bridge.h"
#include "preferences_writer.h"
#include "linux_preferences_store.h"
#include "startup_observation.h"
#include "proton_verification.h"

#include <fcntl.h>
#include <glib/gstdio.h>
#include <sys/stat.h>
#include <unistd.h>

#include <cstring>
#include <string>
#include <utility>
#include <vector>

namespace {
constexpr const char* kPreferences = "desktop.ini";
struct Shell {
  GtkApplication* application;
  GtkWindow* window;
  FlMethodChannel* channel;
  GDBusConnection* bus;
  GKeyFile* preferences;
  GdkPixbuf* icon = nullptr;
  std::string directory;
  std::unique_ptr<UsquePreferencesWriter> preferences_writer;
  std::unique_ptr<UsqueLinuxPreferencesStore> shared_preferences;
  bool files_ready = false;
  bool preferences_valid = false;
  bool window_touched = false;
  std::vector<std::function<void()>> ready_callbacks;
  std::string callback;
  std::string team;
  FlValue* tray = nullptr;
  guint watcher = 0;
  guint item_registration = 0;
  guint menu_registration = 0;
  guint revision = 1;
  bool tray_available = false;
  bool watcher_present = false;
  bool close_to_tray = false;
};
Shell* shell = nullptr;

struct SensitiveClipboard {
  std::vector<guint8> value;
  guint timeout = 0;
  ~SensitiveClipboard() {
    volatile guint8* bytes = value.data();
    for (size_t i = 0; i < value.size(); ++i) bytes[i] = 0;
    if (timeout != 0) g_source_remove(timeout);
  }
};
SensitiveClipboard* current_sensitive_clipboard = nullptr;

void clipboard_get(GtkClipboard*, GtkSelectionData* selection, guint target, gpointer data) {
  auto* secret = static_cast<SensitiveClipboard*>(data);
  if (target == 1) {
    constexpr guint8 hint[] = {'s', 'e', 'c', 'r', 'e', 't'};
    gtk_selection_data_set(selection, gtk_selection_data_get_target(selection), 8, hint, sizeof(hint));
  } else {
    gtk_selection_data_set_text(selection, reinterpret_cast<const gchar*>(secret->value.data()), secret->value.size());
  }
}
void clipboard_clear(GtkClipboard*, gpointer data) {
  auto* secret = static_cast<SensitiveClipboard*>(data);
  if (current_sensitive_clipboard == secret) current_sensitive_clipboard = nullptr;
  delete secret;
}
gboolean clipboard_expired(gpointer data) {
  auto* secret = static_cast<SensitiveClipboard*>(data);
  secret->timeout = 0;
  if (current_sensitive_clipboard == secret)
    gtk_clipboard_clear(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD));
  return G_SOURCE_REMOVE;
}

FlValue* field(FlValue* map, const char* key) {
  if (map == nullptr || fl_value_get_type(map) != FL_VALUE_TYPE_MAP) return nullptr;
  return fl_value_lookup_string(map, key);
}
bool boolean(FlValue* map, const char* key, bool fallback = false) {
  FlValue* value = field(map, key);
  return value != nullptr && fl_value_get_type(value) == FL_VALUE_TYPE_BOOL
             ? fl_value_get_bool(value) : fallback;
}
const char* string(FlValue* map, const char* key, const char* fallback = "") {
  FlValue* value = field(map, key);
  return value != nullptr && fl_value_get_type(value) == FL_VALUE_TYPE_STRING
             ? fl_value_get_string(value) : fallback;
}
void put(FlValue* map, const char* key, FlValue* value) {
  fl_value_set_string_take(map, key, value);
}
void success(FlMethodCall* call, FlValue* value = nullptr) {
  fl_method_call_respond_success(call, value, nullptr);
}
void fail(FlMethodCall* call, const char* code, const char* message) {
  fl_method_call_respond_error(call, code, message, nullptr, nullptr);
}

void after_files(Shell* self, FlMethodCall* call, std::function<void()> action) {
  g_object_ref(call);
  auto run = [call, action = std::move(action)] { action(); g_object_unref(call); };
  if (self->files_ready) run();
  else self->ready_callbacks.push_back(std::move(run));
}

// Only independent values are created by the worker; channel replies remain
// on the GTK context. Every pending request retains its method-call object.
void read_async(FlMethodCall* call, std::function<FlValue*()> read,
                std::function<void(FlValue*)> finish = {}) {
  struct Job { FlMethodCall* call; std::function<FlValue*()> read;
    std::function<void(FlValue*)> finish;
    ~Job() { g_object_unref(call); }
  };
  auto* job = new Job{FL_METHOD_CALL(g_object_ref(call)), std::move(read), std::move(finish)};
  g_autoptr(GTask) task = g_task_new(nullptr, nullptr,
      [](GObject*, GAsyncResult* result, gpointer) {
        auto* job = static_cast<Job*>(g_task_get_task_data(G_TASK(result)));
        g_autoptr(FlValue) value = static_cast<FlValue*>(g_task_propagate_pointer(G_TASK(result), nullptr));
        if (job->finish) job->finish(value);
        success(job->call, value);
      }, nullptr);
  g_task_set_task_data(task, job, [](gpointer p) { delete static_cast<Job*>(p); });
  g_task_run_in_thread(task, [](GTask* task, gpointer, gpointer p, GCancellable*) {
    g_task_return_pointer(task, static_cast<Job*>(p)->read(),
        reinterpret_cast<GDestroyNotify>(fl_value_unref));
  });
}

void save_preferences(Shell* self, UsquePreferencesWriter::Completion completion = {}) {
  if (!self->files_ready || !self->preferences_valid) {
    if (completion) completion(false);
    return;
  }
  gsize size = 0;
  g_autofree gchar* bytes = g_key_file_to_data(self->preferences, &size, nullptr);
  self->preferences_writer->Save(std::string(bytes, size), std::move(completion));
}

void quit_after_preferences(GApplication* application) {
  g_object_ref(application);
  shell->preferences_writer->Flush([application](bool ok) {
    auto finish = [application, ok](bool shared_ok) {
      if (!ok || !shared_ok) g_warning("Preferences could not be saved.");
      g_application_quit(application);
      g_object_unref(application);
    };
    if (shell->shared_preferences) shell->shared_preferences->Flush(finish);
    else finish(true);
  });
}

std::string autostart_path() {
  return std::string(g_get_user_config_dir()) + "/autostart/" APPLICATION_ID ".desktop";
}

bool start_on_boot() {
  g_autoptr(GKeyFile) file = g_key_file_new();
  return g_key_file_load_from_file(file, autostart_path().c_str(), G_KEY_FILE_NONE, nullptr) &&
         !g_key_file_get_boolean(file, "Desktop Entry", "Hidden", nullptr);
}

bool trusted_file(const char* path, bool executable = false) {
  struct stat metadata {};
  return lstat(path, &metadata) == 0 && S_ISREG(metadata.st_mode) &&
         metadata.st_uid == 0 && (metadata.st_mode & 0022) == 0 &&
         (!executable || (metadata.st_mode & 0111) != 0);
}

FlValue* installation_record() {
  constexpr const char* path = "/etc/usque/install.json";
  if (!trusted_file(path)) return nullptr;
  const int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
  if (fd < 0) return nullptr;
  struct stat metadata {};
  char content[16384];
  const bool safe = fstat(fd, &metadata) == 0 && S_ISREG(metadata.st_mode) &&
      metadata.st_uid == 0 && (metadata.st_mode & 0022) == 0 &&
      metadata.st_size > 0 && metadata.st_size < static_cast<off_t>(sizeof(content));
  const ssize_t count = safe ? read(fd, content, sizeof(content)) : -1;
  close(fd);
  if (count != metadata.st_size || !safe) return nullptr;
  g_autoptr(GBytes) bytes = g_bytes_new(content, count);
  g_autoptr(FlJsonMessageCodec) codec = fl_json_message_codec_new();
  return fl_message_codec_decode_message(FL_MESSAGE_CODEC(codec), bytes, nullptr);
}

bool compatible_record(FlValue* record) {
  FlValue* protocol = field(record, "protocol_version");
  FlValue* schema = field(record, "schema_version");
  return strcmp(string(record, "version"), USQUE_VERSION) == 0 &&
         protocol != nullptr && fl_value_get_type(protocol) == FL_VALUE_TYPE_INT &&
         fl_value_get_int(protocol) == 3 &&
         schema != nullptr && fl_value_get_type(schema) == FL_VALUE_TYPE_INT &&
         fl_value_get_int(schema) == 1 &&
         trusted_file("/usr/lib/usque/usque-engine", true);
}

void quit_finished(GObject* source, GAsyncResult* result, gpointer data) {
  // Engine owns privileged cleanup. Closing the GUI also triggers parent-death
  // recovery if the explicit disconnect acknowledgement was lost.
  g_autoptr(FlMethodResponse) response = fl_method_channel_invoke_method_finish(
      FL_METHOD_CHANNEL(source), result, nullptr);
  (void)response;
  quit_after_preferences(G_APPLICATION(data));
}
void tray_command(Shell* self, const char* command) {
  g_autoptr(FlValue) argument = fl_value_new_string(command);
  fl_method_channel_invoke_method(self->channel, "trayCommand", argument, nullptr,
      strcmp(command, "disconnectAndExit") == 0 ? quit_finished : nullptr,
      strcmp(command, "disconnectAndExit") == 0 ? self->application : nullptr);
}
void show_window(Shell* self) {
  gtk_widget_show(GTK_WIDGET(self->window));
  gtk_window_present(self->window);
}
gboolean delete_window(GtkWidget*, GdkEvent*, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  if (self->tray_available && self->close_to_tray) {
    gtk_widget_hide(GTK_WIDGET(self->window));
  } else {
    tray_command(self, "disconnectAndExit");
  }
  return TRUE;
}

GVariant* menu_properties(Shell* self, int id) {
  GVariantBuilder properties;
  g_variant_builder_init(&properties, G_VARIANT_TYPE("a{sv}"));
  if (id == 0) {
    g_variant_builder_add(&properties, "{sv}", "children-display", g_variant_new_string("submenu"));
  } else {
    const char* key = id == 1 ? "open" : id == 2 ?
        (boolean(self->tray, "connected") ? "disconnect" : "connect") :
        id == 3 ? "tunnel_label" : id == 4 ? "system_proxy_label" : "disconnect_exit";
    g_variant_builder_add(&properties, "{sv}", "label", g_variant_new_string(string(self->tray, key, "Usque")));
    bool enabled = id < 3 || id == 5 || (boolean(self->tray, "outputs_enabled") &&
        (id != 3 || boolean(self->tray, "tunnel") || boolean(self->tray, "tunnel_available")) &&
        (id != 4 || boolean(self->tray, "system_proxy") || boolean(self->tray, "system_proxy_available")));
    g_variant_builder_add(&properties, "{sv}", "enabled", g_variant_new_boolean(enabled));
    g_variant_builder_add(&properties, "{sv}", "visible", g_variant_new_boolean(
        (id != 3 && id != 4) || strlen(string(self->tray, key)) != 0));
    if (id == 3 || id == 4) {
      g_variant_builder_add(&properties, "{sv}", "toggle-type", g_variant_new_string("checkmark"));
      g_variant_builder_add(&properties, "{sv}", "toggle-state", g_variant_new_int32(
          boolean(self->tray, id == 3 ? "tunnel" : "system_proxy") ? 1 : 0));
    }
  }
  return g_variant_builder_end(&properties);
}
GVariant* menu_layout(Shell* self, int id, int depth) {
  GVariantBuilder children;
  g_variant_builder_init(&children, G_VARIANT_TYPE("av"));
  if (id == 0 && depth != 0) {
    for (int child = 1; child <= 5; ++child) {
      g_variant_builder_add(&children, "v", menu_layout(self, child, 0));
    }
  }
  return g_variant_new("(i@a{sv}@av)", id, menu_properties(self, id), g_variant_builder_end(&children));
}

GVariant* tray_pixmap(Shell* self) {
  GVariantBuilder images;
  g_variant_builder_init(&images, G_VARIANT_TYPE("a(iiay)"));
  if (self->icon == nullptr) return g_variant_builder_end(&images);
  const int width = gdk_pixbuf_get_width(self->icon);
  const int height = gdk_pixbuf_get_height(self->icon);
  const int stride = gdk_pixbuf_get_rowstride(self->icon);
  const int channels = gdk_pixbuf_get_n_channels(self->icon);
  const guint8* pixels = gdk_pixbuf_get_pixels(self->icon);
  std::vector<guint8> argb(width * height * 4);
  const char* badge = string(self->tray, "badge", "idle");
  const bool active = strcmp(badge, "idle") != 0;
  const bool green = strcmp(badge, "connected") == 0;
  const bool red = strcmp(badge, "error") == 0;
  for (int y = 0; y < height; ++y) {
    for (int x = 0; x < width; ++x) {
      const guint8* input = pixels + y * stride + x * channels;
      guint8* output = argb.data() + (y * width + x) * 4;
      output[0] = channels == 4 ? input[3] : 255;
      output[1] = input[0]; output[2] = input[1]; output[3] = input[2];
      const int dx = x - (width - 7), dy = y - (height - 7);
      if (active && dx * dx + dy * dy <= 36) {
        output[0] = 255;
        output[1] = green ? 36 : red ? 220 : 235;
        output[2] = green ? 170 : red ? 55 : 170;
        output[3] = green ? 90 : red ? 55 : 40;
      }
    }
  }
  g_variant_builder_add(&images, "(ii@ay)", width, height,
      g_variant_new_fixed_array(G_VARIANT_TYPE_BYTE, argb.data(), argb.size(), 1));
  return g_variant_builder_end(&images);
}

bool menu_event(Shell* self, int id, const char* event) {
  if (id < 0 || id > 5) return false;
  if (strcmp(event, "clicked") != 0) return true;
  if (id == 1) show_window(self);
  if (id == 2) tray_command(self, "toggle");
  if (id == 3 && boolean(self->tray, "outputs_enabled") &&
      (boolean(self->tray, "tunnel") || boolean(self->tray, "tunnel_available")))
    tray_command(self, "toggleTunnel");
  if (id == 4 && boolean(self->tray, "outputs_enabled")) tray_command(self, "toggleSystemProxy");
  if (id == 5) tray_command(self, "disconnectAndExit");
  return true;
}

void dbus_method(GDBusConnection*, const gchar*, const gchar*, const gchar* interface,
                  const gchar* method, GVariant* args, GDBusMethodInvocation* invocation,
                  gpointer data) {
  auto* self = static_cast<Shell*>(data);
  if (strcmp(interface, "org.kde.StatusNotifierItem") == 0) {
    if (strcmp(method, "Activate") == 0 || strcmp(method, "SecondaryActivate") == 0 ||
        strcmp(method, "ContextMenu") == 0) show_window(self);
    g_dbus_method_invocation_return_value(invocation, nullptr);
    return;
  }
  if (strcmp(method, "GetLayout") == 0) {
    gint32 parent, depth;
    g_variant_get_child(args, 0, "i", &parent);
    g_variant_get_child(args, 1, "i", &depth);
    if (parent < 0 || parent > 5) parent = 0;
    g_dbus_method_invocation_return_value(invocation, g_variant_new("(u@(ia{sv}av))",
        self->revision, menu_layout(self, parent, depth)));
  } else if (strcmp(method, "GetGroupProperties") == 0) {
    GVariantBuilder items;
    g_variant_builder_init(&items, G_VARIANT_TYPE("a(ia{sv})"));
    for (int id = 0; id <= 5; ++id)
      g_variant_builder_add(&items, "(i@a{sv})", id, menu_properties(self, id));
    g_dbus_method_invocation_return_value(invocation, g_variant_new("(@a(ia{sv}))", g_variant_builder_end(&items)));
  } else if (strcmp(method, "AboutToShow") == 0) {
    g_dbus_method_invocation_return_value(invocation, g_variant_new("(b)", FALSE));
  } else if (strcmp(method, "AboutToShowGroup") == 0) {
    GVariantBuilder updates, errors;
    g_variant_builder_init(&updates, G_VARIANT_TYPE("ai"));
    g_variant_builder_init(&errors, G_VARIANT_TYPE("ai"));
    g_dbus_method_invocation_return_value(invocation, g_variant_new("(@ai@ai)",
        g_variant_builder_end(&updates), g_variant_builder_end(&errors)));
  } else if (strcmp(method, "GetProperty") == 0) {
    gint32 id;
    const char* name;
    g_variant_get(args, "(i&s)", &id, &name);
    g_autoptr(GVariant) properties = menu_properties(self, id);
    g_autoptr(GVariant) value = g_variant_lookup_value(properties, name, nullptr);
    if (value != nullptr) g_dbus_method_invocation_return_value(invocation, g_variant_new("(v)", value));
    else g_dbus_method_invocation_return_error(invocation, G_DBUS_ERROR,
        G_DBUS_ERROR_INVALID_ARGS, "Unknown menu property");
  } else if (strcmp(method, "EventGroup") == 0) {
    g_autoptr(GVariant) events = g_variant_get_child_value(args, 0);
    GVariantBuilder errors;
    g_variant_builder_init(&errors, G_VARIANT_TYPE("ai"));
    for (gsize index = 0; index < g_variant_n_children(events); ++index) {
      g_autoptr(GVariant) event = g_variant_get_child_value(events, index);
      gint32 id;
      const char* name;
      g_variant_get_child(event, 0, "i", &id);
      g_variant_get_child(event, 1, "&s", &name);
      if (!menu_event(self, id, name)) g_variant_builder_add(&errors, "i", id);
    }
    g_dbus_method_invocation_return_value(invocation, g_variant_new("(@ai)", g_variant_builder_end(&errors)));
  } else if (strcmp(method, "Event") == 0) {
    gint32 id;
    const char* event;
    g_variant_get_child(args, 0, "i", &id);
    g_variant_get_child(args, 1, "&s", &event);
    menu_event(self, id, event);
    g_dbus_method_invocation_return_value(invocation, nullptr);
  } else {
    g_dbus_method_invocation_return_error(invocation, G_DBUS_ERROR,
        G_DBUS_ERROR_UNKNOWN_METHOD, "Unsupported menu method");
  }
}

GVariant* dbus_property(GDBusConnection*, const gchar*, const gchar*, const gchar* interface,
                         const gchar* property, GError**, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  if (strcmp(interface, "com.canonical.dbusmenu") == 0) {
    if (strcmp(property, "Version") == 0) return g_variant_new_uint32(3);
    if (strcmp(property, "TextDirection") == 0) return g_variant_new_string("ltr");
    if (strcmp(property, "Status") == 0) return g_variant_new_string("normal");
    if (strcmp(property, "IconThemePath") == 0) return g_variant_new_strv(nullptr, 0);
  }
  if (strcmp(property, "Category") == 0) return g_variant_new_string("ApplicationStatus");
  if (strcmp(property, "Id") == 0) return g_variant_new_string(APPLICATION_ID);
  if (strcmp(property, "Title") == 0) return g_variant_new_string("Usque");
  if (strcmp(property, "Status") == 0) return g_variant_new_string("Active");
  if (strcmp(property, "WindowId") == 0) return g_variant_new_uint32(0);
  if (strcmp(property, "IconName") == 0) return g_variant_new_string(self->icon == nullptr ? "network-vpn" : "");
  if (strcmp(property, "IconPixmap") == 0) return tray_pixmap(self);
  if (strcmp(property, "ToolTip") == 0) return g_variant_new("(s@a(iiay)ss)", "",
      tray_pixmap(self), "Usque", string(self->tray, "status"));
  if (strcmp(property, "IconThemePath") == 0) return g_variant_new_string("");
  if (strcmp(property, "ItemIsMenu") == 0) return g_variant_new_boolean(FALSE);
  if (strcmp(property, "Menu") == 0) return g_variant_new_object_path("/Menu");
  return nullptr;
}

constexpr const char* kTrayXml = R"xml(<node>
<interface name="org.kde.StatusNotifierItem">
<method name="Activate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="SecondaryActivate"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="ContextMenu"><arg type="i" direction="in"/><arg type="i" direction="in"/></method>
<method name="Scroll"><arg type="i" direction="in"/><arg type="s" direction="in"/></method>
<property name="Category" type="s" access="read"/><property name="Id" type="s" access="read"/>
<property name="Title" type="s" access="read"/><property name="Status" type="s" access="read"/>
<property name="WindowId" type="u" access="read"/><property name="IconName" type="s" access="read"/>
<property name="IconPixmap" type="a(iiay)" access="read"/><property name="ToolTip" type="(sa(iiay)ss)" access="read"/>
<property name="IconThemePath" type="s" access="read"/><property name="ItemIsMenu" type="b" access="read"/>
<property name="Menu" type="o" access="read"/><signal name="NewIcon"/><signal name="NewToolTip"/>
</interface>
<interface name="com.canonical.dbusmenu">
<method name="GetLayout"><arg type="i" direction="in"/><arg type="i" direction="in"/><arg type="as" direction="in"/>
<arg type="u" direction="out"/><arg type="(ia{sv}av)" direction="out"/></method>
<method name="GetGroupProperties"><arg type="ai" direction="in"/><arg type="as" direction="in"/><arg type="a(ia{sv})" direction="out"/></method>
<method name="Event"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="in"/><arg type="u" direction="in"/></method>
<method name="AboutToShow"><arg type="i" direction="in"/><arg type="b" direction="out"/></method>
<method name="AboutToShowGroup"><arg type="ai" direction="in"/><arg type="ai" direction="out"/><arg type="ai" direction="out"/></method>
<method name="GetProperty"><arg type="i" direction="in"/><arg type="s" direction="in"/><arg type="v" direction="out"/></method>
<method name="EventGroup"><arg type="a(isvu)" direction="in"/><arg type="ai" direction="out"/></method>
<property name="Version" type="u" access="read"/><property name="TextDirection" type="s" access="read"/>
<property name="Status" type="s" access="read"/><property name="IconThemePath" type="as" access="read"/>
<signal name="LayoutUpdated"><arg type="u"/><arg type="i"/></signal>
</interface></node>)xml";

void publish_availability(Shell* self, bool available) {
  self->tray_available = available;
  if (!available) {
    self->close_to_tray = false;
    show_window(self);
  }
  g_autoptr(FlValue) value = fl_value_new_bool(available);
  fl_method_channel_invoke_method(self->channel, "trayAvailabilityChanged", value, nullptr, nullptr, nullptr);
}
void tray_host_checked(GObject* object, GAsyncResult* result, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  g_autoptr(GVariant) reply = g_dbus_connection_call_finish(G_DBUS_CONNECTION(object), result, nullptr);
  bool available = false;
  if (reply != nullptr && self->watcher_present) {
    g_autoptr(GVariant) wrapped = g_variant_get_child_value(reply, 0);
    g_autoptr(GVariant) value = g_variant_get_variant(wrapped);
    available = g_variant_is_of_type(value, G_VARIANT_TYPE_BOOLEAN) && g_variant_get_boolean(value);
  }
  publish_availability(self, available);
}
void check_tray_host(Shell* self) {
  g_dbus_connection_call(self->bus, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
      "org.freedesktop.DBus.Properties", "Get",
      g_variant_new("(ss)", "org.kde.StatusNotifierWatcher", "IsStatusNotifierHostRegistered"),
      G_VARIANT_TYPE("(v)"), G_DBUS_CALL_FLAGS_NONE, 5000, nullptr, tray_host_checked, self);
}
void tray_registered(GObject* object, GAsyncResult* result, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  g_autoptr(GVariant) reply = g_dbus_connection_call_finish(G_DBUS_CONNECTION(object), result, nullptr);
  if (reply != nullptr && self->watcher_present) check_tray_host(self);
  else publish_availability(self, false);
}
void watcher_appeared(GDBusConnection* connection, const gchar*, const gchar*, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  self->watcher_present = true;
  g_dbus_connection_call(connection, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
      "org.kde.StatusNotifierWatcher", "RegisterStatusNotifierItem",
      g_variant_new("(s)", "/StatusNotifierItem"), nullptr, G_DBUS_CALL_FLAGS_NONE,
      5000, nullptr, tray_registered, data);
}
void watcher_vanished(GDBusConnection*, const gchar*, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  self->watcher_present = false;
  publish_availability(self, false);
}
void watcher_signal(GDBusConnection*, const gchar*, const gchar*, const gchar*,
                      const gchar*, GVariant*, gpointer data) {
  check_tray_host(static_cast<Shell*>(data));
}

void pick_files(Shell* self, FlMethodCall* call) {
  GtkFileChooserNative* dialog = gtk_file_chooser_native_new(nullptr, self->window,
      GTK_FILE_CHOOSER_ACTION_OPEN, nullptr, nullptr);
  gtk_file_chooser_set_select_multiple(GTK_FILE_CHOOSER(dialog), TRUE);
  g_autoptr(FlValue) files = fl_value_new_list();
  if (gtk_native_dialog_run(GTK_NATIVE_DIALOG(dialog)) == GTK_RESPONSE_ACCEPT) {
    GSList* paths = gtk_file_chooser_get_filenames(GTK_FILE_CHOOSER(dialog));
    if (g_slist_length(paths) > 128) {
      g_slist_free_full(paths, g_free);
      g_object_unref(dialog);
      fail(call, "CHAIN_FILE_COUNT_LIMIT", "Select at most 128 files.");
      return;
    }
    for (GSList* item = paths; item != nullptr; item = item->next) {
      const char* path = static_cast<char*>(item->data);
      g_autofree gchar* name = g_path_get_basename(path);
      FlValue* entry = fl_value_new_map();
      put(entry, "name", fl_value_new_string(name));
      int fd = open(path, O_RDONLY | O_CLOEXEC | O_NONBLOCK);
      struct stat metadata {};
      const bool valid = fd >= 0 && fstat(fd, &metadata) == 0 && S_ISREG(metadata.st_mode);
      if (!valid || metadata.st_size <= 0 || metadata.st_size > 128 * 1024) {
        put(entry, "error", fl_value_new_string(valid && metadata.st_size > 128 * 1024
            ? "CHAIN_FILE_TOO_LARGE" : "CHAIN_FILE_READ_FAILED"));
      } else {
        g_autofree guint8* bytes = static_cast<guint8*>(g_malloc(metadata.st_size));
        ssize_t total = 0;
        while (total < metadata.st_size) {
          ssize_t count = read(fd, bytes + total, metadata.st_size - total);
          if (count <= 0) break;
          total += count;
        }
        if (total != metadata.st_size) put(entry, "error", fl_value_new_string("CHAIN_FILE_READ_FAILED"));
        else put(entry, "bytes", fl_value_new_uint8_list(bytes, total));
        volatile guint8* sensitive = bytes;
        for (ssize_t i = 0; i < metadata.st_size; ++i) sensitive[i] = 0;
      }
      if (fd >= 0) close(fd);
      fl_value_append_take(files, entry);
    }
    g_slist_free_full(paths, g_free);
  }
  g_object_unref(dialog);
  success(call, files);
}

void method_call(FlMethodChannel*, FlMethodCall* call, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  if (usque_engine_method(call)) return;
  if (usque_proton_verification_method(self->window, call)) return;
  const char* method = fl_method_call_get_name(call);
  FlValue* args = fl_method_call_get_args(call);
  if (strcmp(method, "guiStartupObservation") == 0) {
    g_autoptr(FlValue) value = usque_startup_snapshot(); success(call, value);
  } else if (strcmp(method, "readLinuxPreferences") == 0) {
    after_files(self, call, [self, call] {
      g_object_ref(call);
      self->shared_preferences->Read([call](bool ok, const std::string& contents) {
        if (ok) { g_autoptr(FlValue) value = fl_value_new_string(contents.c_str()); success(call, value); }
        else fail(call, "PREFERENCE_LOAD_FAILED", "Saved preferences could not be read. Retry without resetting data.");
        g_object_unref(call);
      });
    });
  } else if (strcmp(method, "writeLinuxPreferences") == 0) {
    if (args == nullptr || fl_value_get_type(args) != FL_VALUE_TYPE_STRING) {
      fail(call, "PREFERENCE_SAVE_FAILED", "Invalid preferences."); return;
    }
    std::string contents(fl_value_get_string(args));
    after_files(self, call, [self, call, contents = std::move(contents)] {
      g_object_ref(call);
      self->shared_preferences->Write(contents, [call](bool ok) {
        g_autoptr(FlValue) value = fl_value_new_bool(ok); success(call, value); g_object_unref(call);
      });
    });
  } else if (strcmp(method, "copySensitiveText") == 0) {
    FlValue* value = field(args, "value");
    FlValue* timeout = field(args, "clear_after_seconds");
    if (value == nullptr || fl_value_get_type(value) != FL_VALUE_TYPE_UINT8_LIST ||
        fl_value_get_length(value) == 0 || fl_value_get_length(value) > 64 * 1024 ||
        timeout == nullptr || fl_value_get_type(timeout) != FL_VALUE_TYPE_INT ||
        fl_value_get_int(timeout) < 1 || fl_value_get_int(timeout) > 300 ||
        !g_utf8_validate(reinterpret_cast<const gchar*>(fl_value_get_uint8_list(value)), fl_value_get_length(value), nullptr)) {
      fail(call, "CLIPBOARD_FAILED", "The sensitive clipboard value is invalid.");
      return;
    }
    auto* secret = new SensitiveClipboard {};
    const guint8* bytes = fl_value_get_uint8_list(value);
    secret->value.assign(bytes, bytes + fl_value_get_length(value));
    GtkTargetEntry targets[] = {
        {const_cast<gchar*>("UTF8_STRING"), 0, 0},
        {const_cast<gchar*>("text/plain;charset=utf-8"), 0, 0},
        {const_cast<gchar*>("x-kde-passwordManagerHint"), 0, 1},
    };
    if (!gtk_clipboard_set_with_data(gtk_clipboard_get(GDK_SELECTION_CLIPBOARD),
        targets, G_N_ELEMENTS(targets), clipboard_get, clipboard_clear, secret)) {
      delete secret;
      fail(call, "CLIPBOARD_FAILED", "The desktop clipboard is unavailable.");
      return;
    }
    current_sensitive_clipboard = secret;
    secret->timeout = g_timeout_add_seconds(fl_value_get_int(timeout), clipboard_expired, secret);
    // Do not request clipboard-manager persistence. The sensitive hint is
    // advisory; external clipboard managers remain outside the app's control.
    success(call);
  } else if (strcmp(method, "getLinuxServiceState") == 0) {
    read_async(call, [] {
    g_autoptr(FlValue) record = installation_record();
    const char* appdir = g_getenv("APPDIR");
    g_autofree gchar* executable = g_file_read_link("/proc/self/exe", nullptr);
    const bool appimage_bundle = appdir != nullptr && appdir[0] == '/' &&
        executable != nullptr &&
        std::string(executable) == std::string(appdir) + "/usr/lib/usque/usque";
    FlValue* state = fl_value_new_map();
    put(state, "installed", fl_value_new_bool(record != nullptr));
    put(state, "compatible", fl_value_new_bool(compatible_record(record)));
    const bool installed_appimage = record != nullptr &&
        strcmp(string(record, "format"), "appimage") == 0 &&
        trusted_file("/usr/lib/usque/usque-update", true);
    put(state, "can_install", fl_value_new_bool(appimage_bundle &&
        (record == nullptr || (installed_appimage && !compatible_record(record)))));
    put(state, "can_repair", fl_value_new_bool(record != nullptr && trusted_file("/usr/lib/usque/usque-update", true)));
    return state;
    });
  } else if (strcmp(method, "linuxEngineExecutable") == 0) {
    read_async(call, [] {
    g_autoptr(FlValue) record = installation_record();
    return compatible_record(record)
        ? fl_value_new_string("/usr/lib/usque/usque-engine") : fl_value_new_null();
    });
  } else if (strcmp(method, "platformPreferences") == 0) {
    after_files(self, call, [self, call] { read_async(call, [] {
    FlValue* prefs = fl_value_new_map();
    put(prefs, "start_on_boot", fl_value_new_bool(start_on_boot()));
    return prefs;
    }, [self](FlValue* prefs) {
    put(prefs, "close_to_tray", fl_value_new_bool(self->close_to_tray && self->tray_available));
    put(prefs, "tray_available", fl_value_new_bool(self->tray_available));
    }); });
  } else if (strcmp(method, "setCloseToTray") == 0) {
    bool enabled = boolean(args, "enabled");
    after_files(self, call, [self, call, enabled] {
    if (enabled && !self->tray_available) {
      fail(call, "TRAY_UNAVAILABLE", "The desktop session has no status notifier host.");
      return;
    }
    g_key_file_set_boolean(self->preferences, "Window", "close-to-tray", enabled);
    g_object_ref(call);
    save_preferences(self, [self, call, enabled](bool ok) {
      if (!ok) fail(call, "PREFERENCE_SAVE_FAILED", "The window preference could not be saved.");
      else { self->close_to_tray = enabled; success(call); }
      g_object_unref(call);
    });
    });
  } else if (strcmp(method, "setStartOnBoot") == 0) {
    const auto path = autostart_path();
    if (!boolean(args, "enabled")) {
      if (g_unlink(path.c_str()) == 0 || errno == ENOENT) success(call);
      else fail(call, "PREFERENCE_SAVE_FAILED", "The login startup entry could not be removed.");
      return;
    }
    g_autofree gchar* executable = g_file_read_link("/proc/self/exe", nullptr);
    // AppImage extraction paths are transient. Persist its original file path.
    const char* appimage = g_getenv("APPIMAGE");
    const char* launch = appimage != nullptr && appimage[0] == '/' ? appimage : executable;
    if (launch == nullptr || launch[0] != '/' || strchr(launch, '\n') != nullptr) {
      fail(call, "PREFERENCE_SAVE_FAILED", "The application launch path is unavailable.");
      return;
    }
    std::string quoted = "\"";
    for (const char* p = launch; *p != '\0'; ++p) {
      if (*p == '\\' || *p == '"' || *p == '$' || *p == '`') quoted += '\\';
      if (*p == '%') quoted += '%';
      quoted += *p;
    }
    quoted += "\"";
    g_autoptr(GKeyFile) file = g_key_file_new();
    g_key_file_set_string(file, "Desktop Entry", "Type", "Application");
    g_key_file_set_string(file, "Desktop Entry", "Name", "Usque");
    g_key_file_set_string(file, "Desktop Entry", "Exec", quoted.c_str());
    g_key_file_set_boolean(file, "Desktop Entry", "Terminal", FALSE);
    g_autofree gchar* parent = g_path_get_dirname(path.c_str());
    gsize length;
    g_autofree gchar* content = g_key_file_to_data(file, &length, nullptr);
    if (g_mkdir_with_parents(parent, 0700) != 0 || !g_file_set_contents_full(path.c_str(), content, length,
        G_FILE_SET_CONTENTS_CONSISTENT, 0600, nullptr)) {
      fail(call, "PREFERENCE_SAVE_FAILED", "The login startup entry could not be saved.");
    } else success(call);
  } else if (strcmp(method, "updateTrayState") == 0) {
    g_clear_pointer(&self->tray, fl_value_unref);
    self->tray = fl_value_ref(args);
    if (self->bus != nullptr) g_dbus_connection_emit_signal(self->bus, nullptr, "/Menu",
        "com.canonical.dbusmenu", "LayoutUpdated", g_variant_new("(ui)", ++self->revision, 0), nullptr);
    if (self->bus != nullptr) {
      g_dbus_connection_emit_signal(self->bus, nullptr, "/StatusNotifierItem",
          "org.kde.StatusNotifierItem", "NewIcon", nullptr, nullptr);
      g_dbus_connection_emit_signal(self->bus, nullptr, "/StatusNotifierItem",
          "org.kde.StatusNotifierItem", "NewToolTip", nullptr, nullptr);
    }
    success(call);
  } else if (strcmp(method, "showTrayNotification") == 0) {
    if (!gtk_window_is_active(self->window)) {
      g_autoptr(GNotification) notice = g_notification_new(string(args, "title", "Usque"));
      g_notification_set_body(notice, string(args, "body"));
      g_notification_set_default_action(notice, "app.show-window");
      g_application_send_notification(G_APPLICATION(self->application), "connection", notice);
    }
    g_autoptr(FlValue) value = fl_value_new_bool(TRUE);
    success(call, value);
  } else if (strcmp(method, "readChainConfigurations") == 0) {
    pick_files(self, call);
  } else if (strcmp(method, "selectDiagnosticsDestination") == 0 ||
             strcmp(method, "selectWarpSecretDestination") == 0) {
    GtkFileChooserNative* dialog = gtk_file_chooser_native_new(nullptr, self->window,
        GTK_FILE_CHOOSER_ACTION_SAVE, nullptr, nullptr);
    gtk_file_chooser_set_do_overwrite_confirmation(GTK_FILE_CHOOSER(dialog), TRUE);
    gtk_file_chooser_set_current_name(GTK_FILE_CHOOSER(dialog),
        strcmp(method, "selectDiagnosticsDestination") == 0 ? "usque-diagnostics.zip" : "warp-account.json");
    g_autoptr(FlValue) value = nullptr;
    if (gtk_native_dialog_run(GTK_NATIVE_DIALOG(dialog)) == GTK_RESPONSE_ACCEPT) {
      g_autofree gchar* path = gtk_file_chooser_get_filename(GTK_FILE_CHOOSER(dialog));
      if (path != nullptr) value = fl_value_new_string(path);
    }
    g_object_unref(dialog);
    success(call, value);
  } else if (strcmp(method, "beginZeroTrustLogin") == 0) {
    const char* team = string(args, "team_name");
    constexpr auto match_flags = static_cast<GRegexMatchFlags>(0);
    g_autoptr(GRegex) pattern = g_regex_new("^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$", G_REGEX_CASELESS, match_flags, nullptr);
    if (!g_regex_match(pattern, team, match_flags, nullptr)) {
      fail(call, "ZERO_TRUST_TEAM_INVALID", "The Zero Trust team name is invalid.");
      return;
    }
    g_autofree gchar* normalized = g_ascii_strdown(team, -1);
    self->team = normalized;
    self->callback.clear();
    g_autoptr(FlValue) url = fl_value_new_string(("https://" + self->team + ".cloudflareaccess.com/warp").c_str());
    success(call, url);
  } else if (strcmp(method, "consumeZeroTrustCallback") == 0) {
    g_autoptr(FlValue) value = self->callback.empty() ? nullptr : fl_value_new_string(self->callback.c_str());
    self->callback.clear();
    success(call, value);
  } else if (strcmp(method, "cancelZeroTrustLogin") == 0) {
    self->team.clear(); self->callback.clear(); success(call);
  } else if (strcmp(method, "exitApplication") == 0) {
    usque_proton_verification_cancel();
    success(call); quit_after_preferences(G_APPLICATION(self->application));
  } else {
    fl_method_call_respond_not_implemented(call, nullptr);
  }
}

gboolean window_state(GtkWidget* widget, GdkEvent*, gpointer data) {
  auto* self = static_cast<Shell*>(data);
  if (!self->files_ready) {
    int width, height; gtk_window_get_size(self->window, &width, &height);
    self->window_touched |= width != 1280 || height != 720 || gtk_window_is_maximized(self->window);
    return FALSE;
  }
  const bool maximized = gtk_window_is_maximized(GTK_WINDOW(widget));
  g_key_file_set_boolean(self->preferences, "Window", "maximized", maximized);
  if (!maximized) {
    int width, height;
    gtk_window_get_size(self->window, &width, &height);
    g_key_file_set_integer(self->preferences, "Window", "width", width);
    g_key_file_set_integer(self->preferences, "Window", "height", height);
  }
  save_preferences(self);
  return FALSE;
}

void load_shell_files(Shell* self) {
  struct Files {
    std::string directory, executable = "usque";
    GKeyFile* preferences = g_key_file_new();
    GdkPixbuf* icon = nullptr;
    bool valid = false;
    ~Files() { g_key_file_unref(preferences); if (icon) g_object_unref(icon); }
  };
  auto* files = new Files{}; files->directory = self->directory;
  g_autoptr(GTask) task = g_task_new(nullptr, nullptr,
      [](GObject*, GAsyncResult* result, gpointer data) {
        auto* self = static_cast<Shell*>(data);
        auto* files = static_cast<Files*>(g_task_get_task_data(G_TASK(result)));
        g_task_propagate_boolean(G_TASK(result), nullptr);
        g_key_file_unref(self->preferences);
        self->preferences = g_key_file_ref(files->preferences);
        self->preferences_valid = files->valid;
        self->icon = files->icon ? GDK_PIXBUF(g_object_ref(files->icon)) : nullptr;
        self->shared_preferences = std::make_unique<UsqueLinuxPreferencesStore>(
            g_get_user_data_dir(), APPLICATION_ID, files->executable);
        self->close_to_tray = files->valid && g_key_file_get_boolean(self->preferences, "Window", "close-to-tray", nullptr);
        self->files_ready = true;
        if (!self->window_touched) {
          int width = g_key_file_get_integer(self->preferences, "Window", "width", nullptr);
          int height = g_key_file_get_integer(self->preferences, "Window", "height", nullptr);
          if (width >= 480 && width <= 7680 && height >= 360 && height <= 4320)
            gtk_window_resize(self->window, width, height);
          if (g_key_file_get_boolean(self->preferences, "Window", "maximized", nullptr)) gtk_window_maximize(self->window);
        } else window_state(GTK_WIDGET(self->window), nullptr, self);
        usque_startup_mark(1, !files->valid);
        auto callbacks = std::move(self->ready_callbacks);
        for (auto& callback : callbacks) callback();
        if (self->bus) g_dbus_connection_emit_signal(self->bus, nullptr, "/StatusNotifierItem",
            "org.kde.StatusNotifierItem", "NewIcon", nullptr, nullptr);
      }, self);
  g_task_set_task_data(task, files, [](gpointer p) { delete static_cast<Files*>(p); });
  g_task_run_in_thread(task, [](GTask* task, gpointer, gpointer data, GCancellable*) {
    auto* files = static_cast<Files*>(data);
    g_autofree gchar* executable = g_file_read_link("/proc/self/exe", nullptr);
    if (executable) {
      g_autofree gchar* directory = g_path_get_dirname(executable);
      g_autofree gchar* name = g_path_get_basename(executable);
      files->executable = name;
      auto dot = files->executable.rfind('.');
      if (dot != std::string::npos && dot != 0) files->executable.resize(dot);
      auto path = std::string(directory) + "/data/flutter_assets/assets/branding/usque-ui-icon.png";
      files->icon = gdk_pixbuf_new_from_file_at_scale(path.c_str(), 32, 32, TRUE, nullptr);
    }
    g_autoptr(GError) error = nullptr;
    files->valid = g_key_file_load_from_file(files->preferences,
        (files->directory + "/" + kPreferences).c_str(), G_KEY_FILE_NONE, &error) ||
        g_error_matches(error, G_FILE_ERROR, G_FILE_ERROR_NOENT);
    g_task_return_boolean(task, TRUE);
  });
}
}  // namespace

void usque_desktop_open_uri(const char* uri) {
  if (shell == nullptr || shell->team.empty() || uri == nullptr || strlen(uri) > 65536) return;
  for (const char* cursor = uri; *cursor != '\0'; ++cursor)
    if (static_cast<unsigned char>(*cursor) <= 0x20 || *cursor == 0x7f) return;
  g_autoptr(GUri) parsed = g_uri_parse(uri, G_URI_FLAGS_ENCODED, nullptr);
  if (parsed == nullptr) return;
  const std::string host = shell->team + ".cloudflareaccess.com";
  const char* query = g_uri_get_query(parsed);
  if (g_ascii_strcasecmp(g_uri_get_scheme(parsed), "com.cloudflare.warp") != 0 ||
      g_uri_get_host(parsed) == nullptr || g_ascii_strcasecmp(g_uri_get_host(parsed), host.c_str()) != 0 ||
      g_strcmp0(g_uri_get_path(parsed), "/auth") != 0 || g_uri_get_userinfo(parsed) != nullptr ||
      g_uri_get_port(parsed) != -1 || g_uri_get_fragment(parsed) != nullptr || query == nullptr ||
      !g_str_has_prefix(query, "token=") || query[6] == '\0' || strchr(query, '&') != nullptr) return;
  shell->callback = uri;
  shell->team.clear();
  show_window(shell);
  fl_method_channel_invoke_method(shell->channel, "zeroTrustCallbackArrived", nullptr, nullptr, nullptr, nullptr);
}

void usque_register_desktop_shell(GtkApplication* application, GtkWindow* window,
                                   FlBinaryMessenger* messenger) {
  shell = new Shell {};
  shell->application = application;
  shell->window = window;
  shell->directory = std::string(g_get_user_config_dir()) + "/usque";
  shell->preferences = g_key_file_new();
  shell->preferences_writer = std::make_unique<UsquePreferencesWriter>(
      shell->directory + "/" + kPreferences);
  // Window positioning belongs to the compositor on Wayland. GTK restores size
  // and maximization on both systems without moving windows to absent monitors.
  g_signal_connect(window, "configure-event", G_CALLBACK(window_state), shell);
  g_signal_connect(window, "window-state-event", G_CALLBACK(window_state), shell);
  g_signal_connect(window, "delete-event", G_CALLBACK(delete_window), shell);
  for (const char* signal : {"button-press-event", "key-press-event"}) {
    g_signal_connect(window, signal, G_CALLBACK(+[](GtkWidget*, GdkEvent*, gpointer data) -> gboolean {
      static_cast<Shell*>(data)->window_touched = true; return FALSE;
    }), shell);
  }
  g_autoptr(FlStandardMethodCodec) codec = fl_standard_method_codec_new();
  shell->channel = fl_method_channel_new(messenger,
      "io.github.georgexie2333.usque/engine", FL_METHOD_CODEC(codec));
  fl_method_channel_set_method_call_handler(shell->channel, method_call, shell, nullptr);
  usque_register_engine_events(messenger);
  g_autoptr(GSimpleAction) show = g_simple_action_new("show-window", nullptr);
  g_signal_connect(show, "activate", G_CALLBACK(+[](GSimpleAction*, GVariant*, gpointer data) {
    show_window(static_cast<Shell*>(data));
  }), shell);
  g_action_map_add_action(G_ACTION_MAP(application), G_ACTION(show));
  shell->bus = g_application_get_dbus_connection(G_APPLICATION(application));
  if (shell->bus != nullptr) {
    g_autoptr(GDBusNodeInfo) info = g_dbus_node_info_new_for_xml(kTrayXml, nullptr);
    static const GDBusInterfaceVTable vtable = {dbus_method, dbus_property, nullptr, {nullptr}};
    shell->item_registration = g_dbus_connection_register_object(shell->bus, "/StatusNotifierItem",
        info->interfaces[0], &vtable, shell, nullptr, nullptr);
    shell->menu_registration = g_dbus_connection_register_object(shell->bus, "/Menu",
        info->interfaces[1], &vtable, shell, nullptr, nullptr);
    g_dbus_connection_signal_subscribe(shell->bus, "org.kde.StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher", "StatusNotifierHostRegistered", "/StatusNotifierWatcher",
        nullptr, G_DBUS_SIGNAL_FLAGS_NONE, watcher_signal, shell, nullptr);
    g_dbus_connection_signal_subscribe(shell->bus, "org.kde.StatusNotifierWatcher",
        "org.kde.StatusNotifierWatcher", "StatusNotifierHostUnregistered", "/StatusNotifierWatcher",
        nullptr, G_DBUS_SIGNAL_FLAGS_NONE, watcher_signal, shell, nullptr);
    if (shell->item_registration != 0 && shell->menu_registration != 0)
      shell->watcher = g_bus_watch_name_on_connection(shell->bus, "org.kde.StatusNotifierWatcher",
          G_BUS_NAME_WATCHER_FLAGS_NONE, watcher_appeared, watcher_vanished, shell, nullptr);
  }
  load_shell_files(shell);
}
