/// Supplemental feature strings for Indonesian.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowId = <String, String>{
  'local_proxy_settings': 'Pengaturan proksi lokal',
  'proxy_switches_hint':
      'Perubahan sakelar disimpan otomatis. Terapkan perubahan alamat pendengar dan DNS dengan Terapkan perubahan.',
  'cc_label': 'Kontrol kongesti HTTP/3',
  'cc_help': 'Berlaku pada koneksi manual berikutnya.',
  'cc_upgrade': 'Perbarui Usque di Pengaturan untuk memakai opsi ini.',
  'cc_h2': 'Opsi ini hanya memengaruhi koneksi HTTP/3.',
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
  'home_traffic_idle': 'Lalu lintas muncul setelah terhubung',
  'home_traffic_waiting': 'Menunggu data lalu lintas',
  'home_traffic_unavailable': 'Riwayat lalu lintas tidak tersedia',
  'home_traffic_stale': 'Pembaruan lalu lintas tertunda',
  'home_outputs_next': 'Tersedia setelah terhubung',
  'home_outputs_retry': 'VPN dan proksi untuk koneksi berikutnya',
  'connection_protection_group': 'Koneksi & perlindungan',
  'proxy_routing_group': 'Proksi & perutean',
  'application_group': 'Aplikasi',
  'proxy_settings_link': 'Alamat listener, port, autentikasi, dan DNS.',
  'proxy_auth_separate':
      'Gunakan Simpan nama pengguna dan kata sandi di bawah untuk menerapkan perubahan ini.',
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
      'Menggunakan DNS jaringan saat ini; pemeriksaan DNS terenkripsi tidak berlaku.',
  'nq_finding_unsupported':
      'Perbarui Usque untuk menggunakan DNS terenkripsi. Kueri tidak akan beralih ke DNS tanpa enkripsi.',
  'nq_finding_dns_custom_valid':
      'Konfigurasi DNS terenkripsi kustom valid. Cadangan teks biasa dinonaktifkan.',
  'nq_finding_stale': 'Bacaan kedaluwarsa atau jaringan fisik berubah.',
  'nq_finding_rtt_high': 'Waktu bolak-balik terukur lebih tinggi dari biasa.',
  'nq_finding_healthy':
      'Pengukuran koneksi yang tersedia berada dalam kisaran normal.',
  'nq_finding_loss_high':
      'Kehilangan paket pada selang pengukuran ini lebih tinggi dari biasa.',
  'nq_finding_queue_pressure':
      'Ada lalu lintas menunggu dikirim, atau data telah dibuang selama koneksi ini.',
  'nq_finding_pmtu_degraded':
      'Usque tidak dapat memastikan ukuran paket yang sesuai untuk koneksi ini.',
  'nq_finding_migration_reconnect':
      'Koneksi ini perlu disambungkan ulang saat berpindah jaringan.',
  'nq_finding_dns_changed':
      'Mode DNS tersimpan berbeda dari koneksi yang sedang berjalan.',
  'nq_finding_dns_runtime': 'DNS terenkripsi berfungsi.',
  'nq_finding_dns_degraded':
      'DNS terenkripsi bermasalah. Kueri yang gagal tidak akan memakai DNS jaringan tanpa enkripsi.',
  'nq_finding_probe_unsafe':
      'Pengukuran ini tidak tersedia pada status saat ini.',
  'nq_finding_probe_success': 'Pemeriksaan ini lulus.',
  'nq_finding_probe_cancelled': 'Pemeriksaan ini dibatalkan.',
  'nq_finding_probe_timeout': 'Pemeriksaan diagnostik melewati batas waktu',
  'nq_finding_probe_failed': 'Pemeriksaan ini gagal.',
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
      'Versi ini tidak menampilkan kualitas koneksi. Anda tetap dapat menyambung dan memutuskan. Periksa pembaruan di Pengaturan.',
  'nq_empty': 'Hubungkan untuk melihat pengukuran.',
  'nq_stale_help': 'Pembaruan terhenti. Menampilkan hasil pengukuran terakhir.',
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
  'nq_pmtu_help':
      'Ini adalah ukuran paket yang dapat dibawa jalur jaringan. Usque memeriksanya otomatis untuk mengurangi paket hilang. Pemeriksaan tidak menaikkan MTU VPN di Pengaturan jaringan lanjutan.',
  'nq_migration': 'Migrasi jaringan',
  'nq_migration_help':
      'Usque mencoba mempertahankan koneksi saat berpindah jaringan, misalnya dari Wi-Fi ke seluler. Keduanya harus memakai versi IP yang sama, IPv4 atau IPv6. Hanya satu jaringan digunakan setiap saat; kecepatannya tidak digabung.',
  'nq_attempts': 'Percobaan',
  'nq_successes': 'Berhasil',
  'nq_failures': 'Gagal',
  'nq_last_duration': 'Durasi terakhir',
  'nq_direct_dns': 'DNS langsung',
  'nq_system_dns': 'DNS jaringan saat ini',
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
      'Jaringan baru tidak dapat memakai versi IP yang sama. Sambungkan ulang.',
  'nq_reason_socket_protect_failed':
      'Usque tidak dapat memakai jaringan baru dengan aman. Jika koneksi belum pulih, sambungkan ulang secara manual.',
  'nq_reason_generation_changed_during_setup':
      'Jaringan berubah lagi selama penyiapan.',
  'nq_reason_peer_cid_unavailable':
      'Server tidak dapat mempertahankan koneksi di jaringan baru. Jika belum pulih, sambungkan ulang secara manual.',
  'nq_reason_local_cid_unavailable':
      'Usque tidak dapat mempertahankan koneksi di jaringan baru. Jika belum pulih, sambungkan ulang secara manual.',
  'nq_reason_path_probe_rejected':
      'Jaringan baru gagal dalam pemeriksaan koneksi. Pastikan jaringan dapat mengakses Internet.',
  'nq_reason_path_validation_timeout':
      'Jaringan baru tidak merespons tepat waktu. Periksa jaringan dan sambungkan ulang jika perlu.',
  'nq_reason_superseded': 'Jaringan berubah lagi sebelum perpindahan selesai.',
  'nq_reason_promotion_failed':
      'Usque tidak dapat menyelesaikan perpindahan jaringan dengan aman. Jika koneksi belum pulih, sambungkan ulang secara manual.',
  'nq_reason_connection_closed':
      'Koneksi terputus saat berpindah jaringan. Sambungkan kembali.',
  'nq_reason_unsupported': 'Migrasi tidak tersedia pada koneksi ini.',
  'nq_reason_unknown': 'Tidak ada alasan yang didukung.',
  'nq_dns_custom': 'Resolver terenkripsi kustom',
  'nq_dns_server': 'Domain server DNS',
  'nq_dns_path': 'Jalur HTTPS',
  'nq_dns_port': 'Port (0 memakai nilai baku)',
  'nq_dns_bootstrap': 'Alamat IP server DNS',
  'nq_dns_bootstrap_help':
      'Masukkan 1–8 IP dari penyedia DNS, satu per baris, misalnya 1.1.1.1. Usque langsung menghubungi alamat ini tanpa mencari nama server terlebih dahulu.',
  'nq_dns_no_fallback':
      'Jika DNS langsung terenkripsi gagal, kueri gagal. Tidak pernah kembali ke DNS sistem atau teks biasa.',
  'nq_dns_system_privacy':
      'Penyedia DNS jaringan saat ini mungkin melihat domain yang diminta oleh lalu lintas langsung.',
  'nq_dns_scope':
      'Untuk lalu lintas yang cocok dengan aturan negara koneksi langsung. DNS lalu lintas VPN tidak berubah.',
  'nq_dns_no_capability':
      'Perbarui Usque untuk memakai DNS terenkripsi pada koneksi langsung. Pengaturan disimpan. Anda dapat memilih DNS jaringan saat ini jika menerima dampak privasinya.',
  'nq_dns_invalid_name':
      'Masukkan domain seperti dns.example.com, tanpa https://, port, atau spasi.',
  'nq_dns_invalid_path':
      'Masukkan jalur seperti /dns-query, maksimal 256 karakter. Hapus spasi dan bagian yang diawali ? atau #.',
  'nq_dns_invalid_bootstrap': 'Masukkan 1–8 alamat IP server.',
  'nq_dns_invalid_port': 'Masukkan port 1–65535, atau 0 untuk nilai bawaan.',
  'nq_dns_invalid_mode': 'Pilih mode DNS yang didukung.',
  'nq_doctor_deep_title': 'Jalankan pemeriksaan jaringan mendalam?',
  'nq_doctor_deep_body':
      'Pemeriksaan dapat mengirim lalu lintas uji. Berlangsung hingga 15 detik dan dapat dibatalkan. Pengaturan koneksi Anda tidak akan berubah.',
  'nq_doctor_deep_run': 'Jalankan pemeriksaan mendalam',
  'nq_doctor_evidence':
      'Pemeriksaan ini tidak dapat memastikan apakah terjadi kebocoran DNS.',
};

const Map<String, String> kWindowsRecoveryId = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Komponen koneksi Usque perlu diperbarui bersama. Periksa pembaruan di Pengaturan. Koneksi VPN baru belum dimulai.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'Pembersihan koneksi VPN sebelumnya belum selesai. Tutup Usque sepenuhnya dan buka kembali. Jika tetap gagal, buka Diagnostik.',
  'WINDOWS_RECOVERY_FAILED':
      'Status jaringan VPN sebelumnya tidak dapat dipulihkan sepenuhnya. Koneksi VPN baru belum dimulai. Coba sambungkan lagi atau tinjau diagnostik lokal.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows tidak dapat memulihkan status jaringan VPN sebelumnya setelah tiga percobaan otomatis. Coba lagi saat siap, atau tinjau diagnostik lokal.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Perbaikan otomatis dihentikan karena pemulihan pengaturan VPN sebelumnya tidak dapat dipastikan aman. Periksa pembaruan di Pengaturan; jika berlanjut, ekspor paket dari Diagnostik.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Pemulihan jaringan Windows memakan waktu lebih lama dari yang diharapkan. Koneksi VPN baru belum dimulai. Tunggu pemulihan selesai sebelum mencoba lagi.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Status jaringan berubah atau masih digunakan sesi lain. Pemulihan otomatis dihentikan untuk melindungi koneksi aktif.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Instalasi ini tidak dapat memulihkan pengaturan VPN sebelumnya secara otomatis. Perbarui Usque di Pengaturan lalu coba lagi.',
};

const String kWindowsAdapterCleanupId =
    'Adaptor jaringan virtual dari koneksi sebelumnya tidak dapat dihapus atau penghapusannya belum dapat dipastikan. Koneksi VPN baru belum dimulai.';

const Map<String, String> kL4Id = <String, String>{
  'l4_quic_not_ready': 'Menyiapkan koneksi L4',
  'l4_unsupported_packets': 'Paket tidak didukung atau rusak ditolak',
  'l4_budget_rejections': 'Penerimaan sumber daya ditolak',
  'l4_not_applicable': 'Tidak berlaku (L4)',
  'l4_mode': 'L4 (eksperimental)',
  'l4_transport_hint':
      'Hanya TCP. Aplikasi yang memerlukan UDP mungkin tidak berfungsi. Mode otomatis tidak memilih L4.',
  'l4_explanation':
      'L4 meneruskan TCP lewat HTTP/3 dan mendukung VPN serta proksi SOCKS5 dan HTTP. Kueri DNS VPN diubah menjadi TCP. Aplikasi yang membutuhkan UDP lain, Ping jarak jauh, fragmen IP, atau header ekstensi mungkin tidak berfungsi. Pilih L4 manual; mode otomatis tidak memilihnya.',
  'l4_unsupported':
      'Versi Usque ini tidak mendukung L4. Periksa pembaruan di Pengaturan.',
  'l4_sni_identity':
      'Diatur otomatis oleh akun. Nama server mode koneksi lain tetap disimpan.',
  'l4_edge_requires_l4':
      'DNS yang diselesaikan di tepi memerlukan L4. Pilih mode DNS proksi lain sebelum beralih ke Auto, H3, atau H2.',
  'proxy_dns_edge_resolved':
      'Tepi Cloudflare (hanya L4; tanpa pencarian lokal)',
  'l4_verified': 'L4 telah berhasil membuat koneksi aplikasi',
  'l4_unverified': 'Server terhubung; koneksi aplikasi belum dikonfirmasi',
  'l4_status_unknown': 'Status koneksi aplikasi belum dapat dipastikan',
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
      'Tutup Usque sepenuhnya, buka kembali, lalu simpan lagi. Jika gagal, periksa pembaruan di Pengaturan.',
  'settings_save_failed':
      'Pengaturan tidak dapat disimpan. Suntingan Anda tetap ada.',
  'settings_reconnect': 'Hubungkan ulang',
};

const Map<String, String> kChainId = <String, String>{
  "batch_title": "Impor konfigurasi",
  "batch_counts":
      "Siap: {ready} · Belum lengkap: {pending} · Gagal: {failed} · Tersimpan: {saved}",
  "batch_ready": "Siap diimpor",
  "batch_pending": "Lengkapi nama atau kredensial",
  "batch_saved": "Diimpor",
  "batch_close": "Tutup",
  "batch_import": "Impor entri valid ({count})",
  "batch_checking": "Memeriksa {done} dari {total}",
  "batch_saving": "Menyimpan konfigurasi…",
  "batch_uncertain":
      "Penyimpanan terputus. Tutup dan periksa pustaka sebelum mengimpor ulang; beberapa entri mungkin sudah tersimpan.",
  "file_count_limit": "Pilih maksimal 128 berkas sekaligus.",
  'duplicate_directive': 'Direktif ini hanya boleh muncul sekali.',
  'mixed_protocols':
      'Semua titik akhir remote harus memakai angkutan TCP atau UDP yang sama.',
  'conflicting_protocol':
      'Nilai remote bertentangan dengan pengaturan angkutan global.',
  'too_many_endpoints': 'Gunakan paling banyak 16 titik akhir remote.',
  'conflicting_authentication':
      'CLIENT_CERT bertentangan dengan sertifikat tersemat atau mode autentikasi.',
  'serialized_size_limit':
      'Catatan terenkripsi akan melampaui batas ukuran penyimpanan.',
  'multi_endpoint_unavailable':
      'Perbarui mesin untuk memakai konfigurasi dengan beberapa titik akhir.',
  'candidates': 'Titik akhir awal',
  'random_order':
      'Titik akhir dicoba dalam urutan acak baru pada setiap koneksi.',
  'file_order': 'Titik akhir dicoba sesuai urutan berkas.',
  'attempting': 'Titik akhir yang dicoba',
  'actual_endpoint': 'Titik akhir yang terhubung',
  'attempt_failures': 'Percobaan yang gagal',
  'failure_transport': 'angkutan tertutup',
  'failure_authentication': 'autentikasi',
  'failure_certificate': 'sertifikat',
  'failure_configuration': 'konfigurasi',
  'failure_address_changed': 'alamat berubah',
  'failure_protocol': 'protokol',
  'failure_cleanup': 'pembersihan',
  'failure_reason': 'Kegagalan: {reason}.',
  'manage': 'Kelola',
  'dns_fallback': 'DNS terowongan (OpenVPN dapat menegosiasikan DNS)',
  'dns_unavailable_title': 'Tidak ada DNS lewat pintu keluar ini',
  'dns_unavailable':
      'Tidak ada peladen DNS yang dapat dijangkau lewat pintu keluar ini. Gunakan alamat IP atau pilih pintu keluar lain dengan DNS yang dapat dijangkau.',
  'authentication_failed':
      'Autentikasi gagal. Perbarui kredensial sebelum menghubungkan lagi.',
  'profile_limit': 'Pustaka konfigurasi penuh (128 profil).',
  'metadata_limit': 'Metadata pustaka konfigurasi sudah penuh.',
  'title': 'Proksi berantai',
  'subtitle': 'Pilih pintu keluar yang dicapai melalui WARP.',
  'source': 'Sumber pintu keluar',
  'enable': 'Aktifkan proksi berantai',
  'import_file': 'Impor berkas',
  'paste': 'Tempel konfigurasi',
  'profiles': 'Konfigurasi tersimpan',
  'empty': 'Impor konfigurasi untuk memilih pintu keluar.',
  'empty_hint_openvpn':
      'Impor berkas .ovpn atau tempel teksnya. Titik akhir TCP dan UDP, sertifikat tersemat, serta nama pengguna/kata sandi didukung.',
  'empty_hint_wireguard':
      'Impor berkas .conf atau tempel teksnya. Satu bagian [Interface] dan satu [Peer] didukung.',
  'import_limits':
      'Konfigurasi harus berupa teks UTF-8 hingga 128 KiB. Jika pemilih berkas tidak ada, tempel teksnya.',
  'enable_to_choose': 'Aktifkan proksi berantai untuk memilih konfigurasi.',
  'select_required': 'Pilih konfigurasi tersimpan sebelum menerapkan.',
  'pending_disable': 'Menunggu: nonaktifkan proksi berantai',
  'apply_reconnect': 'Terapkan dan hubungkan ulang',
  'requires_connect_ip': 'Memerlukan CONNECT-IP',
  'menu': 'Tindakan konfigurasi',
  'preview': 'Periksa konfigurasi',
  'save_import': 'Simpan konfigurasi',
  'name': 'Nama',
  'configuration': 'Teks konfigurasi',
  'file_loaded': 'Konfigurasi dimuat dari berkas ({lines} baris).',
  'username': 'Nama pengguna',
  'password': 'Kata sandi',
  'key_password': 'Kata sandi kunci pribadi',
  'show_password': 'Tampilkan kata sandi',
  'hide_password': 'Sembunyikan kata sandi',
  'credentials': 'Perbarui kredensial',
  'rename': 'Ubah nama',
  'delete': 'Hapus',
  'cancel': 'Batal',
  'save': 'Simpan',
  'apply': 'Terapkan perubahan',
  'clear': 'Hapus pilihan',
  'current': 'Koneksi saat ini',
  'saved': 'Pilihan tersimpan',
  'draft': 'Pilihan yang menunggu',
  'disconnected': 'Tidak terhubung',
  'disabled': 'Tidak diaktifkan',
  'enabled_idle': 'Aktif · tidak terhubung',
  'disconnecting': 'Memutuskan koneksi',
  'file_read_failed': 'Berkas konfigurasi tidak dapat dibaca.',
  'file_encoding_invalid': 'Berkas konfigurasi harus berupa teks UTF-8.',
  'file_busy': 'Pemilih berkas sudah terbuka.',
  'connected': 'Terhubung',
  'connecting': 'Menghubungkan',
  'error': 'Koneksi gagal',
  'no_selection': 'Belum ada konfigurasi yang dipilih',
  'l4': 'Konfigurasi ini memerlukan mode CONNECT-IP.',
  'switch_mode': 'Beralih ke CONNECT-IP dan terapkan',
  'unsupported': 'Mesin ini tidak mendukung sumber pintu keluar ini.',
  'scope':
      'Aturan langsung eksplisit yang sudah ada tetap berlaku. Lalu lintas lain memakai pintu keluar yang dipilih.',
  'allowed': 'Tujuan yang diizinkan',
  'dns': 'DNS',
  'addresses': 'Alamat terowongan',
  'address_family': 'Keluarga alamat',
  'transport': 'Angkutan',
  'endpoint': 'Peladen',
  'restricted': 'Tujuan di luar AllowedIPs diblokir pada jalur proksi.',
  'delete_confirm':
      'Hapus konfigurasi tersimpan ini? Berkas asli yang diimpor tidak berubah.',
  'profile_in_use':
      'Pilih konfigurasi lain atau hapus pilihan tersimpan sebelum menghapus konfigurasi ini.',
  'stale_revision': 'Konfigurasi berubah. Segarkan daftar, lalu coba lagi.',
  'secure_storage_failed':
      'Konfigurasi terenkripsi tidak dapat dibaca atau disimpan.',
  'invalid_configuration':
      'Konfigurasi tidak valid atau berisi opsi yang tidak didukung.',
  'looks_like_wireguard':
      'Ini tampak seperti konfigurasi WireGuard. Ubah sumber pintu keluar ke WireGuard.',
  'looks_like_openvpn':
      'Ini tampak seperti konfigurasi OpenVPN. Ubah sumber pintu keluar ke OpenVPN.',
  'error_location': '{message} ({field}, baris {line})',
  'error_field': '{message} ({field})',
  'file_unavailable': 'Pemilih berkas tidak tersedia. Tempel teks konfigurasi.',
  'invalid_size_or_encoding':
      'Gunakan konfigurasi UTF-8 berukuran paling besar 128 KiB.',
  'unsupported_directive': 'Direktif OpenVPN ini tidak didukung.',
  'unsupported_or_duplicate_field':
      'Bidang ini tidak didukung atau terduplikasi.',
  'unsupported_or_duplicate_section':
      'Gunakan satu bagian Interface dan satu Peer.',
  'missing_field': 'Ada bidang wajib yang belum diisi.',
  'invalid_name': 'Gunakan nama 1 hingga 64 karakter tanpa karakter kontrol.',
  'invalid_key':
      'Kunci harus berupa kunci Base64 yang valid sepanjang 32 byte.',
  'checking': 'Memeriksa konfigurasi…',
  'changed': 'Perubahan disimpan',
};
