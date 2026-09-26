/// Supplemental feature strings for Polish.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowPl = <String, String>{
  'local_proxy_settings': 'Ustawienia lokalnego proxy',
  'proxy_switches_hint':
      'Zmiany przełączników są zapisywane automatycznie. Zmiany nasłuchu i DNS zatwierdź przyciskiem Zastosuj zmiany.',
  'cc_label': 'Kontrola przeciążenia HTTP/3',
  'cc_help': 'Zacznie obowiązywać przy następnym ręcznym połączeniu.',
  'cc_upgrade': 'Zaktualizuj Usque w Ustawieniach, aby użyć tej opcji.',
  'cc_h2': 'Ta opcja dotyczy tylko połączeń HTTP/3.',
  'cc_saved': 'Zapisano',
  'cc_pending': 'Oczekuje na następne ręczne połączenie.',
  'save_changes': 'Zastosuj zmiany',
  'saving_changes': 'Stosowanie zmian…',
  'unsaved_changes': 'Niezastosowane zmiany',
  'changes_applied': 'Zastosowano zmiany',
  'changes_apply_hint':
      'Zmiany zaczną obowiązywać dopiero po ich zastosowaniu.',
  'changes_failed':
      'Nie można zastosować zmian. Sprawdź zapisane wartości i spróbuj '
      'ponownie.',
  'form_errors': 'Sprawdź podświetlone pola przed zastosowaniem zmian.',
  'discard_changes_title': 'Odrzucić niezastosowane zmiany?',
  'discard_changes_body':
      'Twoje zmiany nie zostały zastosowane. Kontynuuj edycję, aby je '
      'zapisać, albo odrzuć je, aby wyjść.',
  'keep_editing': 'Kontynuuj edycję',
  'discard_changes': 'Odrzuć zmiany',
  'invalid_port': 'Wpisz port z zakresu 1–65535.',
  'listener_exposure': 'Adresy nasłuchu zezwalają na dostęp z sieci lokalnej',
  'invalid_ipv4': 'Wpisz prawidłowy adres IPv4, na przykład 127.0.0.1.',
  'invalid_ipv6': 'Wpisz prawidłowy adres IPv6, na przykład ::1.',
  'output_running': 'Działa',
  'output_waiting': 'Włączone · nie działa',
  'output_disabled': 'Wyłączone',
  'output_starting': 'Uruchamianie',
  'output_stopping': 'Zatrzymywanie',
  'output_reconnecting': 'Ponowne łączenie',
  'output_degraded': 'Ograniczone',
  'output_error': 'Błąd',
  'output_unknown': 'Stan niedostępny',
  'shared_network_scope': 'Ustawienia sieci są wspólne dla wszystkich kont.',
  'connection_details': 'Szczegóły połączenia',
  'home_overview': 'Przegląd połączenia',
  'home_exit_region': 'Region wyjścia',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Ruch',
  'home_traffic_window': 'Ostatnie 60 sekund',
  'home_traffic_idle': 'Ruch pojawi się po połączeniu',
  'home_traffic_waiting': 'Oczekiwanie na dane ruchu',
  'home_traffic_unavailable': 'Brak historii ruchu',
  'home_traffic_stale': 'Aktualizacja ruchu opóźniona',
  'home_outputs_next': 'Dostępne po połączeniu',
  'home_outputs_retry': 'VPN i proxy przy następnym połączeniu',
  'connection_protection_group': 'Połączenie i ochrona',
  'proxy_routing_group': 'Proxy i trasowanie',
  'application_group': 'Aplikacja',
  'proxy_settings_link': 'Adresy nasłuchu, porty, uwierzytelnianie i DNS.',
  'proxy_auth_separate':
      'Użyj przycisku Zapisz nazwę użytkownika i hasło poniżej, aby zastosować te zmiany.',
  'reset_draft_hint':
      'W tym formularzu zostaną wczytane wartości domyślne. Zastosuj '
      'zmiany, aby zaczęły obowiązywać.',
};

const Map<String, String> kNetworkQualityPl = <String, String>{
  'nq_range': 'Zakres',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Czas rundy',
  'diag_check_quality_packet_loss': 'Utrata pakietów',
  'diag_check_quality_queue_pressure': 'Obciążenie kolejki',
  'diag_check_quality_pmtu': 'MTU ścieżki',
  'diag_check_transport_migration_capability': 'Migracja w tej samej rodzinie',
  'diag_check_dns_direct_encrypted_configuration':
      'Konfiguracja DNS bezpośredniego',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Stan działania DNS bezpośredniego',
  'diag_check_dns_direct_encrypted_reachability': 'Dostępność szyfrowanego DNS',
  'diag_check_transport_h3_path_validation_probe': 'Izolowane uzgadnianie QUIC',
  'nq_finding_unavailable': 'Ten pomiar jest niedostępny w bieżącym stanie.',
  'nq_finding_invalid_configuration':
      'Niestandardowa konfiguracja DNS jest nieprawidłowa.',
  'nq_finding_dns_system':
      'Używany jest DNS bieżącej sieci; kontrole szyfrowanego DNS nie mają zastosowania.',
  'nq_finding_unsupported':
      'Zaktualizuj Usque, aby używać szyfrowanego DNS. Zapytania nie przejdą na nieszyfrowany DNS.',
  'nq_finding_dns_custom_valid':
      'Niestandardowa konfiguracja szyfrowanego DNS jest prawidłowa. '
      'Przełączenie na nieszyfrowany DNS jest wyłączone.',
  'nq_finding_stale':
      'Odczyt jest nieaktualny albo sieć fizyczna uległa zmianie.',
  'nq_finding_rtt_high': 'Zmierzony czas rundy jest podwyższony.',
  'nq_finding_healthy': 'Dostępne pomiary połączenia mieszczą się w normie.',
  'nq_finding_loss_high': 'Utrata pakietów w interwale jest podwyższona.',
  'nq_finding_queue_pressure':
      'Ruch czeka na wysłanie lub podczas tego połączenia odrzucono część danych.',
  'nq_finding_pmtu_degraded':
      'Usque nie zdołało potwierdzić odpowiedniego rozmiaru pakietów dla tego połączenia.',
  'nq_finding_migration_reconnect':
      'Przy zmianie sieci to połączenie musi zostać nawiązane ponownie.',
  'nq_finding_dns_changed':
      'Zapisany tryb DNS różni się od trybu działającego połączenia.',
  'nq_finding_dns_runtime': 'Szyfrowany DNS działa.',
  'nq_finding_dns_degraded':
      'Szyfrowany DNS ma problemy. Nieudane zapytania nie zostaną wysłane do nieszyfrowanego DNS sieci.',
  'nq_finding_probe_unsafe': 'Ten pomiar jest niedostępny w bieżącym stanie.',
  'nq_finding_probe_success': 'To sprawdzenie zakończyło się powodzeniem.',
  'nq_finding_probe_cancelled': 'To sprawdzenie zostało anulowane.',
  'nq_finding_probe_timeout':
      'Sprawdzenie diagnostyki przekroczyło limit czasu',
  'nq_finding_probe_failed': 'To sprawdzenie nie powiodło się.',
  'diag_fix_nq_profile':
      'Przejrzyj niestandardowe pola DNS i nazwę certyfikatu. Nie wyłączaj '
      'weryfikacji TLS.',
  'diag_fix_nq_retry': 'Poczekaj na stabilną sieć, a następnie ponów.',
  'diag_fix_nq_network':
      'Sprawdź lokalną łączność i porównaj świeżą próbkę, zanim zmienisz '
      'ustawienia.',
  'diag_fix_nq_reconnect':
      'Połącz ponownie, aby zastosować zapisaną konfigurację.',
  'nav_network_quality': 'Jakość',
  'network_quality': 'Jakość sieci',
  'nq_subtitle': 'Odczytaj połączenie, nie tylko prędkość.',
  'nq_local_only': 'Tylko pomiary lokalne. Nic nie jest wysyłane.',
  'nq_doctor': 'Uruchom diagnostę sieci',
  'nq_doctor_help':
      'Sprawdzenia standardowe odczytują tylko stan lokalny. Nie otwierają '
      'połączeń zewnętrznych ani nie zmieniają ustawień.',
  'nq_live': 'Na żywo',
  'nq_stale': 'Nieaktualne odczyty',
  'nq_updated': 'Ostatnia próbka',
  'nq_seconds': '{count} s temu',
  'nq_good': 'Dobra',
  'nq_fair': 'Średnia',
  'nq_poor': 'Słaba',
  'nq_limited': 'Ograniczone dane',
  'nq_disconnected': 'Rozłączono',
  'nq_connecting': 'Łączenie',
  'nq_connected': 'Połączono',
  'nq_unavailable': 'Niedostępne',
  'nq_not_ready': 'Niegotowe',
  'nq_unsupported': 'Nieobsługiwane',
  'nq_capability_missing':
      'Ta wersja nie pokazuje jakości połączenia. Łączenie i rozłączanie nadal działają. Sprawdź aktualizacje w Ustawieniach.',
  'nq_empty': 'Połącz się, aby zobaczyć pomiary.',
  'nq_stale_help':
      'Aktualizacje są wstrzymane. Wyświetlane są ostatnie odczyty.',
  'nq_rtt': 'Czas rundy',
  'nq_latest': 'Najnowszy',
  'nq_smoothed': 'Wygładzony',
  'nq_minimum': 'Minimalny',
  'nq_h2_ping': 'PING protokołu HTTP/2',
  'nq_h3_rtt': 'Pomiar ścieżki QUIC',
  'nq_throughput': 'Przepustowość',
  'nq_download': 'Pobieranie',
  'nq_upload': 'Wysyłanie',
  'nq_one_second': '1 sekunda',
  'nq_five_seconds': 'Średnia z 5 sekund',
  'nq_loss': 'Utrata pakietów',
  'nq_loss_h2': 'HTTP/2 nie udostępnia porównywalnej utraty pakietów.',
  'nq_loss_interval':
      'Zmierzono w ostatnim interwale; to nie utrata z całego czasu '
      'działania.',
  'nq_congestion': 'Przeciążenie',
  'nq_cwnd': 'Okno przeciążenia',
  'nq_in_flight': 'Bajty w transmisji',
  'nq_send_rate': 'Szybkość dostarczania',
  'nq_h2_window': 'Okna odbioru HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Połączenie',
  'nq_stalls': 'Wstrzymania pojemności',
  'nq_pmtu': 'MTU ścieżki',
  'nq_outer_pmtu': 'Limit ładunku UDP zewnętrznego',
  'nq_inner_payload': 'Limit ładunku CONNECT-IP',
  'nq_pmtu_help':
      'To rozmiar pakietu obsługiwany przez ścieżkę sieciową. Usque sprawdza go automatycznie, aby ograniczać straty. Kontrola nie zwiększa MTU VPN z Zaawansowanych ustawień sieci.',
  'nq_migration': 'Migracja sieci',
  'nq_migration_help':
      'Usque próbuje utrzymać połączenie przy zmianie sieci, np. z Wi-Fi na komórkową. Obie muszą używać tej samej wersji IP, IPv4 lub IPv6. Ruch płynie jedną siecią naraz; prędkości nie są sumowane.',
  'nq_attempts': 'Próby',
  'nq_successes': 'Udane',
  'nq_failures': 'Nieudane',
  'nq_last_duration': 'Ostatni czas trwania',
  'nq_direct_dns': 'DNS bezpośredni',
  'nq_system_dns': 'DNS bieżącej sieci',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Gotowe',
  'nq_degraded': 'Pogorszony',
  'nq_timeouts': 'Przekroczenia czasu',
  'nq_last_rtt': 'Ostatni RTT',
  'nq_dns_redacted':
      'Nazwy resolvera i adresy bootstrap są pokazywane tylko w '
      'ustawieniach.',
  'nq_queues': 'Obciążenie kolejki',
  'nq_queue_details': 'Kolejki niskiego poziomu',
  'nq_queue_empty': 'Brak jeszcze pomiarów kolejki.',
  'nq_current_capacity': 'Bieżące / pojemność',
  'nq_high_water': 'Poziom maksymalny',
  'nq_drops': 'Odrzucenia',
  'nq_oldest': 'Najstarszy element',
  'nq_tunToTransport': 'Urządzenie → transport',
  'nq_proxyToTransport': 'Proxy → warstwa transportu',
  'nq_transportOutgoing': 'Wychodzący transport',
  'nq_h3DatagramSend': 'Datagramy QUIC',
  'nq_h3WireSend': 'Wyjście UDP',
  'nq_transportToTun': 'Transport → urządzenie',
  'nq_transportToProxy': 'Warstwa transportu → proxy',
  'nq_directDns': 'Żądania DNS bezpośredniego',
  'nq_unknown_queue': 'Inna kolejka',
  'nq_trends': 'Ostatnie 60 sekund',
  'nq_samples': 'próbek',
  'nq_pause': 'Wstrzymaj wykresy',
  'nq_resume': 'Wznów wykresy',
  'nq_paused': 'Wykresy wstrzymane',
  'nq_gaps': 'Brakujące próbki są lukami.',
  'nq_phase_idle': 'Bezczynny',
  'nq_phase_preparing_socket': 'Przygotowywanie ścieżki',
  'nq_phase_probing': 'Sondowanie',
  'nq_phase_validated': 'Zweryfikowano',
  'nq_phase_promoting': 'Przełączanie ścieżki',
  'nq_phase_stable': 'Stabilna',
  'nq_phase_aborted': 'Przerwano',
  'nq_phase_revalidating': 'Ponowna weryfikacja',
  'nq_phase_degraded': 'Pogorszony',
  'nq_phase_unknown': 'Niegotowe',
  'nq_phase_unsupported': 'Nieobsługiwane',
  'nq_reason_family_unavailable':
      'Nowa sieć nie obsługuje tej samej wersji IP. Należy połączyć się ponownie.',
  'nq_reason_socket_protect_failed':
      'Usque nie mogło bezpiecznie użyć nowej sieci. Jeśli połączenie nie wróci, połącz się ręcznie.',
  'nq_reason_generation_changed_during_setup':
      'Sieć zmieniła się ponownie podczas przygotowania.',
  'nq_reason_peer_cid_unavailable':
      'Serwer nie utrzymał połączenia w nowej sieci. W razie potrzeby połącz się ręcznie.',
  'nq_reason_local_cid_unavailable':
      'Usque nie utrzymało połączenia w nowej sieci. W razie potrzeby połącz się ręcznie.',
  'nq_reason_path_probe_rejected':
      'Nowa sieć nie przeszła kontroli połączenia. Sprawdź jej dostęp do Internetu.',
  'nq_reason_path_validation_timeout':
      'Nowa sieć nie odpowiedziała na czas. Sprawdź ją i w razie potrzeby połącz się ponownie.',
  'nq_reason_superseded':
      'Sieć zmieniła się ponownie przed zakończeniem przełączania.',
  'nq_reason_promotion_failed':
      'Usque nie zakończyło bezpiecznie zmiany sieci. Jeśli połączenie nie wróci, połącz się ręcznie.',
  'nq_reason_connection_closed':
      'Połączenie zamknięto podczas zmiany sieci. Połącz się ponownie.',
  'nq_reason_unsupported': 'Migracja jest niedostępna w tym połączeniu.',
  'nq_reason_unknown': 'Brak dostępnego obsługiwanego powodu.',
  'nq_dns_custom': 'Niestandardowy szyfrowany resolver',
  'nq_dns_server': 'Domena serwera DNS',
  'nq_dns_path': 'Ścieżka HTTPS',
  'nq_dns_port': 'Port (0 używa wartości domyślnej)',
  'nq_dns_bootstrap': 'Adresy IP serwera DNS',
  'nq_dns_bootstrap_help':
      'Wpisz 1–8 IP od dostawcy DNS, po jednym w wierszu, np. 1.1.1.1. Usque łączy się z nimi bezpośrednio, bez wcześniejszego wyszukiwania nazwy serwera.',
  'nq_dns_no_fallback':
      'Jeśli szyfrowany DNS bezpośredni zawiedzie, zapytanie kończy się '
      'niepowodzeniem. Nigdy nie następuje przełączenie na systemowy ani '
      'nieszyfrowany DNS.',
  'nq_dns_system_privacy':
      'Dostawca DNS bieżącej sieci może widzieć domeny żądane przez ruch bezpośredni.',
  'nq_dns_scope':
      'Dotyczy ruchu pasującego do reguł krajów połączeń bezpośrednich. DNS ruchu VPN pozostaje bez zmian.',
  'nq_dns_no_capability':
      'Zaktualizuj Usque, aby używać szyfrowanego DNS dla ruchu bezpośredniego. Ustawienia zostaną zachowane. Możesz wybrać DNS bieżącej sieci, jeśli akceptujesz wpływ na prywatność.',
  'nq_dns_invalid_name':
      'Wpisz domenę, np. dns.example.com, bez https://, portu i spacji.',
  'nq_dns_invalid_path':
      'Wpisz ścieżkę, np. /dns-query, do 256 znaków. Usuń spacje i części zaczynające się od ? lub #.',
  'nq_dns_invalid_bootstrap': 'Wpisz od 1 do 8 adresów IP serwera.',
  'nq_dns_invalid_port': 'Wpisz port od 1 do 65535 lub 0, aby użyć domyślnego.',
  'nq_dns_invalid_mode': 'Wybierz obsługiwany tryb DNS.',
  'nq_doctor_deep_title': 'Uruchomić głębokie sprawdzenia sieci?',
  'nq_doctor_deep_body':
      'Testy mogą wysyłać ruch próbny. Trwają do 15 sekund i można je anulować. Ustawienia połączenia nie ulegną zmianie.',
  'nq_doctor_deep_run': 'Uruchom głębokie sprawdzenia',
  'nq_doctor_evidence':
      'Te testy nie pozwalają potwierdzić, czy występują wycieki DNS.',
};

const Map<String, String> kWindowsRecoveryPl = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Składniki połączenia Usque wymagają wspólnej aktualizacji. Sprawdź aktualizacje w Ustawieniach. Nie uruchomiono nowego połączenia VPN.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'Czyszczenie poprzedniego połączenia VPN nie zostało zakończone. Całkowicie zamknij Usque i otwórz ponownie. Jeśli to nie pomoże, otwórz Diagnostykę.',
  'WINDOWS_RECOVERY_FAILED':
      'Nie można w pełni przywrócić poprzedniego stanu sieci VPN. Nie '
      'rozpoczęto nowego połączenia VPN. Ponów połączenie albo sprawdź '
      'lokalną diagnostykę.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows nie mógł przywrócić poprzedniego stanu sieci VPN po trzech '
      'automatycznych próbach. Ponów, gdy będziesz gotowy, albo sprawdź '
      'lokalną diagnostykę.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Automatyczną naprawę zatrzymano, ponieważ nie potwierdzono bezpiecznego przywrócenia poprzednich ustawień VPN. Sprawdź aktualizacje w Ustawieniach; jeśli problem pozostanie, wyeksportuj pakiet diagnostyczny.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Odzyskiwanie sieci Windows trwa dłużej niż oczekiwano. Nie '
      'rozpoczęto nowego połączenia VPN. Poczekaj na zakończenie '
      'odzyskiwania, zanim ponowisz próbę.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Stan sieci uległ zmianie albo jest nadal używany przez inną sesję. '
      'Automatyczne odzyskiwanie zostało zatrzymane, aby chronić aktywne '
      'połączenie.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Ta instalacja nie przywraca automatycznie poprzednich ustawień VPN. Zaktualizuj Usque w Ustawieniach i spróbuj ponownie.',
};

const String kWindowsAdapterCleanupPl =
    'Nie udało się usunąć wirtualnej karty sieciowej poprzedniego połączenia lub potwierdzić jej usunięcia. Nie uruchomiono nowego połączenia VPN.';

const Map<String, String> kL4Pl = <String, String>{
  'l4_quic_not_ready': 'Przygotowywanie połączenia L4',
  'l4_unsupported_packets': 'Odrzucono nieobsługiwane lub uszkodzone pakiety',
  'l4_budget_rejections': 'Odrzucone przyjęcia zasobów',
  'l4_not_applicable': 'Nie dotyczy (L4)',
  'l4_mode': 'L4 (eksperymentalny)',
  'l4_transport_hint':
      'Tylko TCP. Aplikacje wymagające UDP mogą nie działać. Tryb automatyczny nie wybiera L4.',
  'l4_explanation':
      'L4 przenosi TCP przez HTTP/3 i działa z VPN oraz proxy SOCKS5 i HTTP. Zapytania DNS z VPN są zamieniane na TCP. Aplikacje wymagające innego ruchu UDP, zdalnego Ping, fragmentów IP lub nagłówków rozszerzeń mogą nie działać. Wybierz L4 ręcznie; tryb automatyczny go nie wybiera.',
  'l4_unsupported':
      'L4 jest niedostępne w tej wersji Usque. Sprawdź aktualizacje w Ustawieniach.',
  'l4_sni_identity':
      'Ustawiane automatycznie przez konto. Nazwa serwera dla innych trybów połączenia pozostaje zachowana.',
  'l4_edge_requires_l4':
      'DNS rozwiązywany na brzegu wymaga L4. Wybierz inny tryb DNS proxy przed przełączeniem na Auto, H3 lub H2.',
  'proxy_dns_edge_resolved':
      'Brzeg Cloudflare (tylko L4; bez lokalnego wyszukiwania)',
  'l4_verified': 'L4 pomyślnie nawiązało połączenie aplikacji',
  'l4_unverified':
      'Serwer połączony; połączenie aplikacji jeszcze niepotwierdzone',
  'l4_status_unknown': 'Nie można potwierdzić stanu połączenia aplikacji',
  'l4_sessions': 'Sesje / opróżnianie',
  'l4_flows': 'Aktywne / oczekujące strumienie',
  'l4_connect': 'CONNECT sukcesy / błędy / przekroczenia czasu',
  'l4_buffers': 'Zużyty budżet bufora aplikacji (bajty)',
  'l4_backpressure': 'Przeciwciśnienie wysyłania / odbierania',
  'l4_tun_flows': 'TUN TCP / półotwarte',
  'l4_udp': 'Odrzucone pakiety UDP',
  'l4_dns': 'Konwersje DNS sukcesy / błędy / przekroczenia czasu',
  'l4_migration':
      'Strumienie zachowane przez migrację / zakończone przez przebudowę',
  'l4_na':
      'Sterowanie adresem CONNECT-IP, kolejki DATAGRAM, MTU ładunku wewnętrznego i limit czasu UDP: nie dotyczy w L4.',
};

const Map<String, String> kNetworkSettingsPl = <String, String>{
  'settings_applying': 'Zapisano, trwa stosowanie',
  'settings_applied': 'Zapisano i zastosowano',
  'settings_deferred':
      'Zapisano, zacznie obowiązywać przy następnym ręcznym połączeniu',
  'settings_failed': 'Zapisano, stosowanie nie powiodło się',
  'settings_unknown': 'Wynik jeszcze niepotwierdzony',
  'settings_saved': 'Zapisano',
  'settings_unsupported':
      'Całkowicie zamknij Usque, otwórz ponownie i zapisz jeszcze raz. Jeśli to nie pomoże, sprawdź aktualizacje w Ustawieniach.',
  'settings_save_failed':
      'Nie udało się zapisać ustawień. Twoje zmiany zostały zachowane.',
  'settings_reconnect': 'Połącz ponownie',
};

const Map<String, String> kChainPl = <String, String>{
  "batch_title": "Importuj konfiguracje",
  "batch_counts":
      "Gotowe: {ready} · Niepełne: {pending} · Błędy: {failed} · Zapisane: {saved}",
  "batch_ready": "Gotowa do importu",
  "batch_pending": "Uzupełnij nazwę lub dane logowania",
  "batch_saved": "Zaimportowano",
  "batch_close": "Zamknij",
  "batch_import": "Importuj poprawne wpisy ({count})",
  "batch_checking": "Sprawdzanie {done} z {total}",
  "batch_saving": "Zapisywanie konfiguracji…",
  "batch_uncertain":
      "Zapisywanie przerwano. Zamknij i sprawdź bibliotekę przed ponownym importem; część wpisów mogła już zostać zapisana.",
  "file_count_limit": "Wybierz najwyżej 128 plików naraz.",
  'duplicate_directive': 'Ta dyrektywa może wystąpić tylko raz.',
  'mixed_protocols':
      'Wszystkie punkty remote muszą używać tego samego transportu TCP albo UDP.',
  'conflicting_protocol':
      'Wartość remote koliduje z globalnym ustawieniem transportu.',
  'too_many_endpoints': 'Użyj najwyżej 16 punktów remote.',
  'conflicting_authentication':
      'CLIENT_CERT koliduje z osadzonym certyfikatem albo trybem uwierzytelniania.',
  'serialized_size_limit':
      'Zaszyfrowany rekord przekroczyłby limit rozmiaru przechowywania.',
  'multi_endpoint_unavailable':
      'Zaktualizuj silnik, aby używać konfiguracji z wieloma punktami.',
  'candidates': 'Punkty startowe',
  'random_order':
      'Punkty są próbowane w nowej kolejności losowej przy każdym połączeniu.',
  'file_order': 'Punkty są próbowane w kolejności z pliku.',
  'attempting': 'Próbowany punkt',
  'actual_endpoint': 'Połączony punkt',
  'attempt_failures': 'Nieudane próby',
  'failure_transport': 'transport zamknięty',
  'failure_authentication': 'uwierzytelnianie',
  'failure_certificate': 'certyfikat',
  'failure_configuration': 'konfiguracja',
  'failure_address_changed': 'zmiana adresu',
  'failure_protocol': 'protokół',
  'failure_cleanup': 'czyszczenie',
  'failure_reason': 'Przyczyna: {reason}.',
  'manage': 'Zarządzaj',
  'dns_fallback': 'DNS tunelu (OpenVPN może negocjować DNS)',
  'dns_unavailable_title': 'Brak DNS przez to wyjście',
  'dns_unavailable':
      'Przez to wyjście nie można osiągnąć żadnego serwera DNS. Użyj adresów IP albo wybierz inne wyjście z osiągalnym DNS.',
  'authentication_failed':
      'Uwierzytelnianie nie powiodło się. Zaktualizuj dane logowania przed ponownym połączeniem.',
  'profile_limit': 'Biblioteka konfiguracji jest pełna (128 profili).',
  'metadata_limit': 'Metadane biblioteki konfiguracji są pełne.',
  'title': 'Proxy łańcuchowe',
  'subtitle': 'Wybierz wyjście osiągane przez WARP.',
  'source': 'Źródło wyjścia',
  'enable': 'Włącz proxy łańcuchowe',
  'import_file': 'Importuj plik',
  'paste': 'Wklej konfigurację',
  'profiles': 'Zapisane konfiguracje',
  'empty': 'Zaimportuj konfigurację, aby wybrać wyjście.',
  'empty_hint_openvpn':
      'Zaimportuj plik .ovpn albo wklej jego treść. Obsługiwane są punkty TCP i UDP, osadzone certyfikaty oraz nazwa użytkownika i hasło.',
  'empty_hint_wireguard':
      'Zaimportuj plik .conf albo wklej jego treść. Obsługiwana jest jedna sekcja [Interface] i jedna [Peer].',
  'import_limits':
      'Konfiguracje muszą być tekstem UTF-8 do 128 KiB. Bez wyboru pliku wklej tekst.',
  'enable_to_choose': 'Włącz proxy łańcuchowe, aby wybrać konfigurację.',
  'select_required': 'Wybierz zapisaną konfigurację, zanim ją zastosujesz.',
  'pending_disable': 'Oczekuje: wyłączenie proxy łańcuchowego',
  'apply_reconnect': 'Zastosuj i połącz ponownie',
  'requires_connect_ip': 'Wymaga CONNECT-IP',
  'menu': 'Działania konfiguracji',
  'preview': 'Sprawdź konfigurację',
  'save_import': 'Zapisz konfigurację',
  'name': 'Nazwa',
  'configuration': 'Tekst konfiguracji',
  'file_loaded': 'Konfiguracja wczytana z pliku ({lines} wierszy).',
  'username': 'Nazwa użytkownika',
  'password': 'Hasło',
  'key_password': 'Hasło klucza prywatnego',
  'show_password': 'Pokaż hasło',
  'hide_password': 'Ukryj hasło',
  'credentials': 'Zaktualizuj dane logowania',
  'rename': 'Zmień nazwę',
  'delete': 'Usuń',
  'cancel': 'Anuluj',
  'save': 'Zapisz',
  'apply': 'Zastosuj zmiany',
  'clear': 'Wyczyść wybór',
  'current': 'Bieżące połączenie',
  'saved': 'Zapisany wybór',
  'draft': 'Wybór oczekujący',
  'disconnected': 'Niepołączony',
  'disabled': 'Niewłączony',
  'enabled_idle': 'Włączony · niepołączony',
  'disconnecting': 'Rozłączanie',
  'file_read_failed': 'Nie udało się odczytać pliku konfiguracji.',
  'file_encoding_invalid': 'Plik konfiguracji musi być tekstem UTF-8.',
  'file_busy': 'Wybór pliku jest już otwarty.',
  'connected': 'Połączono',
  'connecting': 'Łączenie',
  'error': 'Połączenie nie powiodło się',
  'no_selection': 'Nie wybrano konfiguracji',
  'l4': 'Ta konfiguracja wymaga trybu CONNECT-IP.',
  'switch_mode': 'Przełącz na CONNECT-IP i zastosuj',
  'unsupported': 'Ten silnik nie obsługuje tego źródła wyjścia.',
  'scope':
      'Istniejące jawne reguły bezpośrednie nadal obowiązują. Pozostały ruch używa wybranego wyjścia.',
  'allowed': 'Dozwolone miejsca docelowe',
  'dns': 'DNS',
  'addresses': 'Adresy tunelu',
  'address_family': 'Rodzina adresów',
  'transport': 'Protokół transportowy',
  'endpoint': 'Serwer',
  'restricted':
      'Miejsca docelowe poza AllowedIPs są blokowane na ścieżce proxy.',
  'delete_confirm':
      'Usunąć tę zapisaną konfigurację? Oryginalny zaimportowany plik pozostaje bez zmian.',
  'profile_in_use':
      'Wybierz inną konfigurację albo wyczyść zapisany wybór, zanim usuniesz tę konfigurację.',
  'stale_revision':
      'Konfiguracja się zmieniła. Odśwież listę i spróbuj ponownie.',
  'secure_storage_failed':
      'Nie udało się odczytać ani zapisać zaszyfrowanej konfiguracji.',
  'invalid_configuration':
      'Konfiguracja jest nieprawidłowa albo zawiera nieobsługiwane opcje.',
  'looks_like_wireguard':
      'To wygląda na konfigurację WireGuard. Przełącz źródło wyjścia na WireGuard.',
  'looks_like_openvpn':
      'To wygląda na konfigurację OpenVPN. Przełącz źródło wyjścia na OpenVPN.',
  'error_location': '{message} ({field}, wiersz {line})',
  'error_field': '{message} ({field})',
  'file_unavailable': 'Wybór pliku jest niedostępny. Wklej tekst konfiguracji.',
  'invalid_size_or_encoding': 'Użyj konfiguracji UTF-8 o rozmiarze do 128 KiB.',
  'unsupported_directive': 'Ta dyrektywa OpenVPN nie jest obsługiwana.',
  'unsupported_or_duplicate_field':
      'To pole jest nieobsługiwane albo powtórzone.',
  'unsupported_or_duplicate_section':
      'Użyj jednej sekcji Interface i jednej sekcji Peer.',
  'missing_field': 'Brakuje wymaganego pola.',
  'invalid_name':
      'Użyj nazwy o długości od 1 do 64 znaków bez znaków sterujących.',
  'invalid_key':
      'Klucz musi być prawidłowym kluczem Base64 o długości 32 bajtów.',
  'checking': 'Sprawdzanie konfiguracji…',
  'changed': 'Zmiany zapisane',
};
