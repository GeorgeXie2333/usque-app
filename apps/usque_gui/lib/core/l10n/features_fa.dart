/// Supplemental feature strings for Persian.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowFa = <String, String>{
  'local_proxy_settings': 'تنظیمات پراکسی محلی',
  'proxy_switches_hint':
      'تغییر کلیدها خودکار ذخیره می‌شود. ویرایش آدرس‌های شنود و DNS را با «اعمال تغییرات» اعمال کنید.',
  'cc_label': 'کنترل ازدحام HTTP/3',
  'cc_help': 'در اتصال دستی بعدی‌تان اعمال می‌شود.',
  'cc_upgrade': 'Usque را از تنظیمات به‌روز کنید.',
  'cc_h2': 'این گزینه فقط بر اتصال‌های HTTP/3 اثر دارد.',
  'cc_saved': 'ذخیره شد',
  'cc_pending': 'در انتظار اتصال دستی بعدی.',
  'save_changes': 'اعمال تغییرات',
  'saving_changes': 'در حال اعمال تغییرات…',
  'unsaved_changes': 'تغییرات اعمال‌نشده',
  'changes_applied': 'تغییرات اعمال شد',
  'changes_apply_hint': 'ویرایش‌ها فقط پس از اعمال مؤثر می‌شوند.',
  'changes_failed':
      'تغییرات اعمال نشد. مقدارهای ذخیره‌شده را بازبینی کنید و دوباره تلاش کنید.',
  'form_errors': 'پیش از اعمال تغییرات، فیلدهای برجسته‌شده را بررسی کنید.',
  'discard_changes_title': 'تغییرات اعمال‌نشده کنار گذاشته شود؟',
  'discard_changes_body':
      'ویرایش‌های شما هنوز اعمال نشده‌اند. برای حفظشان به ویرایش ادامه دهید، یا برای ترک صفحه آن‌ها را کنار بگذارید.',
  'keep_editing': 'ادامهٔ ویرایش',
  'discard_changes': 'کنار گذاشتن تغییرات',
  'invalid_port': 'یک پورت از 1 تا 65535 وارد کنید.',
  'listener_exposure': 'نشانی‌های شنونده اجازهٔ دسترسی از شبکهٔ محلی می‌دهند',
  'invalid_ipv4': 'یک نشانی IPv4 معتبر وارد کنید، مثلاً 127.0.0.1.',
  'invalid_ipv6': 'یک نشانی IPv6 معتبر وارد کنید، مثلاً ::1.',
  'output_running': 'در حال اجرا',
  'output_waiting': 'فعال · در حال اجرا نیست',
  'output_disabled': 'غیرفعال',
  'output_starting': 'در حال شروع',
  'output_stopping': 'در حال توقف',
  'output_reconnecting': 'در حال اتصال مجدد',
  'output_degraded': 'محدود',
  'output_error': 'خطا',
  'output_unknown': 'وضعیت در دسترس نیست',
  'shared_network_scope': 'تنظیمات شبکه بین همهٔ حساب‌ها مشترک است.',
  'connection_details': 'جزئیات اتصال',
  'home_overview': 'نمای کلی اتصال',
  'home_exit_region': 'منطقهٔ خروج',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'ترافیک',
  'home_traffic_window': '۶۰ ثانیهٔ اخیر',
  'home_traffic_idle': 'ترافیک پس از اتصال نمایش داده می‌شود',
  'home_traffic_waiting': 'در انتظار داده‌های ترافیک',
  'home_traffic_unavailable': 'سابقهٔ ترافیکی وجود ندارد',
  'home_traffic_stale': 'به‌روزرسانی ترافیک با تأخیر انجام می‌شود',
  'home_outputs_next': 'پس از اتصال در دسترس است',
  'home_outputs_retry': 'VPN و پروکسی‌های اتصال بعدی',
  'connection_protection_group': 'اتصال و حفاظت',
  'proxy_routing_group': 'پروکسی و مسیریابی',
  'application_group': 'برنامه',
  'proxy_settings_link': 'نشانی شنونده، پورت‌ها، احراز هویت و DNS.',
  'proxy_auth_separate':
      'برای اعمال این تغییرات، از دکمهٔ «ذخیرهٔ نام کاربری و گذرواژه» در پایین استفاده کنید.',
  'reset_draft_hint':
      'پیش‌فرض‌ها در این فرم بارگذاری می‌شوند. برای مؤثر شدن، تغییرات را اعمال کنید.',
};

const kNetworkQualityFa = <String, String>{
  'nq_range': 'بازه',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'زمان رفت‌وبرگشت',
  'diag_check_quality_packet_loss': 'ازدست‌رفتن بسته',
  'diag_check_quality_queue_pressure': 'فشار صف',
  'diag_check_quality_pmtu': 'MTU مسیر',
  'diag_check_transport_migration_capability': 'مهاجرت در همان خانواده',
  'diag_check_dns_direct_encrypted_configuration': 'پیکربندی DNS مستقیم',
  'diag_check_dns_direct_encrypted_runtime_state': 'اجرای DNS مستقیم',
  'diag_check_dns_direct_encrypted_reachability':
      'دسترسی‌پذیری DNS رمزنگاری‌شده',
  'diag_check_transport_h3_path_validation_probe': 'دست‌دهی مستقل QUIC',
  'nq_finding_unavailable': 'این اندازه‌گیری در وضعیت فعلی در دسترس نیست.',
  'nq_finding_invalid_configuration': 'پیکربندی DNS سفارشی نامعتبر است.',
  'nq_finding_dns_system':
      'DNS شبکهٔ فعلی در حال استفاده است. بررسی DNS رمزگذاری‌شده در این حالت کاربرد ندارد.',
  'nq_finding_unsupported':
      'برای استفاده از DNS رمزگذاری‌شده، Usque را به‌روز کنید. تغییر خودکار به DNS رمزگذاری‌نشده انجام نمی‌شود.',
  'nq_finding_dns_custom_valid':
      'پیکربندی DNS رمزنگاری‌شدهٔ سفارشی معتبر است. بازگشت به متن ساده غیرفعال است.',
  'nq_finding_stale': 'خوانش کهنه است یا شبکهٔ فیزیکی تغییر کرده است.',
  'nq_finding_rtt_high': 'زمان رفت‌وبرگشت اندازه‌گیری‌شده بالاست.',
  'nq_finding_healthy': 'شاخص‌های قابل‌اندازه‌گیری اتصال عادی‌اند.',
  'nq_finding_loss_high': 'ازدست‌رفتن بسته در این بازه بالاست.',
  'nq_finding_queue_pressure':
      'در این اتصال، داده‌هایی در انتظار ارسال هستند یا کنار گذاشته شده‌اند.',
  'nq_finding_pmtu_degraded': 'اندازهٔ مناسب بسته‌ها برای مسیر شبکه تأیید نشد.',
  'nq_finding_migration_reconnect': 'پس از تغییر شبکه، اتصال دوباره لازم است.',
  'nq_finding_dns_changed': 'حالت DNS ذخیره‌شده با اتصال در حال اجرا فرق دارد.',
  'nq_finding_dns_runtime': 'DNS رمزگذاری‌شده کار می‌کند.',
  'nq_finding_dns_degraded':
      'DNS رمزگذاری‌شده مشکل دارد. درخواست‌های ناموفق به DNS رمزگذاری‌نشدهٔ شبکهٔ فعلی فرستاده نشدند.',
  'nq_finding_probe_unsafe': 'این اندازه‌گیری در وضعیت فعلی در دسترس نیست.',
  'nq_finding_probe_success': 'این بررسی موفق بود.',
  'nq_finding_probe_cancelled': 'این بررسی لغو شد.',
  'nq_finding_probe_timeout': 'بررسی تشخیص از مهلت گذشت',
  'nq_finding_probe_failed': 'این بررسی ناموفق بود.',
  'diag_fix_nq_profile':
      'فیلدهای DNS سفارشی و نام گواهی را بازبینی کنید. تأیید TLS را غیرفعال نکنید.',
  'diag_fix_nq_retry': 'منتظر شبکهٔ پایدار بمانید، سپس دوباره تلاش کنید.',
  'diag_fix_nq_network':
      'اتصال محلی را بررسی کنید و پیش از تغییر تنظیمات یک نمونهٔ تازه را مقایسه کنید.',
  'diag_fix_nq_reconnect': 'برای اعمال پیکربندی ذخیره‌شده دوباره متصل شوید.',
  'nav_network_quality': 'کیفیت',
  'network_quality': 'کیفیت شبکه',
  'nq_subtitle': 'اتصال را بخوانید، نه فقط سرعت را.',
  'nq_local_only': 'اندازه‌گیری‌ها فقط محلی است. چیزی بارگذاری نمی‌شود.',
  'nq_doctor': 'اجرای Network Doctor',
  'nq_doctor_help':
      'بررسی‌های استاندارد فقط وضعیت محلی را می‌خوانند. اتصال خارجی باز نمی‌کنند و تنظیمات شما را تغییر نمی‌دهند.',
  'nq_live': 'زنده',
  'nq_stale': 'خوانش‌های کهنه',
  'nq_updated': 'آخرین نمونه',
  'nq_seconds': '{count} ثانیه پیش',
  'nq_good': 'خوب',
  'nq_fair': 'متوسط',
  'nq_poor': 'ضعیف',
  'nq_limited': 'دادهٔ محدود',
  'nq_disconnected': 'قطع‌شده',
  'nq_connecting': 'در حال اتصال',
  'nq_connected': 'متصل',
  'nq_unavailable': 'در دسترس نیست',
  'nq_not_ready': 'آماده نیست',
  'nq_unsupported': 'پشتیبانی نمی‌شود',
  'nq_capability_missing':
      'این نسخه نمی‌تواند کیفیت اتصال را نشان دهد. اتصال و قطع اتصال همچنان کار می‌کنند. Usque را از تنظیمات به‌روز کنید.',
  'nq_empty': 'برای دیدن اندازه‌گیری‌ها متصل شوید.',
  'nq_stale_help':
      'به‌روزرسانی متوقف شده است. آخرین مقادیر نمایش داده می‌شوند.',
  'nq_rtt': 'زمان رفت‌وبرگشت',
  'nq_latest': 'تازه‌ترین',
  'nq_smoothed': 'هموارشده',
  'nq_minimum': 'کمینه',
  'nq_h2_ping': 'PING پروتکل HTTP/2',
  'nq_h3_rtt': 'اندازه‌گیری مسیر QUIC',
  'nq_throughput': 'توان عملیاتی',
  'nq_download': 'دانلود',
  'nq_upload': 'آپلود',
  'nq_one_second': '۱ ثانیه',
  'nq_five_seconds': 'میانگین ۵ ثانیه',
  'nq_loss': 'ازدست‌رفتن بسته',
  'nq_loss_h2': 'HTTP/2 ازدست‌رفتن بستهٔ قابل‌مقایسه ارائه نمی‌دهد.',
  'nq_loss_interval':
      'روی بازهٔ اخیر اندازه‌گیری شده است؛ افت تجمعی کل اتصال نیست.',
  'nq_congestion': 'ازدحام',
  'nq_cwnd': 'پنجرهٔ ازدحام',
  'nq_in_flight': 'بایت‌های در حال ارسال',
  'nq_send_rate': 'نرخ تحویل',
  'nq_h2_window': 'پنجره‌های دریافت HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'اتصال',
  'nq_stalls': 'توقف‌های ظرفیت',
  'nq_pmtu': 'MTU مسیر',
  'nq_outer_pmtu': 'سقف بار UDP بیرونی',
  'nq_inner_payload': 'سقف بار CONNECT-IP',
  'nq_pmtu_help':
      'اندازهٔ بسته‌ای که مسیر شبکه می‌تواند منتقل کند. بررسی خودکار به کاهش از دست رفتن بسته‌ها کمک می‌کند و MTU مربوط به VPN را که در تنظیمات پیشرفتهٔ شبکه تعیین شده افزایش نمی‌دهد.',
  'nq_migration': 'مهاجرت شبکه',
  'nq_migration_help':
      'هنگام جابه‌جایی بین Wi-Fi و اینترنت همراه، تلاش می‌کند اتصال را حفظ کند. هر دو شبکه باید از یک نسخهٔ IP، یعنی IPv4 یا IPv6، استفاده کنند. در هر لحظه فقط یک مسیر شبکه به کار می‌رود و سرعت شبکه‌ها با هم جمع نمی‌شود.',
  'nq_attempts': 'تلاش‌ها',
  'nq_successes': 'موفق',
  'nq_failures': 'ناموفق',
  'nq_last_duration': 'آخرین مدت',
  'nq_direct_dns': 'DNS مستقیم',
  'nq_system_dns': 'DNS شبکهٔ فعلی',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'آماده',
  'nq_degraded': 'مختل',
  'nq_timeouts': 'پایان‌مهلت‌ها',
  'nq_last_rtt': 'آخرین RTT',
  'nq_dns_redacted':
      'نام حل‌کننده‌ها و نشانی‌های راه‌انداز فقط در تنظیمات نشان داده می‌شود.',
  'nq_queues': 'فشار صف',
  'nq_queue_details': 'صف‌های سطح پایین',
  'nq_queue_empty': 'هنوز اندازه‌گیری صفی نیست.',
  'nq_current_capacity': 'جاری / ظرفیت',
  'nq_high_water': 'بیشینهٔ عمق',
  'nq_drops': 'دورریخته‌ها',
  'nq_oldest': 'قدیمی‌ترین مورد',
  'nq_tunToTransport': 'دستگاه → انتقال',
  'nq_proxyToTransport': 'پروکسی → انتقال',
  'nq_transportOutgoing': 'خروجی انتقال',
  'nq_h3DatagramSend': 'داده‌گرام‌های QUIC',
  'nq_h3WireSend': 'خروجی UDP',
  'nq_transportToTun': 'انتقال → دستگاه',
  'nq_transportToProxy': 'انتقال → پروکسی',
  'nq_directDns': 'درخواست‌های DNS مستقیم',
  'nq_unknown_queue': 'صف دیگر',
  'nq_trends': '۶۰ ثانیهٔ اخیر',
  'nq_samples': 'نمونه',
  'nq_pause': 'مکث نمودارها',
  'nq_resume': 'ادامهٔ نمودارها',
  'nq_paused': 'نمودارها متوقف شدند',
  'nq_gaps': 'نمونه‌های ازدست‌رفته شکاف‌اند.',
  'nq_phase_idle': 'بیکار',
  'nq_phase_preparing_socket': 'آماده‌سازی مسیر',
  'nq_phase_probing': 'در حال کاوش',
  'nq_phase_validated': 'اعتبارسنجی شد',
  'nq_phase_promoting': 'تعویض مسیر',
  'nq_phase_stable': 'پایدار',
  'nq_phase_aborted': 'متوقف شد',
  'nq_phase_revalidating': 'اعتبارسنجی مجدد',
  'nq_phase_degraded': 'مختل',
  'nq_phase_unknown': 'آماده نیست',
  'nq_phase_unsupported': 'پشتیبانی نمی‌شود',
  'nq_reason_family_unavailable':
      'شبکهٔ جدید نمی‌تواند از همان نسخهٔ IP استفاده کند. دوباره متصل شوید.',
  'nq_reason_socket_protect_failed':
      'استفادهٔ امن از شبکهٔ جدید ممکن نشد. اگر اتصال خودکار برنگشت، دوباره متصل شوید.',
  'nq_reason_generation_changed_during_setup':
      'هنگام آماده‌سازی شبکه دوباره تغییر کرد.',
  'nq_reason_peer_cid_unavailable':
      'سرور نتوانست اتصال را در شبکهٔ جدید حفظ کند. در صورت نیاز دوباره متصل شوید.',
  'nq_reason_local_cid_unavailable':
      'Usque نتوانست اتصال را در شبکهٔ جدید حفظ کند. در صورت نیاز دوباره متصل شوید.',
  'nq_reason_path_probe_rejected':
      'بررسی اتصال شبکهٔ جدید ناموفق بود. دسترسی به اینترنت را بررسی کنید.',
  'nq_reason_path_validation_timeout':
      'شبکهٔ جدید به‌موقع پاسخ نداد. دسترسی به اینترنت را بررسی کنید و در صورت نیاز دوباره متصل شوید.',
  'nq_reason_superseded': 'پیش از پایان جابه‌جایی، شبکه دوباره تغییر کرد.',
  'nq_reason_promotion_failed':
      'جابه‌جایی شبکه به‌صورت امن کامل نشد. اگر اتصال برنگشت، دوباره تلاش کنید.',
  'nq_reason_connection_closed':
      'هنگام تغییر شبکه اتصال بسته شد. دوباره متصل شوید.',
  'nq_reason_unsupported': 'مهاجرت در این اتصال در دسترس نیست.',
  'nq_reason_unknown': 'دلیل پشتیبانی‌شده‌ای در دسترس نیست.',
  'nq_dns_custom': 'حل‌کنندهٔ رمزنگاری‌شدهٔ سفارشی',
  'nq_dns_server': 'نام دامنهٔ سرور DNS',
  'nq_dns_path': 'مسیر HTTPS',
  'nq_dns_port': 'پورت (0 از پیش‌فرض استفاده می‌کند)',
  'nq_dns_bootstrap': 'نشانی‌های IP سرور DNS',
  'nq_dns_bootstrap_help':
      'از ۱ تا ۸ نشانی IP ارائه‌دهندهٔ DNS را وارد کنید، هر نشانی در یک خط. نمونه: 1.1.1.1. این نشانی‌ها اتصال مستقیم را بدون نیاز به پیدا کردن نشانی از روی نام سرور ممکن می‌کنند.',
  'nq_dns_no_fallback':
      'اگر DNS مستقیم رمزنگاری‌شده شکست بخورد، پرس‌وجو شکست می‌خورد. هرگز به DNS سیستم یا متن ساده برنمی‌گردد.',
  'nq_dns_system_privacy':
      'ارائه‌دهندهٔ DNS شبکهٔ فعلی ممکن است دامنه‌های درخواستی برای ترافیک مستقیم را ببیند.',
  'nq_dns_scope':
      'فقط بر ترافیک مطابق قوانین اتصال مستقیم کشورها و مناطق اثر دارد. DNS ترافیک VPN تغییر نمی‌کند.',
  'nq_dns_no_capability':
      'برای DNS رمزگذاری‌شده در اتصال‌های مستقیم، Usque را به‌روز کنید. تنظیمات ذخیره‌شده حفظ می‌شوند. اگر پیامدهای حریم خصوصی را می‌پذیرید، می‌توانید خودتان «DNS شبکهٔ فعلی» را انتخاب کنید.',
  'nq_dns_invalid_name':
      'نام دامنه‌ای مانند dns.example.com وارد کنید؛ بدون https://، درگاه یا فاصله.',
  'nq_dns_invalid_path':
      'مسیری مانند /dns-query با حداکثر ۲۵۶ نویسه وارد کنید؛ بدون فاصله یا بخش‌های حاوی ? یا #.',
  'nq_dns_invalid_bootstrap': 'از ۱ تا ۸ نشانی IP برای سرور DNS وارد کنید.',
  'nq_dns_invalid_port':
      'درگاهی از ۱ تا ۶۵۵۳۵ وارد کنید، یا برای درگاه پیش‌فرض ۰ بنویسید.',
  'nq_dns_invalid_mode': 'یک حالت DNS پشتیبانی‌شده انتخاب کنید.',
  'nq_doctor_deep_title': 'بررسی‌های عمیق شبکه اجرا شود؟',
  'nq_doctor_deep_body':
      'بررسی‌ها ممکن است ترافیک آزمایشی ارسال کنند. حداکثر ۱۵ ثانیه طول می‌کشند و قابل لغو هستند. تنظیمات اتصال شما تغییر نمی‌کند.',
  'nq_doctor_deep_run': 'اجرای بررسی‌های عمیق',
  'nq_doctor_evidence':
      'این بررسی‌ها نمی‌توانند وجود یا نبود نشت DNS را تأیید کنند.',
};

const Map<String, String> kWindowsRecoveryFa = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'اجزای Usque را از تنظیمات با هم به‌روز کنید. هیچ اتصال VPN جدیدی شروع نشده است.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'پاک‌سازی اتصال قبلی کامل نشده است. Usque را کاملاً ببندید، دوباره باز کنید و تلاش کنید. اگر مشکل ادامه داشت، عیب‌یابی را باز کنید.',
  'WINDOWS_RECOVERY_FAILED':
      'وضعیت شبکهٔ VPN قبلی به‌طور کامل بازیابی نشد. اتصال VPN جدیدی شروع نشد. اتصال را دوباره امتحان کنید یا عیب‌یابی محلی را بررسی کنید.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows پس از سه تلاش خودکار نتوانست وضعیت شبکهٔ VPN قبلی را بازیابی کند. وقتی آماده بودید دوباره تلاش کنید، یا عیب‌یابی محلی را بررسی کنید.',
  'WINDOWS_RECOVERY_BLOCKED':
      'چون بازیابی امن تأیید نشد، ترمیم خودکار متوقف شد. Usque را از تنظیمات به‌روز کنید. اگر مشکل ادامه داشت، از عیب‌یابی گزارش‌ها را خروجی بگیرید.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'بازیابی شبکهٔ Windows بیشتر از انتظار طول می‌کشد. اتصال VPN جدیدی شروع نشده است. پیش از تلاش دوباره صبر کنید تا بازیابی تمام شود.',
  'WINDOWS_RECOVERY_CONFLICT':
      'وضعیت شبکه تغییر کرده یا هنوز در نشست دیگری در حال استفاده است. برای محافظت از اتصال فعال، بازیابی خودکار متوقف شد.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'این نصب نمی‌تواند اتصال VPN قبلی را خودکار بازیابی کند. Usque را از تنظیمات به‌روز کنید و دوباره تلاش کنید.',
};

const String kWindowsAdapterCleanupFa =
    'آداپتور شبکهٔ مجازی اتصال قبلی حذف نشد یا حذف آن تأیید نشد. هیچ اتصال VPN جدیدی شروع نشده است.';

const Map<String, String> kL4Fa = <String, String>{
  'l4_quic_not_ready': 'در حال آماده‌سازی اتصال L4',
  'l4_unsupported_packets': 'بسته‌های پشتیبانی‌نشده یا معیوب رد شدند',
  'l4_budget_rejections': 'رد پذیرش منابع',
  'l4_not_applicable': 'اعمال نمی‌شود (L4)',
  'l4_mode': 'L4 (آزمایشی)',
  'l4_transport_hint':
      'فقط TCP. برنامه‌های نیازمند UDP ممکن است کار نکنند. حالت خودکار L4 را انتخاب نمی‌کند.',
  'l4_explanation':
      'L4 ترافیک TCP را از طریق HTTP/3 منتقل می‌کند و با VPN و پروکسی‌های SOCKS5 و HTTP کار می‌کند. درخواست‌های DNS مربوط به VPN به TCP تبدیل می‌شوند. دیگر ترافیک UDP، پینگ از راه دور، قطعه‌ها و افزونه‌های IP پشتیبانی نمی‌شوند و ممکن است برخی برنامه‌ها کار نکنند. باید L4 را خودتان انتخاب کنید؛ خودکار فعال نمی‌شود.',
  'l4_unsupported':
      'این نسخه از L4 پشتیبانی نمی‌کند. Usque را از تنظیمات به‌روز کنید.',
  'l4_sni_identity':
      'نام سرور را حساب به‌طور خودکار تعیین می‌کند. نام سرور ذخیره‌شده برای حالت‌های دیگر اتصال حفظ می‌شود.',
  'l4_edge_requires_l4':
      'DNS حل‌شده در لبه به L4 نیاز دارد. پیش از رفتن به Auto، H3 یا H2 حالت DNS پیشکار دیگری را انتخاب کنید.',
  'proxy_dns_edge_resolved': 'لبه Cloudflare (فقط L4؛ بدون جستجوی محلی)',
  'l4_verified': 'یک اتصال برنامه از طریق L4 برقرار شد',
  'l4_unverified': 'به سرور متصل شد؛ اتصال برنامه هنوز تأیید نشده است',
  'l4_status_unknown': 'وضعیت اتصال برنامه در دسترس نیست',
  'l4_sessions': 'نشست‌ها / تخلیه',
  'l4_flows': 'جریان‌های فعال / در انتظار',
  'l4_connect': 'CONNECT موفقیت / شکست / مهلت',
  'l4_buffers': 'بودجهٔ بافر برنامهٔ استفاده‌شده (بایت)',
  'l4_backpressure': 'فشار معکوس ارسال / دریافت',
  'l4_tun_flows': 'TUN TCP / نیمه‌باز',
  'l4_udp': 'بسته‌های UDP ردشده',
  'l4_dns': 'تبدیل DNS موفقیت / شکست / مهلت',
  'l4_migration': 'جریان‌های حفظ‌شده با مهاجرت / پایان‌یافته با بازسازی',
  'l4_na':
      'کنترل نشانی CONNECT-IP، صف‌های DATAGRAM، MTU بار داخلی و مهلت UDP: در L4 اعمال نمی‌شود.',
};

const Map<String, String> kNetworkSettingsFa = <String, String>{
  'settings_applying': 'ذخیره شد، در حال اعمال',
  'settings_applied': 'ذخیره و اعمال شد',
  'settings_deferred': 'ذخیره شد، در اتصال دستی بعدی اعمال می‌شود',
  'settings_failed': 'ذخیره شد، اعمال ناموفق بود',
  'settings_unknown': 'نتیجه هنوز تأیید نشده است',
  'settings_saved': 'ذخیره شد',
  'settings_unsupported':
      'Usque را کاملاً ببندید و دوباره باز کنید، سپس دوباره ذخیره کنید. اگر مشکل ادامه داشت، Usque را از تنظیمات به‌روز کنید.',
  'settings_save_failed': 'تنظیمات ذخیره نشد. ویرایش‌های شما حفظ شده‌اند.',
  'settings_reconnect': 'اتصال دوباره',
};

const Map<String, String> kChainFa = <String, String>{
  'duplicate_directive': 'این دستور فقط یک بار می‌تواند بیاید.',
  'mixed_protocols':
      'همهٔ نقطه‌های remote باید از یک انتقال TCP یا UDP استفاده کنند.',
  'conflicting_protocol': 'remote با تنظیم سراسری انتقال در تعارض است.',
  'too_many_endpoints': 'بیش از ۱۶ نقطهٔ remote استفاده نکنید.',
  'conflicting_authentication':
      'CLIENT_CERT با گواهی درون‌خطی یا حالت احراز هویت در تعارض است.',
  'serialized_size_limit':
      'رکورد رمزشده از حد اندازهٔ ذخیره‌سازی فراتر می‌رود.',
  'multi_endpoint_unavailable':
      'برای استفاده از پیکربندی‌های چندنقطه‌ای، موتور را به‌روز کنید.',
  'candidates': 'نقطه‌های آغاز',
  'random_order':
      'در هر اتصال، نقطه‌ها با ترتیب تصادفی تازه‌ای آزموده می‌شوند.',
  'file_order': 'نقطه‌ها به ترتیب فایل آزموده می‌شوند.',
  'attempting': 'نقطهٔ در حال آزمون',
  'actual_endpoint': 'نقطهٔ متصل‌شده',
  'attempt_failures': 'تلاش‌های ناموفق',
  'failure_transport': 'انتقال بسته شد',
  'failure_authentication': 'احراز هویت',
  'failure_certificate': 'گواهی',
  'failure_configuration': 'پیکربندی',
  'failure_address_changed': 'نشانی تغییر کرد',
  'failure_protocol': 'پروتکل',
  'failure_cleanup': 'پاک‌سازی',
  'failure_reason': 'علت خرابی: {reason}.',
  'manage': 'مدیریت',
  'dns_fallback': 'DNS تونل (OpenVPN می‌تواند DNS را مذاکره کند)',
  'dns_unavailable_title': 'از این خروجی DNS در دسترس نیست',
  'dns_unavailable':
      'هیچ کارساز DNS از طریق این خروجی در دسترس نیست. از نشانی IP استفاده کنید یا خروجی دیگری با DNS در دسترس انتخاب کنید.',
  'authentication_failed':
      'احراز هویت ناموفق بود. پیش از اتصال دوباره، اطلاعات ورود را به‌روز کنید.',
  'profile_limit': 'کتابخانهٔ پیکربندی پر است (۱۲۸ نمایه).',
  'metadata_limit': 'فرادادهٔ کتابخانهٔ پیکربندی پر است.',
  'title': 'پروکسی زنجیره‌ای',
  'subtitle': 'خروجی‌ای را انتخاب کنید که از راه WARP به آن می‌رسید.',
  'source': 'منبع خروجی',
  'enable': 'فعال کردن پروکسی زنجیره‌ای',
  'import_file': 'وارد کردن پرونده',
  'paste': 'چسباندن پیکربندی',
  'profiles': 'پیکربندی‌های ذخیره‌شده',
  'empty': 'برای انتخاب خروجی، یک پیکربندی وارد کنید.',
  'empty_hint_openvpn':
      'یک پروندهٔ ‎.ovpn وارد کنید یا متن آن را بچسبانید. نقطهٔ TCP و UDP، گواهی درون‌خطی و نام کاربری/گذرواژه پشتیبانی می‌شود.',
  'empty_hint_wireguard':
      'یک پروندهٔ ‎.conf وارد کنید یا متن آن را بچسبانید. یک بخش [Interface] و یک بخش [Peer] پشتیبانی می‌شود.',
  'import_limits':
      'پیکربندی باید متن UTF-8 تا ۱۲۸ کیبی‌بایت باشد. اگر انتخابگر پرونده نیست، متن را بچسبانید.',
  'enable_to_choose': 'برای انتخاب پیکربندی، پروکسی زنجیره‌ای را فعال کنید.',
  'select_required': 'پیش از اعمال، یک پیکربندی ذخیره‌شده انتخاب کنید.',
  'pending_disable': 'در انتظار اعمال: خاموش کردن پروکسی زنجیره‌ای',
  'apply_reconnect': 'اعمال و اتصال دوباره',
  'requires_connect_ip': 'به CONNECT-IP نیاز دارد',
  'menu': 'کنش‌های پیکربندی',
  'preview': 'بررسی پیکربندی',
  'save_import': 'ذخیرهٔ پیکربندی',
  'name': 'نام',
  'configuration': 'متن پیکربندی',
  'file_loaded': 'پیکربندی از پرونده بار شد ({lines} خط).',
  'username': 'نام کاربری',
  'password': 'گذرواژه',
  'key_password': 'گذرواژهٔ کلید خصوصی',
  'show_password': 'نمایش گذرواژه',
  'hide_password': 'پنهان کردن گذرواژه',
  'credentials': 'به‌روزرسانی اطلاعات ورود',
  'rename': 'تغییر نام',
  'delete': 'حذف',
  'cancel': 'لغو',
  'save': 'ذخیره',
  'apply': 'اعمال تغییرات',
  'clear': 'پاک کردن انتخاب',
  'current': 'اتصال فعلی',
  'saved': 'انتخاب ذخیره‌شده',
  'draft': 'انتخاب در انتظار',
  'disconnected': 'متصل نیست',
  'disabled': 'فعال نیست',
  'enabled_idle': 'فعال · متصل نیست',
  'disconnecting': 'در حال قطع اتصال',
  'file_read_failed': 'پروندهٔ پیکربندی خوانده نشد.',
  'file_encoding_invalid': 'پروندهٔ پیکربندی باید متن UTF-8 باشد.',
  'file_busy': 'انتخابگر پرونده از قبل باز است.',
  'connected': 'متصل',
  'connecting': 'در حال اتصال',
  'error': 'اتصال ناموفق بود',
  'no_selection': 'پیکربندی‌ای انتخاب نشده است',
  'l4': 'این پیکربندی به حالت CONNECT-IP نیاز دارد.',
  'switch_mode': 'رفتن به CONNECT-IP و اعمال',
  'unsupported': 'این موتور از این منبع خروجی پشتیبانی نمی‌کند.',
  'scope':
      'قاعده‌های مستقیمِ صریح موجود همچنان اعمال می‌شوند. بقیهٔ ترافیک از خروجی انتخاب‌شده استفاده می‌کند.',
  'allowed': 'مقصدهای مجاز',
  'dns': 'DNS',
  'addresses': 'نشانی‌های تونل',
  'address_family': 'خانوادهٔ نشانی',
  'transport': 'انتقال',
  'endpoint': 'کارساز',
  'restricted': 'مقصدهای بیرون از AllowedIPs در مسیر پروکسی مسدود می‌شوند.',
  'delete_confirm':
      'این پیکربندی ذخیره‌شده حذف شود؟ پروندهٔ اصلی واردشده تغییر نمی‌کند.',
  'profile_in_use':
      'پیش از حذف این پیکربندی، پیکربندی دیگری انتخاب کنید یا انتخاب ذخیره‌شده را پاک کنید.',
  'stale_revision':
      'پیکربندی تغییر کرده است. فهرست را تازه کنید و دوباره تلاش کنید.',
  'secure_storage_failed': 'پیکربندی رمزشده خوانده یا ذخیره نشد.',
  'invalid_configuration':
      'پیکربندی نامعتبر است یا گزینه‌های پشتیبانی‌نشده دارد.',
  'looks_like_wireguard':
      'این شبیه پیکربندی WireGuard است. منبع خروجی را به WireGuard تغییر دهید.',
  'looks_like_openvpn':
      'این شبیه پیکربندی OpenVPN است. منبع خروجی را به OpenVPN تغییر دهید.',
  'error_location': '{message} ({field}، خط {line})',
  'error_field': '{message} ({field})',
  'file_unavailable':
      'انتخابگر پرونده در دسترس نیست. متن پیکربندی را بچسبانید.',
  'invalid_size_or_encoding':
      'از پیکربندی UTF-8 با اندازهٔ حداکثر ۱۲۸ کیبی‌بایت استفاده کنید.',
  'unsupported_directive': 'این دستور OpenVPN پشتیبانی نمی‌شود.',
  'unsupported_or_duplicate_field': 'این فیلد پشتیبانی نمی‌شود یا تکراری است.',
  'unsupported_or_duplicate_section':
      'یک بخش Interface و یک بخش Peer به کار ببرید.',
  'missing_field': 'یک فیلد ضروری وجود ندارد.',
  'invalid_name': 'نامی با ۱ تا ۶۴ نویسه و بدون نویسه‌های کنترلی وارد کنید.',
  'invalid_key': 'کلید باید یک کلید Base64 معتبر ۳۲ بایتی باشد.',
  'checking': 'در حال بررسی پیکربندی…',
  'changed': 'تغییرات ذخیره شد',
};
