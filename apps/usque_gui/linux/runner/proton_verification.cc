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
  void* (*data_manager)(void*) = nullptr;
  void* (*proxy_new)(const char*,const char* const*) = nullptr;
  void (*proxy_free)(void*) = nullptr;
  void (*proxy_set)(void*,int,void*) = nullptr;
  gboolean (*auth_proxy)(void*) = nullptr;
  const char* (*auth_host)(void*) = nullptr;
  unsigned (*auth_port)(void*) = nullptr;
  const char* (*auth_realm)(void*) = nullptr;
  void* (*credential_new)(const char*,const char*,int) = nullptr;
  void (*credential_free)(void*) = nullptr;
  void (*authenticate)(void*,void*) = nullptr;
  void (*auth_cancel)(void*) = nullptr;
  GQuark (*network_error)() = nullptr;
  GQuark (*policy_error)() = nullptr;
  bool ProxySymbols() {
    return Symbol(data_manager,"webkit_web_context_get_website_data_manager") && Symbol(proxy_new,"webkit_network_proxy_settings_new") && Symbol(proxy_free,"webkit_network_proxy_settings_free") && Symbol(proxy_set,"webkit_website_data_manager_set_network_proxy_settings") && Symbol(auth_proxy,"webkit_authentication_request_is_for_proxy") && Symbol(auth_host,"webkit_authentication_request_get_host") && Symbol(auth_port,"webkit_authentication_request_get_port") && Symbol(auth_realm,"webkit_authentication_request_get_realm") && Symbol(credential_new,"webkit_credential_new") && Symbol(credential_free,"webkit_credential_free") && Symbol(authenticate,"webkit_authentication_request_authenticate") && Symbol(auth_cancel,"webkit_authentication_request_cancel");
  }
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
          Symbol(network_error,"webkit_network_error_quark") && Symbol(policy_error,"webkit_policy_error_quark") &&
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
  unsigned probe_stage=0;
  int64_t load_deadline=0;
  bool loaded=false;
  std::set<GObject*> resources;
  bool Active() const { return !finishing && usque::proton::Now()<handoff.expires && g_get_monotonic_time()<monotonic_deadline; }
};
std::unique_ptr<Session> current;
void Finish(bool unavailable=false,const std::string* message=nullptr) {
  if (!current || current->finishing) return;
  auto session=std::move(current);
  session->finishing=true;
  for (auto* resource:session->resources) {
    g_signal_handlers_disconnect_by_data(resource,session.get());
    g_object_unref(resource);
  }
  session->resources.clear();
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
void NetworkFailed() {
  if (current && current->handoff.version==2 && !current->handoff.proxy_port) { auto message=current->handoff.NetworkFailure(); Finish(false,&message); }
  else Finish(true);
}

// WebView load-failed covers navigation, not scripts, images or fetch/XHR.
// Keep resource handlers owned by the session so a late error cannot affect a
// replacement window. Cancellation and policy rejection are not network loss.
bool ResourceTransportError(const GError* error) {
  if (!error) return false;
  if (error->domain==G_RESOLVER_ERROR || error->domain==G_TLS_ERROR) return true;
  if (error->domain==G_IO_ERROR) {
    switch (error->code) {
      case G_IO_ERROR_HOST_NOT_FOUND:
      case G_IO_ERROR_HOST_UNREACHABLE:
      case G_IO_ERROR_NETWORK_UNREACHABLE:
      case G_IO_ERROR_CONNECTION_REFUSED:
      case G_IO_ERROR_CONNECTION_CLOSED:
      case G_IO_ERROR_NOT_CONNECTED:
      case G_IO_ERROR_TIMED_OUT:
      case G_IO_ERROR_PARTIAL_INPUT:
      case G_IO_ERROR_PROXY_FAILED: return true;
      default: return false;
    }
  }
  if (error->domain==api.network_error()) {
    // WebKitNetworkError transport / failed, plus WebCore's request timeout.
    return error->code==300 || error->code==399 || error->code==-1001;
  }
  // WebKitGTK 4.0 can use libsoup 2 transport status codes. HTTP statuses,
  // cancellation, malformed HTTP, redirect errors and unknown domains stay out.
  return error->domain==g_quark_from_static_string("soup-http-error-quark") &&
    error->code>=2 && error->code<=7;
}
void ResourceFailed(void*,GError* error,gpointer data) {
  auto* s=static_cast<Session*>(data);
  if (current.get()!=s || !s->Active() || s->probe_stage || !error) return;
  if (ResourceTransportError(error)) NetworkFailed();
}
void ResourceTlsFailed(void*,void*,guint,gpointer data) {
  auto* s=static_cast<Session*>(data);
  if (current.get()==s && s->Active() && !s->probe_stage) NetworkFailed();
}
void ResourceFinished(GObject* resource,gpointer data) {
  auto* s=static_cast<Session*>(data);
  if (current.get()!=s || !s->resources.erase(resource)) return;
  g_signal_handlers_disconnect_by_data(resource,s);
  g_object_unref(resource);
}
void ResourceStarted(void*,GObject* resource,void*,gpointer data) {
  auto* s=static_cast<Session*>(data);
  if (current.get()!=s || !s->Active() || s->probe_stage) return;
  if (s->resources.size()>=256) { Finish(true); return; }
  if (!s->resources.insert(resource).second) return;
  g_object_ref(resource);
  g_signal_connect(resource,"failed",G_CALLBACK(ResourceFailed),s);
  g_signal_connect(resource,"failed-with-tls-errors",G_CALLBACK(ResourceTlsFailed),s);
  g_signal_connect(resource,"finished",G_CALLBACK(ResourceFinished),s);
}
gboolean Authenticate(void*,void* request,gpointer data) {
  auto* s=static_cast<Session*>(data);
  const char* host=api.auth_host(request); const char* realm=api.auth_realm(request);
  if (current.get()!=s || !s->Active() || !api.auth_proxy(request) || !host || std::string(host)!="127.0.0.1" || api.auth_port(request)!=s->handoff.proxy_port || !realm || realm!=s->handoff.proxy_nonce) { api.auth_cancel(request); return TRUE; }
  void* credential=api.credential_new(s->handoff.proxy_username.c_str(),s->handoff.proxy_password.c_str(),0);
  if (!credential) { api.auth_cancel(request); return TRUE; }
  api.authenticate(request,credential); api.credential_free(credential); return TRUE;
}
gboolean LoadFailed(void*,int,const char* url,GError* error,gpointer data) {
  auto* s=static_cast<Session*>(data);
  if (current.get()!=s) return TRUE;
  if (error && ((error->domain==api.network_error() && error->code==302) || error->domain==api.policy_error())) return TRUE;
  if (s->probe_stage==1 && url && url==s->handoff.ProbeHttps()) {
    s->probe_stage=2; api.load(s->view,s->handoff.Probe("ready").c_str());
  } else NetworkFailed();
  return TRUE;
}
void LoadChanged(void*,int event,gpointer data) {
  auto* s=static_cast<Session*>(data);
  if (current.get()==s && !s->probe_stage && event==3) {
    const char* url=api.uri(s->view); if (url && s->handoff.Matches(url,Sha)) s->loaded=true;
  }
}
gboolean Closed(GtkWidget*,GdkEvent*,gpointer) { Finish(); return TRUE; }
void Destroyed(GtkWidget*,gpointer) { if (current) current->window=nullptr; Finish(); }
gboolean Expired(gpointer data) {
  auto* session=static_cast<Session*>(data);
  if (!current || current.get()!=session) return G_SOURCE_REMOVE;
  if (!session->loaded && session->load_deadline && g_get_monotonic_time()>=session->load_deadline) { session->timer=0; NetworkFailed(); return G_SOURCE_REMOVE; }
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
    // The CAPTCHA is cross-origin on verify-api.proton.me. Allow only its
    // exact path and current challenge hash/ForceWebMessaging query, alongside
    // the official page origin. Never infer top from an empty frame name.
    // Main-resource responses and bridge messages still require the exact
    // challenge root URL below. Subresources retain Proton's own CSP.
    if (session->probe_stage) {
      if (url && session->probe_stage==2 && url==session->handoff.Ready(Sha)) {
        api.ignore(decision); session->probe_stage=0; session->load_deadline=g_get_monotonic_time()+15000000;
        api.load(session->view,session->handoff.url.c_str());
      } else if (url && session->handoff.ProbeUrl(url)) api.use(decision); else api.ignore(decision);
      return TRUE;
    }
    if (url && session->handoff.NavigationAllowed(url,Sha)) api.use(decision);
    else api.ignore(decision);
  } else if (type==2) { // WEBKIT_POLICY_DECISION_TYPE_RESPONSE
    if (api.main_resource(decision)) {
      const char* url=api.uri(session->view);
      if (!url || !(session->probe_stage ? session->handoff.ProbeUrl(url) : session->handoff.Matches(url,Sha))) { api.ignore(decision); return TRUE; }
    }
    api.use(decision);
  } else api.ignore(decision); // new windows
  return TRUE;
}
gboolean Permission(void*,void* request,gpointer) { api.deny(request); return TRUE; }
void Download(void*,void* download,gpointer) { api.download_cancel(download); }
GtkWidget* NewWindow(void*,void*,gpointer) { return nullptr; }
gboolean TlsError(void*,const char*,void*,int,gpointer) { NetworkFailed(); return TRUE; }
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
  if (session->handoff.proxy_port) {
    if (!api.ProxySymbols()) { Finish(true); return; }
    void* settings=api.proxy_new(session->handoff.ProxyOrigin().c_str(),nullptr);
    void* manager=api.data_manager(session->context);
    if (!settings || !manager) { if (settings) api.proxy_free(settings); Finish(true); return; }
    api.proxy_set(manager,2,settings); api.proxy_free(settings); // CUSTOM; no DIRECT bypass
    session->probe_stage=1;
  }
  unsigned char random[16];
  if (getrandom(random,sizeof(random),0)!=sizeof(random)) { Finish(true); return; }
  constexpr char hex[]="0123456789abcdef";
  for (unsigned char c:random) { session->nonce+=hex[c>>4]; session->nonce+=hex[c&15]; }
  const auto script=usque::proton::Script(session->handoff,"window.webkit.messageHandlers.proton.postMessage(JSON.stringify({nonce:'"+session->nonce+"',message}));");
  void* user_script=api.script_new(script.c_str(),1,0,nullptr,nullptr); // top frame, document start
  if (!user_script) { Finish(true); return; }
  api.add_script(session->manager,user_script); api.script_unref(user_script);
  session->view=GTK_WIDGET(g_object_new(api.view_type(),"web-context",session->context,"user-content-manager",session->manager,nullptr));
  if (!session->view) { Finish(true); return; }
  void* settings=api.settings(session->view);
  for (const char* setting : {"enable-developer-extras","enable-html5-database","enable-html5-local-storage","enable-page-cache","enable-offline-web-application-cache","enable-media-stream","enable-webrtc","enable-media-source","enable-webaudio","enable-write-console-messages-to-stdout","allow-file-access-from-file-urls","allow-universal-access-from-file-urls","javascript-can-open-windows-automatically"}) SetBoolean(settings,setting,FALSE);
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
  if (session->handoff.proxy_port) g_signal_connect(session->view,"authenticate",G_CALLBACK(Authenticate),session);
  g_signal_connect(session->view,"load-failed",G_CALLBACK(LoadFailed),session);
  g_signal_connect(session->view,"resource-load-started",G_CALLBACK(ResourceStarted),session);
  g_signal_connect(session->view,"load-changed",G_CALLBACK(LoadChanged),session);
  g_signal_connect(session->view,"load-failed-with-tls-errors",G_CALLBACK(TlsError),session);
  g_signal_connect(session->context,"download-started",G_CALLBACK(Download),session);
  session->timer=g_timeout_add(100,Expired,session);
  gtk_widget_show_all(session->window);
  session->load_deadline=g_get_monotonic_time()+(session->probe_stage ? 5000000 : 15000000);
  api.load(session->view,(session->probe_stage ? session->handoff.Probe() : session->handoff.url).c_str());
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
