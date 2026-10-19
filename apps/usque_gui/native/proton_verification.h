#ifndef USQUE_NATIVE_PROTON_VERIFICATION_H_
#define USQUE_NATIVE_PROTON_VERIFICATION_H_

// This bridge is deliberately limited to the official Proton challenge page.
// It never automates a puzzle or exposes a general-purpose browser interface.
#include <algorithm>
#include <chrono>
#include <cstdint>
#include <functional>
#include <map>
#include <set>
#include <string>
#include <vector>

namespace usque::proton {
inline int64_t Now() {
  return std::chrono::duration_cast<std::chrono::milliseconds>(
      std::chrono::system_clock::now().time_since_epoch()).count();
}
inline void Wipe(std::string& value) {
  volatile char* p = value.empty() ? nullptr : &value[0];
  for (size_t i = 0; i < value.size(); ++i) p[i] = 0;
  value.clear();
}
struct Erase {
  std::string& value;
  ~Erase() { Wipe(value); }
};
struct Json {
  enum Kind { kString, kObject, kArray, kNumber } kind = kString;
  std::string text;
  std::map<std::string, Json> object;
  std::vector<Json> array;
  ~Json() { Wipe(text); }
};
// The handoff and success schema contain ASCII strings, arrays, objects and an
// integer timestamp only. Reject all other JSON forms, duplicate keys and deep
// structures instead of building a general-purpose parser.
class Parser {
 public:
  explicit Parser(const std::string& input) : input_(input) {}
  bool Parse(Json& result) { return Value(result, 0) && Space() == input_.size(); }
 private:
  size_t Space() {
    while (pos_ < input_.size() && (input_[pos_] == ' ' || input_[pos_] == '\n' ||
           input_[pos_] == '\r' || input_[pos_] == '\t')) ++pos_;
    return pos_;
  }
  bool String(std::string& out) {
    if (Space() >= input_.size() || input_[pos_++] != '"') return false;
    while (pos_ < input_.size()) {
      unsigned char c = input_[pos_++];
      if (c == '"') return true;
      if (c == '\\') {
        if (pos_ >= input_.size()) return false;
        c = input_[pos_++];
        if (c == 'u') {
          if (input_.size() - pos_ < 4) return false;
          unsigned value = 0;
          for (unsigned i = 0; i < 4; ++i) {
            const char h = input_[pos_++];
            const int digit = h >= '0' && h <= '9' ? h-'0' : h >= 'a' && h <= 'f' ? h-'a'+10 : h >= 'A' && h <= 'F' ? h-'A'+10 : -1;
            if (digit < 0) return false;
            value = value * 16 + static_cast<unsigned>(digit);
          }
          if (value < 0x20 || value > 0x7e) return false;
          c = static_cast<unsigned char>(value);
        } else if (c != '"' && c != '\\' && c != '/') return false;
      }
      if (c < 0x20 || c > 0x7e || out.size() >= 16384) return false;
      out += static_cast<char>(c);
    }
    return false;
  }
  bool Value(Json& out, unsigned depth) {
    if (depth > 3 || Space() >= input_.size()) return false;
    const char first = input_[pos_];
    if (first == '"') return String(out.text);
    if (first >= '0' && first <= '9') {
      out.kind = Json::kNumber;
      if (first == '0' && pos_+1 < input_.size() && input_[pos_+1] >= '0' && input_[pos_+1] <= '9') return false;
      while (pos_ < input_.size() && input_[pos_] >= '0' && input_[pos_] <= '9') {
        if (out.text.size() >= 16) return false;
        out.text += input_[pos_++];
      }
      return true;
    }
    if (first != '{' && first != '[') return false;
    out.kind = first == '{' ? Json::kObject : Json::kArray;
    const char end = first == '{' ? '}' : ']';
    ++pos_;
    if (Space() < input_.size() && input_[pos_] == end) { ++pos_; return true; }
    for (unsigned count = 0; count < 16; ++count) {
      std::string key;
      if (first == '{' && (!String(key) || Space() >= input_.size() || input_[pos_++] != ':')) return false;
      Json child;
      if (!Value(child, depth+1)) return false;
      if (first == '{') {
        if (!out.object.emplace(key, child).second) return false;
      } else out.array.push_back(child);
      if (Space() >= input_.size()) return false;
      const char delimiter = input_[pos_++];
      if (delimiter == end) return true;
      if (delimiter != ',') return false;
    }
    return false;
  }
  const std::string& input_;
  size_t pos_ = 0;
};
inline const Json* Field(const Json& value, const char* name, Json::Kind kind) {
  const auto found = value.object.find(name);
  return found != value.object.end() && found->second.kind == kind ? &found->second : nullptr;
}
inline bool Printable(const std::string& value, size_t maximum) {
  return !value.empty() && value.size() <= maximum && std::all_of(value.begin(), value.end(), [](unsigned char c) { return c >= 0x21 && c <= 0x7e; });
}
inline bool Decode(const std::string& value, std::string& output) {
  for (size_t i = 0; i < value.size(); ++i) {
    unsigned char c = value[i];
    if (c == '%') {
      if (i+2 >= value.size()) return false;
      unsigned number = 0;
      for (unsigned j = 0; j < 2; ++j) {
        char h = value[++i];
        int digit = h >= '0' && h <= '9' ? h-'0' : h >= 'a' && h <= 'f' ? h-'a'+10 : h >= 'A' && h <= 'F' ? h-'A'+10 : -1;
        if (digit < 0) return false;
        number = number*16 + static_cast<unsigned>(digit);
      }
      c = static_cast<unsigned char>(number);
    } else if (c == '+') c = ' ';
    if (c < 0x21 || c > 0x7e) return false;
    output += static_cast<char>(c);
  }
  return true;
}
inline bool Official(const std::string& url) {
  constexpr char origin[]="https://verify.proton.me";
  const size_t size=sizeof(origin)-1;
  return !url.empty() && url.size()<=16384 && url.compare(0,size,origin)==0 &&
    (url.size()==size || url[size]=='/' || url[size]=='?' || url[size]=='#') &&
    std::all_of(url.begin(),url.end(),[](unsigned char c){return c>=0x21 && c<=0x7e && c!='\\';});
}
using Digest = std::function<std::string(const std::string&)>;
struct Handoff {
  std::string job_id, challenge_id, url, digest;
  std::set<std::string> methods;
  int64_t expires = 0;
  ~Handoff() { Wipe(url); }
  bool Matches(const std::string& candidate, const Digest& sha, int64_t now = Now()) const {
    constexpr char origin[] = "https://verify.proton.me";
    if (now >= expires || candidate.size() > 16384 || candidate.compare(0, sizeof(origin)-1, origin) != 0) return false;
    std::string rest = candidate.substr(sizeof(origin)-1);
    Erase erase_rest{rest};
    if (!rest.empty() && rest[0] == '/') rest.erase(0, 1);
    if (rest.empty() || rest[0] != '?' || rest.find('#') != std::string::npos) return false;
    unsigned count = 0;
    std::string token;
    Erase erase_token{token};
    for (size_t start = 1; start <= rest.size();) {
      const size_t end = rest.find('&', start);
      auto pair = rest.substr(start, end == std::string::npos ? end : end-start);
      Erase erase_pair{pair};
      const size_t equals = pair.find('=');
      std::string key, value;
      Erase erase_value{value};
      if (!Decode(pair.substr(0, equals), key)) return false;
      if (key == "token") {
        if (++count != 1 || equals == std::string::npos || !Decode(pair.substr(equals+1), value) || !Printable(value,8192)) return false;
        token = value;
      }
      if (end == std::string::npos) break;
      start = end+1;
    }
    const std::string actual = count == 1 ? sha(token) : "";
    Wipe(token);
    return actual.size() == 64 && actual == digest;
  }
  bool Accept(const std::string& message) const {
    if (message.empty() || message.size() > 32768 || Now() >= expires) return false;
    Json root;
    if (!Parser(message).Parse(root) || root.kind != Json::kObject || root.object.size() != 2) return false;
    const Json* type = Field(root,"type",Json::kString);
    const Json* payload = Field(root,"payload",Json::kObject);
    if (!type || type->text != "HUMAN_VERIFICATION_SUCCESS" || !payload || payload->object.size() != 2) return false;
    const Json* method = Field(*payload,"type",Json::kString);
    const Json* token = Field(*payload,"token",Json::kString);
    return method && methods.count(method->text) && token && Printable(token->text,8192);
  }
};
inline bool Parse(const std::string& input, Handoff& handoff, const Digest& sha, int64_t now = Now()) {
  if (input.empty() || input.size() > 65536) return false;
  Json root;
  if (!Parser(input).Parse(root) || root.kind != Json::kObject || root.object.size() != 6) return false;
  const auto job = Field(root,"job_id",Json::kString), challenge = Field(root,"challenge_id",Json::kString),
    url = Field(root,"url",Json::kString), digest = Field(root,"challenge_sha256",Json::kString),
    expiry = Field(root,"expires_at",Json::kNumber), methods = Field(root,"methods",Json::kArray);
  if (!job || !challenge || !url || !digest || !expiry || !methods || !Printable(job->text,128) || !Printable(challenge->text,128) || digest->text.size()!=64 ||
      !std::all_of(digest->text.begin(),digest->text.end(),[](char c){return (c>='0'&&c<='9')||(c>='a'&&c<='f');})) return false;
  int64_t stamp = 0;
  for (char c : expiry->text) stamp = stamp*10 + c-'0';
  if (stamp <= now || stamp-now > 300000 || methods->array.empty()) return false;
  handoff.job_id=job->text; handoff.challenge_id=challenge->text; handoff.url=url->text;
  handoff.digest=digest->text; handoff.expires=stamp;
  for (const auto& method : methods->array) {
    if (method.kind != Json::kString || (method.text!="captcha"&&method.text!="sms"&&method.text!="email") || !handoff.methods.insert(method.text).second) return false;
  }
  return handoff.Matches(handoff.url,sha,now);
}
inline std::string Script(const Handoff& h, const std::string& send, bool captcha_only = false) {
  std::string methods;
  for (const auto& method : h.methods) { if (!methods.empty()) methods+=','; methods+='"'+method+'"'; }
  return "(()=>{'use strict';const captchaOnly="+std::string(captcha_only ? "true" : "false")+",expires="+std::to_string(h.expires)+",expected='"+h.digest+"',methods=["+methods+R"JS(];
const origin='https://verify.proton.me';let initial;
try{initial=new URL(window.location.href).searchParams.getAll('token');}catch(_){return;}
const same=()=>{try{const u=new URL(window.location.href),v=u.searchParams.getAll('token');return window.top===window&&u.origin===origin&&!u.username&&!u.password&&!u.hash&&(u.pathname==='/'||u.pathname==='')&&(!captchaOnly||(u.searchParams.getAll('embed').length===1&&u.searchParams.get('embed')==='true'&&u.searchParams.getAll('methods').length===1&&u.searchParams.get('methods')==='captcha'))&&v.length===1&&v[0]===initial[0]&&Date.now()<expires;}catch(_){return false;}};
if(initial.length!==1||!same())return;
(async()=>{const hash=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(initial[0]))),v=>v.toString(16).padStart(2,'0')).join('');
if(hash!==expected||!same())return;
let used=false;const stop=()=>{used=true;clearTimeout(timer);window.removeEventListener('message',receive,true);};
const receive=e=>{if(used)return;if(!same()){stop();return;}if(e.origin!==origin)return;
let message;
if(!captchaOnly&&e.source===window&&typeof e.data==='string'&&e.data.length<=32768){message=e.data;}
else if(methods.includes('captcha')&&e.data&&typeof e.data==='object'&&!Array.isArray(e.data)&&e.data.type==='pm_captcha'&&typeof e.data.token==='string'&&/^[\x21-\x7e]{1,8192}$/.test(e.data.token)){
const top=new URL(window.location.href);if(top.searchParams.getAll('embed').length!==1||top.searchParams.get('embed')!=='true')return;
const frames=Array.from(document.querySelectorAll('iframe')).filter(frame=>{try{const u=new URL(frame.src),tokens=u.searchParams.getAll('Token'),force=u.searchParams.getAll('ForceWebMessaging');return u.origin===origin&&!u.username&&!u.password&&u.pathname==='/core/v4/captcha'&&!u.hash&&tokens.length===1&&tokens[0]===initial[0]&&force.length===1&&force[0]==='1';}catch(_){return false;}});
if(frames.length!==1||e.source!==frames[0].contentWindow)return;
message=JSON.stringify({type:'HUMAN_VERIFICATION_SUCCESS',payload:{token:e.data.token,type:'captcha'}});
}else return;
let m;try{m=JSON.parse(message);}catch(_){return;}if(!m||Object.keys(m).length!==2||m.type!=='HUMAN_VERIFICATION_SUCCESS'||!m.payload||Object.keys(m.payload).length!==2||!methods.includes(m.payload.type)||typeof m.payload.token!=='string'||!/^[\x21-\x7e]{1,8192}$/.test(m.payload.token))return;
stop();)JS"+send+R"JS(};const timer=setTimeout(stop,Math.max(0,expires-Date.now()));window.addEventListener('message',receive,true);
})().catch(()=>{});})();)JS";
}
// The captured official page deliberately emits no top-level success message
// when it detects WebView2. Its documented ForceWebMessaging captcha iframe
// still emits pm_captcha. Restrict Windows to that independently bound pathway;
// preserve the page's platform detection and all other browser capabilities.
inline bool CaptchaOnly(Handoff& h) {
  if (!h.methods.count("captcha")) return false;
  const size_t query=h.url.find('?');
  if (query==std::string::npos) return false;
  std::string url=h.url.substr(0,query)+"?";
  Erase erase_url{url};
  for (size_t start=query+1;start<=h.url.size();) {
    const size_t end=h.url.find('&',start);
    auto pair=h.url.substr(start,end==std::string::npos ? end : end-start);
    Erase erase_pair{pair};
    std::string key;
    if (!Decode(pair.substr(0,pair.find('=')),key)) return false;
    if (key!="methods" && key!="embed") { url+=pair; url+='&'; }
    if (end==std::string::npos) break;
    start=end+1;
  }
  url+="methods=captcha&embed=true";
  Wipe(h.url); h.url=std::move(url);
  h.methods={"captcha"};
  return true;
}
inline std::string CaptchaScript(const Handoff& h,const std::string& nonce) {
  return Script(h,"window.chrome.webview.postMessage(JSON.stringify({nonce:'"+nonce+"',message}));",true);
}
}  // namespace usque::proton
#endif
