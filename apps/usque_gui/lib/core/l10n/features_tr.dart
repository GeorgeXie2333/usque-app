/// Supplemental feature strings for Turkish.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowTr = <String, String>{
  'local_proxy_settings': 'Yerel proxy ayarları',
  'proxy_switches_hint':
      'Anahtar değişiklikleri otomatik kaydedilir. Dinleme ve DNS düzenlemeleri için Değişiklikleri uygula seçeneğini kullanın.',
  'cc_label': 'HTTP/3 tıkanıklık denetimi',
  'cc_help': 'Sonraki manuel bağlantınızda uygulanır.',
  'cc_upgrade': 'Ayarlar’dan Usque’yi güncelleyin.',
  'cc_h2': 'Bu seçenek yalnızca HTTP/3 bağlantılarını etkiler.',
  'cc_saved': 'Kaydedildi',
  'cc_pending': 'Sonraki manuel bağlantı bekleniyor.',
  'save_changes': 'Değişiklikleri uygula',
  'saving_changes': 'Değişiklikler uygulanıyor…',
  'unsaved_changes': 'Uygulanmamış değişiklikler',
  'changes_applied': 'Değişiklikler uygulandı',
  'changes_apply_hint': 'Düzenlemeler ancak uyguladıktan sonra geçerli olur.',
  'changes_failed':
      'Değişiklikler uygulanamadı. Kayıtlı değerleri gözden geçirip yeniden deneyin.',
  'form_errors':
      'Değişiklikleri uygulamadan önce vurgulanan alanları kontrol edin.',
  'discard_changes_title': 'Uygulanmamış değişiklikler atılsın mı?',
  'discard_changes_body':
      'Düzenlemeleriniz henüz uygulanmadı. Kaydetmek için düzenlemeye devam edin veya çıkmak için atın.',
  'keep_editing': 'Düzenlemeye devam et',
  'discard_changes': 'Değişiklikleri at',
  'invalid_port': '1 ile 65535 arasında bir port girin.',
  'listener_exposure': 'Dinleyici adresleri yerel ağ erişimine izin verir',
  'invalid_ipv4': 'Geçerli bir IPv4 adresi girin, örneğin 127.0.0.1.',
  'invalid_ipv6': 'Geçerli bir IPv6 adresi girin, örneğin ::1.',
  'output_running': 'Çalışıyor',
  'output_waiting': 'Etkin · çalışmıyor',
  'output_disabled': 'Devre dışı',
  'output_starting': 'Başlatılıyor',
  'output_stopping': 'Durduruluyor',
  'output_reconnecting': 'Yeniden bağlanılıyor',
  'output_degraded': 'Sınırlı',
  'output_error': 'Hata',
  'output_unknown': 'Durum kullanılamıyor',
  'shared_network_scope': 'Ağ ayarları tüm hesaplar tarafından paylaşılır.',
  'connection_details': 'Bağlantı ayrıntıları',
  'home_overview': 'Bağlantı özeti',
  'home_exit_region': 'Çıkış bölgesi',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Trafik',
  'home_traffic_window': 'Son 60 saniye',
  'home_traffic_idle': 'Trafik bağlandıktan sonra gösterilir',
  'home_traffic_waiting': 'Trafik verileri bekleniyor',
  'home_traffic_unavailable': 'Trafik geçmişi yok',
  'home_traffic_stale': 'Trafik güncellemeleri gecikiyor',
  'home_outputs_next': 'Bağlandıktan sonra kullanılabilir',
  'home_outputs_retry': 'Bir sonraki bağlantıda kullanılacak VPN ve proxy’ler',
  'connection_protection_group': 'Bağlantı ve koruma',
  'proxy_routing_group': 'Proxy ve yönlendirme',
  'application_group': 'Uygulama',
  'proxy_settings_link':
      'Dinleyici adresleri, portlar, kimlik doğrulama ve DNS.',
  'proxy_auth_separate':
      'Bu değişiklikleri uygulamak için aşağıdaki “Kullanıcı adı ve parolayı kaydet” düğmesini kullanın.',
  'reset_draft_hint':
      'Varsayılanlar bu forma yüklenecek. Geçerli olmaları için değişiklikleri uygulayın.',
};

const kNetworkQualityTr = <String, String>{
  'nq_range': 'Aralık',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Gidiş-dönüş süresi',
  'diag_check_quality_packet_loss': 'Paket kaybı',
  'diag_check_quality_queue_pressure': 'Kuyruk baskısı',
  'diag_check_quality_pmtu': 'Yol MTU’su',
  'diag_check_transport_migration_capability': 'Aynı ailede taşıma',
  'diag_check_dns_direct_encrypted_configuration':
      'Doğrudan DNS yapılandırması',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Doğrudan DNS çalışma durumu',
  'diag_check_dns_direct_encrypted_reachability':
      'Şifreli DNS erişilebilirliği',
  'diag_check_transport_h3_path_validation_probe':
      'Yalıtılmış QUIC el sıkışması',
  'nq_finding_unavailable': 'Bu ölçüm geçerli durumda kullanılamıyor.',
  'nq_finding_invalid_configuration': 'Özel DNS yapılandırması geçersiz.',
  'nq_finding_dns_system':
      'Mevcut ağın DNS’i kullanılıyor. Şifreli DNS denetimi bu durumda uygulanmaz.',
  'nq_finding_unsupported':
      'Şifreli DNS kullanmak için Usque’yi güncelleyin. Şifresiz DNS’e otomatik geçilmez.',
  'nq_finding_dns_custom_valid':
      'Özel şifreli DNS yapılandırması geçerli. Düz metne geri dönüş kapalı.',
  'nq_finding_stale': 'Okuma eski veya fiziksel ağ değişti.',
  'nq_finding_rtt_high': 'Ölçülen gidiş-dönüş süresi yüksek.',
  'nq_finding_healthy': 'Ölçülebilen bağlantı değerleri normal.',
  'nq_finding_loss_high': 'Bu aralıktaki paket kaybı yüksek.',
  'nq_finding_queue_pressure':
      'Bu bağlantıda gönderilmeyi bekleyen veya atılan veriler var.',
  'nq_finding_pmtu_degraded': 'Ağ yoluna uygun paket boyutu doğrulanamadı.',
  'nq_finding_migration_reconnect':
      'Ağ değişikliği için yeniden bağlanmak gerekiyor.',
  'nq_finding_dns_changed': 'Kayıtlı DNS kipi çalışan bağlantıdan farklı.',
  'nq_finding_dns_runtime': 'Şifreli DNS çalışıyor.',
  'nq_finding_dns_degraded':
      'Şifreli DNS’te sorun var. Başarısız sorgular mevcut ağın şifresiz DNS’ine gönderilmedi.',
  'nq_finding_probe_unsafe': 'Bu ölçüm geçerli durumda kullanılamıyor.',
  'nq_finding_probe_success': 'Bu denetim geçti.',
  'nq_finding_probe_cancelled': 'Bu denetim iptal edildi.',
  'nq_finding_probe_timeout': 'Tanılama denetimi zaman aşımına uğradı',
  'nq_finding_probe_failed': 'Bu denetim başarısız oldu.',
  'diag_fix_nq_profile':
      'Özel DNS alanlarını ve sertifika adını gözden geçirin. TLS doğrulamasını kapatmayın.',
  'diag_fix_nq_retry': 'Ağın kararlı olmasını bekleyin, sonra yeniden deneyin.',
  'diag_fix_nq_network':
      'Yerel bağlantıyı denetleyin ve ayarları değiştirmeden önce yeni bir örneği karşılaştırın.',
  'diag_fix_nq_reconnect':
      'Kayıtlı yapılandırmayı uygulamak için yeniden bağlanın.',
  'nav_network_quality': 'Kalite',
  'network_quality': 'Ağ kalitesi',
  'nq_subtitle': 'Yalnızca hıza değil, bağlantıya bakın.',
  'nq_local_only': 'Yalnızca yerel ölçümler. Hiçbir şey yüklenmez.',
  'nq_doctor': 'Ağ tanılmasını çalıştır',
  'nq_doctor_help':
      'Standart denetimler yalnızca yerel durumu okur. Dış bağlantı açmaz ve ayarlarınızı değiştirmez.',
  'nq_live': 'Canlı',
  'nq_stale': 'Eski okumalar',
  'nq_updated': 'Son örnek',
  'nq_seconds': '{count} sn önce',
  'nq_good': 'İyi',
  'nq_fair': 'Orta',
  'nq_poor': 'Zayıf',
  'nq_limited': 'Sınırlı veri',
  'nq_disconnected': 'Bağlantı kesildi',
  'nq_connecting': 'Bağlanılıyor',
  'nq_connected': 'Bağlandı',
  'nq_unavailable': 'Kullanılamıyor',
  'nq_not_ready': 'Hazır değil',
  'nq_unsupported': 'Desteklenmiyor',
  'nq_capability_missing':
      'Bu sürüm bağlantı kalitesini gösteremiyor. Bağlanma ve bağlantıyı kesme çalışmaya devam eder. Ayarlar’dan Usque’yi güncelleyin.',
  'nq_empty': 'Ölçümleri görmek için bağlanın.',
  'nq_stale_help': 'Güncellemeler durakladı. Son ölçümler gösteriliyor.',
  'nq_rtt': 'Gidiş-dönüş süresi',
  'nq_latest': 'En son',
  'nq_smoothed': 'Yumuşatılmış',
  'nq_minimum': 'En düşük',
  'nq_h2_ping': 'HTTP/2 protokolü PING',
  'nq_h3_rtt': 'QUIC yol ölçümü',
  'nq_throughput': 'Verim',
  'nq_download': 'İndirme',
  'nq_upload': 'Yükleme',
  'nq_one_second': '1 saniye',
  'nq_five_seconds': '5 saniyelik ortalama',
  'nq_loss': 'Paket kaybı',
  'nq_loss_h2': 'HTTP/2 karşılaştırılabilir paket kaybı sunmaz.',
  'nq_loss_interval': 'Son aralıkta ölçüldü; ömür boyu kayıp değil.',
  'nq_congestion': 'Tıkanıklık',
  'nq_cwnd': 'Tıkanıklık penceresi',
  'nq_in_flight': 'Yoldaki baytlar',
  'nq_send_rate': 'Teslim hızı',
  'nq_h2_window': 'HTTP/2 alım pencereleri',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Bağlantı',
  'nq_stalls': 'Kapasite duraksamaları',
  'nq_pmtu': 'Yol MTU’su',
  'nq_outer_pmtu': 'Dış UDP yük sınırı',
  'nq_inner_payload': 'CONNECT-IP yük sınırı',
  'nq_pmtu_help':
      'Ağ yolunun taşıyabildiği paket boyutudur. Otomatik denetimler paket kaybını azaltmaya yardımcı olur ve gelişmiş ağ ayarlarındaki VPN MTU değerini artırmaz.',
  'nq_migration': 'Ağ taşıması',
  'nq_migration_help':
      'Wi-Fi ile mobil veri arasında geçişte bağlantıyı korumayı dener. Her iki ağ da aynı IP sürümünü (IPv4 veya IPv6) kullanmalıdır. Aynı anda tek ağ yolu kullanılır; ağların hızları birleştirilmez.',
  'nq_attempts': 'Denemeler',
  'nq_successes': 'Başarılı',
  'nq_failures': 'Başarısız',
  'nq_last_duration': 'Son süre',
  'nq_direct_dns': 'Doğrudan DNS',
  'nq_system_dns': 'Mevcut ağın DNS’i',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Hazır',
  'nq_degraded': 'Bozulmuş',
  'nq_timeouts': 'Zaman aşımları',
  'nq_last_rtt': 'Son RTT',
  'nq_dns_redacted':
      'Çözümleyici adları ve önyükleme adresleri yalnızca ayarlarda gösterilir.',
  'nq_queues': 'Kuyruk baskısı',
  'nq_queue_details': 'Düşük düzey kuyruklar',
  'nq_queue_empty': 'Henüz kuyruk ölçümü yok.',
  'nq_current_capacity': 'Anlık / kapasite',
  'nq_high_water': 'En yüksek seviye',
  'nq_drops': 'Düşmeler',
  'nq_oldest': 'En eski öğe',
  'nq_tunToTransport': 'Cihaz → aktarım',
  'nq_proxyToTransport': 'Proxy → aktarım',
  'nq_transportOutgoing': 'Giden aktarım',
  'nq_h3DatagramSend': 'QUIC datagramları',
  'nq_h3WireSend': 'UDP çıkışı',
  'nq_transportToTun': 'Aktarım → cihaz',
  'nq_transportToProxy': 'Aktarım → proxy',
  'nq_directDns': 'Doğrudan DNS istekleri',
  'nq_unknown_queue': 'Diğer kuyruk',
  'nq_trends': 'Son 60 saniye',
  'nq_samples': 'örnek',
  'nq_pause': 'Grafikleri duraklat',
  'nq_resume': 'Grafikleri sürdür',
  'nq_paused': 'Grafikler duraklatıldı',
  'nq_gaps': 'Eksik örnekler boşluktur.',
  'nq_phase_idle': 'Boşta',
  'nq_phase_preparing_socket': 'Yol hazırlanıyor',
  'nq_phase_probing': 'Sondalanıyor',
  'nq_phase_validated': 'Doğrulandı',
  'nq_phase_promoting': 'Yol değiştiriliyor',
  'nq_phase_stable': 'Kararlı',
  'nq_phase_aborted': 'İptal edildi',
  'nq_phase_revalidating': 'Yeniden doğrulanıyor',
  'nq_phase_degraded': 'Bozulmuş',
  'nq_phase_unknown': 'Hazır değil',
  'nq_phase_unsupported': 'Desteklenmiyor',
  'nq_reason_family_unavailable':
      'Yeni ağ aynı IP sürümünü kullanamıyor. Yeniden bağlanın.',
  'nq_reason_socket_protect_failed':
      'Yeni ağ güvenli biçimde kullanılamadı. Bağlantı kendiliğinden geri gelmezse yeniden bağlanın.',
  'nq_reason_generation_changed_during_setup':
      'Kurulum sırasında ağ yeniden değişti.',
  'nq_reason_peer_cid_unavailable':
      'Sunucu, yeni ağda bağlantıyı koruyamadı. Gerekirse yeniden bağlanın.',
  'nq_reason_local_cid_unavailable':
      'Usque, yeni ağda bağlantıyı koruyamadı. Gerekirse yeniden bağlanın.',
  'nq_reason_path_probe_rejected':
      'Yeni ağın bağlantı denetimi başarısız oldu. İnternet erişimini kontrol edin.',
  'nq_reason_path_validation_timeout':
      'Yeni ağ zamanında yanıt vermedi. İnternet erişimini kontrol edin ve gerekirse yeniden bağlanın.',
  'nq_reason_superseded': 'Geçiş tamamlanmadan ağ yeniden değişti.',
  'nq_reason_promotion_failed':
      'Ağ geçişi güvenli biçimde tamamlanamadı. Bağlantı geri gelmezse yeniden deneyin.',
  'nq_reason_connection_closed':
      'Ağ değiştirilirken bağlantı kapandı. Yeniden bağlanın.',
  'nq_reason_unsupported': 'Bu bağlantıda taşıma kullanılamıyor.',
  'nq_reason_unknown': 'Desteklenen bir neden yok.',
  'nq_dns_custom': 'Özel şifreli çözümleyici',
  'nq_dns_server': 'DNS sunucusunun alan adı',
  'nq_dns_path': 'HTTPS yolu',
  'nq_dns_port': 'Port (0 varsayılanı kullanır)',
  'nq_dns_bootstrap': 'DNS sunucusunun IP adresleri',
  'nq_dns_bootstrap_help':
      'DNS sağlayıcınızın verdiği 1–8 IP adresini, her satıra bir adres gelecek şekilde girin. Örnek: 1.1.1.1. Bu adreslerle sunucu adı önceden çözümlenmeden doğrudan bağlantı kurulur.',
  'nq_dns_no_fallback':
      'Şifreli doğrudan DNS başarısız olursa sorgu başarısız olur. Sistem veya düz metin DNS’ine asla geri dönmez.',
  'nq_dns_system_privacy':
      'Mevcut ağın DNS sağlayıcısı, doğrudan bağlantı trafiğinde sorgulanan alan adlarını görebilir.',
  'nq_dns_scope':
      'Yalnızca doğrudan bağlanılacak ülke/bölge kurallarıyla eşleşen trafiği etkiler. VPN trafiğinin DNS ayarı değişmez.',
  'nq_dns_no_capability':
      'Doğrudan bağlantılar için şifreli DNS kullanmak üzere Usque’yi güncelleyin. Kayıtlı ayarlar korunur. Gizlilik etkisini kabul ediyorsanız “Mevcut ağın DNS’i” seçeneğini kendiniz seçebilirsiniz.',
  'nq_dns_invalid_name':
      'dns.example.com gibi bir alan adı girin. https://, port veya boşluk eklemeyin.',
  'nq_dns_invalid_path':
      '/dns-query gibi, en fazla 256 karakterlik bir yol girin. Boşluk, ? veya # içeren bölümler kullanmayın.',
  'nq_dns_invalid_bootstrap': 'DNS sunucusu için 1–8 IP adresi girin.',
  'nq_dns_invalid_port':
      '1–65535 arasında bir port veya varsayılan port için 0 girin.',
  'nq_dns_invalid_mode': 'Desteklenen bir DNS modu seçin.',
  'nq_doctor_deep_title': 'Derin ağ denetimleri çalıştırılsın mı?',
  'nq_doctor_deep_body':
      'Kontroller test trafiği gönderebilir. En fazla 15 saniye sürer ve iptal edilebilir. Bağlantı ayarlarınız değişmez.',
  'nq_doctor_deep_run': 'Derin denetimleri çalıştır',
  'nq_doctor_evidence':
      'Bu kontroller DNS sızıntısı olup olmadığını doğrulayamaz.',
};

const Map<String, String> kWindowsRecoveryTr = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Usque bileşenlerini Ayarlar’dan birlikte güncelleyin. Yeni bir VPN bağlantısı başlatılmadı.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'Önceki bağlantının temizliği tamamlanmadı. Usque’yi tamamen kapatıp yeniden açın ve tekrar deneyin. Sorun sürerse Tanılama’yı açın.',
  'WINDOWS_RECOVERY_FAILED':
      'Önceki VPN ağ durumu tam olarak geri yüklenemedi. Yeni bir VPN bağlantısı başlatılmadı. Bağlantıyı yeniden deneyin veya yerel tanılamayı inceleyin.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows, üç otomatik denemeden sonra önceki VPN ağ durumunu geri yükleyemedi. Hazır olduğunuzda yeniden deneyin veya yerel tanılamayı inceleyin.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Güvenli geri yükleme doğrulanamadığı için otomatik onarım durduruldu. Ayarlar’dan Usque’yi güncelleyin. Sorun sürerse Tanılama’dan günlükleri dışa aktarın.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Windows ağ kurtarması beklenenden uzun sürüyor. Yeni bir VPN bağlantısı başlatılmadı. Yeniden denemeden önce kurtarmanın bitmesini bekleyin.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Ağ durumu değişti veya başka bir oturum tarafından hâlâ kullanılıyor. Etkin bağlantıyı korumak için otomatik kurtarma durduruldu.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Bu kurulum önceki VPN bağlantısını otomatik olarak geri yükleyemiyor. Ayarlar’dan Usque’yi güncelleyip yeniden deneyin.',
};

const String kWindowsAdapterCleanupTr =
    'Önceki bağlantının sanal ağ bağdaştırıcısı kaldırılamadı veya kaldırıldığı doğrulanamadı. Yeni bir VPN bağlantısı başlatılmadı.';

const Map<String, String> kL4Tr = <String, String>{
  'l4_quic_not_ready': 'L4 bağlantısı hazırlanıyor',
  'l4_unsupported_packets': 'Desteklenmeyen veya bozuk paketler reddedildi',
  'l4_budget_rejections': 'Kaynak kabulleri reddedildi',
  'l4_not_applicable': 'Uygulanamaz (L4)',
  'l4_mode': 'L4 (deneysel)',
  'l4_transport_hint':
      'Yalnızca TCP. UDP gerektiren uygulamalar çalışmayabilir. Otomatik mod L4 içermez.',
  'l4_explanation':
      'L4, TCP trafiğini HTTP/3 üzerinden taşır ve VPN, SOCKS5 ile HTTP proxy’lerinde kullanılabilir. VPN’in DNS sorguları TCP’ye dönüştürülür. Diğer UDP trafiği, uzak Ping, IP parçaları ve uzantıları desteklenmez; bazı uygulamalar çalışmayabilir. L4’ü kendiniz seçmeniz gerekir; otomatik olarak açılmaz.',
  'l4_unsupported':
      'Bu sürüm L4’ü desteklemiyor. Ayarlar’dan Usque’yi güncelleyin.',
  'l4_sni_identity':
      'Sunucu adı hesap tarafından otomatik belirlenir. Diğer bağlantı modları için kaydedilen sunucu adı korunur.',
  'l4_edge_requires_l4':
      'Kenarda çözülen DNS L4 gerektirir. Auto, H3 veya H2’ye geçmeden önce başka bir vekil DNS kipi seçin.',
  'proxy_dns_edge_resolved': 'Cloudflare kenarı (yalnızca L4; yerel arama yok)',
  'l4_verified': 'L4 üzerinden bir uygulama bağlantısı kuruldu',
  'l4_unverified':
      'Sunucuya bağlanıldı; uygulama bağlantısı henüz doğrulanmadı',
  'l4_status_unknown': 'Uygulama bağlantısının durumu alınamıyor',
  'l4_sessions': 'Oturumlar / boşaltma',
  'l4_flows': 'Etkin / bekleyen akışlar',
  'l4_connect': 'CONNECT başarı / hata / zaman aşımı',
  'l4_buffers': 'Kullanılan uygulama tampon bütçesi (bayt)',
  'l4_backpressure': 'Gönderme / alma geri basıncı',
  'l4_tun_flows': 'TUN TCP / yarı açık',
  'l4_udp': 'Reddedilen UDP paketleri',
  'l4_dns': 'DNS dönüşümleri başarı / hata / zaman aşımı',
  'l4_migration': 'Göçle korunan / yeniden kurulumla biten akışlar',
  'l4_na':
      'CONNECT-IP adres denetimi, DATAGRAM kuyrukları, iç yük MTU’su ve UDP zaman aşımı: L4’te uygulanamaz.',
};

const Map<String, String> kNetworkSettingsTr = <String, String>{
  'settings_applying': 'Kaydedildi, uygulanıyor',
  'settings_applied': 'Kaydedildi ve uygulandı',
  'settings_deferred': 'Kaydedildi, sonraki elle bağlantıda geçerli olur',
  'settings_failed': 'Kaydedildi, uygulama başarısız',
  'settings_unknown': 'Sonuç henüz doğrulanmadı',
  'settings_saved': 'Kaydedildi',
  'settings_unsupported':
      'Usque’yi tamamen kapatıp yeniden açın, ardından tekrar kaydedin. Sorun sürerse Ayarlar’dan Usque’yi güncelleyin.',
  'settings_save_failed': 'Ayarlar kaydedilemedi. Düzenlemeleriniz korundu.',
  'settings_reconnect': 'Yeniden bağlan',
};

const Map<String, String> kChainTr = <String, String>{
  'duplicate_directive': 'Bu yönerge yalnızca bir kez geçebilir.',
  'mixed_protocols':
      'Tüm remote uç noktaları aynı TCP veya UDP aktarımını kullanmalıdır.',
  'conflicting_protocol': 'remote, genel aktarım ayarıyla çelişiyor.',
  'too_many_endpoints': 'En fazla 16 remote uç noktası kullanın.',
  'conflicting_authentication':
      'CLIENT_CERT, gömülü sertifika veya kimlik doğrulama moduyla çelişiyor.',
  'serialized_size_limit': 'Şifreli kayıt depolama boyutu sınırını aşardı.',
  'multi_endpoint_unavailable':
      'Birden çok uç noktalı yapılandırmalar için motoru güncelleyin.',
  'candidates': 'Başlangıç uç noktaları',
  'random_order':
      'Uç noktalar her bağlantıda yeni bir rastgele sırayla denenir.',
  'file_order': 'Uç noktalar dosya sırasıyla denenir.',
  'attempting': 'Denenen uç nokta',
  'actual_endpoint': 'Bağlanılan uç nokta',
  'attempt_failures': 'Başarısız denemeler',
  'failure_transport': 'aktarım kapandı',
  'failure_authentication': 'kimlik doğrulama',
  'failure_certificate': 'sertifika',
  'failure_configuration': 'yapılandırma',
  'failure_address_changed': 'adres değişti',
  'failure_protocol': 'protokol',
  'failure_cleanup': 'temizleme',
  'failure_reason': 'Hata: {reason}.',
  'manage': 'Yönet',
  'dns_fallback': 'Tünel DNS (OpenVPN DNS görüşebilir)',
  'dns_unavailable_title': 'Bu çıkışta DNS yok',
  'dns_unavailable':
      'Bu çıkış üzerinden erişilebilen bir DNS sunucusu yok. IP adresleri kullanın veya erişilebilir DNS içeren bir yapılandırmayı yeniden içe aktarın.',
  'authentication_failed':
      'Kimlik doğrulama başarısız. Yeniden bağlanmadan önce kimlik bilgilerini güncelleyin.',
  'profile_limit': 'Yapılandırma kitaplığı dolu (128 profil).',
  'metadata_limit': 'Yapılandırma kitaplığının üst verisi dolu.',
  'title': 'Zincir proxy',
  'subtitle': 'WARP üzerinden ulaşılan bir çıkış seçin.',
  'source': 'Çıkış kaynağı',
  'enable': 'Zincir proxy’yi aç',
  'import_file': 'Dosya içe aktar',
  'paste': 'Yapılandırmayı yapıştır',
  'profiles': 'Kayıtlı yapılandırmalar',
  'empty': 'Bir çıkış seçmek için yapılandırma içe aktarın.',
  'empty_hint_openvpn':
      'Bir .ovpn dosyası içe aktarın veya metnini yapıştırın. TCP ve UDP uç noktaları, gömülü sertifikalar ve kullanıcı adı/parola desteklenir.',
  'empty_hint_wireguard':
      'Bir .conf dosyası içe aktarın veya metnini yapıştırın. Bir [Interface] ve bir [Peer] bölümü desteklenir.',
  'import_limits':
      'Yapılandırmalar en fazla 128 KiB UTF-8 metin olmalıdır. Dosya seçici yoksa metni yapıştırın.',
  'enable_to_choose': 'Yapılandırma seçmek için zincir proxy’yi açın.',
  'select_required': 'Uygulamadan önce kayıtlı bir yapılandırma seçin.',
  'pending_disable': 'Bekliyor: zincir proxy’yi kapat',
  'apply_reconnect': 'Uygula ve yeniden bağlan',
  'requires_connect_ip': 'CONNECT-IP gerekir',
  'menu': 'Yapılandırma işlemleri',
  'preview': 'Yapılandırmayı denetle',
  'save_import': 'Yapılandırmayı kaydet',
  'name': 'Ad',
  'configuration': 'Yapılandırma metni',
  'file_loaded': 'Yapılandırma dosyadan yüklendi ({lines} satır).',
  'username': 'Kullanıcı adı',
  'password': 'Parola',
  'key_password': 'Özel anahtar parolası',
  'show_password': 'Parolayı göster',
  'hide_password': 'Parolayı gizle',
  'credentials': 'Kimlik bilgilerini güncelle',
  'rename': 'Yeniden adlandır',
  'delete': 'Sil',
  'cancel': 'İptal',
  'save': 'Kaydet',
  'apply': 'Değişiklikleri uygula',
  'clear': 'Seçimi temizle',
  'current': 'Geçerli bağlantı',
  'saved': 'Kayıtlı seçim',
  'draft': 'Bekleyen seçim',
  'disconnected': 'Bağlı değil',
  'disabled': 'Açık değil',
  'enabled_idle': 'Açık · bağlı değil',
  'disconnecting': 'Bağlantı kesiliyor',
  'file_read_failed': 'Yapılandırma dosyası okunamadı.',
  'file_encoding_invalid': 'Yapılandırma dosyası UTF-8 metin olmalıdır.',
  'file_busy': 'Bir dosya seçici zaten açık.',
  'connected': 'Bağlandı',
  'connecting': 'Bağlanıyor',
  'error': 'Bağlantı başarısız',
  'no_selection': 'Yapılandırma seçilmedi',
  'l4': 'Bu yapılandırma CONNECT-IP modunu gerektirir.',
  'switch_mode': 'CONNECT-IP’ye geç ve uygula',
  'unsupported': 'Bu motor bu çıkış kaynağını desteklemiyor.',
  'scope':
      'Var olan açık doğrudan kurallar geçerli kalır. Diğer trafik seçilen çıkışı kullanır.',
  'allowed': 'İzin verilen hedefler',
  'dns': 'DNS',
  'addresses': 'Tünel adresleri',
  'address_family': 'Adres ailesi',
  'transport': 'Aktarım',
  'endpoint': 'Sunucu',
  'restricted': 'AllowedIPs dışındaki hedefler proxy yolunda engellenir.',
  'delete_confirm':
      'Kayıtlı bu yapılandırma silinsin mi? Özgün içe aktarılan dosya değişmez.',
  'profile_in_use':
      'Bu yapılandırmayı silmeden önce başka bir yapılandırma seçin veya kayıtlı seçimi temizleyin.',
  'stale_revision': 'Yapılandırma değişti. Listeyi yenileyip yeniden deneyin.',
  'secure_storage_failed': 'Şifreli yapılandırma okunamadı veya kaydedilemedi.',
  'invalid_configuration':
      'Yapılandırma geçersiz veya desteklenmeyen seçenekler içeriyor.',
  'looks_like_wireguard':
      'Bu bir WireGuard yapılandırmasına benziyor. Çıkış kaynağını WireGuard (Custom) olarak değiştirin.',
  'looks_like_openvpn':
      'Bu bir OpenVPN yapılandırmasına benziyor. Çıkış kaynağını OpenVPN (Custom) olarak değiştirin.',
  'error_location': '{message} ({field}, satır {line})',
  'error_field': '{message} ({field})',
  'file_unavailable': 'Dosya seçici yok. Yapılandırma metnini yapıştırın.',
  'invalid_size_or_encoding':
      'En fazla 128 KiB boyutunda UTF-8 yapılandırma kullanın.',
  'unsupported_directive': 'Bu OpenVPN yönergesi desteklenmiyor.',
  'unsupported_or_duplicate_field': 'Bu alan desteklenmiyor veya yinelenmiş.',
  'unsupported_or_duplicate_section':
      'Bir Interface ve bir Peer bölümü kullanın.',
  'missing_field': 'Zorunlu bir alan eksik.',
  'invalid_key': 'Anahtar, geçerli 32 baytlık bir Base64 anahtarı olmalıdır.',
  'checking': 'Yapılandırma denetleniyor…',
  'changed': 'Değişiklikler kaydedildi',
};
