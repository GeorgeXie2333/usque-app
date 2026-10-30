#include "include/cef_app.h"
#include "include/cef_sandbox_mac.h"
#include "include/wrapper/cef_library_loader.h"
#include "include/cef_frame.h"
#include "include/cef_process_message.h"
#include "include/cef_v8.h"
#include <cstring>

class OneShotBridge final : public CefV8Handler {
 public:
  bool Execute(const CefString& name, CefRefPtr<CefV8Value>,
      const CefV8ValueList& arguments, CefRefPtr<CefV8Value>& result,
      CefString& exception) override {
    if (used_ || name != "cefQuery" || arguments.size() != 1 || !arguments[0]->IsObject()) return false;
    const auto options = arguments[0];
    std::vector<CefString> keys;
    if (!options->GetKeys(keys) || keys.empty() || keys.size() > 2) return false;
    for (const auto& key : keys) if (key != "request" && key != "persistent") return false;
    const auto persistent = options->GetValue("persistent");
    if (options->HasValue("persistent") && (!persistent->IsBool() || persistent->GetBoolValue())) return false;
    const auto request = options->GetValue("request");
    if (!request || !request->IsString() || request->GetStringValue().length() > 32768) return false;
    const auto text = request->GetStringValue();
    if (text.ToString().size() > 32768) return false;
    const auto context = CefV8Context::GetCurrentContext();
    const auto frame = context ? context->GetFrame() : nullptr;
    if (!frame || !frame->IsMain()) return false;
    used_ = true;
    const auto message = CefProcessMessage::Create("usque.proton.verification.result.v1");
    message->GetArgumentList()->SetString(0, text);
    frame->SendProcessMessage(PID_BROWSER, message);
    result = CefV8Value::CreateBool(true);
    return true;
  }
 private:
  bool used_ = false;
  IMPLEMENT_REFCOUNTING(OneShotBridge);
};

// CEF's sandbox must initialize before loading the framework or executing any
// renderer code. A failure terminates the child; there is no unsandboxed retry.
class Renderer final : public CefApp, public CefRenderProcessHandler {
 public:
  CefRefPtr<CefRenderProcessHandler> GetRenderProcessHandler() override { return this; }
  void OnContextCreated(CefRefPtr<CefBrowser> browser, CefRefPtr<CefFrame> frame,
                        CefRefPtr<CefV8Context> context) override {
    if (!frame->IsMain()) return;
    context->GetGlobal()->SetValue("cefQuery", CefV8Value::CreateFunction("cefQuery", new OneShotBridge),
        static_cast<cef_v8_propertyattribute_t>(V8_PROPERTY_ATTRIBUTE_READONLY | V8_PROPERTY_ATTRIBUTE_DONTDELETE));
  }
 private:
  IMPLEMENT_REFCOUNTING(Renderer);
};

int main(int argc, char** argv) {
  CefScopedSandboxContext sandbox;
  if (!sandbox.Initialize(argc, argv)) return 1;
  CefScopedLibraryLoader library;
  if (!library.LoadInHelper()) return 1;
  CefMainArgs arguments(argc, argv);
  CefRefPtr<CefApp> app;
  for (int index = 1; index < argc; ++index) {
    if (std::strcmp(argv[index], "--type=renderer") == 0) {
      app = new Renderer;
      break;
    }
  }
  return CefExecuteProcess(arguments, app, nullptr);
}
