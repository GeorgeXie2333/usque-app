#include "proton_verification.h"

#ifdef NDEBUG
#undef NDEBUG
#endif
#include <cassert>
#include <iostream>

namespace {
const std::string digest(64,'a');
std::string Sha(const std::string& token) { return std::string(64,token=="challenge~key"?'a':'b'); }
std::string Handoff(const std::string& url,int64_t expires) {
  return "{\"job_id\":\"job-one\",\"challenge_id\":\"challenge-one\",\"url\":\""+url+"\",\"expires_at\":"+std::to_string(expires)+",\"methods\":[\"captcha\"],\"challenge_sha256\":\""+digest+"\"}";
}
bool Parse(const std::string& input,int64_t now) { usque::proton::Handoff result;return usque::proton::Parse(input,result,Sha,now); }
}
int main() {
  const auto now=usque::proton::Now();
  const std::string url="https://verify.proton.me/?token=challenge%7Ekey&methods=captcha";
  usque::proton::Handoff handoff;
  assert(usque::proton::Parse(Handoff(url,now+60000),handoff,Sha,now));
  assert(handoff.Matches(url,Sha,now));
  assert(usque::proton::Official("https://verify.proton.me/core/v4/captcha?Token=frame"));
  for (const auto candidate : {"https://verify.proton.me.evil/", "https://verify.proton.me@evil/", "https://verify.proton.me:443/", "http://verify.proton.me/", "https://verify.proton.me/\nheader"}) assert(!usque::proton::Official(candidate));
  assert(!handoff.Matches(url,Sha,now+60000));
  assert(!Parse(Handoff(url,now),now));
  assert(!Parse(Handoff(url,now+300001),now));
  for (const char* candidate : {
      "http://verify.proton.me/?token=challenge~key",
      "https://verify.proton.me.evil/?token=challenge~key",
      "https://verify.proton.me@evil/?token=challenge~key",
      "https://verify.proton.me:443/?token=challenge~key",
      "https://verify.proton.me/path?token=challenge~key",
      "https://verify.proton.me/?token=challenge~key#fragment",
      "https://verify.proton.me/?token=other",
      "https://verify.proton.me/?token=challenge~key&token=challenge~key",
      "https://verify.proton.me/?token=challenge~key&%74oken=challenge~key",
      "https://verify.proton.me/?token=challenge%00key",
      "https://verify.proton.me/?token=challenge%GGkey",
      "https://verify.proton.me/?methods=captcha"}) assert(!handoff.Matches(candidate,Sha,now));
  auto windows=handoff;
  windows.url += "&methods=email&embed=false";
  windows.methods.insert("email");
  assert(usque::proton::CaptchaOnly(windows));
  assert(windows.Matches(windows.url,Sha,now));
  assert(windows.methods==std::set<std::string>{"captcha"});
  assert(windows.url.find("methods=captcha&embed=true")!=std::string::npos);
  assert(windows.url.find("methods=email")==std::string::npos);
  windows.methods={"sms"};
  assert(!usque::proton::CaptchaOnly(windows));
  const std::string message=R"({"type":"HUMAN_VERIFICATION_SUCCESS","payload":{"type":"captcha","token":"manual-only"}})";
  assert(handoff.Accept(message));
  for (const char* malformed : {
      "", "null", "[]", "{}", "{", "{\"type\":true}",
      "{\"type\":\"HUMAN_VERIFICATION_SUCCESS\",\"type\":\"HUMAN_VERIFICATION_SUCCESS\",\"payload\":{\"type\":\"captcha\",\"token\":\"x\"}}",
      "{\"type\":\"HUMAN_VERIFICATION_SUCCESS\",\"payload\":{\"type\":\"email\",\"token\":\"x\"}}",
      "{\"type\":\"HUMAN_VERIFICATION_SUCCESS\",\"payload\":{\"type\":\"captcha\",\"token\":\"x\\r\\nheader\"}}",
      "{\"type\":\"HUMAN_VERIFICATION_SUCCESS\",\"payload\":{\"type\":\"captcha\",\"token\":\"\"}}",
      "{\"type\":\"HUMAN_VERIFICATION_SUCCESS\",\"payload\":{\"type\":\"captcha\",\"token\":\"x\",\"extra\":\"x\"}}"}) assert(!handoff.Accept(malformed));
  assert(!handoff.Accept(message+" trailing"));
  assert(!handoff.Accept(std::string(32769,'x')));
  for (size_t i=0;i<message.size();++i) assert(!handoff.Accept(message.substr(0,i)));
  // Deterministic malformed corpus tests bounded nesting, escapes and lengths.
  for (unsigned depth=1;depth<40;++depth) {
    std::string nested(depth,'[');nested+='0';nested+=std::string(depth,']');
    assert(!handoff.Accept(nested));
  }
  for (unsigned byte=0;byte<256;++byte) {
    std::string input=Handoff(url,now+60000);input[0]=static_cast<char>(byte);
    if (byte!='{') assert(!Parse(input,now));
  }
  auto duplicate=Handoff(url,now+60000);duplicate.insert(1,"\"job_id\":\"duplicate\",");
  assert(!Parse(duplicate,now));
  std::cout << "Proton verification policy tests passed\n";
}
