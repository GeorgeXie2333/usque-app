/// Supplemental feature strings for Turkish.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowTr = <String, String>{
  'cc_label': 'HTTP/3 tıkanıklık denetimi',
  'cc_help': 'Sonraki manuel bağlantınızda uygulanır.',
  'cc_upgrade': 'Engine güncellemesi gerekli.',
  'cc_h2': 'HTTP/2 sistem TCP’sini kullanır.',
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
  'home_traffic_idle': 'Bağlandıktan sonra başlar',
  'home_traffic_waiting': 'Örnekler bekleniyor',
  'home_traffic_unavailable': 'Geçmiş kullanılamıyor',
  'home_traffic_stale': 'Örnekler gecikti',
  'home_outputs_next': 'Çıkışlar bağlandıktan sonra etkinleşir',
  'home_outputs_retry': 'Çıkışlar sonraki deneme için yapılandırıldı',
  'connection_protection_group': 'Bağlantı ve koruma',
  'proxy_routing_group': 'Proxy ve yönlendirme',
  'application_group': 'Uygulama',
  'proxy_settings_link':
      'Dinleyici adresleri, portlar, kimlik doğrulama ve DNS.',
  'proxy_auth_separate':
      'Kimlik bilgileri «Kimlik bilgilerini kaydet» ile ayrı kaydedilir.',
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
      'Fiziksel sistem DNS’i seçili; şifreli DNS denetimleri uygulanmaz.',
  'nq_finding_unsupported':
      'Bu Engine’de şifreli DNS kullanılamıyor; düz metne geri dönüşe izin verilmez.',
  'nq_finding_dns_custom_valid':
      'Özel şifreli DNS yapılandırması geçerli. Düz metne geri dönüş kapalı.',
  'nq_finding_stale': 'Okuma eski veya fiziksel ağ değişti.',
  'nq_finding_rtt_high': 'Ölçülen gidiş-dönüş süresi yüksek.',
  'nq_finding_healthy': 'Kullanılabilir yerel ölçüm beklenen aralıkta.',
  'nq_finding_loss_high': 'Bu aralıktaki paket kaybı yüksek.',
  'nq_finding_queue_pressure':
      'Bir kuyruk baskı altında veya bu bağlantı sırasında düşme kaydetti.',
  'nq_finding_pmtu_degraded': 'Yol MTU doğrulaması bozulmuş.',
  'nq_finding_migration_reconnect':
      'Bu yolda taşıma kullanılamıyor; ağ değişikliğinde tam yeniden bağlantı kullanılır.',
  'nq_finding_dns_changed': 'Kayıtlı DNS kipi çalışan bağlantıdan farklı.',
  'nq_finding_dns_runtime':
      'Şifreli DNS başarılı. Yerel durum, DNS sızıntısı olmadığının harici bir kanıtı değildir.',
  'nq_finding_dns_degraded':
      'Şifreli DNS bozulmuş; başarısız doğrudan sorgular sistem DNS’ine geri dönmez.',
  'nq_finding_probe_unsafe':
      'Sonda atlandı: gerekli güvenli durum veya kayıtlı kimlik yok. Etkin bir tünel asla çoğaltılmaz.',
  'nq_finding_probe_success':
      'Kimliği doğrulanmış sonda tamamlandı. Bu harici bir paket sızıntısı testi değildir.',
  'nq_finding_probe_cancelled': 'Sonda iptal edildi ve temizlik istendi.',
  'nq_finding_probe_timeout': 'Sınırlı sonda son tarihinden önce bitmedi.',
  'nq_finding_probe_failed':
      'Kimliği doğrulanmış sonda başarısız oldu; güvensiz geri dönüş denenmedi.',
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
      'Bu Engine ağ kalitesi sağlamaz. Mevcut bağlantı kontrolleriniz çalışmayı sürdürür.',
  'nq_empty':
      'Ölçümleri görmek için bağlanın. Bilinmeyen değerler sıfır olarak gösterilmez.',
  'nq_stale_help':
      'Kaynak güncellemeyi durdurdu. Bunlar önceki okumalar; boşluklar boşluk olarak kalır.',
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
  'nq_pmtu_help': 'Yol keşfi cihazın TUN MTU’sunu artırmaz.',
  'nq_migration': 'Ağ taşıması',
  'nq_migration_help':
      'Bir bağlantı, bir veri yolu. Yalnızca aynı IP ailesi; çok yollu değil.',
  'nq_attempts': 'Denemeler',
  'nq_successes': 'Başarılı',
  'nq_failures': 'Başarısız',
  'nq_last_duration': 'Son süre',
  'nq_direct_dns': 'Doğrudan DNS',
  'nq_system_dns': 'Fiziksel sistem DNS’i',
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
      'Geçerli IP ailesi kullanılamıyor; tam yeniden bağlantı kullanılır.',
  'nq_reason_socket_protect_failed': 'Korumalı aday soket hazırlanamadı.',
  'nq_reason_generation_changed_during_setup':
      'Kurulum sırasında ağ yeniden değişti.',
  'nq_reason_peer_cid_unavailable':
      'Karşı uçta yedek bağlantı tanımlayıcısı yok.',
  'nq_reason_local_cid_unavailable':
      'Yerel bir bağlantı tanımlayıcısı kullanılamıyor.',
  'nq_reason_path_probe_rejected': 'Aday yol doğrulanamadı.',
  'nq_reason_path_validation_timeout':
      'Yol doğrulaması zaman aşımına uğradı; yeniden bağlantı kullanılabilir.',
  'nq_reason_superseded':
      'Daha yeni bir ağ değişikliği bu denemenin yerini aldı.',
  'nq_reason_promotion_failed': 'Yol değişimi güvenle tamamlanamadı.',
  'nq_reason_connection_closed': 'Taşıma sırasında bağlantı kapandı.',
  'nq_reason_unsupported': 'Bu bağlantıda taşıma kullanılamıyor.',
  'nq_reason_unknown': 'Desteklenen bir neden yok.',
  'nq_dns_custom': 'Özel şifreli çözümleyici',
  'nq_dns_server': 'TLS sunucu adı',
  'nq_dns_path': 'HTTPS yolu',
  'nq_dns_port': 'Port (0 varsayılanı kullanır)',
  'nq_dns_bootstrap': 'Önyükleme IP adresleri',
  'nq_dns_bootstrap_help':
      '1–8 sayısal IP adresi girin, satır başına bir tane. Ana bilgisayar adı sorgusu kullanılmaz.',
  'nq_dns_no_fallback':
      'Şifreli doğrudan DNS başarısız olursa sorgu başarısız olur. Sistem veya düz metin DNS’ine asla geri dönmez.',
  'nq_dns_system_privacy':
      'Fiziksel sistem DNS’i, doğrudan sorgu adlarını fiziksel ağın DNS sağlayıcısına açığa çıkarabilir.',
  'nq_dns_scope':
      'Yalnızca Geo ile seçilen doğrudan sorgular için kullanılır. Tünel DNS’i değişmez.',
  'nq_dns_no_capability':
      'Bu Engine şifreli doğrudan DNS kullanamaz. Kayıtlı ayarlar korunur. Açıkça Sistem DNS’ini seçebilirsiniz.',
  'nq_dns_invalid_name':
      'Boşluk, URL sözdizimi veya joker karakter içermeyen bir DNS adı girin.',
  'nq_dns_invalid_path':
      'En fazla 256 karakterlik bir /path kullanın; sorgu, parça veya boşluk olmasın.',
  'nq_dns_invalid_bootstrap':
      '1–8 benzersiz tek noktaya yayın IP’si kullanın; belirsiz, çok noktaya yayın, yayın veya IPv6 bağlantı-yerel adresi olmasın.',
  'nq_dns_invalid_port': '0–65535 girin.',
  'nq_dns_invalid_mode': 'Desteklenen bir DNS modu seçin.',
  'nq_doctor_deep_title': 'Derin ağ denetimleri çalıştırılsın mı?',
  'nq_doctor_deep_body':
      'Derin denetimler, yapılandırdığınız çözümleyiciye bir DNS test sorgusu gönderebilir ve korumalı bir QUIC yolunu doğrulayabilir. En fazla 15 saniye sürer, iptal edilebilir, ikinci bir veri taşıyan tünel oluşturmaz ve DNS’inizi, rotanızı, profilinizi veya aktarımınızı değiştirmez.',
  'nq_doctor_deep_run': 'Derin denetimleri çalıştır',
  'nq_doctor_evidence':
      'Yerel denetimler yapılandırmayı ve gözlemlenen durumu açıklar. Sıfır DNS sızıntısının dış kanıtı değildir.',
};

const Map<String, String> kWindowsRecoveryTr = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'Önceki VPN ağ durumu tam olarak geri yüklenemedi. Yeni bir VPN bağlantısı başlatılmadı. Bağlantıyı yeniden deneyin veya yerel tanılamayı inceleyin.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows, üç otomatik denemeden sonra önceki VPN ağ durumunu geri yükleyemedi. Hazır olduğunuzda yeniden deneyin veya yerel tanılamayı inceleyin.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Önceki Windows ağ durumu güvenle doğrulanamadığı için otomatik onarım durdu. Agent’ı yeniden başlatın veya Usque’yi güncelleyin, ardından yerel tanılamayı inceleyin.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Windows ağ kurtarması beklenenden uzun sürüyor. Yeni bir VPN bağlantısı başlatılmadı. Yeniden denemeden önce kurtarmanın bitmesini bekleyin.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Ağ durumu değişti veya başka bir oturum tarafından hâlâ kullanılıyor. Etkin bağlantıyı korumak için otomatik kurtarma durduruldu.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Bu Windows Agent güvenli otomatik kurtarmayı desteklemiyor. Uygulamayı ve Agent’ı birlikte güncelleyin, ardından yeniden deneyin.',
};

const String kWindowsAdapterCleanupTr =
    'Önceki Wintun bağdaştırıcısı kaldırılamadı veya kaldırıldığı doğrulanamadı. Yeni bir VPN bağlantısı başlatılmadı.';
