#include "proton_verification.h"
#include "../../native/proton_verification.h"

#include <WebView2.h>
#include <WebView2EnvironmentOptions.h>
#include <bcrypt.h>
#include <sddl.h>
#include <wrl.h>

#include <filesystem>
#include <limits>

using Microsoft::WRL::Callback;
using Microsoft::WRL::ComPtr;
namespace {
std::wstring Wide(const std::string& text) {
  if (text.empty() || text.size()>65536) return {};
  int count=MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,text.data(),static_cast<int>(text.size()),nullptr,0);
  if (!count) return {};
  std::wstring result(count,L'\0');
  if (!MultiByteToWideChar(CP_UTF8,MB_ERR_INVALID_CHARS,text.data(),static_cast<int>(text.size()),result.data(),count)) return {};
  return result;
}
std::string Utf8(const wchar_t* text) {
  if (!text) return {};
  const size_t length=wcsnlen_s(text,65537);
  if (length>65536) return {};
  int count=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text,static_cast<int>(length),nullptr,0,nullptr,nullptr);
  if (!count) return {};
  std::string result(count,'\0');
  if (!WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,text,static_cast<int>(length),result.data(),count,nullptr,nullptr)) return {};
  return result;
}
std::string Hex(const unsigned char* bytes,size_t size) {
  constexpr char hex[]="0123456789abcdef";
  std::string result;
  for (size_t i=0;i<size;++i) { result+=hex[bytes[i]>>4]; result+=hex[bytes[i]&15]; }
  return result;
}
std::string Sha(const std::string& value) {
  BCRYPT_ALG_HANDLE provider=nullptr;
  BCRYPT_HASH_HANDLE hash=nullptr;
  unsigned char output[32]{};
  bool ok=BCryptOpenAlgorithmProvider(&provider,BCRYPT_SHA256_ALGORITHM,nullptr,0)>=0 &&
    BCryptCreateHash(provider,&hash,nullptr,0,nullptr,0,0)>=0 &&
    BCryptHashData(hash,reinterpret_cast<PUCHAR>(const_cast<char*>(value.data())),static_cast<ULONG>(value.size()),0)>=0 &&
    BCryptFinishHash(hash,output,sizeof(output),0)>=0;
  if (hash) BCryptDestroyHash(hash);
  if (provider) BCryptCloseAlgorithmProvider(provider,0);
  return ok ? Hex(output,sizeof(output)) : "";
}
// Only this user and SYSTEM can access the fresh, random profile directory.
std::wstring PrivateDirectory() {
  HANDLE token=nullptr;
  if (!OpenProcessToken(GetCurrentProcess(),TOKEN_QUERY,&token)) return {};
  DWORD count=0;
  GetTokenInformation(token,TokenUser,nullptr,0,&count);
  std::vector<unsigned char> storage(count);
  bool ok=count && GetTokenInformation(token,TokenUser,storage.data(),count,&count);
  CloseHandle(token);
  if (!ok) return {};
  LPWSTR sid=nullptr;
  if (!ConvertSidToStringSidW(reinterpret_cast<TOKEN_USER*>(storage.data())->User.Sid,&sid)) return {};
  const std::wstring sddl=L"D:P(A;;FA;;;SY)(A;;FA;;;"+std::wstring(sid)+L")";
  LocalFree(sid);
  PSECURITY_DESCRIPTOR descriptor=nullptr;
  if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl.c_str(),SDDL_REVISION_1,&descriptor,nullptr)) return {};
  wchar_t base[MAX_PATH+1]{};
  unsigned char random[16];
  std::wstring directory;
  if (GetTempPathW(MAX_PATH,base) && BCryptGenRandom(nullptr,random,sizeof(random),BCRYPT_USE_SYSTEM_PREFERRED_RNG)>=0) {
    directory=std::wstring(base)+L"usque-proton-"+Wide(Hex(random,sizeof(random)));
    SECURITY_ATTRIBUTES attributes{sizeof(attributes),descriptor,FALSE};
    if (!CreateDirectoryW(directory.c_str(),&attributes)) directory.clear();
  }
  LocalFree(descriptor);
  return directory;
}
}  // namespace
struct ProtonVerification::Session : std::enable_shared_from_this<Session> {
  usque::proton::Handoff handoff;
  std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result;
  HWND window=nullptr;
  ComPtr<ICoreWebView2Environment> environment;
  ComPtr<ICoreWebView2Controller> controller;
  ComPtr<ICoreWebView2> webview;
  std::wstring directory;
  std::string nonce;
  std::string pending_message;
  static constexpr UINT kComplete=WM_APP+0x3d5;
  ULONGLONG deadline=0;
  bool finished=false;
  bool Active() const { return !finished && usque::proton::Now()<handoff.expires && GetTickCount64()<deadline; }
  ~Session() {
    usque::proton::Wipe(pending_message);
    if (controller) controller->Close();
    webview.Reset(); controller.Reset(); environment.Reset();
    // InPrivate is mandatory: challenge cookies/storage are never persisted.
    // Directory removal is best-effort because the runtime may release handles
    // asynchronously; the directory was ACL-restricted before process launch.
    if (!directory.empty()) { std::error_code error; std::filesystem::remove_all(directory,error); }
  }
  void Finish(bool unavailable=false,const std::string* message=nullptr) {
    if (finished) return;
    auto keep=shared_from_this();
    finished=true;
    usque::proton::Wipe(pending_message);
    if (window) { HWND target=window; window=nullptr; KillTimer(target,1); DestroyWindow(target); }
    if (controller) controller->Close();
    webview.Reset(); controller.Reset(); environment.Reset();
    if (result) {
      if (message) {
        flutter::EncodableValue value(std::vector<uint8_t>(message->begin(),message->end()));
        result->Success(value);
        auto& bytes=std::get<std::vector<uint8_t>>(value);
        SecureZeroMemory(bytes.data(),bytes.size());
      } else if (unavailable) result->Error("PROTON_VERIFICATION_UNAVAILABLE","The official verification window is unavailable.");
      else result->Success();
      result.reset();
    }
  }
  static LRESULT CALLBACK Proc(HWND window,UINT message,WPARAM wparam,LPARAM lparam) {
    auto* self=reinterpret_cast<Session*>(GetWindowLongPtrW(window,GWLP_USERDATA));
    if (message==WM_NCCREATE) {
      self=static_cast<Session*>(reinterpret_cast<CREATESTRUCTW*>(lparam)->lpCreateParams);
      SetWindowLongPtrW(window,GWLP_USERDATA,reinterpret_cast<LONG_PTR>(self));
    }
    if (self) {
      if (message==kComplete) {
        std::string ready=std::move(self->pending_message);
        usque::proton::Erase erase{ready};
        if (ready.empty()) return 0;
        LPWSTR current=nullptr;
        const bool bound=self->webview && SUCCEEDED(self->webview->get_Source(&current)) && self->Matches(current);
        if (current) SecureZeroMemory(current,wcslen(current)*sizeof(wchar_t));
        CoTaskMemFree(current);
        if (bound) self->Finish(false,&ready); else self->Finish();
        return 0;
      }
      if (message==WM_CLOSE) { self->Finish(); return 0; }
      if (message==WM_TIMER && wparam==1 && !self->Active()) { self->Finish(); return 0; }
      if (message==WM_SIZE && self->controller) { RECT bounds; GetClientRect(window,&bounds); self->controller->put_Bounds(bounds); return 0; }
      if (message==WM_NCDESTROY) SetWindowLongPtrW(window,GWLP_USERDATA,0);
    }
    return DefWindowProcW(window,message,wparam,lparam);
  }
  bool Matches(const wchar_t* url) {
    std::string value=Utf8(url);
    usque::proton::Erase erase{value};
    return Active() && handoff.Matches(value,Sha);
  }
  void Setup() {
    if (!Active()) { Finish(); return; }
    ComPtr<ICoreWebView2Settings> settings;
    ComPtr<ICoreWebView2Settings4> settings4;
    ComPtr<ICoreWebView2_4> webview4;
    if (FAILED(webview->get_Settings(&settings)) || FAILED(settings.As(&settings4)) || FAILED(webview.As(&webview4)) ||
        FAILED(settings->put_IsScriptEnabled(TRUE)) || FAILED(settings->put_IsWebMessageEnabled(FALSE)) ||
        FAILED(settings->put_AreDevToolsEnabled(FALSE)) || FAILED(settings->put_AreDefaultContextMenusEnabled(FALSE)) ||
        FAILED(settings->put_AreDefaultScriptDialogsEnabled(FALSE)) || FAILED(settings->put_IsStatusBarEnabled(FALSE)) || FAILED(settings->put_IsBuiltInErrorPageEnabled(FALSE)) ||
        FAILED(settings4->put_AreBrowserAcceleratorKeysEnabled(FALSE)) ||
        FAILED(settings->put_AreHostObjectsAllowed(FALSE)) || FAILED(settings4->put_IsGeneralAutofillEnabled(FALSE)) ||
        FAILED(settings4->put_IsPasswordAutosaveEnabled(FALSE))) { Finish(true); return; }
    const std::weak_ptr<Session> weak=shared_from_this();
    EventRegistrationToken token{};
    if (FAILED(webview->add_NavigationStarting(Callback<ICoreWebView2NavigationStartingEventHandler>([weak](ICoreWebView2*,ICoreWebView2NavigationStartingEventArgs* args)->HRESULT {
      if (auto self=weak.lock()) {
        LPWSTR url=nullptr;
        bool allowed=SUCCEEDED(args->get_Uri(&url)) && self->Matches(url);
        if (url) SecureZeroMemory(url,wcslen(url)*sizeof(wchar_t));
        CoTaskMemFree(url);
        if (!allowed) { args->put_Cancel(TRUE); self->Finish(); }
      } else args->put_Cancel(TRUE);
      return S_OK;
    }).Get(),&token)) || FAILED(webview->add_NewWindowRequested(Callback<ICoreWebView2NewWindowRequestedEventHandler>([](ICoreWebView2*,ICoreWebView2NewWindowRequestedEventArgs* args)->HRESULT { args->put_Handled(TRUE); return S_OK; }).Get(),&token)) ||
        FAILED(webview->add_PermissionRequested(Callback<ICoreWebView2PermissionRequestedEventHandler>([](ICoreWebView2*,ICoreWebView2PermissionRequestedEventArgs* args)->HRESULT { args->put_State(COREWEBVIEW2_PERMISSION_STATE_DENY); return S_OK; }).Get(),&token)) ||
        FAILED(webview4->add_DownloadStarting(Callback<ICoreWebView2DownloadStartingEventHandler>([](ICoreWebView2*,ICoreWebView2DownloadStartingEventArgs* args)->HRESULT { args->put_Cancel(TRUE); return S_OK; }).Get(),&token)) ||
        FAILED(webview->add_ScriptDialogOpening(Callback<ICoreWebView2ScriptDialogOpeningEventHandler>([weak](ICoreWebView2*,ICoreWebView2ScriptDialogOpeningEventArgs* args)->HRESULT {
          COREWEBVIEW2_SCRIPT_DIALOG_KIND kind;
          if (FAILED(args->get_Kind(&kind)) || kind!=COREWEBVIEW2_SCRIPT_DIALOG_KIND_ALERT) return S_OK;
          // Default dialogs are disabled: acknowledge without displaying or
          // logging their text. Only our bound nonce envelope can complete.
          args->Accept();
          auto self=weak.lock();
          if (!self || !self->Active() || !self->pending_message.empty()) return S_OK;
          LPWSTR source=nullptr,current=nullptr,text=nullptr;
          const bool bound=SUCCEEDED(args->get_Uri(&source)) && self->Matches(source) && SUCCEEDED(self->webview->get_Source(&current)) && self->Matches(current);
          const bool received=bound && SUCCEEDED(args->get_Message(&text));
          std::string message=received ? Utf8(text) : "";
          if (text) SecureZeroMemory(text,wcslen(text)*sizeof(wchar_t));
          if (source) SecureZeroMemory(source,wcslen(source)*sizeof(wchar_t));
          if (current) SecureZeroMemory(current,wcslen(current)*sizeof(wchar_t));
          CoTaskMemFree(source); CoTaskMemFree(current); CoTaskMemFree(text);
          usque::proton::Json wrapper;
          if (message.size()<=32768 && usque::proton::Parser(message).Parse(wrapper) && wrapper.kind==usque::proton::Json::kObject && wrapper.object.size()==2) {
            const auto nonce=usque::proton::Field(wrapper,"nonce",usque::proton::Json::kString), body=usque::proton::Field(wrapper,"message",usque::proton::Json::kString);
            unsigned different=nonce && nonce->text.size()==self->nonce.size() ? 0 : 1;
            if (nonce) for (size_t i=0;i<self->nonce.size();++i) different |= static_cast<unsigned char>(self->nonce[i]) ^ static_cast<unsigned char>(i<nonce->text.size()?nonce->text[i]:0);
            if (!different && body && self->handoff.Accept(body->text)) {
              self->pending_message=body->text;
              // Close the browser only after the synchronous alert callback
              // has returned to WebView2, avoiding dialog/event reentrancy.
              if (!PostMessageW(self->window,kComplete,0,0)) self->Finish(true);
            }
          }
          usque::proton::Wipe(message);
          return S_OK;
        }).Get(),&token))) { Finish(true); return; }
    // Default runtime TLS validation is retained; no certificate-error override.
    const auto script=Wide(usque::proton::CaptchaScript(handoff,nonce));
    HRESULT status=webview->AddScriptToExecuteOnDocumentCreated(script.c_str(),Callback<ICoreWebView2AddScriptToExecuteOnDocumentCreatedCompletedHandler>([weak](HRESULT error,LPCWSTR)->HRESULT {
      auto self=weak.lock();
      if (!self) return S_OK;
      if (FAILED(error) || !self->Active()) { self->Finish(true); return S_OK; }
      RECT bounds; GetClientRect(self->window,&bounds);
      self->controller->put_Bounds(bounds);
      self->controller->put_IsVisible(TRUE);
      if (FAILED(self->webview->Navigate(Wide(self->handoff.url).c_str()))) self->Finish(true);
      return S_OK;
    }).Get());
    if (FAILED(status)) Finish(true);
  }
  void Start(HWND parent) {
    const int64_t remaining=handoff.expires-usque::proton::Now();
    if (remaining<=0) { Finish(); return; }
    deadline=GetTickCount64()+static_cast<ULONGLONG>(remaining);
    unsigned char random[16];
    if (BCryptGenRandom(nullptr,random,sizeof(random),BCRYPT_USE_SYSTEM_PREFERRED_RNG)<0 || (directory=PrivateDirectory()).empty()) { Finish(true); return; }
    nonce=Hex(random,sizeof(random));
    WNDCLASSW type{};
    type.lpfnWndProc=Proc; type.hInstance=GetModuleHandleW(nullptr); type.lpszClassName=L"UsqueProtonVerification";
    type.hCursor=LoadCursorW(nullptr,IDC_ARROW);
    if (!RegisterClassW(&type) && GetLastError()!=ERROR_CLASS_ALREADY_EXISTS) { Finish(true); return; }
    window=CreateWindowExW(WS_EX_DLGMODALFRAME,type.lpszClassName,L"Usque \u00b7 Proton VPN",WS_OVERLAPPEDWINDOW|WS_VISIBLE,CW_USEDEFAULT,CW_USEDEFAULT,560,740,parent,nullptr,type.hInstance,this);
    if (!window || !SetTimer(window,1,100,nullptr)) { Finish(true); return; }
    const std::weak_ptr<Session> weak=shared_from_this();
    // InPrivate controller options are mandatory. An old or missing installed
    // Evergreen runtime returns unavailable; the app never installs a browser.
    auto options=Microsoft::WRL::Make<CoreWebView2EnvironmentOptions>();
    if (!options || FAILED(options->put_AllowSingleSignOnUsingOSPrimaryAccount(FALSE)) ||
        FAILED(options->put_ExclusiveUserDataFolderAccess(TRUE)) ||
        FAILED(options->put_IsCustomCrashReportingEnabled(TRUE)) ||
        FAILED(options->put_ReleaseChannels(COREWEBVIEW2_RELEASE_CHANNELS_STABLE))) { Finish(true); return; }
    const HRESULT status=CreateCoreWebView2EnvironmentWithOptions(nullptr,directory.c_str(),options.Get(),
      Callback<ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler>([weak](HRESULT error,ICoreWebView2Environment* environment)->HRESULT {
        auto self=weak.lock(); if (!self) return S_OK;
        if (FAILED(error) || !environment || !self->Active()) { self->Finish(true); return S_OK; }
        self->environment=environment;
        ComPtr<ICoreWebView2Environment10> modern;
        ComPtr<ICoreWebView2ControllerOptions> options;
        if (FAILED(environment->QueryInterface(IID_PPV_ARGS(&modern))) || FAILED(modern->CreateCoreWebView2ControllerOptions(&options)) ||
            FAILED(options->put_IsInPrivateModeEnabled(TRUE))) { self->Finish(true); return S_OK; }
        HRESULT status=modern->CreateCoreWebView2ControllerWithOptions(self->window,options.Get(),
          Callback<ICoreWebView2CreateCoreWebView2ControllerCompletedHandler>([weak](HRESULT error,ICoreWebView2Controller* controller)->HRESULT {
            auto live=weak.lock(); if (!live) { if (controller) controller->Close(); return S_OK; }
            if (FAILED(error) || !controller || !live->Active()) { if (controller) controller->Close(); live->Finish(true); return S_OK; }
            live->controller=controller;
            if (FAILED(controller->get_CoreWebView2(&live->webview))) live->Finish(true); else live->Setup();
            return S_OK;
          }).Get());
        if (FAILED(status)) self->Finish(true);
        return S_OK;
      }).Get());
    if (FAILED(status)) Finish(true);
  }
};
ProtonVerification::ProtonVerification(HWND parent) : parent_(parent) {}
ProtonVerification::~ProtonVerification() { Cancel(); }
void ProtonVerification::Cancel() { if (current_) { current_->Finish(); current_.reset(); } }
void ProtonVerification::Begin(const std::vector<uint8_t>& handoff,std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result) {
  Cancel();
  current_=std::make_shared<Session>();
  current_->result=std::move(result);
  std::string bytes(handoff.begin(),handoff.end());
  bool valid=usque::proton::Parse(bytes,current_->handoff,Sha);
  usque::proton::Wipe(bytes);
  if (!valid || !usque::proton::CaptchaOnly(current_->handoff) || usque::proton::Now() >= current_->handoff.expires) current_->Finish(true); else current_->Start(parent_);
}
