/// Supplemental feature strings for Ukrainian.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowUk = <String, String>{
  'cc_label': 'Керування перевантаженням HTTP/3',
  'cc_help': 'Набирає чинності під час наступного ручного зʼєднання.',
  'cc_upgrade': 'Потрібне оновлення Engine.',
  'cc_h2': 'HTTP/2 використовує системний TCP.',
  'cc_saved': 'Збережено',
  'cc_pending': 'Чекає на наступне ручне зʼєднання.',
  'save_changes': 'Застосувати зміни',
  'saving_changes': 'Застосування змін…',
  'unsaved_changes': 'Незастосовані зміни',
  'changes_applied': 'Зміни застосовано',
  'changes_apply_hint': 'Правки набирають чинності лише після застосування.',
  'changes_failed':
      'Не вдалося застосувати зміни. Перегляньте збережені значення '
      'й спробуйте знову.',
  'form_errors': 'Виправте підсвічені поля, перш ніж застосовувати зміни.',
  'discard_changes_title': 'Відкинути незастосовані зміни?',
  'discard_changes_body':
      'Ваші правки ще не застосовано. Продовжте редагування, щоб '
      'зберегти їх, або відкиньте, щоб вийти.',
  'keep_editing': 'Лишатися в редакторі',
  'discard_changes': 'Відкинути зміни',
  'invalid_port': 'Укажіть порт у межах від 1 до 65535.',
  'listener_exposure': 'Адреси слухача відкривають доступ із локальної мережі',
  'invalid_ipv4': 'Укажіть чинну адресу IPv4, наприклад 127.0.0.1.',
  'invalid_ipv6': 'Укажіть чинну адресу IPv6, наприклад ::1.',
  'output_running': 'Працює',
  'output_waiting': 'Увімкнено · ще не працює',
  'output_disabled': 'Вимкнено',
  'output_starting': 'Запускається',
  'output_stopping': 'Зупиняється',
  'output_reconnecting': 'Повторне зʼєднання',
  'output_degraded': 'З обмеженнями',
  'output_error': 'Помилка',
  'output_unknown': 'Стан невідомий',
  'shared_network_scope':
      'Мережеві налаштування спільні для всіх облікових записів.',
  'connection_details': 'Подробиці зʼєднання',
  'home_overview': 'Огляд зʼєднання',
  'home_exit_region': 'Зона виходу',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Обмін даними',
  'home_traffic_window': 'Минулі 60 секунд',
  'home_traffic_idle': 'Починається після зʼєднання',
  'home_traffic_waiting': 'Очікування зразків',
  'home_traffic_unavailable': 'Історії немає',
  'home_traffic_stale': 'Зразки запізнилися',
  'home_outputs_next': 'Виходи буде ввімкнено після зʼєднання',
  'home_outputs_retry': 'Виходи налаштовано для наступної спроби',
  'connection_protection_group': 'Зʼєднання і захист',
  'proxy_routing_group': 'Проксі та маршрути',
  'application_group': 'Програма',
  'proxy_settings_link': 'Адреси слухача, порти, автентифікація та DNS.',
  'proxy_auth_separate':
      'Облікові дані зберігаються окремо командою '
      '«Зберегти облікові дані».',
  'reset_draft_hint':
      'У форму буде завантажено типові значення. Застосуйте зміни, '
      'щоб вони набрали чинності.',
};

const kNetworkQualityUk = <String, String>{
  'nq_range': 'Інтервал',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Затримка туди й назад',
  'diag_check_quality_packet_loss': 'Втрата пакетів',
  'diag_check_quality_queue_pressure': 'Тиск на чергу',
  'diag_check_quality_pmtu': 'MTU шляху',
  'diag_check_transport_migration_capability':
      'Міграція в межах того самого сімейства',
  'diag_check_dns_direct_encrypted_configuration':
      'Конфігурація DNS прямого доступу',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Робочий стан DNS прямого доступу',
  'diag_check_dns_direct_encrypted_reachability':
      'Досяжність зашифрованого DNS',
  'diag_check_transport_h3_path_validation_probe':
      'Ізольоване рукостискання QUIC',
  'nq_finding_unavailable': 'Це вимірювання недоступне в поточному стані.',
  'nq_finding_invalid_configuration': 'Власна конфігурація DNS недійсна.',
  'nq_finding_dns_system':
      'Вибрано системний DNS фізичної мережі; перевірки '
      'зашифрованого DNS не застосовуються.',
  'nq_finding_unsupported':
      'Зашифрований DNS недоступний у цьому Engine; перехід на '
      'незашифрований текст заборонено.',
  'nq_finding_dns_custom_valid':
      'Власна конфігурація зашифрованого DNS дійсна. Перехід на '
      'незашифрований текст вимкнено.',
  'nq_finding_stale': 'Показник застарів або фізична мережа змінилася.',
  'nq_finding_rtt_high': 'Виміряна затримка туди й назад підвищена.',
  'nq_finding_healthy':
      'Доступне місцеве вимірювання перебуває в очікуваному '
      'інтервалі.',
  'nq_finding_loss_high': 'Втрата пакетів за цей проміжок підвищена.',
  'nq_finding_queue_pressure':
      'Черга зазнає тиску або під час цього зʼєднання вже '
      'зафіксовано відкидання.',
  'nq_finding_pmtu_degraded': 'Підтвердження MTU шляху погіршено.',
  'nq_finding_migration_reconnect':
      'Міграція на цьому шляху недоступна; зміна мережі виконується '
      'повним повторним зʼєднанням.',
  'nq_finding_dns_changed':
      'Збережений режим DNS відрізняється від активного зʼєднання.',
  'nq_finding_dns_runtime':
      'Зашифрований DNS спрацював успішно. Місцевий стан не є '
      'зовнішнім доказом відсутності витоків.',
  'nq_finding_dns_degraded':
      'Зашифрований DNS погіршено; невдалі прямі запити не '
      'переходять на системний DNS.',
  'nq_finding_probe_unsafe':
      'Зонд пропущено: немає потрібного безпечного стану або '
      'збереженої ідентичності. Активний тунель ніколи не '
      'дублюється.',
  'nq_finding_probe_success':
      'Автентифікований зонд завершено. Це не зовнішня перевірка '
      'витоку пакетів.',
  'nq_finding_probe_cancelled': 'Зонд скасовано й запрошено очищення.',
  'nq_finding_probe_timeout':
      'Обмежений за часом зонд не встиг завершитися до кінцевого '
      'терміну.',
  'nq_finding_probe_failed':
      'Автентифікований зонд завершився збоєм; небезпечного '
      'резервного переходу не було.',
  'diag_fix_nq_profile':
      'Перегляньте поля власного DNS і назву сертифіката. Не '
      'вимикайте перевірку TLS.',
  'diag_fix_nq_retry': 'Дочекайтеся стійкої мережі, тоді повторіть спробу.',
  'diag_fix_nq_network':
      'Перевірте місцеву звʼязність і порівняйте новий зразок, перш '
      'ніж змінювати налаштування.',
  'diag_fix_nq_reconnect':
      'Зʼєднайтеся знову, щоб застосувати збережену конфігурацію.',
  'nav_network_quality': 'Якість',
  'network_quality': 'Якість мережі',
  'nq_subtitle': 'Дивіться на зʼєднання, а не лише на швидкість.',
  'nq_local_only': 'Лише місцеві вимірювання. Нічого не вивантажується.',
  'nq_doctor': 'Запустити мережеву діагностику',
  'nq_doctor_help':
      'Звичайні перевірки читають лише місцевий стан. Вони не '
      'відкривають зовнішніх зʼєднань і не змінюють налаштувань.',
  'nq_live': 'Наживо',
  'nq_stale': 'Застарілі показники',
  'nq_updated': 'Останній зразок',
  'nq_seconds': '{count} с тому',
  'nq_good': 'Добре',
  'nq_fair': 'Задовільно',
  'nq_poor': 'Погано',
  'nq_limited': 'Обмежені дані',
  'nq_disconnected': 'Відʼєднано',
  'nq_connecting': 'Зʼєднання',
  'nq_connected': 'Зʼєднано',
  'nq_unavailable': 'Недоступно',
  'nq_not_ready': 'Ще не готово',
  'nq_unsupported': 'Підтримки немає',
  'nq_capability_missing':
      'Цей Engine не надає відомостей про якість мережі. Наявні '
      'елементи керування зʼєднанням і надалі працюють.',
  'nq_empty':
      'Зʼєднайтеся, щоб побачити вимірювання. Невідомі значення не '
      'показуються нулем.',
  'nq_stale_help':
      'Джерело припинило оновлення. Це попередні показники; '
      'прогалини лишаються прогалинами.',
  'nq_rtt': 'Затримка туди й назад',
  'nq_latest': 'Найсвіжіше',
  'nq_smoothed': 'Згладжене',
  'nq_minimum': 'Найменше',
  'nq_h2_ping': 'PING протоколу HTTP/2',
  'nq_h3_rtt': 'Вимірювання шляху QUIC',
  'nq_throughput': 'Пропускна здатність',
  'nq_download': 'Завантаження',
  'nq_upload': 'Вивантаження',
  'nq_one_second': 'одна секунда',
  'nq_five_seconds': 'Усереднення за 5 с',
  'nq_loss': 'Втрата пакетів',
  'nq_loss_h2': 'HTTP/2 не показує порівнянної втрати пакетів.',
  'nq_loss_interval':
      'Пораховано за останній проміжок; це не втрата за весь час '
      'життя.',
  'nq_congestion': 'Перевантаження',
  'nq_cwnd': 'Вікно перевантаження',
  'nq_in_flight': 'Байти в дорозі',
  'nq_send_rate': 'Швидкість передавання',
  'nq_h2_window': 'Вікна приймання HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Зʼєднання',
  'nq_stalls': 'Зупинки через місткість',
  'nq_pmtu': 'MTU шляху',
  'nq_outer_pmtu': 'Верхня межа зовнішнього навантаження UDP',
  'nq_inner_payload': 'Верхня межа навантаження CONNECT-IP',
  'nq_pmtu_help': 'Виявлення шляху не підвищує MTU пристрою TUN.',
  'nq_migration': 'Міграція мережі',
  'nq_migration_help':
      'Одне зʼєднання, один шлях даних. Лише те саме сімейство IP; '
      'це не багатошляховий режим.',
  'nq_attempts': 'Спроби',
  'nq_successes': 'Вдало',
  'nq_failures': 'Невдачі',
  'nq_last_duration': 'Остання тривалість',
  'nq_direct_dns': 'DNS прямого доступу',
  'nq_system_dns': 'Системний DNS фізичної мережі',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'У готовності',
  'nq_degraded': 'Погіршено',
  'nq_timeouts': 'Перевищення часу',
  'nq_last_rtt': 'Остання затримка',
  'nq_dns_redacted':
      'Назви резолверів і адреси bootstrap видно лише в '
      'налаштуваннях.',
  'nq_queues': 'Тиск на чергу',
  'nq_queue_details': 'Черги нижчого рівня',
  'nq_queue_empty': 'Вимірювань черги ще немає.',
  'nq_current_capacity': 'Поточне / місткість',
  'nq_high_water': 'Найвища позначка',
  'nq_drops': 'Відкидання',
  'nq_oldest': 'Найдавніший запис',
  'nq_tunToTransport': 'Пристрій → транспорт',
  'nq_proxyToTransport': 'Проксі → транспорт',
  'nq_transportOutgoing': 'Транспорт на вихід',
  'nq_h3DatagramSend': 'Датаграми QUIC',
  'nq_h3WireSend': 'Вивід UDP',
  'nq_transportToTun': 'Транспорт → пристрій',
  'nq_transportToProxy': 'Транспорт → проксі',
  'nq_directDns': 'Запити DNS прямого доступу',
  'nq_unknown_queue': 'Інша черга',
  'nq_trends': 'Минулі 60 секунд',
  'nq_samples': 'зразків',
  'nq_pause': 'Пауза діаграм',
  'nq_resume': 'Продовжити діаграми',
  'nq_paused': 'Діаграми призупинено',
  'nq_gaps': 'Пропущені зразки показано як прогалини.',
  'nq_phase_idle': 'Бездіяльність',
  'nq_phase_preparing_socket': 'Приготування шляху',
  'nq_phase_probing': 'Перевірка зондом',
  'nq_phase_validated': 'Підтверджено',
  'nq_phase_promoting': 'Перемикання шляху',
  'nq_phase_stable': 'Стійко',
  'nq_phase_aborted': 'Припинено',
  'nq_phase_revalidating': 'Повторне підтвердження',
  'nq_phase_degraded': 'Погіршено',
  'nq_phase_unknown': 'Ще не готово',
  'nq_phase_unsupported': 'Підтримки немає',
  'nq_reason_family_unavailable':
      'Поточне сімейство IP недоступне; виконується повне повторне '
      'зʼєднання.',
  'nq_reason_socket_protect_failed':
      'Захищений сокет-кандидат не вдалося підготувати.',
  'nq_reason_generation_changed_during_setup':
      'Мережа знову змінилася під час готування.',
  'nq_reason_peer_cid_unavailable':
      'У вузла немає запасного ідентифікатора зʼєднання.',
  'nq_reason_local_cid_unavailable':
      'Місцевий ідентифікатор зʼєднання недоступний.',
  'nq_reason_path_probe_rejected': 'Шлях-кандидат не вдалося підтвердити.',
  'nq_reason_path_validation_timeout':
      'Підтвердження шляху перевищило час очікування; повторне '
      'зʼєднання доступне.',
  'nq_reason_superseded': 'Новіша зміна мережі замінила цю спробу.',
  'nq_reason_promotion_failed':
      'Перемикання шляху не вдалося безпечно завершити.',
  'nq_reason_connection_closed': 'Зʼєднання закрилося під час міграції.',
  'nq_reason_unsupported': 'Міграція недоступна на цьому зʼєднанні.',
  'nq_reason_unknown': 'Немає підтримуваного пояснення.',
  'nq_dns_custom': 'Власний зашифрований резолвер',
  'nq_dns_server': 'Назва сервера TLS',
  'nq_dns_path': 'Шлях HTTPS',
  'nq_dns_port': 'Порт (0 бере значення за замовчуванням)',
  'nq_dns_bootstrap': 'Адреси IP для bootstrap',
  'nq_dns_bootstrap_help':
      'Укажіть 1–8 числових IP-адрес, по одній у рядку. Пошук за '
      'назвою вузла не застосовується.',
  'nq_dns_no_fallback':
      'Якщо зашифрований DNS прямого доступу не спрацьовує, запит '
      'зазнає невдачі. Переходу на системний або незашифрований DNS '
      'ніколи не відбувається.',
  'nq_dns_system_privacy':
      'Системний DNS фізичної мережі може розкривати назви прямих '
      'запитів постачальнику DNS цієї мережі.',
  'nq_dns_scope':
      'Лише для прямих запитів, які відібрав Geo. DNS тунелю '
      'лишається без змін.',
  'nq_dns_no_capability':
      'Цей Engine не може використовувати зашифрований DNS прямого '
      'доступу. Збережені параметри зберігаються. Ви можете явно '
      'вибрати системний DNS.',
  'nq_dns_invalid_name':
      'Укажіть імʼя DNS без пробілів, синтаксису URL і '
      'підстановочних знаків.',
  'nq_dns_invalid_path':
      'Укажіть /шлях завдовжки до 256 символів без запиту, фрагмента '
      'й пробілів.',
  'nq_dns_invalid_bootstrap':
      'Укажіть 1–8 різних індивідуальних IP; без невизначених, '
      'групових, широкомовних і link-local адрес IPv6.',
  'nq_dns_invalid_port': 'Укажіть 0–65535.',
  'nq_dns_invalid_mode': 'Оберіть підтримуваний режим DNS.',
  'nq_doctor_deep_title': 'Запустити поглиблені перевірки мережі?',
  'nq_doctor_deep_body':
      'Поглиблені перевірки можуть надіслати тестовий запит DNS '
      'налаштованому резолверу й підтвердити захищений шлях QUIC. '
      'Вони тривають щонайбільше 15 секунд, їх можна скасувати, вони '
      'ніколи не створюють другого тунелю з даними й ніколи не '
      'змінюють DNS, маршрути, профіль або транспорт.',
  'nq_doctor_deep_run': 'Запустити поглиблені перевірки',
  'nq_doctor_evidence':
      'Місцеві перевірки описують конфігурацію та спостережуваний '
      'стан. Вони не є зовнішнім доказом відсутності витоків DNS.',
};

const Map<String, String> kWindowsRecoveryUk = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'Попередній мережевий стан VPN не вдалося повністю відновити. '
      'Нове зʼєднання VPN не розпочиналося. Повторіть зʼєднання або '
      'перегляньте місцеву діагностику.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows не зміг відновити попередній мережевий стан VPN після '
      'трьох автоматичних спроб. Повторіть спробу, коли будете '
      'готові, або перегляньте місцеву діагностику.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Автоматичне відновлення зупинено, бо попередній мережевий '
      'стан Windows не вдалося безпечно підтвердити. Перезапустіть '
      'Agent або оновіть Usque, тоді перегляньте місцеву '
      'діагностику.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Відновлення мережі Windows триває довше, ніж очікувалося. '
      'Нове зʼєднання VPN не розпочиналося. Дочекайтеся завершення '
      'відновлення, перш ніж повторювати спробу.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Мережевий стан змінився або досі зайнятий іншим сеансом. '
      'Автоматичне відновлення зупинено, щоб захистити активне '
      'зʼєднання.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Цей Windows Agent не підтримує безпечного автоматичного '
      'відновлення. Оновіть програму та Agent разом, тоді повторіть '
      'спробу.',
};

const String kWindowsAdapterCleanupUk =
    'Попередній адаптер Wintun не вдалося вилучити або підтвердити '
    'його вилучення. Нове зʼєднання VPN не розпочиналося.';

const Map<String, String> kL4Uk = <String, String>{
  'l4_quic_not_ready': 'Очікування готового сеансу QUIC',
  'l4_unsupported_packets': 'Відхилено непідтримувані або пошкоджені пакети',
  'l4_budget_rejections': 'Відмови в допуску ресурсів',
  'l4_not_applicable': 'Не застосовується (L4)',
  'l4_mode': 'L4 (експериментальний)',
  'l4_transport_hint': 'Лише TCP; DNS TUN — через TCP. Auto не включає L4.',
  'l4_explanation':
      'Лише TCP поверх HTTP/3. Підтримуються VPN/TUN, SOCKS5 і HTTP; DNS TUN перетворюється на TCP. Auto ніколи не вибирає L4. Інший UDP, віддалений ping, фрагменти IP і заголовки розширень не підтримуються; деякі програми можуть не працювати.',
  'l4_unsupported':
      'Цей рушій не оголосив повної підтримки L4. Увімкнути L4 не можна.',
  'l4_sni_identity':
      'Лише читання: виводиться з завантаженої ідентичності облікового запису. Наявний SNI CONNECT-IP зберігається.',
  'l4_edge_requires_l4':
      'DNS, який розвʼязується на межі, потребує L4. Виберіть інший режим DNS проксі перед перемиканням на Auto, H3 або H2.',
  'proxy_dns_edge_resolved': 'Межа Cloudflare (лише L4; без локального запиту)',
  'l4_verified': 'L4 CONNECT підтверджено',
  'l4_unverified': 'QUIC готовий; L4 CONNECT ще не підтверджено',
  'l4_status_unknown': 'Стан перевірки L4 невідомий',
  'l4_sessions': 'Сеанси / завершення',
  'l4_flows': 'Активні / очікувальні потоки',
  'l4_connect': 'CONNECT успіхи / збої / тайм-аути',
  'l4_buffers': 'Використаний бюджет буфера програми (байти)',
  'l4_backpressure': 'Зворотний тиск надсилання / приймання',
  'l4_tun_flows': 'TUN TCP / напіввідкриті',
  'l4_udp': 'Відхилені пакети UDP',
  'l4_dns': 'Перетворення DNS успіхи / збої / тайм-аути',
  'l4_migration': 'Потоки, збережені міграцією / завершені перезбиранням',
  'l4_na':
      'Керування адресою CONNECT-IP, черги DATAGRAM, MTU внутрішнього навантаження і тайм-аут UDP: у L4 не застосовується.',
};

const Map<String, String> kNetworkSettingsUk = <String, String>{
  'settings_applying': 'Збережено, застосовується',
  'settings_applied': 'Збережено й застосовано',
  'settings_deferred':
      'Збережено, набуде чинності під час наступного ручного зʼєднання',
  'settings_failed': 'Збережено, застосувати не вдалося',
  'settings_unknown': 'Результат ще не підтверджено',
  'settings_saved': 'Збережено',
  'settings_unsupported':
      'Перезапустіть або оновіть Engine, щоб зберегти мережеві параметри.',
  'settings_save_failed':
      'Не вдалося зберегти параметри. Ваші зміни збережено.',
  'settings_reconnect': 'Зʼєднати знову',
};
