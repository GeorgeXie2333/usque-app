/// Supplemental feature strings for Polish.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowPl = <String, String>{
  'cc_label': 'Kontrola przeciążenia HTTP/3',
  'cc_help': 'Zacznie obowiązywać przy następnym ręcznym połączeniu.',
  'cc_upgrade': 'Wymagana aktualizacja Engine.',
  'cc_h2': 'HTTP/2 korzysta z systemowego TCP.',
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
  'home_traffic_idle': 'Zaczyna się po połączeniu',
  'home_traffic_waiting': 'Oczekiwanie na próbki',
  'home_traffic_unavailable': 'Historia niedostępna',
  'home_traffic_stale': 'Próbki opóźnione',
  'home_outputs_next': 'Wyjścia włączane po połączeniu',
  'home_outputs_retry': 'Wyjścia skonfigurowane na następną próbę',
  'connection_protection_group': 'Połączenie i ochrona',
  'proxy_routing_group': 'Proxy i trasowanie',
  'application_group': 'Aplikacja',
  'proxy_settings_link': 'Adresy nasłuchu, porty, uwierzytelnianie i DNS.',
  'proxy_auth_separate':
      'Poświadczenia są zapisywane osobno przyciskiem Zapisz '
      'poświadczenia.',
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
      'Wybrano fizyczny systemowy DNS; sprawdzenia szyfrowanego DNS nie '
      'mają zastosowania.',
  'nq_finding_unsupported':
      'Szyfrowany DNS jest niedostępny w tym Engine; przełączenie na '
      'nieszyfrowany DNS nie jest dozwolone.',
  'nq_finding_dns_custom_valid':
      'Niestandardowa konfiguracja szyfrowanego DNS jest prawidłowa. '
      'Przełączenie na nieszyfrowany DNS jest wyłączone.',
  'nq_finding_stale':
      'Odczyt jest nieaktualny albo sieć fizyczna uległa zmianie.',
  'nq_finding_rtt_high': 'Zmierzony czas rundy jest podwyższony.',
  'nq_finding_healthy':
      'Dostępny pomiar lokalny mieści się w oczekiwanym zakresie.',
  'nq_finding_loss_high': 'Utrata pakietów w interwale jest podwyższona.',
  'nq_finding_queue_pressure':
      'Kolejka jest obciążona albo zarejestrowała odrzucenia podczas tego '
      'połączenia.',
  'nq_finding_pmtu_degraded': 'Weryfikacja MTU ścieżki jest pogorszona.',
  'nq_finding_migration_reconnect':
      'Migracja jest niedostępna na tej ścieżce; zmiana sieci powoduje '
      'pełne ponowne połączenie.',
  'nq_finding_dns_changed':
      'Zapisany tryb DNS różni się od trybu działającego połączenia.',
  'nq_finding_dns_runtime':
      'Szyfrowany DNS zakończył się powodzeniem. Stan lokalny nie jest '
      'zewnętrznym dowodem braku wycieków.',
  'nq_finding_dns_degraded':
      'Szyfrowany DNS jest pogorszony; nieudane zapytania bezpośrednie nie '
      'przełączają się na systemowy DNS.',
  'nq_finding_probe_unsafe':
      'Pominięto sondę: wymagany bezpieczny stan lub zapisana tożsamość '
      'jest niedostępna. Aktywny tunel nigdy nie jest duplikowany.',
  'nq_finding_probe_success':
      'Uwierzytelniona sonda została ukończona. To nie jest zewnętrzny '
      'test wycieku pakietów.',
  'nq_finding_probe_cancelled': 'Sonda anulowana i zażądano czyszczenia.',
  'nq_finding_probe_timeout':
      'Ograniczona sonda nie zakończyła się przed terminem.',
  'nq_finding_probe_failed':
      'Uwierzytelniona sonda nie powiodła się; nie próbowano '
      'niebezpiecznego przełączenia.',
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
  'nq_doctor': 'Uruchom doktora sieci',
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
      'Ten Engine nie udostępnia jakości sieci. Istniejące elementy '
      'sterowania połączeniem nadal działają.',
  'nq_empty':
      'Połącz, aby zobaczyć pomiary. Nieznane wartości nie są pokazywane '
      'jako zero.',
  'nq_stale_help':
      'Źródło przestało się aktualizować. To poprzednie odczyty; luki '
      'pozostają lukami.',
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
  'nq_pmtu_help': 'Wykrywanie ścieżki nie zwiększa MTU TUN urządzenia.',
  'nq_migration': 'Migracja sieci',
  'nq_migration_help':
      'Jedno połączenie, jedna ścieżka danych. Tylko ta sama rodzina IP; '
      'to nie wielościeżkowość.',
  'nq_attempts': 'Próby',
  'nq_successes': 'Udane',
  'nq_failures': 'Nieudane',
  'nq_last_duration': 'Ostatni czas trwania',
  'nq_direct_dns': 'DNS bezpośredni',
  'nq_system_dns': 'Fizyczny systemowy DNS',
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
      'Bieżąca rodzina IP jest niedostępna; używane jest pełne ponowne '
      'połączenie.',
  'nq_reason_socket_protect_failed':
      'Nie można przygotować chronionego gniazda kandydującego.',
  'nq_reason_generation_changed_during_setup':
      'Sieć zmieniła się ponownie podczas przygotowania.',
  'nq_reason_peer_cid_unavailable':
      'Druga strona nie ma zapasowego identyfikatora połączenia.',
  'nq_reason_local_cid_unavailable':
      'Lokalny identyfikator połączenia jest niedostępny.',
  'nq_reason_path_probe_rejected':
      'Nie można zweryfikować ścieżki kandydującej.',
  'nq_reason_path_validation_timeout':
      'Weryfikacja ścieżki przekroczyła limit czasu; dostępne jest ponowne '
      'połączenie.',
  'nq_reason_superseded': 'Nowsza zmiana sieci zastąpiła tę próbę.',
  'nq_reason_promotion_failed':
      'Nie można bezpiecznie dokończyć przełączenia ścieżki.',
  'nq_reason_connection_closed':
      'Połączenie zostało zamknięte podczas migracji.',
  'nq_reason_unsupported': 'Migracja jest niedostępna w tym połączeniu.',
  'nq_reason_unknown': 'Brak dostępnego obsługiwanego powodu.',
  'nq_dns_custom': 'Niestandardowy szyfrowany resolver',
  'nq_dns_server': 'Nazwa serwera TLS',
  'nq_dns_path': 'Ścieżka HTTPS',
  'nq_dns_port': 'Port (0 używa wartości domyślnej)',
  'nq_dns_bootstrap': 'Adresy IP bootstrap',
  'nq_dns_bootstrap_help':
      'Wpisz 1–8 numerycznych adresów IP, po jednym w wierszu. Nazwy '
      'hostów nie są rozwiązywane.',
  'nq_dns_no_fallback':
      'Jeśli szyfrowany DNS bezpośredni zawiedzie, zapytanie kończy się '
      'niepowodzeniem. Nigdy nie następuje przełączenie na systemowy ani '
      'nieszyfrowany DNS.',
  'nq_dns_system_privacy':
      'Fizyczny systemowy DNS może ujawnić nazwy zapytań bezpośrednich '
      'dostawcy DNS sieci fizycznej.',
  'nq_dns_scope':
      'Używane tylko do zapytań bezpośrednich wybranych przez Geo. DNS '
      'tunelu pozostaje bez zmian.',
  'nq_dns_no_capability':
      'Ten Engine nie może używać szyfrowanego DNS bezpośredniego. '
      'Zapisane ustawienia zostaną zachowane. Możesz jawnie wybrać '
      'systemowy DNS.',
  'nq_dns_invalid_name':
      'Wpisz nazwę DNS bez spacji, składni URL ani symboli wieloznacznych.',
  'nq_dns_invalid_path':
      'Użyj ścieżki zaczynającej się od / o długości do 256 znaków, bez '
      'zapytania, fragmentu ani spacji.',
  'nq_dns_invalid_bootstrap':
      'Użyj 1–8 unikatowych adresów IP unicast; bez adresu nieokreślonego, '
      'multicast, broadcast ani IPv6 link-local.',
  'nq_dns_invalid_port': 'Wpisz wartość 0–65535.',
  'nq_dns_invalid_mode': 'Wybierz obsługiwany tryb DNS.',
  'nq_doctor_deep_title': 'Uruchomić głębokie sprawdzenia sieci?',
  'nq_doctor_deep_body':
      'Głębokie sprawdzenia mogą wysłać testowe zapytanie DNS do '
      'skonfigurowanego resolvera i zweryfikować chronioną ścieżkę QUIC. '
      'Trwają najwyżej 15 sekund, można je anulować, nigdy nie tworzą '
      'drugiego tunelu przenoszącego dane i nigdy nie zmieniają DNS, tras, '
      'profilu ani transportu.',
  'nq_doctor_deep_run': 'Uruchom głębokie sprawdzenia',
  'nq_doctor_evidence':
      'Lokalne sprawdzenia opisują konfigurację i zaobserwowany stan. Nie '
      'stanowią zewnętrznego dowodu braku wycieków DNS.',
};

const Map<String, String> kWindowsRecoveryPl = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'Nie można w pełni przywrócić poprzedniego stanu sieci VPN. Nie '
      'rozpoczęto nowego połączenia VPN. Ponów połączenie albo sprawdź '
      'lokalną diagnostykę.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows nie mógł przywrócić poprzedniego stanu sieci VPN po trzech '
      'automatycznych próbach. Ponów, gdy będziesz gotowy, albo sprawdź '
      'lokalną diagnostykę.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Automatyczna naprawa została zatrzymana, ponieważ poprzedniego '
      'stanu sieci Windows nie można było bezpiecznie zweryfikować. '
      'Uruchom ponownie Agent albo zaktualizuj Usque, a następnie sprawdź '
      'lokalną diagnostykę.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Odzyskiwanie sieci Windows trwa dłużej niż oczekiwano. Nie '
      'rozpoczęto nowego połączenia VPN. Poczekaj na zakończenie '
      'odzyskiwania, zanim ponowisz próbę.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Stan sieci uległ zmianie albo jest nadal używany przez inną sesję. '
      'Automatyczne odzyskiwanie zostało zatrzymane, aby chronić aktywne '
      'połączenie.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Ten Agent systemu Windows nie obsługuje bezpiecznego automatycznego '
      'odzyskiwania. Zaktualizuj aplikację i Agent razem, a następnie '
      'ponów.',
};

const String kWindowsAdapterCleanupPl =
    'Nie można usunąć poprzedniego adaptera Wintun albo nie można '
    'potwierdzić jego usunięcia. Nie rozpoczęto nowego połączenia VPN.';
