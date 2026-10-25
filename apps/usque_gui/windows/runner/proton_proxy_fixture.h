// Loopback-only browser fixture. No upstream socket or DNS lookup is possible.
#include <winsock2.h>
#include <atomic>
#include <thread>
#include <string>

class ProtonProxyFixture {
 public:
  unsigned port=0;
  std::atomic<unsigned> connects{0}, targets{0}, authenticated{0};
  const std::string nonce=std::string(32,'c');
  bool wrong_realm=false;
  bool forged_proof=false;
  bool resource_failure=false;
  ProtonProxyFixture(bool wrong=false, bool forged=false, bool failure=false) : wrong_realm(wrong), forged_proof(forged), resource_failure(failure) {
    WSADATA data{};
    if (WSAStartup(MAKEWORD(2,2),&data)!=0) return;
    server_=socket(AF_INET,SOCK_STREAM,IPPROTO_TCP);
    sockaddr_in address{};address.sin_family=AF_INET;address.sin_addr.s_addr=htonl(INADDR_LOOPBACK);
    if (server_==INVALID_SOCKET || bind(server_,reinterpret_cast<sockaddr*>(&address),sizeof(address)) || listen(server_,8)) return;
    int size=sizeof(address);
    if (getsockname(server_,reinterpret_cast<sockaddr*>(&address),&size)) return;
    port=ntohs(address.sin_port);
    thread_=std::thread([this] { Serve(); });
  }
  ~ProtonProxyFixture() { stopped_=true;closesocket(server_);if(thread_.joinable())thread_.join();WSACleanup(); }
 private:
  SOCKET server_=INVALID_SOCKET;
  std::thread thread_;
  std::atomic<bool> stopped_{false};
  void Serve() {
    while (!stopped_) {
      SOCKET client=accept(server_,nullptr,nullptr);
      if (client==INVALID_SOCKET) break;
      DWORD timeout=1500;setsockopt(client,SOL_SOCKET,SO_RCVTIMEO,reinterpret_cast<const char*>(&timeout),sizeof(timeout));
      std::string request;char byte;
      while(request.size()<16384 && recv(client,&byte,1,0)==1) { request+=byte;if(request.size()>=4 && request.substr(request.size()-4)=="\r\n\r\n")break; }
      std::string lower=request;
      for(char& c:lower) if(c>='A'&&c<='Z') c=static_cast<char>(c-'A'+'a');
      const auto header=lower.find("\r\nproxy-authorization: basic ");
      const bool auth=header!=std::string::npos && request.substr(header+29,134)=="YWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWFhYWE6YmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYmJiYg==\r\n";
      std::string status="403 Forbidden",extra,body;
      const auto host=nonce+".usque-proton-proxy.invalid";
      if (resource_failure) { status="502 Synthetic Network Failure"; }
      else if (!auth) { status="407 Proxy Authentication Required";extra="Proxy-Authenticate: Basic realm=\""+(wrong_realm ? "wrong" : nonce)+"\"\r\n"; }
      else {
        ++authenticated;
        if(request.rfind("CONNECT "+host+":443 ",0)==0) { ++connects;status="502 Probe Complete"; }
        else if(request.rfind("GET http://"+host+"/probe ",0)==0) { status="302 Found";extra="Location: https://"+host+"/\r\n"; }
        else if(request.rfind("GET http://"+host+"/ready ",0)==0 && connects) { status="200 OK";body="<script>location.replace('usque-proxy-ready://"+nonce+"/"+(forged_proof ? std::string(64,'0') : "a0fab1377f49a759b57f63318262ebe89fabfc990e8e93ceac2984561482b9d4")+"')</script>"; }
        else if(request.rfind("CONNECT verify.proton.me:443 ",0)==0) { ++targets;status="502 Synthetic Upstream Unavailable"; }
      }
      const auto answer="HTTP/1.1 "+status+"\r\nConnection: close\r\nCache-Control: no-store\r\nContent-Type: text/html\r\nContent-Length: "+std::to_string(body.size())+"\r\n"+extra+"\r\n"+body;
      send(client,answer.data(),static_cast<int>(answer.size()),0);closesocket(client);
    }
  }
};
