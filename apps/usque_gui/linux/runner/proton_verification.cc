#include "proton_verification.h"
#include "../../native/proton_verification.h"

#include <dlfcn.h>
#include <sys/random.h>
#include <cstring>
#include <memory>

namespace {
struct Api {
  void* library = nullptr;
  void* (*context_new)() = nullptr;
  void* (*manager_new)() = nullptr;
  void* (*script_new)(const char*,int,int,const char* const*,const char* const*) = nullptr;
  void (*script_unref)(void*) = nullptr;
  void (*add_script)(void*,void*) = nullptr;
  gboolean (*register_handler)(void*,const char*) = nullptr;
  GType (*view_type)() = nullptr;
  void* (*settings)(void*) = nullptr;
  void (*load)(void*,const char*) = nullptr;
  const char* (*uri)(void*) = nullptr;
  void (*ignore)(void*) = nullptr;
  void (*use)(void*) = nullptr;
  void* (*navigation_action)(void*) = nullptr;
  void* (*request)(void*) = nullptr;
  const char* (*request_uri)(void*) = nullptr;
  gboolean (*main_resource)(void*) = nullptr;
  void (*deny)(void*) = nullptr;
  void (*download_cancel)(void*) = nullptr;
  void* (*js_value)(void*) = nullptr;
  gboolean (*is_string)(void*) = nullptr;
  char* (*to_string)(void*) = nullptr;
  template<class T> bool Symbol(T& target,const char* name) {
    target = reinterpret_cast<T>(dlsym(library,name)); return target != nullptr;
  }
  bool Load() {
    if (library) return true;
    for (const char* name : {"libwebkit2gtk-4.1.so.0","libwebkit2gtk-4.0.so.37"}) {
      library=dlopen(name,RTLD_NOW|RTLD_LOCAL);
      if (!library) continue;
      if (Symbol(context_new,"webkit_web_context_new_ephemeral") &&
          Symbol(manager_new,"webkit_user_content_manager_new") &&
          Symbol(script_new,"webkit_user_script_new") &&
          Symbol(script_unref,"webkit_user_script_unref") &&
          Symbol(add_script,"webkit_user_content_manager_add_script") &&
          Symbol(register_handler,"webkit_user_content_manager_register_script_message_handler") &&
          Symbol(view_type,"webkit_web_view_get_type") &&
          Symbol(settings,"webkit_web_view_get_settings") &&
          Symbol(load,"webkit_web_view_load_uri") && Symbol(uri,"webkit_web_view_get_uri") &&
          Symbol(ignore,"webkit_policy_decision_ignore") && Symbol(use,"webkit_policy_decision_use") &&
          Symbol(navigation_action,"webkit_navigation_policy_decision_get_navigation_action") &&
          Symbol(request,"webkit_navigation_action_get_request") && Symbol(request_uri,"webkit_uri_request_get_uri") &&
          Symbol(main_resource,"webkit_response_policy_decision_is_main_frame_main_resource") &&
          Symbol(deny,"webkit_permission_request_deny") && Symbol(download_cancel,"webkit_download_cancel") &&
          Symbol(js_value,"webkit_javascript_result_get_js_value") &&
          Symbol(is_string,"jsc_value_is_string") && Symbol(to_string,"jsc_value_to_string")) return true;
      dlclose(library); library=nullptr;
    }
    return false;
  }
};
Api api;
std::string Sha(const std::string& value) {
  g_autofree gchar* digest=g_compute_checksum_for_data(G_CHECKSUM_SHA256,
      reinterpret_cast<const guchar*>(value.data()),value.size());
  return digest ? digest : "";
}
struct Session {
  usque::proton::Handoff handoff;
  GtkWidget* window=nullptr;
  GtkWidget* view=nullptr;
  GObject* context=nullptr;
  GObject* manager=nullptr;
  FlMethodCall* call=nullptr;
  guint timer=0;
  int64_t monotonic_deadline=0;
  std::string nonce;
  bool finishing=false;
  bool Active() const { return !finishing && usque::proton::Now()<handoff.expires && g_get_monotonic_time()<monotonic_deadline; }
};
std::unique_ptr<Session> current;
void Finish(bool unavailable=false,const std::string* message=nullptr) {
  if (!current || current->finishing) return;
  auto session=std::move(current);
  session->finishing=true;
  if (session->timer) { g_source_remove(session->timer); session->timer=0; }
  if (session->view) g_signal_handlers_disconnect_by_data(session->view,session.get());
  if (session->manager) g_signal_handlers_disconnect_by_data(session->manager,session.get());
  if (session->context) g_signal_handlers_disconnect_by_data(session->context,session.get());
  if (session->window) {
    g_signal_handlers_disconnect_by_data(session->window,session.get());
    gtk_widget_destroy(session->window);
  }
  if (session->call) {
    if (message) {
      g_autoptr(FlValue) result=fl_value_new_uint8_list(reinterpret_cast<const uint8_t*>(message->data()),message->size());
      fl_method_call_respond_success(session->call,result,nullptr);
    } else if (unavailable) {
      fl_method_call_respond_error(session->call,"PROTON_VERIFICATION_UNAVAILABLE","The official verification window is unavailable.",nullptr,nullptr);
    } else fl_method_call_respond_success(session->call,nullptr,nullptr);
    g_object_unref(session->call);
  }
  if (session->manager) g_object_unref(session->manager);
  if (session->context) g_object_unref(session->context);
}
gboolean Closed(GtkWidget*,GdkEvent*,gpointer) { Finish(); return TRUE; }
void Destroyed(GtkWidget*,gpointer) { if (current) current->window=nullptr; Finish(); }
gboolean Expired(gpointer data) {
  auto* session=static_cast<Session*>(data);
  if (!current || current.get()!=session) return G_SOURCE_REMOVE;
  if (!session->Active()) { session->timer=0; Finish(); return G_SOURCE_REMOVE; }
  return G_SOURCE_CONTINUE;
}
void Message(void*,void* result,gpointer data) {
  auto* session=static_cast<Session*>(data);
  if (!current || current.get()!=session) return;
  if (!session->Active()) { Finish(); return; }
  const char* url=api.uri(session->view);
  void* value=api.js_value(result);
  if (!url || !session->handoff.Matches(url,Sha) || !value || !api.is_string(value)) return;
  gchar* text=api.to_string(value);
  if (!text) return;
  const size_t size=strnlen(text,32769);
  std::string message(text,size);
  volatile char* wipe=text;
  for (size_t i=0;i<size;++i) wipe[i]=0;
  g_free(text);
  usque::proton::Json wrapper;
  if (size<=32768 && usque::proton::Parser(message).Parse(wrapper) && wrapper.kind==usque::proton::Json::kObject && wrapper.object.size()==2) {
    const auto nonce=usque::proton::Field(wrapper,"nonce",usque::proton::Json::kString);
    const auto body=usque::proton::Field(wrapper,"message",usque::proton::Json::kString);
    unsigned difference=nonce && nonce->text.size()==session->nonce.size() ? 0 : 1;
    if (nonce) for (size_t i=0;i<session->nonce.size();++i) difference |= static_cast<unsigned char>(session->nonce[i]) ^ static_cast<unsigned char>(i<nonce->text.size()?nonce->text[i]:0);
    if (!difference && body && session->handoff.Accept(body->text)) Finish(false,&body->text);
  }
  usque::proton::Wipe(message);
}
gboolean Policy(void*,void* decision,int type,gpointer data) {
  auto* session=static_cast<Session*>(data);
  if (!current || current.get()!=session || !session->Active()) { api.ignore(decision); return TRUE; }
  if (type==0) { // WEBKIT_POLICY_DECISION_TYPE_NAVIGATION_ACTION
    void* action=api.navigation_action(decision);
    void* request=action ? api.request(action) : nullptr;
    const char* url=request ? api.request_uri(request) : nullptr;
    // Older WebKitGTK has no reliable main-frame navigation-action marker.
    // Require the official origin for every frame; the same-origin
    // /core/v4/captcha iframe must remain available. Never infer top from an
    // empty frame name. Main-resource responses and every bridge message also
    // require the exact challenge hash/root URL below. Subresources remain
    // subject to Proton's own CSP.
    if (url && usque::proton::Official(url)) api.use(decision);
    else api.ignore(decision);
  } else if (type==2) { // WEBKIT_POLICY_DECISION_TYPE_RESPONSE
    if (api.main_resource(decision)) {
      const char* url=api.uri(session->view);
      if (!url || !session->handoff.Matches(url,Sha)) { api.ignore(decision); return TRUE; }
    }
    api.use(decision);
  } else api.ignore(decision); // new windows
  return TRUE;
}
gboolean Permission(void*,void* request,gpointer) { api.deny(request); return TRUE; }
void Download(void*,void* download,gpointer) { api.download_cancel(download); }
GtkWidget* NewWindow(void*,void*,gpointer) { return nullptr; }
gboolean TlsError(void*,const char*,void*,int,gpointer) { Finish(true); return TRUE; }
void SetBoolean(void* settings,const char* name,gboolean value) {
  if (g_object_class_find_property(G_OBJECT_GET_CLASS(settings),name)) g_object_set(settings,name,value,nullptr);
}
[[maybe_unused]] void Start(GtkWindow* parent,FlMethodCall* call) {
  Finish();
  current=std::make_unique<Session>();
  current->call=FL_METHOD_CALL(g_object_ref(call));
  FlValue* args=fl_method_call_get_args(call);
  FlValue* value=args && fl_value_get_type(args)==FL_VALUE_TYPE_MAP ? fl_value_lookup_string(args,"handoff") : nullptr;
  if (!value || fl_value_get_type(value)!=FL_VALUE_TYPE_UINT8_LIST || fl_value_get_length(value)>65536) { Finish(true); return; }
  std::string bytes(reinterpret_cast<const char*>(fl_value_get_uint8_list(value)),fl_value_get_length(value));
  bool valid=usque::proton::Parse(bytes,current->handoff,Sha);
  usque::proton::Wipe(bytes);
  if (!valid || !api.Load()) { Finish(true); return; }
  auto* session=current.get();
  const int64_t remaining=session->handoff.expires-usque::proton::Now();
  if (remaining<=0) { Finish(); return; }
  session->monotonic_deadline=g_get_monotonic_time()+remaining*1000;
  session->context=G_OBJECT(api.context_new());
  session->manager=G_OBJECT(api.manager_new());
  if (!session->context || !session->manager || !api.register_handler(session->manager,"proton")) { Finish(true); return; }
  unsigned char random[16];
  if (getrandom(random,sizeof(random),0)!=sizeof(random)) { Finish(true); return; }
  constexpr char hex[]="0123456789abcdef";
  for (unsigned char c:random) { session->nonce+=hex[c>>4]; session->nonce+=hex[c&15]; }
  const auto script=usque::proton::Script(session->handoff,"window.webkit.messageHandlers.proton.postMessage(JSON.stringify({nonce:'"+session->nonce+"',message:e.data}));");
  void* user_script=api.script_new(script.c_str(),1,0,nullptr,nullptr); // top frame, document start
  if (!user_script) { Finish(true); return; }
  api.add_script(session->manager,user_script); api.script_unref(user_script);
  session->view=GTK_WIDGET(g_object_new(api.view_type(),"web-context",session->context,"user-content-manager",session->manager,nullptr));
  if (!session->view) { Finish(true); return; }
  void* settings=api.settings(session->view);
  for (const char* setting : {"enable-developer-extras","enable-html5-database","enable-html5-local-storage","enable-page-cache","enable-offline-web-application-cache","enable-media-stream","enable-media-source","enable-webaudio","enable-write-console-messages-to-stdout","allow-file-access-from-file-urls","allow-universal-access-from-file-urls","javascript-can-open-windows-automatically"}) SetBoolean(settings,setting,FALSE);
  session->window=gtk_window_new(GTK_WINDOW_TOPLEVEL);
  gtk_window_set_title(GTK_WINDOW(session->window),"Usque · Proton VPN");
  gtk_window_set_default_size(GTK_WINDOW(session->window),520,680);
  gtk_window_set_transient_for(GTK_WINDOW(session->window),parent);
  gtk_window_set_modal(GTK_WINDOW(session->window),TRUE);
  gtk_window_set_destroy_with_parent(GTK_WINDOW(session->window),TRUE);
  gtk_container_add(GTK_CONTAINER(session->window),session->view);
  g_signal_connect(session->window,"delete-event",G_CALLBACK(Closed),session);
  g_signal_connect(session->window,"destroy",G_CALLBACK(Destroyed),session);
  g_signal_connect(session->manager,"script-message-received::proton",G_CALLBACK(Message),session);
  g_signal_connect(session->view,"decide-policy",G_CALLBACK(Policy),session);
  g_signal_connect(session->view,"permission-request",G_CALLBACK(Permission),session);
  g_signal_connect(session->view,"create",G_CALLBACK(NewWindow),session);
  g_signal_connect(session->view,"load-failed-with-tls-errors",G_CALLBACK(TlsError),session);
  g_signal_connect(session->context,"download-started",G_CALLBACK(Download),session);
  session->timer=g_timeout_add(100,Expired,session);
  gtk_widget_show_all(session->window);
  api.load(session->view,session->handoff.url.c_str());
}
}  // namespace
bool usque_proton_verification_method(GtkWindow* parent,FlMethodCall* call) {
  const char* name=fl_method_call_get_name(call);
  if (strcmp(name,"cancelProtonHumanVerification")==0) { Finish(); fl_method_call_respond_success(call,nullptr,nullptr); return true; }
  if (strcmp(name,"completeProtonHumanVerification")!=0) return false;
#ifdef USQUE_PREVIEW
  (void)parent;
  fl_method_call_respond_error(call,"PROTON_VERIFICATION_UNAVAILABLE","Verification is unavailable in preview.",nullptr,nullptr);
#else
  Start(parent,call);
#endif
  return true;
}
void usque_proton_verification_cancel() { Finish(); }
