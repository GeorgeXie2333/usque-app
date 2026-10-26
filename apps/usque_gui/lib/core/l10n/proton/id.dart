const kProtonId = <String, String>{
  'fallback_account':
      'Tidak dapat menghubungi Proton. Pilih akun WARP yang sudah ada dan coba lagi.',
  'fallback_failed':
      'Tidak dapat terhubung melalui WARP. Periksa akun WARP yang dipilih dan jaringan, lalu coba lagi.',
  'fallback_starting': 'Menghubungkan melalui WARP…',
  'fallback_active': 'Operasi akun ini menggunakan WARP, termasuk verifikasi.',
  'connection_api':
      'Permintaan API Proton gagal. Coba lagi atau periksa akun Proton di Proksi berantai.',
  'connection_authentication':
      'Otorisasi Proton gagal. Buka Proksi berantai dan masuk lagi ke Proton.',
  'connection_certificate':
      'Proton menolak sertifikat koneksi dan pemulihan gagal. Hubungkan ulang atau masuk lagi ke Proton.',
  'connection_restricted':
      'Server Proton dibatasi sementara. Coba lagi atau pilih server lain.',
  'connection_sessions':
      'Batas koneksi Proton secara bersamaan telah tercapai. Putuskan sesi Proton lain dan coba lagi.',
  'connection_key':
      'Proton mendeteksi kunci koneksi ini dalam beberapa sesi. Putuskan sesi lainnya dan coba lagi.',
  'connection_policy':
      'Proton membatasi koneksi ini berdasarkan kebijakan akun atau penggunaan. Periksa akun atau pilih server lain.',
  'connection_server':
      'Server Proton tidak dapat mengizinkan koneksi. Coba server lain.',
  'connection_protocol':
      'Respons koneksi Proton tidak dapat divalidasi. Coba lagi atau pilih server lain.',
  'connection_transport':
      'Tidak dapat menghubungi Proton. Periksa jaringan atau pilih server lain.',
  'connection_reconnect': 'Otorisasi Proton diperbarui. Menghubungkan ulang…',
  'connection_review':
      'Buka Proksi berantai untuk memeriksa akun Proton dan server yang dipilih.',
  'connection_retry': 'Coba lagi koneksi Proton atau pilih server lain.',
  'title': 'Proton VPN',
  'guest': 'Lanjutkan sebagai tamu',
  'login': 'Masuk ke Proton',
  'username': 'Nama pengguna',
  'password': 'Kata sandi',
  'totp': 'Kode aplikasi autentikator',
  'submit': 'Lanjutkan',
  'verify': 'Selesaikan verifikasi',
  'verification_hint': 'Selesaikan teka-teki di jendela verifikasi resmi.',
  'choose_node': 'Pilih server gratis',
  'working': 'Menyiapkan Proton VPN…',
  'waiting_totp': 'Masukkan kode aplikasi autentikator untuk melanjutkan.',
  'waiting_verification': 'Verifikasi diperlukan',
  'waiting_node': 'Pilih server untuk menyelesaikan penyiapan.',
  'completed':
      'Akun disimpan. Pilih akun tersebut dan terapkan perubahan untuk menggunakan Proton VPN.',
  'cancelled': 'Penyiapan dibatalkan',
  'failed': 'Penyiapan Proton gagal. Coba lagi saat Anda siap.',
  'result_unconfirmed':
      'Hasil penyiapan Proton belum dikonfirmasi. Muat ulang status untuk melanjutkan atau batalkan penyiapan yang ada sebelum memulai lagi.',
  'unavailable': 'Proton VPN tidak tersedia di mesin ini.',
  'bad_credentials':
      'Periksa nama pengguna Proton, kata sandi, atau kode autentikator Anda.',
  'rate_limited':
      'Proton membatasi permintaan ini. Tunggu sebelum mencoba lagi.',
  'network':
      'Tidak dapat menghubungi Proton. Periksa jaringan atau VPN, lalu coba lagi.',
  'verification_failed':
      'Verifikasi tidak dapat diselesaikan. Mulai penyiapan lagi.',
  'verification_invalid':
      'Proton menolak hasil verifikasi. Mulai penyiapan lagi untuk mendapatkan tantangan baru.',
  'accounts': 'Akun Proton',
  'add_account': 'Tambah akun',
  'account_actions': 'Tindakan akun',
  'resolve_on_connect': 'Server dan otorisasi diperiksa saat menghubungkan.',
  'account': 'Akun Proton',
  'signed_in': 'Sudah masuk',
  'session_requires_login': 'Sesi memerlukan masuk ulang',
  'selected_server': 'Server',
  'automatic_server': 'Otomatis',
  'automatic_hint': 'Pilih server dengan latensi terendah saat menghubungkan.',
  'available_servers': 'Server yang tersedia',
  'choose_server': 'Pilih server',
  'search_servers': 'Cari berdasarkan nama, negara, atau kota',
  'load_servers': 'Muat daftar server',
  'change_server': 'Ganti server',
  'refresh_servers': 'Muat ulang daftar',
  'remove_account': 'Hapus akun',
  'remove_account_confirm':
      'Hapus akun ini dan pilihan server yang disimpan? Keluar juga mencabut sesi yang disimpan aplikasi ini. Menghapus hanya secara lokal tidak mengubah sesi jarak jauh. Pilihan proksi berantai yang disimpan akan dihapus.',
  'forget_local': 'Hapus hanya secara lokal',
  'sign_out_remove': 'Keluar dan hapus',
  'current_account': 'Akun terhubung',
  'saved_account': 'Pilihan tersimpan',
  'next_connection': 'Pilihan server disimpan untuk koneksi berikutnya.',
  'disconnect_to_change':
      'Putuskan akun ini sebelum mengganti server atau menghapusnya.',
  'cleanup_pending':
      'Pencabutan sesi sebelumnya belum dapat dikonfirmasi. Keluar lagi atau pilih secara tegas untuk menghapus akun hanya secara lokal.',
  'selected_server_unavailable':
      'Server yang dipilih tidak tersedia. Pilih server lain atau Otomatis.',
  'close': 'Tutup',
  'verification_error': 'Kesalahan verifikasi',
  'refresh': 'Perbarui otorisasi atau ganti server',
  'logout': 'Keluar dan hapus',
  'forget': 'Hapus hanya dari perangkat ini',
  'forget_confirm':
      'Hapus konfigurasi Proton dan sesi tersimpannya dari perangkat ini? Ini tidak mencabut sesi Proton jarak jauh.',
  'removed': 'Akun Proton dihapus dari perangkat ini.',
  'needs_login': 'Masuk lagi',
  'expires': 'Otorisasi berakhir',
  'unsupported_second_factor':
      'Akun ini memerlukan metode masuk yang tidak didukung di sini.',
  'reauthentication_required': 'Masuk lagi untuk memperbarui sesi Proton ini.',
  'profile_in_use': 'Putuskan akun ini sebelum mengubah atau menghapusnya.',
  'session_busy':
      'Operasi Proton lain masih berjalan. Tunggu atau batalkan terlebih dahulu.',
  'api_rejected':
      'Proton menolak permintaan ini. Kode di bawah menunjukkan respons server.',
  'session_not_allowed': 'Proton tidak mengizinkan klien ini membuat sesi.',
  'credentialless_disabled': 'Akses tamu Proton saat ini tidak tersedia.',
  'no_free_nodes':
      'Proton tidak mengembalikan server WireGuard yang tersedia untuk akun ini.',
  'invalid_directory':
      'Proton mengembalikan daftar server yang tidak didukung.',
  'invalid_client_configuration':
      'Proton tidak mengembalikan port WireGuard yang dapat digunakan.',
  'stage': 'Langkah yang gagal',
  'stage_anonymous_session': 'Buat sesi anonim',
  'stage_credentialless': 'Buat akun tamu',
  'stage_session_scopes': 'Periksa otorisasi tamu',
  'stage_protected_user': 'Periksa akun',
  'stage_vpn_permissions': 'Periksa akses VPN',
  'stage_session_refresh': 'Perbarui sesi',
  'stage_srp_info': 'Mulai proses masuk',
  'stage_srp_proof': 'Verifikasi proses masuk',
  'stage_totp': 'Verifikasi kode autentikator',
  'stage_directory': 'Muat server',
  'stage_certificate': 'Minta otorisasi VPN',
  'stage_secure_storage': 'Simpan konfigurasi',
  'stage_session_revoke': 'Cabut sesi',
  'expired':
      'Permintaan verifikasi atau masukan kedaluwarsa. Mulai penyiapan lagi.',
  'underlay_changed': 'Lingkungan jaringan berubah. Mulai penyiapan lagi.',
  'remote_unknown': 'Pencabutan sesi jarak jauh tidak dapat dikonfirmasi.',
  'cancelled_remote_unknown':
      'Dibatalkan di perangkat ini. Pencabutan sesi jarak jauh tidak dapat dikonfirmasi.',
  'privacy':
      'Kata sandi Anda hanya digunakan untuk masuk. Sesi dan kunci dilindungi di perangkat ini.',
  'empty': 'Masuk atau lanjutkan sebagai tamu untuk menambahkan akun Proton.',
};
