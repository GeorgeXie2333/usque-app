/// Supplemental feature strings for Indonesian.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowId = <String, String>{
  'cc_label': 'Kontrol kongesti HTTP/3',
  'cc_help': 'Berlaku pada koneksi manual berikutnya.',
  'cc_upgrade': 'Pembaruan Engine diperlukan.',
  'cc_h2': 'HTTP/2 memakai TCP sistem.',
  'cc_saved': 'Disimpan',
  'cc_pending': 'Menunggu koneksi manual berikutnya.',
  'save_changes': 'Terapkan perubahan',
  'saving_changes': 'Menerapkan perubahan…',
  'unsaved_changes': 'Perubahan belum diterapkan',
  'changes_applied': 'Perubahan diterapkan',
  'changes_apply_hint': 'Suntingan baru berlaku setelah Anda menerapkannya.',
  'changes_failed':
      'Perubahan tidak dapat diterapkan. Tinjau nilai tersimpan, lalu coba lagi.',
  'form_errors': 'Periksa kolom yang disorot sebelum menerapkan perubahan.',
  'discard_changes_title': 'Buang perubahan yang belum diterapkan?',
  'discard_changes_body':
      'Suntingan Anda belum diterapkan. Lanjutkan mengedit untuk menyimpannya, atau buang untuk keluar.',
  'keep_editing': 'Lanjutkan mengedit',
  'discard_changes': 'Buang perubahan',
  'invalid_port': 'Masukkan port dari 1 sampai 65535.',
  'listener_exposure': 'Alamat listener mengizinkan akses LAN',
  'invalid_ipv4': 'Masukkan alamat IPv4 yang valid, misalnya 127.0.0.1.',
  'invalid_ipv6': 'Masukkan alamat IPv6 yang valid, misalnya ::1.',
  'output_running': 'Berjalan',
  'output_waiting': 'Diaktifkan · belum berjalan',
  'output_disabled': 'Nonaktif',
  'output_starting': 'Memulai',
  'output_stopping': 'Menghentikan',
  'output_reconnecting': 'Menyambungkan ulang',
  'output_degraded': 'Terbatas',
  'output_error': 'Kesalahan',
  'output_unknown': 'Status tidak tersedia',
  'shared_network_scope': 'Setelan jaringan digunakan bersama oleh semua akun.',
  'connection_details': 'Detail koneksi',
  'home_overview': 'Ringkasan koneksi',
  'home_exit_region': 'Wilayah keluar',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Lalu lintas',
  'home_traffic_window': '60 detik terakhir',
  'home_traffic_idle': 'Mulai setelah tersambung',
  'home_traffic_waiting': 'Menunggu sampel',
  'home_traffic_unavailable': 'Riwayat tidak tersedia',
  'home_traffic_stale': 'Sampel tertunda',
  'home_outputs_next': 'Keluaran diaktifkan setelah tersambung',
  'home_outputs_retry': 'Keluaran disiapkan untuk percobaan berikutnya',
  'connection_protection_group': 'Koneksi & perlindungan',
  'proxy_routing_group': 'Proksi & perutean',
  'application_group': 'Aplikasi',
  'proxy_settings_link': 'Alamat listener, port, autentikasi, dan DNS.',
  'proxy_auth_separate':
      'Kredensial disimpan terpisah lewat Simpan kredensial.',
  'reset_draft_hint':
      'Nilai default akan dimuat ke formulir ini. Terapkan perubahan agar berlaku.',
};

const Map<String, String> kNetworkQualityId = <String, String>{
  'nq_range': 'Rentang',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Waktu bolak-balik',
  'diag_check_quality_packet_loss': 'Kehilangan paket',
  'diag_check_quality_queue_pressure': 'Tekanan antrean',
  'diag_check_quality_pmtu': 'MTU jalur',
  'diag_check_transport_migration_capability': 'Migrasi keluarga yang sama',
  'diag_check_dns_direct_encrypted_configuration': 'Konfigurasi DNS langsung',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Status berjalan DNS langsung',
  'diag_check_dns_direct_encrypted_reachability':
      'Keterjangkauan DNS terenkripsi',
  'diag_check_transport_h3_path_validation_probe': 'Handshake QUIC terisolasi',
  'nq_finding_unavailable':
      'Pengukuran ini tidak tersedia pada status saat ini.',
  'nq_finding_invalid_configuration': 'Konfigurasi DNS kustom tidak valid.',
  'nq_finding_dns_system':
      'DNS sistem fisik dipilih; pemeriksaan DNS terenkripsi tidak berlaku.',
  'nq_finding_unsupported':
      'DNS terenkripsi tidak tersedia di Engine ini; cadangan teks biasa tidak diizinkan.',
  'nq_finding_dns_custom_valid':
      'Konfigurasi DNS terenkripsi kustom valid. Cadangan teks biasa dinonaktifkan.',
  'nq_finding_stale': 'Bacaan kedaluwarsa atau jaringan fisik berubah.',
  'nq_finding_rtt_high': 'Waktu bolak-balik terukur lebih tinggi dari biasa.',
  'nq_finding_healthy':
      'Pengukuran lokal yang tersedia berada dalam rentang yang diharapkan.',
  'nq_finding_loss_high':
      'Kehilangan paket pada selang pengukuran ini lebih tinggi dari biasa.',
  'nq_finding_queue_pressure':
      'Ada antrean yang tertekan atau mencatat pembuangan selama koneksi ini.',
  'nq_finding_pmtu_degraded': 'Validasi MTU jalur menurun.',
  'nq_finding_migration_reconnect':
      'Migrasi tidak tersedia di jalur ini; perubahan jaringan memakai sambungan ulang penuh.',
  'nq_finding_dns_changed':
      'Mode DNS tersimpan berbeda dari koneksi yang sedang berjalan.',
  'nq_finding_dns_runtime':
      'DNS terenkripsi berhasil. Status lokal bukan bukti bebas kebocoran eksternal.',
  'nq_finding_dns_degraded':
      'DNS terenkripsi menurun; kueri langsung yang gagal tidak kembali ke DNS sistem.',
  'nq_finding_probe_unsafe':
      'Probe dilewati: status aman yang diperlukan atau identitas tersimpan tidak tersedia. Terowongan aktif tidak pernah diduplikasi.',
  'nq_finding_probe_success':
      'Probe terautentikasi selesai. Ini bukan uji kebocoran paket eksternal.',
  'nq_finding_probe_cancelled': 'Probe dibatalkan dan pembersihan diminta.',
  'nq_finding_probe_timeout':
      'Probe berbatas waktu tidak selesai sebelum tenggat.',
  'nq_finding_probe_failed':
      'Probe terautentikasi gagal; cadangan tidak aman tidak dicoba.',
  'diag_fix_nq_profile':
      'Tinjau kolom DNS kustom dan nama sertifikat. Jangan nonaktifkan verifikasi TLS.',
  'diag_fix_nq_retry': 'Tunggu jaringan stabil, lalu coba lagi.',
  'diag_fix_nq_network':
      'Periksa konektivitas lokal dan bandingkan sampel baru sebelum mengubah setelan.',
  'diag_fix_nq_reconnect':
      'Sambungkan ulang untuk menerapkan konfigurasi tersimpan.',
  'nav_network_quality': 'Kualitas',
  'network_quality': 'Kualitas jaringan',
  'nq_subtitle': 'Baca koneksinya, bukan hanya kecepatannya.',
  'nq_local_only':
      'Pengukuran hanya di perangkat ini. Tidak ada yang diunggah.',
  'nq_doctor': 'Jalankan Network Doctor',
  'nq_doctor_help':
      'Pemeriksaan standar hanya membaca status lokal. Tidak membuka koneksi eksternal atau mengubah setelan Anda.',
  'nq_live': 'Langsung',
  'nq_stale': 'Bacaan kedaluwarsa',
  'nq_updated': 'Sampel terakhir',
  'nq_seconds': '{count} detik lalu',
  'nq_good': 'Baik',
  'nq_fair': 'Cukup',
  'nq_poor': 'Buruk',
  'nq_limited': 'Data terbatas',
  'nq_disconnected': 'Terputus',
  'nq_connecting': 'Menyambungkan',
  'nq_connected': 'Tersambung',
  'nq_unavailable': 'Tidak tersedia',
  'nq_not_ready': 'Belum siap',
  'nq_unsupported': 'Tidak didukung',
  'nq_capability_missing':
      'Engine ini tidak menyediakan kualitas jaringan. Kontrol koneksi yang ada tetap berfungsi.',
  'nq_empty':
      'Sambungkan untuk melihat pengukuran. Nilai yang tidak diketahui tidak ditampilkan sebagai nol.',
  'nq_stale_help':
      'Sumber berhenti memperbarui. Ini bacaan sebelumnya; celah tetap celah.',
  'nq_rtt': 'Waktu bolak-balik',
  'nq_latest': 'Terbaru',
  'nq_smoothed': 'Dihaluskan',
  'nq_minimum': 'Terkecil',
  'nq_h2_ping': 'PING protokol HTTP/2',
  'nq_h3_rtt': 'Pengukuran jalur QUIC',
  'nq_throughput': 'Debit data',
  'nq_download': 'Unduh',
  'nq_upload': 'Unggah',
  'nq_one_second': '1 detik',
  'nq_five_seconds': 'Rata-rata 5 detik',
  'nq_loss': 'Kehilangan paket',
  'nq_loss_h2': 'HTTP/2 tidak menampilkan kehilangan paket yang sebanding.',
  'nq_loss_interval':
      'Diukur pada selang terakhir; bukan kehilangan sepanjang masa.',
  'nq_congestion': 'Kemacetan',
  'nq_cwnd': 'Jendela kemacetan',
  'nq_in_flight': 'Bytes dalam pengiriman',
  'nq_send_rate': 'Laju pengiriman',
  'nq_h2_window': 'Jendela terima HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Koneksi',
  'nq_stalls': 'Jeda kapasitas',
  'nq_pmtu': 'MTU jalur',
  'nq_outer_pmtu': 'Batas muatan UDP luar',
  'nq_inner_payload': 'Batas muatan CONNECT-IP',
  'nq_pmtu_help': 'Penemuan jalur tidak menaikkan MTU TUN perangkat.',
  'nq_migration': 'Migrasi jaringan',
  'nq_migration_help':
      'Satu koneksi, satu jalur data. Hanya keluarga IP yang sama; bukan banyak jalur.',
  'nq_attempts': 'Percobaan',
  'nq_successes': 'Berhasil',
  'nq_failures': 'Gagal',
  'nq_last_duration': 'Durasi terakhir',
  'nq_direct_dns': 'DNS langsung',
  'nq_system_dns': 'DNS sistem fisik',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Siap',
  'nq_degraded': 'Menurun',
  'nq_timeouts': 'Habis waktu',
  'nq_last_rtt': 'RTT terakhir',
  'nq_dns_redacted':
      'Nama resolver dan alamat bootstrap hanya ditampilkan di Setelan.',
  'nq_queues': 'Tekanan antrean',
  'nq_queue_details': 'Antrean tingkat rendah',
  'nq_queue_empty': 'Belum ada pengukuran antrean.',
  'nq_current_capacity': 'Saat ini / kapasitas',
  'nq_high_water': 'Puncak tertinggi',
  'nq_drops': 'Pembuangan',
  'nq_oldest': 'Item tertua',
  'nq_tunToTransport': 'Perangkat → transport',
  'nq_proxyToTransport': 'Proksi → transport',
  'nq_transportOutgoing': 'Transport keluar',
  'nq_h3DatagramSend': 'Datagram QUIC',
  'nq_h3WireSend': 'Keluaran UDP',
  'nq_transportToTun': 'Transport → perangkat',
  'nq_transportToProxy': 'Transport → proksi',
  'nq_directDns': 'Permintaan DNS langsung',
  'nq_unknown_queue': 'Antrean lain',
  'nq_trends': '60 detik terakhir',
  'nq_samples': 'sampel',
  'nq_pause': 'Jeda grafik',
  'nq_resume': 'Lanjutkan grafik',
  'nq_paused': 'Grafik dijeda',
  'nq_gaps': 'Sampel yang hilang ditampilkan sebagai celah.',
  'nq_phase_idle': 'Menganggur',
  'nq_phase_preparing_socket': 'Menyiapkan jalur',
  'nq_phase_probing': 'Menyelidik',
  'nq_phase_validated': 'Divalidasi',
  'nq_phase_promoting': 'Beralih jalur',
  'nq_phase_stable': 'Stabil',
  'nq_phase_aborted': 'Dihentikan',
  'nq_phase_revalidating': 'Memvalidasi ulang',
  'nq_phase_degraded': 'Menurun',
  'nq_phase_unknown': 'Belum siap',
  'nq_phase_unsupported': 'Tidak didukung',
  'nq_reason_family_unavailable':
      'Keluarga IP saat ini tidak tersedia; sambungan ulang penuh digunakan.',
  'nq_reason_socket_protect_failed':
      'Soket kandidat terlindungi tidak dapat disiapkan.',
  'nq_reason_generation_changed_during_setup':
      'Jaringan berubah lagi selama penyiapan.',
  'nq_reason_peer_cid_unavailable':
      'Peer tidak memiliki pengenal koneksi cadangan.',
  'nq_reason_local_cid_unavailable': 'Pengenal koneksi lokal tidak tersedia.',
  'nq_reason_path_probe_rejected': 'Jalur kandidat tidak dapat divalidasi.',
  'nq_reason_path_validation_timeout':
      'Validasi jalur habis waktu; sambungan ulang tersedia.',
  'nq_reason_superseded':
      'Perubahan jaringan yang lebih baru menggantikan percobaan ini.',
  'nq_reason_promotion_failed':
      'Peralihan jalur tidak dapat diselesaikan dengan aman.',
  'nq_reason_connection_closed': 'Koneksi ditutup selama migrasi.',
  'nq_reason_unsupported': 'Migrasi tidak tersedia pada koneksi ini.',
  'nq_reason_unknown': 'Tidak ada alasan yang didukung.',
  'nq_dns_custom': 'Resolver terenkripsi kustom',
  'nq_dns_server': 'Nama server TLS',
  'nq_dns_path': 'Jalur HTTPS',
  'nq_dns_port': 'Port (0 memakai nilai baku)',
  'nq_dns_bootstrap': 'Alamat IP bootstrap',
  'nq_dns_bootstrap_help':
      'Masukkan 1–8 alamat IP numerik, satu per baris. Pencarian nama host tidak digunakan.',
  'nq_dns_no_fallback':
      'Jika DNS langsung terenkripsi gagal, kueri gagal. Tidak pernah kembali ke DNS sistem atau teks biasa.',
  'nq_dns_system_privacy':
      'DNS sistem fisik dapat menampilkan nama kueri langsung kepada penyedia DNS jaringan fisik.',
  'nq_dns_scope':
      'Hanya untuk kueri langsung yang dipilih Geo. DNS terowongan tidak berubah.',
  'nq_dns_no_capability':
      'Engine ini tidak dapat memakai DNS langsung terenkripsi. Setelan tersimpan tetap ada. Anda dapat secara tegas memilih DNS sistem.',
  'nq_dns_invalid_name':
      'Masukkan nama DNS tanpa spasi, sintaksis URL, atau karakter pengganti.',
  'nq_dns_invalid_path':
      'Gunakan /path hingga 256 karakter, tanpa kueri, fragmen, atau spasi.',
  'nq_dns_invalid_bootstrap':
      'Gunakan 1–8 IP unicast unik; tanpa alamat tak ditentukan, multicast, siaran, atau tautan-lokal IPv6.',
  'nq_dns_invalid_port': 'Masukkan 0–65535.',
  'nq_dns_invalid_mode': 'Pilih mode DNS yang didukung.',
  'nq_doctor_deep_title': 'Jalankan pemeriksaan jaringan mendalam?',
  'nq_doctor_deep_body':
      'Pemeriksaan mendalam dapat mengirim kueri uji DNS ke resolver yang dikonfigurasi dan memvalidasi jalur QUIC terlindungi. Paling lama 15 detik, dapat dibatalkan, tidak pernah membuat terowongan data kedua, dan tidak pernah mengubah DNS, rute, profil, atau transport Anda.',
  'nq_doctor_deep_run': 'Jalankan pemeriksaan mendalam',
  'nq_doctor_evidence':
      'Pemeriksaan lokal menjelaskan konfigurasi dan status teramati. Itu bukan bukti eksternal bahwa tidak ada kebocoran DNS.',
};

const Map<String, String> kWindowsRecoveryId = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'Status jaringan VPN sebelumnya tidak dapat dipulihkan sepenuhnya. Koneksi VPN baru belum dimulai. Coba sambungkan lagi atau tinjau diagnostik lokal.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows tidak dapat memulihkan status jaringan VPN sebelumnya setelah tiga percobaan otomatis. Coba lagi saat siap, atau tinjau diagnostik lokal.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Perbaikan otomatis dihentikan karena status jaringan Windows sebelumnya tidak dapat diverifikasi dengan aman. Mulai ulang Agent atau perbarui Usque, lalu tinjau diagnostik lokal.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Pemulihan jaringan Windows memakan waktu lebih lama dari yang diharapkan. Koneksi VPN baru belum dimulai. Tunggu pemulihan selesai sebelum mencoba lagi.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Status jaringan berubah atau masih digunakan sesi lain. Pemulihan otomatis dihentikan untuk melindungi koneksi aktif.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Agent Windows ini tidak mendukung pemulihan otomatis yang aman. Perbarui aplikasi dan Agent bersama-sama, lalu coba lagi.',
};

const String kWindowsAdapterCleanupId =
    'Adapter Wintun sebelumnya tidak dapat dihapus atau penghapusannya tidak dapat diverifikasi. Koneksi VPN baru belum dimulai.';

const Map<String, String> kL4Id = <String, String>{
  'l4_quic_not_ready': 'Menunggu sesi QUIC yang siap',
  'l4_unsupported_packets': 'Paket tidak didukung atau rusak ditolak',
  'l4_budget_rejections': 'Penerimaan sumber daya ditolak',
  'l4_not_applicable': 'Tidak berlaku (L4)',
  'l4_mode': 'L4 (eksperimental)',
  'l4_explanation':
      'Hanya TCP melalui HTTP/3. Mendukung VPN/TUN, SOCKS5, dan HTTP; DNS TUN diubah menjadi TCP. Auto tidak pernah memilih L4. UDP lain, ping jarak jauh, fragmen IP, dan header ekstensi tidak didukung; beberapa aplikasi mungkin tidak berfungsi.',
  'l4_unsupported':
      'Mesin ini belum menyatakan dukungan L4 lengkap. L4 tidak dapat diaktifkan.',
  'l4_sni_identity':
      'Hanya baca: diturunkan dari identitas akun yang dimuat. SNI CONNECT-IP yang ada dipertahankan.',
  'l4_edge_requires_l4':
      'DNS yang diselesaikan di tepi memerlukan L4. Pilih mode DNS proksi lain sebelum beralih ke Auto, H3, atau H2.',
  'proxy_dns_edge_resolved':
      'Tepi Cloudflare (hanya L4; tanpa pencarian lokal)',
  'l4_verified': 'L4 CONNECT terverifikasi',
  'l4_unverified': 'QUIC siap; L4 CONNECT belum terverifikasi',
  'l4_status_unknown': 'Status verifikasi L4 tidak diketahui',
  'l4_sessions': 'Sesi / pengosongan',
  'l4_flows': 'Aliran aktif / menunggu',
  'l4_connect': 'CONNECT berhasil / gagal / habis waktu',
  'l4_buffers': 'Anggaran penyangga aplikasi yang terpakai (byte)',
  'l4_backpressure': 'Tekanan balik kirim / terima',
  'l4_tun_flows': 'TUN TCP / setengah terbuka',
  'l4_udp': 'Paket UDP yang ditolak',
  'l4_dns': 'Konversi DNS berhasil / gagal / habis waktu',
  'l4_migration': 'Aliran dipertahankan migrasi / diakhiri pembangunan ulang',
  'l4_na':
      'Kontrol alamat CONNECT-IP, antrean DATAGRAM, MTU muatan dalam, dan batas waktu UDP: tidak berlaku di L4.',
};

const Map<String, String> kNetworkSettingsId = <String, String>{
  'settings_applying': 'Disimpan, sedang diterapkan',
  'settings_applied': 'Disimpan dan diterapkan',
  'settings_deferred': 'Disimpan, berlaku pada koneksi manual berikutnya',
  'settings_failed': 'Disimpan, penerapan gagal',
  'settings_unknown': 'Hasil belum dikonfirmasi',
  'settings_saved': 'Disimpan',
  'settings_unsupported':
      'Mulai ulang atau perbarui Engine untuk menyimpan pengaturan jaringan.',
  'settings_save_failed':
      'Pengaturan tidak dapat disimpan. Suntingan Anda tetap ada.',
  'settings_reconnect': 'Hubungkan ulang',
};
