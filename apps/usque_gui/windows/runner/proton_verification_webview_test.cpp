// Exercise the production Session in an actual WebView2 with distinct HTTPS
// origins. Every document is intercepted synthetic HTML; no Proton API,
// credentials, puzzle solver, sidecar or VPN is used.
#include "proton_verification.cpp"

#include <flutter/method_result_functions.h>
#include <shlwapi.h>

#include <iostream>

class ProtonVerificationTest {
 public:
  static int Run(const char* name, bool close_after_result, bool wrong_origin,
                 bool wrong_challenge) {
    auto session=std::make_shared<ProtonVerification::Session>();
    session->handoff.url="https://verify.proton.me/?token=synthetic-frame-challenge&embed=true&methods=captcha";
    session->handoff.digest=Sha("synthetic-frame-challenge");
    session->handoff.methods={"captcha"};
    session->handoff.expires=usque::proton::Now()+20000;
    session->deadline=GetTickCount64()+20000;
    session->nonce="0123456789abcdef0123456789abcdef";
    session->directory=PrivateDirectory();
    if (session->directory.empty()) return 1;
    int replies=0,outcome=-1;
    session->result=std::make_unique<flutter::MethodResultFunctions<flutter::EncodableValue>>(
      [&](const flutter::EncodableValue* value) {
        ++replies;
        const auto* bytes=value ? std::get_if<std::vector<uint8_t>>(value) : nullptr;
        const std::string message=bytes ? std::string(bytes->begin(),bytes->end()) : "";
        outcome=bytes && session->handoff.Accept(message) &&
          message.find("\"token\":\"synthetic-frame-result\"")!=std::string::npos ? 1 : 0;
      },
      [&](const std::string&,const std::string&,const flutter::EncodableValue*) { ++replies;outcome=2; },
      [&]() { ++replies;outcome=3; });
    WNDCLASSW type{};
    type.lpfnWndProc=ProtonVerification::Session::Proc;
    type.hInstance=GetModuleHandleW(nullptr);
    type.lpszClassName=L"UsqueProtonOfflineVerificationTest";
    if (!RegisterClassW(&type) && GetLastError()!=ERROR_CLASS_ALREADY_EXISTS) return 1;
    // Keep the host hidden; successful production Finish() must destroy it.
    session->window=CreateWindowW(type.lpszClassName,L"Offline verification test",
      WS_OVERLAPPEDWINDOW,0,0,500,500,nullptr,nullptr,type.hInstance,session.get());
    if (!session->window) return 1;
    const HWND original_window=session->window;
    const std::weak_ptr<ProtonVerification::Session> weak=session;
    auto options=Microsoft::WRL::Make<CoreWebView2EnvironmentOptions>();
    if (!options || FAILED(options->put_AllowSingleSignOnUsingOSPrimaryAccount(FALSE)) ||
        FAILED(options->put_ExclusiveUserDataFolderAccess(TRUE)) ||
        FAILED(options->put_IsCustomCrashReportingEnabled(TRUE)) ||
        FAILED(options->put_ReleaseChannels(COREWEBVIEW2_RELEASE_CHANNELS_STABLE))) return 1;
    const HRESULT started=CreateCoreWebView2EnvironmentWithOptions(nullptr,
      session->directory.c_str(),options.Get(),
      Callback<ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler>(
        [weak,close_after_result,wrong_origin,wrong_challenge](HRESULT error,ICoreWebView2Environment* environment)->HRESULT {
          auto live=weak.lock();
          if (!live) return S_OK;
          if (FAILED(error) || !environment) { live->Finish(true);return S_OK; }
          live->environment=environment;
          ComPtr<ICoreWebView2Environment10> modern;
          ComPtr<ICoreWebView2ControllerOptions> controller_options;
          if (FAILED(environment->QueryInterface(IID_PPV_ARGS(&modern))) ||
              FAILED(modern->CreateCoreWebView2ControllerOptions(&controller_options)) ||
              FAILED(controller_options->put_IsInPrivateModeEnabled(TRUE))) { live->Finish(true);return S_OK; }
          const HRESULT status=modern->CreateCoreWebView2ControllerWithOptions(live->window,controller_options.Get(),
            Callback<ICoreWebView2CreateCoreWebView2ControllerCompletedHandler>(
              [weak,close_after_result,wrong_origin,wrong_challenge](HRESULT result,ICoreWebView2Controller* controller)->HRESULT {
                auto current=weak.lock();
                if (!current) { if (controller) controller->Close();return S_OK; }
                if (FAILED(result) || !controller) { current->Finish(true);return S_OK; }
                current->controller=controller;
                if (FAILED(controller->get_CoreWebView2(&current->webview))) { current->Finish(true);return S_OK; }
                EventRegistrationToken token{};
                ComPtr<ICoreWebView2_22> resources;
                // Intercept *all* request contexts before navigating. Unknown
                // URLs get an inert error response rather than reaching a host.
                // Include cross-origin frames/workers explicitly; the legacy
                // filter's frame behavior is runtime-dependent.
                if (FAILED(current->webview.As(&resources)) ||
                    FAILED(resources->AddWebResourceRequestedFilterWithRequestSourceKinds(L"*",COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,COREWEBVIEW2_WEB_RESOURCE_REQUEST_SOURCE_KINDS_ALL)) ||
                    FAILED(current->webview->add_WebResourceRequested(
                      Callback<ICoreWebView2WebResourceRequestedEventHandler>(
                        [weak,wrong_origin,wrong_challenge](ICoreWebView2*,ICoreWebView2WebResourceRequestedEventArgs* args)->HRESULT {
                          auto page=weak.lock();
                          if (!page) return E_ABORT;
                          ComPtr<ICoreWebView2WebResourceRequest> request;
                          LPWSTR uri=nullptr;
                          if (FAILED(args->get_Request(&request)) || FAILED(request->get_Uri(&uri))) return E_ABORT;
                          const std::wstring url=uri ? uri : L"";
                          CoTaskMemFree(uri);
                          const std::wstring frame_origin=wrong_origin ? L"https://verify.proton.me" : L"https://verify-api.proton.me";
                          const std::wstring challenge=wrong_challenge ? L"another-challenge" : L"synthetic-frame-challenge";
                          const std::wstring frame=frame_origin+L"/core/v4/captcha?Token="+challenge+L"&ForceWebMessaging=1";
                          std::string html;
                          bool javascript=false;
                          bool found=true;
                          if (url==Wide(page->handoff.url)) {
                            html="<iframe src='"+Utf8(frame.c_str())+"'></iframe><script src='https://verify.proton.me/fixture.js'></script>";
                          } else if (url==L"https://verify.proton.me/fixture.js") {
                            javascript=true;
                            html=
                              "window.addEventListener('message',e=>{if(e.data&&e.data.type==='pm_captcha')document.querySelector('iframe')?.remove();});"
                              "setTimeout(()=>chrome.webview.postMessage('fixture_done'),1200);";
                          } else if (url==frame) {
                            html="<script src='"+Utf8(frame_origin.c_str())+"/frame.js'></script>";
                          } else if (url==frame_origin+L"/frame.js") {
                            javascript=true;
                            html="setTimeout(()=>{for(let i=0;i<2;i++)parent.postMessage({type:'pm_captcha',token:'synthetic-frame-result'},'https://verify.proton.me');},300);";
                          } else { html="Blocked by offline fixture";found=false; }
                          ComPtr<IStream> stream=SHCreateMemStream(reinterpret_cast<const BYTE*>(html.data()),static_cast<UINT>(html.size()));
                          ComPtr<ICoreWebView2WebResourceResponse> response;
                          const std::wstring headers=std::wstring(L"Content-Type: ")+(javascript ? L"text/javascript" : L"text/html")+
                            L"\r\nContent-Security-Policy: sandbox allow-scripts allow-same-origin; default-src 'none'; script-src 'self'; frame-src https://verify-api.proton.me https://verify.proton.me\r\n";
                          if (!stream || FAILED(page->environment->CreateWebResourceResponse(stream.Get(),found ? 200 : 403,found ? L"OK" : L"Blocked",
                              headers.c_str(),&response))) return E_ABORT;
                          return args->put_Response(response.Get());
                        }).Get(),&token))) { current->Finish(true);return S_OK; }
                current->Setup();
                if (current->finished) return S_OK;
                if (FAILED(current->webview->add_WebMessageReceived(
                    Callback<ICoreWebView2WebMessageReceivedEventHandler>(
                      [weak,close_after_result](ICoreWebView2*,ICoreWebView2WebMessageReceivedEventArgs* args)->HRESULT {
                        auto page=weak.lock();
                        if (!page || page->finished) return S_OK;
                        LPWSTR text=nullptr;
                        const bool done=SUCCEEDED(args->TryGetWebMessageAsString(&text)) && text && wcscmp(text,L"fixture_done")==0;
                        CoTaskMemFree(text);
                        if (done || (close_after_result && !page->pending_message.empty())) SendMessageW(page->window,WM_CLOSE,0,0);
                        return S_OK;
                      }).Get(),&token))) current->Finish(true);
                return S_OK;
              }).Get());
          if (FAILED(status)) live->Finish(true);
          return S_OK;
        }).Get());
    if (FAILED(started)) session->Finish(true);
    while (!session->finished && session->Active()) {
      MSG message;
      while (PeekMessageW(&message,nullptr,0,0,PM_REMOVE)) {
        TranslateMessage(&message);DispatchMessageW(&message);
      }
      MsgWaitForMultipleObjects(0,nullptr,FALSE,25,QS_ALLINPUT);
    }
    session->Finish();
    const bool passed=replies==1 && outcome==((wrong_origin || wrong_challenge) ? 0 : 1) && !IsWindow(original_window);
    std::cout<<name<<": "<<(passed ? "passed" : "FAILED")<<" (replies="<<replies<<", outcome="<<outcome<<")\n";
    return passed ? 0 : 1;
  }
};

int main() {
  if (FAILED(CoInitializeEx(nullptr,COINIT_APARTMENTTHREADED))) return 1;
  int result=ProtonVerificationTest::Run("cross-origin auto-close",false,false,false);
  result|=ProtonVerificationTest::Run("close after accepted result",true,false,false);
  result|=ProtonVerificationTest::Run("reject page-origin CAPTCHA",false,true,false);
  result|=ProtonVerificationTest::Run("reject wrong iframe challenge",false,false,true);
  CoUninitialize();
  return result;
}
