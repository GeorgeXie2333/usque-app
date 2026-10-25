// Exercise production resource callbacks with local GObject signals only.
// No WebKit process, network, Engine or VPN is started by this fixture.
#include "proton_verification.cc"
#include <iostream>

namespace {
void Begin() {
  current=std::make_unique<Session>();
  current->handoff.version=2;
  current->handoff.expires=usque::proton::Now()+30000;
  current->monotonic_deadline=g_get_monotonic_time()+30000000;
  current->loaded=true;
}
}
int main() {
  api.network_error=[] { return g_quark_from_static_string("synthetic-network"); };
  api.policy_error=[] { return g_quark_from_static_string("synthetic-policy"); };
  const auto type=g_type_register_static_simple(G_TYPE_OBJECT,"UsqueSyntheticResource",
    sizeof(GObjectClass),nullptr,sizeof(GObject),nullptr,GTypeFlags(0));
  g_signal_new("failed",type,G_SIGNAL_RUN_LAST,0,nullptr,nullptr,nullptr,G_TYPE_NONE,1,G_TYPE_ERROR);
  g_signal_new("finished",type,G_SIGNAL_RUN_LAST,0,nullptr,nullptr,nullptr,G_TYPE_NONE,0);
  g_signal_new("failed-with-tls-errors",type,G_SIGNAL_RUN_LAST,0,nullptr,nullptr,nullptr,G_TYPE_NONE,2,G_TYPE_OBJECT,G_TYPE_UINT);
  auto* resource=G_OBJECT(g_object_new(type,nullptr));
  Begin();
  ResourceStarted(nullptr,resource,nullptr,current.get());
  if (current->resources.size()!=1) return 1;
  auto* canceled=g_error_new_literal(api.network_error(),302,"synthetic cancellation");
  g_signal_emit_by_name(resource,"failed",canceled);
  g_error_free(canceled);
  if (!current || !current->loaded) return 1;
  for (auto* ignored:{g_error_new_literal(api.policy_error(),0,"policy"),
      g_error_new_literal(g_quark_from_static_string(""),0,"CORS"),
      g_error_new_literal(G_IO_ERROR,G_IO_ERROR_CANCELLED,"cancelled"),
      g_error_new_literal(g_quark_from_static_string("soup-http-error-quark"),503,"HTTP response")}) {
    g_signal_emit_by_name(resource,"failed",ignored);
    g_error_free(ignored);
    if (!current || !current->loaded) return 1;
  }
  for (auto* transport:{g_error_new_literal(G_IO_ERROR,G_IO_ERROR_CONNECTION_CLOSED,"EOF"),
      g_error_new_literal(G_RESOLVER_ERROR,G_RESOLVER_ERROR_NOT_FOUND,"DNS"),
      g_error_new_literal(G_IO_ERROR,G_IO_ERROR_TIMED_OUT,"timeout")}) {
    if (!ResourceTransportError(transport)) return 1;
    g_error_free(transport);
  }
  auto* broken=g_error_new_literal(api.network_error(),399,"synthetic connection failure");
  g_signal_emit_by_name(resource,"failed",broken);
  if (current) return 1;
  Begin();
  // The prior resource cannot close a new challenge, even if addresses repeat.
  g_signal_emit_by_name(resource,"failed",broken);
  if (!current) return 1;
  ResourceStarted(nullptr,resource,nullptr,current.get());
  g_signal_emit_by_name(resource,"finished");
  if (!current->resources.empty()) return 1;
  g_signal_emit_by_name(resource,"failed",broken);
  if (!current) return 1;
  ResourceStarted(nullptr,resource,nullptr,current.get());
  g_signal_emit_by_name(resource,"failed-with-tls-errors",nullptr,1u);
  if (current) return 1;
  Begin();
  g_signal_emit_by_name(resource,"failed-with-tls-errors",nullptr,1u);
  if (!current) return 1;
  Finish();
  g_error_free(broken);
  g_object_unref(resource);
  std::cout<<"resource network/TLS failure after load, cancellation, completion and stale callbacks: passed\n";
  return 0;
}
