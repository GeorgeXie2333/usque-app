#import <Cocoa/Cocoa.h>
#import <CommonCrypto/CommonDigest.h>
#import <Security/SecRandom.h>
#include <atomic>
#include <arpa/inet.h>
#include <chrono>
#include <fcntl.h>
#include <poll.h>
#include <unistd.h>

#include "../../native/proton_verification.h"
#include "launch_options.h"
#include "include/cef_app.h"
#include "include/cef_application_mac.h"
#include "include/cef_browser.h"
#include "include/cef_client.h"
#include "include/cef_callback.h"
#include "include/cef_task.h"
#include "include/wrapper/cef_helpers.h"
#include "include/wrapper/cef_library_loader.h"

namespace {
int protocol_output = -1;
std::string Sha(const std::string& text) {
  unsigned char bytes[CC_SHA256_DIGEST_LENGTH];
  CC_SHA256(text.data(), static_cast<CC_LONG>(text.size()), bytes);
  constexpr char digits[] = "0123456789abcdef";
  std::string result;
  for (const auto byte : bytes) { result += digits[byte >> 4]; result += digits[byte & 15]; }
  return result;
}
bool Exact(int descriptor, void* pointer, size_t size, bool writing) {
  auto* bytes = static_cast<unsigned char*>(pointer);
  const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(10);
  while (size) {
    const auto remaining = std::chrono::duration_cast<std::chrono::milliseconds>(deadline - std::chrono::steady_clock::now()).count();
    if (remaining <= 0) return false;
    pollfd ready{descriptor, static_cast<short>(writing ? POLLOUT : POLLIN), 0};
    const auto polled = poll(&ready, 1, static_cast<int>(remaining));
    if (polled < 0 && errno == EINTR) continue;
    if (polled <= 0) return false;
    const auto count = writing ? write(descriptor, bytes, size) : read(descriptor, bytes, size);
    if (count < 0 && errno == EINTR) continue;
    if (count <= 0) return false;
    bytes += count; size -= static_cast<size_t>(count);
  }
  return true;
}
void Respond(NSString* status, NSString* code = nil, const std::string* message = nullptr) {
  NSMutableDictionary* reply = [NSMutableDictionary dictionaryWithObject:status forKey:@"status"];
  if (code) reply[@"code"] = code;
  if (message) reply[@"message"] = [[NSString alloc] initWithBytes:message->data() length:message->size() encoding:NSUTF8StringEncoding];
  NSData* data = [NSJSONSerialization dataWithJSONObject:reply options:0 error:nil];
  if (!data || data.length > 131072) return;
  uint32_t count = htonl(static_cast<uint32_t>(data.length));
  Exact(protocol_output, &count, sizeof(count), true);
  Exact(protocol_output, const_cast<void*>(data.bytes), data.length, true);
}
bool OfficialResource(const std::string& url) {
  if (url.empty() || url.size() > 16384) return false;
  for (const char* host : {"verify.proton.me", "verify-api.proton.me", "account.proton.me"}) {
    const auto origin = std::string("https://") + host;
    if (url.compare(0, origin.size(), origin) == 0 &&
        (url.size() == origin.size() || url[origin.size()] == '/' || url[origin.size()] == '?' || url[origin.size()] == '#') &&
        std::all_of(url.begin(), url.end(), [](unsigned char value) { return value >= 0x21 && value <= 0x7e && value != '\\'; })) return true;
  }
  return false;
}
class Verification;
CefRefPtr<Verification> current;
class ActionTask final : public CefTask {
 public:
  explicit ActionTask(std::function<void()> action) : action_(std::move(action)) {}
  void Execute() override { auto action = std::move(action_); action(); }
 private:
  std::function<void()> action_;
  IMPLEMENT_REFCOUNTING(ActionTask);
};
class Completion final : public CefCompletionCallback {
 public:
  explicit Completion(std::function<void()> action) : action_(std::move(action)) {}
  void OnComplete() override { auto action = std::move(action_); action(); }
 private:
  std::function<void()> action_;
  IMPLEMENT_REFCOUNTING(Completion);
};
}

@interface VerificationApplication : NSApplication <CefAppProtocol>
@property(nonatomic) BOOL handlingSendEvent;
@end

namespace {
class Verification final : public CefClient, public CefLifeSpanHandler,
    public CefLoadHandler, public CefRequestHandler, public CefResourceRequestHandler,
    public CefDisplayHandler, public CefPermissionHandler, public CefDownloadHandler {
 public:
  usque::proton::Handoff handoff;
  bool smoke = false;
  Verification() {
    unsigned char random[16];
    if (SecRandomCopyBytes(kSecRandomDefault, sizeof(random), random) == errSecSuccess) {
      constexpr char hex[] = "0123456789abcdef";
      for (auto byte : random) { nonce_ += hex[byte >> 4]; nonce_ += hex[byte & 15]; }
    }
  }
  ~Verification() override { usque::proton::Wipe(nonce_); }
  CefRefPtr<CefLifeSpanHandler> GetLifeSpanHandler() override { return this; }
  CefRefPtr<CefLoadHandler> GetLoadHandler() override { return this; }
  CefRefPtr<CefRequestHandler> GetRequestHandler() override { return this; }
  CefRefPtr<CefDisplayHandler> GetDisplayHandler() override { return this; }
  CefRefPtr<CefPermissionHandler> GetPermissionHandler() override { return this; }
  CefRefPtr<CefDownloadHandler> GetDownloadHandler() override { return this; }
  void Create() {
    CEF_REQUIRE_UI_THREAD();
    if (finished_) { BeginShutdown(); return; }
    if (nonce_.empty()) { Finish(@"error", @"PROTON_VERIFICATION_BRIDGE"); return; }
    expiry_ = std::chrono::steady_clock::now() + std::chrono::milliseconds(
        smoke ? 15000 : std::max<int64_t>(0, handoff.expires - usque::proton::Now()));
    // This executable accepts exactly one handoff. Its global context is an
    // in-memory profile because settings.cache_path is empty, and its root is
    // private to this process. No user/system browser context is shared.
    ConfigureContext(CefRequestContext::GetGlobalContext());
  }
  void ConfigureContext(CefRefPtr<CefRequestContext> context) {
    CEF_REQUIRE_UI_THREAD();
    context_ = context;
    context_initialized_ = true;
    if (finished_) { BeginShutdown(); return; }
    auto proxy = CefDictionaryValue::Create();
    proxy->SetString("mode", "fixed_servers");
    proxy->SetString("server", "http://127.0.0.1:" + std::to_string(smoke ? 9 : handoff.proxy_port));
    proxy->SetString("bypass_list", "<-loopback>");
    auto value = CefValue::Create(); value->SetDictionary(proxy);
    CefString error;
    if (!context_->SetPreference("proxy", value, error)) { Finish(@"error", @"PROTON_VERIFICATION_LOAD"); return; }
    // Let CEF own the native top-level window and its close delegate. Retaining
    // an application-owned NSWindow/content view around an Alloy child can
    // outlive the browser and stall Chromium's profile teardown.
    CefWindowInfo info;
    info.bounds = CefRect(0, 0, 520, 720);
    info.hidden = smoke;
    CefString(&info.window_name) = "Proton verification";
    info.runtime_style = CEF_RUNTIME_STYLE_ALLOY;
    CefBrowserSettings settings;
    settings.local_storage = STATE_DISABLED;
    settings.javascript_access_clipboard = STATE_DISABLED;
    settings.javascript_dom_paste = STATE_DISABLED;
    probe_stage_ = smoke ? 0 : 1;
    load_deadline_ = std::chrono::steady_clock::now() + std::chrono::seconds(smoke ? 15 : 5);
    browser_creation_pending_ = true;
    if (!CefBrowserHost::CreateBrowser(info, this, smoke ? SmokeUrl() : handoff.Probe(), settings, nullptr, context_)) {
      browser_creation_pending_ = false;
      Finish(@"error", @"PROTON_VERIFICATION_LOAD"); return;
    }
    if (!smoke) [NSApp activateIgnoringOtherApps:YES];
    timer_ = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, dispatch_get_main_queue());
    dispatch_source_set_timer(timer_, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), NSEC_PER_SEC, 0);
    dispatch_source_set_event_handler(timer_, ^{
      auto* keep = current.get();
      if (!keep) return;
      if (std::chrono::steady_clock::now() >= keep->expiry_ ||
          (!keep->smoke && usque::proton::Now() >= keep->handoff.expires)) keep->Finish(@"error", @"PROTON_VERIFICATION_EXPIRED");
      else if (!keep->loaded_ && std::chrono::steady_clock::now() >= keep->load_deadline_) keep->Finish(@"error", @"PROTON_VERIFICATION_LOAD");
    });
    dispatch_resume(timer_);
    if (!smoke) {
      fcntl(STDIN_FILENO, F_SETFL, fcntl(STDIN_FILENO, F_GETFL) | O_NONBLOCK);
      input_watch_ = dispatch_source_create(DISPATCH_SOURCE_TYPE_READ, STDIN_FILENO, 0, dispatch_get_main_queue());
      dispatch_source_set_event_handler(input_watch_, ^{
        unsigned char byte;
        const auto count = read(STDIN_FILENO, &byte, 1);
        if (current && (count >= 0 || (errno != EINTR && errno != EAGAIN))) current->Finish();
      });
      dispatch_resume(input_watch_);
    }
  }
  void Finish(NSString* status = @"cancelled", NSString* code = nil,
               const std::string* message = nullptr, bool close_browser = true) {
    CEF_REQUIRE_UI_THREAD();
    if (finished_.exchange(true)) return;
    if (timer_) { dispatch_source_cancel(timer_); timer_ = nil; }
    if (input_watch_) { dispatch_source_cancel(input_watch_); input_watch_ = nil; }
    Respond(status, code, message);
    if (!close_browser) return;
    // A query callback is still inside CEF's IPC dispatch. Begin teardown on
    // CEF's sequenced UI runner after the result callback has unwound.
    CefRefPtr<Verification> keep = this;
    CefPostTask(TID_UI, new ActionTask([keep] { keep->BeginShutdown(); }));
  }
  void BeginShutdown() {
    CEF_REQUIRE_UI_THREAD();
    if (context_ && !context_initialized_) return;
    if (!shutdown_started_) {
      shutdown_started_ = true;
      if (browser_) browser_->GetHost()->CloseBrowser(true);
    }
    if (browser_ || browser_creation_pending_ || network_shutdown_posted_) return;
    browser_closed_ = true;
    network_shutdown_posted_ = true;
    CefRefPtr<Verification> keep = this;
    CefPostTask(TID_UI, new ActionTask([keep] {
      // CloseAllConnections is defined for use after browser/CEF objects have
      // been released. Run after OnBeforeClose has returned, including the
      // no-browser failure path.
      if (keep->context_) {
        keep->context_->ClearHttpAuthCredentials(new Completion([keep] {
          keep->context_->CloseAllConnections(new Completion([keep] {
            keep->connections_closed_ = true;
            keep->MaybeQuit();
          }));
        }));
      } else {
        keep->connections_closed_ = true;
        keep->MaybeQuit();
      }
    }));
  }
  void MaybeQuit() {
    CEF_REQUIRE_UI_THREAD();
    if (!shutdown_started_ || !browser_closed_ || !connections_closed_ || quit_posted_) return;
    quit_posted_ = true;
    context_ = nullptr;
    // Two sequenced turns allow the just-completed browser/context callbacks
    // and their posted destruction tasks to release their remaining objects.
    CefPostTask(TID_UI, new ActionTask([] {
      CefPostTask(TID_UI, new ActionTask([] { CefQuitMessageLoop(); }));
    }));
  }
  bool AcceptMessage(CefRefPtr<CefBrowser> browser, CefRefPtr<CefFrame> frame,
                     const CefString& input) {
    if (finished_ || !browser_ || !browser_->IsSame(browser) || !frame ||
        !frame->IsMain() || input.length() > 32768) return false;
    auto text = input.ToString(); usque::proton::Erase erase{text};
    if (smoke && frame->GetURL() == SmokeUrl() && text == "sandbox-renderer-ready") {
      std::string message = "{\"sandbox_renderer\":\"ready\"}";
      Finish(@"success", nil, &message); return true;
    }
    if (!handoff.Matches(frame->GetURL().ToString(), Sha) || text.size() > 32768) return false;
    usque::proton::Json wrapper;
    if (!usque::proton::Parser(text).Parse(wrapper) || wrapper.object.size() != 2) return false;
    const auto nonce = usque::proton::Field(wrapper, "nonce", usque::proton::Json::kString);
    const auto message = usque::proton::Field(wrapper, "message", usque::proton::Json::kString);
    if (!nonce || nonce->text != nonce_ || !message || !handoff.Accept(message->text)) return false;
    Finish(@"success", nil, &message->text); return true;
  }
  bool OnProcessMessageReceived(CefRefPtr<CefBrowser> browser, CefRefPtr<CefFrame> frame,
                                CefProcessId source, CefRefPtr<CefProcessMessage> message) override {
    if (source != PID_RENDERER || message->GetName() != "usque.proton.verification.result.v1") return false;
    const auto arguments = message->GetArgumentList();
    if (arguments->GetSize() != 1 || arguments->GetType(0) != VTYPE_STRING) return false;
    return AcceptMessage(browser, frame, arguments->GetString(0));
  }
  void OnAfterCreated(CefRefPtr<CefBrowser> browser) override {
    browser_creation_pending_ = false;
    browser_ = browser;
    if (finished_) browser_->GetHost()->CloseBrowser(true);
  }
  void OnBeforeClose(CefRefPtr<CefBrowser> browser) override {
    if (!finished_) Finish(@"cancelled", nil, nullptr, false);
    browser_ = nullptr;
    browser_closed_ = true;
    BeginShutdown();
    MaybeQuit();
  }
  bool OnBeforePopup(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>, int, const CefString&,
                     const CefString&, WindowOpenDisposition, bool, const CefPopupFeatures&,
                     CefWindowInfo&, CefRefPtr<CefClient>&, CefBrowserSettings&,
                     CefRefPtr<CefDictionaryValue>&, bool*) override { return true; }
  bool OnBeforeBrowse(CefRefPtr<CefBrowser> browser, CefRefPtr<CefFrame> frame,
                      CefRefPtr<CefRequest> request, bool, bool) override {
    const auto url = request->GetURL().ToString();
    if (finished_) return true;
    if (smoke) return url != SmokeUrl();
    if (probe_stage_ == 2 && url == handoff.Ready(Sha)) {
      probe_stage_ = 0; load_deadline_ = std::chrono::steady_clock::now() + std::chrono::seconds(15);
      frame->LoadURL(handoff.url); return true;
    }
    const bool allowed = probe_stage_ ? handoff.ProbeUrl(url) :
        frame->IsMain() ? handoff.Matches(url, Sha) : handoff.NavigationAllowed(url, Sha);
    if (!allowed) Finish(@"error", @"PROTON_VERIFICATION_NAVIGATION");
    return !allowed;
  }
  void OnLoadStart(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, TransitionType) override {
    if (!frame->IsMain()) return;
    if (smoke) { frame->ExecuteJavaScript("window.cefQuery({request:'sandbox-renderer-ready'});", SmokeUrl(), 0); return; }
    if (!probe_stage_ && handoff.Matches(frame->GetURL().ToString(), Sha)) {
      auto script = usque::proton::Script(handoff,
          "window.cefQuery({request:JSON.stringify({nonce:'" + nonce_ + "',message}),persistent:false});");
      frame->ExecuteJavaScript(script, frame->GetURL(), 0); usque::proton::Wipe(script);
    }
  }
  void OnLoadEnd(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, int) override {
    if (frame->IsMain() && (smoke || (!probe_stage_ && handoff.Matches(frame->GetURL().ToString(), Sha)))) loaded_ = true;
  }
  void OnLoadError(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame> frame, ErrorCode error,
                   const CefString&, const CefString& failed_url) override {
    if (finished_ || error == ERR_ABORTED) return;
    if (frame->IsMain() && probe_stage_ == 1 && failed_url == handoff.ProbeHttps()) {
      probe_stage_ = 2; frame->LoadURL(handoff.Probe("ready")); return;
    }
    Finish(@"error", @"PROTON_VERIFICATION_LOAD");
  }
  CefRefPtr<CefResourceRequestHandler> GetResourceRequestHandler(CefRefPtr<CefBrowser>,
      CefRefPtr<CefFrame>, CefRefPtr<CefRequest>, bool, bool download, const CefString&, bool&) override {
    if (download) ErrorLater(@"PROTON_VERIFICATION_DOWNLOAD"); return this;
  }
  void OnRenderProcessTerminated(CefRefPtr<CefBrowser> browser, TerminationStatus,
      int, const CefString&) override {
    Finish(@"error", @"PROTON_VERIFICATION_HOST_STOPPED");
  }
  ReturnValue OnBeforeResourceLoad(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>,
      CefRefPtr<CefRequest> request, CefRefPtr<CefCallback>) override {
    const auto url = request->GetURL().ToString();
    return !finished_ && (smoke ? url == SmokeUrl() : probe_stage_ ? handoff.ProbeUrl(url) : OfficialResource(url)) ? RV_CONTINUE : RV_CANCEL;
  }
  void OnResourceLoadComplete(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>,
      CefRefPtr<CefRequest> request, CefRefPtr<CefResponse>, URLRequestStatus status, int64_t) override {
    // The deliberately failed CONNECT probe can complete on the IO thread
    // after the UI thread has accepted Ready and advanced to the challenge.
    // Identify that expected failure by its immutable, session-bound URL;
    // reading the current stage here would turn callback ordering into a
    // spurious failure of an already established verification page.
    if (!finished_ && status == UR_FAILED && request->GetURL() != handoff.ProbeHttps()) ErrorLater(@"PROTON_VERIFICATION_LOAD");
  }
  bool GetAuthCredentials(CefRefPtr<CefBrowser>, const CefString&, bool is_proxy,
      const CefString& host, int port, const CefString& realm, const CefString& scheme,
      CefRefPtr<CefAuthCallback> callback) override {
    if (!finished_ && is_proxy && host == "127.0.0.1" && port == static_cast<int>(handoff.proxy_port) &&
        realm == handoff.proxy_nonce && scheme == "basic") {
      callback->Continue(handoff.proxy_username, handoff.proxy_password); return true;
    }
    callback->Cancel(); ErrorLater(@"PROTON_VERIFICATION_LOAD"); return true;
  }
  bool OnCertificateError(CefRefPtr<CefBrowser>, cef_errorcode_t, const CefString&,
      CefRefPtr<CefSSLInfo>, CefRefPtr<CefCallback>) override { Finish(@"error", @"PROTON_VERIFICATION_TLS"); return false; }
  bool OnConsoleMessage(CefRefPtr<CefBrowser>, cef_log_severity_t, const CefString&,
      const CefString&, int) override { return true; }
  bool OnRequestMediaAccessPermission(CefRefPtr<CefBrowser>, CefRefPtr<CefFrame>,
      const CefString&, uint32_t, CefRefPtr<CefMediaAccessCallback> callback) override { callback->Cancel(); return true; }
  bool OnShowPermissionPrompt(CefRefPtr<CefBrowser>, uint64_t, const CefString&, uint32_t,
      CefRefPtr<CefPermissionPromptCallback> callback) override { callback->Continue(CEF_PERMISSION_RESULT_DENY); return true; }
  bool OnBeforeDownload(CefRefPtr<CefBrowser>, CefRefPtr<CefDownloadItem>, const CefString&,
      CefRefPtr<CefBeforeDownloadCallback>) override { Finish(@"error", @"PROTON_VERIFICATION_DOWNLOAD"); return false; }
  void ErrorLater(NSString* code) {
    CefRefPtr<Verification> keep = this;
    CefPostTask(TID_UI, new ActionTask([keep, code] { keep->Finish(@"error", code); }));
  }
 private:
  std::string SmokeUrl() const { return "data:text/html,Usque%20isolated%20renderer%20smoke"; }
  std::string nonce_;
  std::atomic<bool> finished_{false};
  std::atomic<unsigned> probe_stage_{0};
  bool loaded_ = false;
  bool browser_creation_pending_ = false;
  bool context_initialized_ = false;
  bool shutdown_started_ = false;
  bool network_shutdown_posted_ = false;
  bool browser_closed_ = false;
  bool connections_closed_ = false;
  bool quit_posted_ = false;
  std::chrono::steady_clock::time_point load_deadline_;
  std::chrono::steady_clock::time_point expiry_;
  CefRefPtr<CefBrowser> browser_;
  CefRefPtr<CefRequestContext> context_;
  dispatch_source_t timer_ = nil;
  dispatch_source_t input_watch_ = nil;
  IMPLEMENT_REFCOUNTING(Verification);
};

class BrowserApp final : public CefApp, public CefBrowserProcessHandler {
 public:
  explicit BrowserApp(CefRefPtr<Verification> client) : client_(client) {}
  CefRefPtr<CefBrowserProcessHandler> GetBrowserProcessHandler() override { return this; }
  void OnContextInitialized() override { client_->Create(); }
  void OnBeforeCommandLineProcessing(const CefString&, CefRefPtr<CefCommandLine> command) override {
    // This one-shot browser has no persistent profile or passwords to protect.
    // Avoid Chromium Safe Storage accessing the user's Keychain (or prompting
    // to create one when the isolated tests run with a temporary HOME). Mock
    // encryption must never be used with a persistent browser profile.
    command->AppendSwitch("use-mock-keychain");
    for (const char* flag : {"disable-background-networking", "disable-component-update", "disable-sync",
                            "disable-default-apps", "disable-breakpad", "no-first-run", "disable-quic", "disable-databases"}) command->AppendSwitch(flag);
    command->AppendSwitchWithValue("force-webrtc-ip-handling-policy", "disable_non_proxied_udp");
  }
  void OnBeforeChildProcessLaunch(CefRefPtr<CefCommandLine> command) override {
    // Apply the same storage policy to the out-of-process network service.
    command->AppendSwitch("use-mock-keychain");
  }
 private:
  CefRefPtr<Verification> client_;
  IMPLEMENT_REFCOUNTING(BrowserApp);
};
}

@implementation VerificationApplication
- (BOOL)isHandlingSendEvent { return _handlingSendEvent; }
- (void)sendEvent:(NSEvent*)event { CefScopedSendingEvent scoped; [super sendEvent:event]; }
- (void)terminate:(id)sender { if (current) current->Finish(); }
@end

int main(int argc, char** argv) {
  const auto launch = usque::proton::macos::ParseLaunchOptions(argc, argv);
  const bool smoke = launch.mode == usque::proton::macos::LaunchMode::kSmoke;
  signal(SIGPIPE, SIG_IGN);
  protocol_output = dup(STDOUT_FILENO);
  if (protocol_output < 0 || fcntl(protocol_output, F_SETFD, FD_CLOEXEC) != 0) return 1;
  const int quiet = open("/dev/null", O_WRONLY | O_CLOEXEC);
  if (quiet < 0) return 1;
  dup2(quiet, STDOUT_FILENO);
  if (!smoke) dup2(quiet, STDERR_FILENO);
  close(quiet);
  // The framework must remain loaded while the outer Cocoa autorelease pool
  // drains, including objects created by CEF initialization and shutdown.
  CefScopedLibraryLoader library;
  @autoreleasepool {
    if (launch.mode == usque::proton::macos::LaunchMode::kInteractiveDisabled) {
      Respond(@"error", @"PROTON_VERIFICATION_INTERACTIVE_DISABLED"); return 1;
    }
    if (launch.mode == usque::proton::macos::LaunchMode::kInvalid) {
      Respond(@"error", @"PROTON_VERIFICATION_INVALID"); return 1;
    }
    CefRefPtr<Verification> client = new Verification;
    client->smoke = smoke;
    if (!smoke) {
      uint32_t length = 0;
      if (!Exact(STDIN_FILENO, &length, 4, false) || (length = ntohl(length)) == 0 || length > 65536) {
        Respond(@"error", @"PROTON_VERIFICATION_INVALID"); return 1;
      }
      std::string input(length, '\0'); usque::proton::Erase erase{input};
      if (!Exact(STDIN_FILENO, input.data(), length, false) || !usque::proton::Parse(input, client->handoff, Sha) || client->handoff.version != 2) {
        Respond(@"error", @"PROTON_VERIFICATION_INVALID"); return 1;
      }
      if (!client->handoff.route_epoch) {
        auto message = client->handoff.NetworkFailure(); Respond(@"success", nil, &message); return 0;
      }
      if (!client->handoff.proxy_port) { Respond(@"error", @"PROTON_VERIFICATION_INVALID"); return 1; }
    }
    // Automation uses this path even for an accidentally valid proxy handoff.
    // Refuse before CEF loading, NSApplication creation or any browser work.
    if (launch.no_interactive) {
      Respond(@"error", @"PROTON_VERIFICATION_INTERACTIVE_DISABLED"); return 1;
    }
    if (!library.LoadInMain()) { Respond(@"error", @"PROTON_VERIFICATION_UNAVAILABLE"); return 1; }
    [VerificationApplication sharedApplication];
    if (![NSApp isKindOfClass:[VerificationApplication class]]) return 1;
    CefMainArgs arguments(argc, argv);
    CefSettings settings;
    settings.no_sandbox = false;
    settings.log_severity = LOGSEVERITY_DISABLE;
    settings.command_line_args_disabled = true;
    settings.persist_session_cookies = false;
    CefString(&settings.cache_path) = "";
    // CEF requires a root for process metadata even when the request context
    // itself is in memory. Isolate it from every installed user browser and
    // from concurrent verification processes, then remove it after shutdown.
    // Keep the root short: macOS Unix-domain sockets have a bounded pathname,
    // and NSTemporaryDirectory plus a UUID can exhaust it before CEF's suffix.
    // mkdtemp atomically creates a private 0700 directory without following an
    // existing pathname. This directory never contains the handoff itself.
    char root_template[] = "/private/tmp/usque-cef-XXXXXX";
    const auto root = mkdtemp(root_template);
    if (!root) {
      Respond(@"error", @"PROTON_VERIFICATION_UNAVAILABLE"); return 1;
    }
    NSString* private_root = [NSString stringWithUTF8String:root];
    CefString(&settings.root_cache_path) = std::string(private_root.UTF8String);
    CefString(&settings.browser_subprocess_path) = std::string([NSBundle.mainBundle.bundlePath UTF8String]) +
        "/Contents/Frameworks/UsqueProtonVerification Helper.app/Contents/MacOS/UsqueProtonVerification Helper";
    CefRefPtr<BrowserApp> app = new BrowserApp(client);
    if (!CefInitialize(arguments, settings, app, nullptr)) {
      [NSFileManager.defaultManager removeItemAtPath:private_root error:nil];
      Respond(@"error", @"PROTON_VERIFICATION_UNAVAILABLE"); return 1;
    }
    current = client;
    @autoreleasepool { CefRunMessageLoop(); }
    current = nullptr; app = nullptr; client = nullptr;
    CefShutdown();
    [NSFileManager.defaultManager removeItemAtPath:private_root error:nil];
  }
  close(protocol_output);
  return 0;
}
