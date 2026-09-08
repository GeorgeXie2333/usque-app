/// Supplemental feature strings for Persian.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowFa = <String, String>{
  'cc_label': 'کنترل ازدحام HTTP/3',
  'cc_help': 'در اتصال دستی بعدی‌تان اعمال می‌شود.',
  'cc_upgrade': 'به‌روزرسانی Engine لازم است.',
  'cc_h2': 'HTTP/2 از TCP سیستم استفاده می‌کند.',
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
  'home_traffic_idle': 'پس از اتصال شروع می‌شود',
  'home_traffic_waiting': 'در انتظار نمونه',
  'home_traffic_unavailable': 'سابقه در دسترس نیست',
  'home_traffic_stale': 'نمونه‌ها به‌تأخیر افتاده',
  'home_outputs_next': 'خروجی‌ها پس از اتصال فعال می‌شوند',
  'home_outputs_retry': 'خروجی‌ها برای تلاش بعدی پیکربندی شده‌اند',
  'connection_protection_group': 'اتصال و حفاظت',
  'proxy_routing_group': 'پروکسی و مسیریابی',
  'application_group': 'برنامه',
  'proxy_settings_link': 'نشانی شنونده، پورت‌ها، احراز هویت و DNS.',
  'proxy_auth_separate':
      'اعتبارنامه‌ها جداگانه با «ذخیره اعتبارنامه» ذخیره می‌شوند.',
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
      'DNS سیستم فیزیکی انتخاب شده است؛ بررسی‌های DNS رمزنگاری‌شده اعمال نمی‌شوند.',
  'nq_finding_unsupported':
      'DNS رمزنگاری‌شده در این Engine در دسترس نیست؛ بازگشت به DNS رمزنشده مجاز نیست.',
  'nq_finding_dns_custom_valid':
      'پیکربندی DNS رمزنگاری‌شدهٔ سفارشی معتبر است. بازگشت به متن ساده غیرفعال است.',
  'nq_finding_stale': 'خوانش کهنه است یا شبکهٔ فیزیکی تغییر کرده است.',
  'nq_finding_rtt_high': 'زمان رفت‌وبرگشت اندازه‌گیری‌شده بالاست.',
  'nq_finding_healthy': 'اندازه‌گیری محلیِ موجود در بازهٔ مورد انتظار است.',
  'nq_finding_loss_high': 'ازدست‌رفتن بسته در این بازه بالاست.',
  'nq_finding_queue_pressure':
      'یک صف تحت فشار است یا در این اتصال افت ثبت کرده است.',
  'nq_finding_pmtu_degraded': 'اعتبارسنجی MTU مسیر مختل شده است.',
  'nq_finding_migration_reconnect':
      'مهاجرت در این مسیر در دسترس نیست؛ تغییر شبکه از اتصال مجدد کامل استفاده می‌کند.',
  'nq_finding_dns_changed': 'حالت DNS ذخیره‌شده با اتصال در حال اجرا فرق دارد.',
  'nq_finding_dns_runtime':
      'DNS رمزنگاری‌شده موفق بوده است. وضعیت محلی اثبات خارجیِ بی‌نشتی نیست.',
  'nq_finding_dns_degraded':
      'DNS رمزنگاری‌شده مختل است؛ پرس‌وجوهای مستقیم ناموفق به DNS سیستم برنمی‌گردند.',
  'nq_finding_probe_unsafe':
      'کاوش اجرا نشد: وضعیت ایمن لازم یا هویت ذخیره‌شده در دسترس نیست. تونل فعال هرگز تکثیر نمی‌شود و مسیر دادهٔ دومی باز نمی‌شود.',
  'nq_finding_probe_success':
      'کاوش احرازشده کامل شد. این آزمون نشت بستهٔ خارجی نیست.',
  'nq_finding_probe_cancelled': 'کاوش لغو شد و پاک‌سازی درخواست شد.',
  'nq_finding_probe_timeout': 'کاوش زمان‌مند پیش از مهلت تمام نشد.',
  'nq_finding_probe_failed':
      'کاوش احرازشده ناموفق بود؛ هیچ بازگشت ناامنی آزموده نشد.',
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
      'این Engine کیفیت شبکه را ارائه نمی‌دهد. کنترل‌های اتصال موجود همچنان کار می‌کنند.',
  'nq_empty':
      'برای دیدن اندازه‌گیری‌ها متصل شوید. مقدارهای نامعلوم صفر نشان داده نمی‌شوند.',
  'nq_stale_help':
      'منبع به‌روزرسانی را متوقف کرده است. این‌ها خوانش‌های قبلی‌اند؛ شکاف‌ها شکاف می‌مانند.',
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
  'nq_pmtu_help': 'کشف مسیر MTU مربوط به TUN دستگاه را افزایش نمی‌دهد.',
  'nq_migration': 'مهاجرت شبکه',
  'nq_migration_help':
      'یک اتصال، یک مسیر داده. فقط همان خانوادهٔ IP؛ چندمسیره نیست.',
  'nq_attempts': 'تلاش‌ها',
  'nq_successes': 'موفق',
  'nq_failures': 'ناموفق',
  'nq_last_duration': 'آخرین مدت',
  'nq_direct_dns': 'DNS مستقیم',
  'nq_system_dns': 'DNS سیستم فیزیکی',
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
      'خانوادهٔ IP فعلی در دسترس نیست؛ از اتصال مجدد کامل استفاده می‌شود.',
  'nq_reason_socket_protect_failed': 'سوکت نامزد محافظت‌شده آماده نشد.',
  'nq_reason_generation_changed_during_setup':
      'هنگام آماده‌سازی شبکه دوباره تغییر کرد.',
  'nq_reason_peer_cid_unavailable': 'همتا شناسهٔ اتصال ذخیره‌ای ندارد.',
  'nq_reason_local_cid_unavailable': 'شناسهٔ اتصال محلی در دسترس نیست.',
  'nq_reason_path_probe_rejected': 'مسیر نامزد اعتبارسنجی نشد.',
  'nq_reason_path_validation_timeout':
      'اعتبارسنجی مسیر از مهلت گذشت؛ اتصال مجدد در دسترس است.',
  'nq_reason_superseded': 'تغییر شبکهٔ تازه‌تری جایگزین این تلاش شد.',
  'nq_reason_promotion_failed': 'تعویض مسیر به‌صورت ایمن کامل نشد.',
  'nq_reason_connection_closed': 'هنگام مهاجرت اتصال بسته شد.',
  'nq_reason_unsupported': 'مهاجرت در این اتصال در دسترس نیست.',
  'nq_reason_unknown': 'دلیل پشتیبانی‌شده‌ای در دسترس نیست.',
  'nq_dns_custom': 'حل‌کنندهٔ رمزنگاری‌شدهٔ سفارشی',
  'nq_dns_server': 'نام سرور TLS',
  'nq_dns_path': 'مسیر HTTPS',
  'nq_dns_port': 'پورت (0 از پیش‌فرض استفاده می‌کند)',
  'nq_dns_bootstrap': 'نشانی‌های IP راه‌انداز',
  'nq_dns_bootstrap_help':
      '۱ تا ۸ نشانی IP عددی وارد کنید، هر خط یکی. از جستجوی نام میزبان استفاده نمی‌شود.',
  'nq_dns_no_fallback':
      'اگر DNS مستقیم رمزنگاری‌شده شکست بخورد، پرس‌وجو شکست می‌خورد. هرگز به DNS سیستم یا متن ساده برنمی‌گردد.',
  'nq_dns_system_privacy':
      'DNS سیستم فیزیکی ممکن است نام‌های پرس‌وجوی مستقیم را برای ارائه‌دهندهٔ DNS شبکهٔ فیزیکی فاش کند.',
  'nq_dns_scope':
      'فقط برای پرس‌وجوهای مستقیم انتخاب‌شده با Geo به کار می‌رود. DNS تونل تغییر نمی‌کند.',
  'nq_dns_no_capability':
      'این Engine نمی‌تواند از DNS مستقیم رمزنگاری‌شده استفاده کند. تنظیمات ذخیره‌شده حفظ می‌شوند. می‌توانید صریحاً DNS سیستم را انتخاب کنید.',
  'nq_dns_invalid_name':
      'یک نام DNS بدون فاصله، نحو URL یا نویسه‌های عام وارد کنید.',
  'nq_dns_invalid_path':
      'از یک /path حداکثر ۲۵۶ نویسه، بدون پرس‌وجو، قطعه یا فاصله استفاده کنید.',
  'nq_dns_invalid_bootstrap':
      'از ۱ تا ۸ نشانی IP تک‌پخش یکتا استفاده کنید؛ نشانی نامشخص، چندپخشی، همه‌پخشی یا پیوند-محلی IPv6 مجاز نیست.',
  'nq_dns_invalid_port': '0–65535 وارد کنید.',
  'nq_dns_invalid_mode': 'یک حالت DNS پشتیبانی‌شده انتخاب کنید.',
  'nq_doctor_deep_title': 'بررسی‌های عمیق شبکه اجرا شود؟',
  'nq_doctor_deep_body':
      'بررسی‌های عمیق ممکن است یک پرس‌وجوی آزمایشی DNS به حل‌کنندهٔ پیکربندی‌شده بفرستند و یک مسیر QUIC محافظت‌شده را اعتبارسنجی کنند. حداکثر ۱۵ ثانیه طول می‌کشند، قابل لغو هستند، هرگز تونل داده‌دار دومی نمی‌سازند و DNS، مسیر، پروفایل یا انتقال را تغییر نمی‌دهند.',
  'nq_doctor_deep_run': 'اجرای بررسی‌های عمیق',
  'nq_doctor_evidence':
      'بررسی‌های محلی پیکربندی و وضعیت مشاهده‌شده را توصیف می‌کنند. اثبات خارجیِ نبود نشت DNS نیستند.',
};

const Map<String, String> kWindowsRecoveryFa = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'وضعیت شبکهٔ VPN قبلی به‌طور کامل بازیابی نشد. اتصال VPN جدیدی شروع نشد. اتصال را دوباره امتحان کنید یا عیب‌یابی محلی را بررسی کنید.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows پس از سه تلاش خودکار نتوانست وضعیت شبکهٔ VPN قبلی را بازیابی کند. وقتی آماده بودید دوباره تلاش کنید، یا عیب‌یابی محلی را بررسی کنید.',
  'WINDOWS_RECOVERY_BLOCKED':
      'تعمیر خودکار متوقف شد، چون وضعیت شبکهٔ Windows قبلی را نمی‌شد به‌صورت ایمن تأیید کرد. Agent را دوباره راه‌اندازی کنید یا Usque را به‌روزرسانی کنید، سپس عیب‌یابی محلی را بررسی کنید.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'بازیابی شبکهٔ Windows بیشتر از انتظار طول می‌کشد. اتصال VPN جدیدی شروع نشده است. پیش از تلاش دوباره صبر کنید تا بازیابی تمام شود.',
  'WINDOWS_RECOVERY_CONFLICT':
      'وضعیت شبکه تغییر کرده یا هنوز در نشست دیگری در حال استفاده است. برای محافظت از اتصال فعال، بازیابی خودکار متوقف شد.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'این Windows Agent از بازیابی خودکار ایمن پشتیبانی نمی‌کند. برنامه و Agent را با هم به‌روزرسانی کنید، سپس دوباره تلاش کنید.',
};

const String kWindowsAdapterCleanupFa =
    'آداپتور Wintun قبلی حذف نشد یا حذف آن تأیید نشد. اتصال VPN جدیدی شروع نشده است.';
