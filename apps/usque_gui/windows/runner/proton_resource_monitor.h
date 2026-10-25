#ifndef USQUE_PROTON_RESOURCE_MONITOR_H_
#define USQUE_PROTON_RESOURCE_MONITOR_H_

#include <windows.data.json.h>
#include <wrl.h>
#include <wrl/wrappers/corewrappers.h>
#include <string>
#include <string_view>

namespace usque::proton {
// This is browser-owned CDP metadata, never a page message. Keep it separate
// from the deliberately restricted handoff parser. No URLs, headers or bodies
// are retained or logged by the resource monitor.
using ResourceJson = Microsoft::WRL::ComPtr<ABI::Windows::Data::Json::IJsonObject>;
inline ResourceJson ResourceEvent(const wchar_t* text) {
  ResourceJson value;
  if (!text || wcsnlen_s(text,65537)>65536) return value;
  Microsoft::WRL::ComPtr<ABI::Windows::Data::Json::IJsonObjectStatics> factory;
  if (SUCCEEDED(RoGetActivationFactory(Microsoft::WRL::Wrappers::HStringReference(RuntimeClass_Windows_Data_Json_JsonObject).Get(),IID_PPV_ARGS(&factory)))) {
    factory->Parse(Microsoft::WRL::Wrappers::HStringReference(text).Get(),&value);
  }
  return value;
}
inline std::wstring ResourceString(const ResourceJson& value,const wchar_t* key) {
  Microsoft::WRL::Wrappers::HString text;
  if (!value || FAILED(value->GetNamedString(Microsoft::WRL::Wrappers::HStringReference(key).Get(),text.GetAddressOf()))) return {};
  UINT32 length=0; const auto* raw=text.GetRawBuffer(&length);
  return {raw,length};
}
inline bool ResourceHas(const ResourceJson& value,const wchar_t* key) {
  Microsoft::WRL::ComPtr<ABI::Windows::Data::Json::IJsonValue> child;
  return value && SUCCEEDED(value->GetNamedValue(Microsoft::WRL::Wrappers::HStringReference(key).Get(),&child));
}
inline bool ResourceNetworkFailure(const wchar_t* text) {
    const auto value=ResourceEvent(text);
    if (!value) return false;
    boolean canceled=false;
    value->GetNamedBoolean(Microsoft::WRL::Wrappers::HStringReference(L"canceled").Get(),&canceled);
    if (canceled || ResourceHas(value,L"blockedReason") || ResourceHas(value,L"corsErrorStatus")) return false;
    const auto type=ResourceString(value,L"type");
    if (type!=L"Document" && type!=L"Script" && type!=L"Stylesheet" &&
        type!=L"Image" && type!=L"Font" && type!=L"XHR" && type!=L"Fetch") return false;
    const auto error=ResourceString(value,L"errorText");
    const std::wstring_view reason(error);
    for (const auto* prefix:{L"net::ERR_CONNECTION_",L"net::ERR_NAME_",L"net::ERR_DNS_",
        L"net::ERR_PROXY_",L"net::ERR_SSL_",L"net::ERR_CERT_",L"net::ERR_ADDRESS_"}) {
      if (reason.rfind(prefix,0)==0) return true;
    }
    for (const auto* code:{L"net::ERR_TIMED_OUT",L"net::ERR_INTERNET_DISCONNECTED",
        L"net::ERR_NETWORK_CHANGED",L"net::ERR_NETWORK_IO_SUSPENDED",L"net::ERR_TUNNEL_CONNECTION_FAILED",
        L"net::ERR_EMPTY_RESPONSE",L"net::ERR_CONTENT_LENGTH_MISMATCH",L"net::ERR_INCOMPLETE_CHUNKED_ENCODING"}) {
      if (reason==code) return true;
    }
  return false;
}
}  // namespace usque::proton
#endif
