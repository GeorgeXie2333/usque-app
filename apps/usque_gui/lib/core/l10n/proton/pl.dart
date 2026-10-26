const kProtonPl = <String, String>{
  'fallback_account':
      'Nie można połączyć się z Proton. Wybierz istniejące konto WARP i spróbuj ponownie.',
  'fallback_failed':
      'Nie można połączyć się przez WARP. Sprawdź wybrane konto WARP i sieć, a następnie spróbuj ponownie.',
  'fallback_starting': 'Łączenie przez WARP…',
  'fallback_active':
      'Ta operacja na koncie korzysta z WARP, również podczas weryfikacji.',
  'connection_api':
      'Żądanie do API Proton nie powiodło się. Spróbuj ponownie lub sprawdź konto Proton w sekcji Proxy łańcuchowe.',
  'connection_authentication':
      'Autoryzacja Proton nie powiodła się. Otwórz Proxy łańcuchowe i zaloguj się ponownie do Proton.',
  'connection_certificate':
      'Proton odrzucił certyfikat połączenia, a odzyskiwanie nie powiodło się. Połącz się lub zaloguj do Proton ponownie.',
  'connection_restricted':
      'Serwer Proton jest tymczasowo objęty ograniczeniami. Spróbuj ponownie lub wybierz inny serwer.',
  'connection_sessions':
      'Osiągnięto limit jednoczesnych połączeń Proton. Rozłącz inną sesję Proton i spróbuj ponownie.',
  'connection_key':
      'Proton wykrył ten klucz połączenia w wielu sesjach. Rozłącz inną sesję i spróbuj ponownie.',
  'connection_policy':
      'Proton ograniczył to połączenie zgodnie z zasadami konta lub użytkowania. Sprawdź konto lub wybierz inny serwer.',
  'connection_server':
      'Serwer Proton nie mógł autoryzować połączenia. Spróbuj innego serwera.',
  'connection_protocol':
      'Nie można zweryfikować odpowiedzi połączenia Proton. Spróbuj ponownie lub wybierz inny serwer.',
  'connection_transport':
      'Nie można połączyć się z Proton. Sprawdź sieć lub wybierz inny serwer.',
  'connection_reconnect':
      'Autoryzacja Proton została odnowiona. Ponowne łączenie…',
  'connection_review':
      'Otwórz Proxy łańcuchowe, aby sprawdzić konto Proton i wybrany serwer.',
  'connection_retry': 'Ponów połączenie Proton lub wybierz inny serwer.',
  'title': 'Proton VPN',
  'guest': 'Kontynuuj jako gość',
  'login': 'Zaloguj się do Proton',
  'username': 'Nazwa użytkownika',
  'password': 'Hasło',
  'totp': 'Kod z aplikacji uwierzytelniającej',
  'submit': 'Kontynuuj',
  'verify': 'Dokończ weryfikację',
  'verification_hint': 'Rozwiąż łamigłówkę w oficjalnym oknie weryfikacji.',
  'choose_node': 'Wybierz bezpłatny serwer',
  'working': 'Przygotowywanie Proton VPN…',
  'waiting_totp': 'Wpisz kod z aplikacji uwierzytelniającej, aby kontynuować.',
  'waiting_verification': 'Wymagana weryfikacja',
  'waiting_node': 'Wybierz serwer, aby zakończyć konfigurację.',
  'completed':
      'Konto zapisane. Wybierz je i zastosuj zmiany, aby korzystać z Proton VPN.',
  'cancelled': 'Konfiguracja anulowana',
  'failed':
      'Konfiguracja Proton nie powiodła się. Spróbuj ponownie w dogodnym momencie.',
  'result_unconfirmed':
      'Wynik konfiguracji Proton nie jest potwierdzony. Odśwież stan, aby kontynuować, lub anuluj obecną konfigurację przed ponownym rozpoczęciem.',
  'unavailable': 'Proton VPN jest niedostępny w tym silniku.',
  'bad_credentials':
      'Sprawdź nazwę użytkownika Proton, hasło lub kod uwierzytelniający.',
  'rate_limited': 'Proton ograniczył to żądanie. Odczekaj przed ponowną próbą.',
  'network':
      'Nie można połączyć się z Proton. Sprawdź sieć lub VPN i spróbuj ponownie.',
  'verification_failed':
      'Nie można dokończyć weryfikacji. Rozpocznij konfigurację ponownie.',
  'verification_invalid':
      'Proton odrzucił wynik weryfikacji. Rozpocznij konfigurację ponownie, aby otrzymać nowe zadanie.',
  'accounts': 'Konta Proton',
  'add_account': 'Dodaj konto',
  'account_actions': 'Działania na koncie',
  'resolve_on_connect': 'Serwer i autoryzacja są sprawdzane podczas łączenia.',
  'account': 'Konto Proton',
  'signed_in': 'Zalogowano',
  'session_requires_login': 'Sesja wymaga logowania',
  'selected_server': 'Serwer',
  'automatic_server': 'Automatycznie',
  'automatic_hint':
      'Podczas łączenia wybierz serwer z najmniejszym opóźnieniem.',
  'available_servers': 'Dostępne serwery',
  'choose_server': 'Wybierz serwer',
  'search_servers': 'Szukaj według nazwy, kraju lub miasta',
  'load_servers': 'Wczytaj listę serwerów',
  'change_server': 'Zmień serwer',
  'refresh_servers': 'Odśwież listę',
  'remove_account': 'Usuń konto',
  'remove_account_confirm':
      'Usunąć to konto i zapisany wybór serwera? Wylogowanie unieważnia też sesje zapisane przez tę aplikację. Usunięcie lokalne nie zmienia sesji zdalnych. Zapisany wybór proxy łańcuchowego zostanie wyczyszczony.',
  'forget_local': 'Usuń tylko lokalnie',
  'sign_out_remove': 'Wyloguj i usuń',
  'current_account': 'Połączone konto',
  'saved_account': 'Zapisany wybór',
  'next_connection': 'Wybór serwera zapisano dla następnego połączenia.',
  'disconnect_to_change':
      'Rozłącz to konto przed zmianą serwera lub usunięciem konta.',
  'cleanup_pending':
      'Nie można potwierdzić unieważnienia wcześniejszej sesji. Wyloguj się ponownie lub wybierz jawnie usunięcie konta tylko lokalnie.',
  'selected_server_unavailable':
      'Wybrany serwer jest niedostępny. Wybierz inny serwer lub opcję Automatycznie.',
  'close': 'Zamknij',
  'verification_error': 'Błąd weryfikacji',
  'refresh': 'Odnów autoryzację lub zmień serwer',
  'logout': 'Wyloguj i usuń',
  'forget': 'Usuń tylko z tego urządzenia',
  'forget_confirm':
      'Usunąć tę konfigurację Proton i zapisaną sesję z tego urządzenia? Nie unieważni to zdalnej sesji Proton.',
  'removed': 'Konto Proton zostało usunięte z tego urządzenia.',
  'needs_login': 'Zaloguj się ponownie',
  'expires': 'Wygaśnięcie autoryzacji',
  'unsupported_second_factor':
      'To konto wymaga metody logowania, która nie jest tutaj obsługiwana.',
  'reauthentication_required':
      'Zaloguj się ponownie, aby odnowić tę sesję Proton.',
  'profile_in_use': 'Rozłącz to konto przed jego zmianą lub usunięciem.',
  'session_busy':
      'Inna operacja Proton nadal trwa. Poczekaj lub najpierw ją anuluj.',
  'api_rejected':
      'Proton odrzucił to żądanie. Poniższe kody identyfikują odpowiedź serwera.',
  'session_not_allowed':
      'Proton nie zezwala temu klientowi na utworzenie sesji.',
  'credentialless_disabled': 'Dostęp gościnny Proton jest obecnie niedostępny.',
  'no_free_nodes':
      'Proton nie zwrócił dostępnych serwerów WireGuard dla tego konta.',
  'invalid_directory': 'Proton zwrócił nieobsługiwaną listę serwerów.',
  'invalid_client_configuration':
      'Proton nie zwrócił dostępnych portów WireGuard.',
  'stage': 'Nieudany krok',
  'stage_anonymous_session': 'Tworzenie anonimowej sesji',
  'stage_credentialless': 'Tworzenie konta gościa',
  'stage_session_scopes': 'Sprawdzanie autoryzacji gościa',
  'stage_protected_user': 'Sprawdzanie konta',
  'stage_vpn_permissions': 'Sprawdzanie dostępu VPN',
  'stage_session_refresh': 'Odnawianie sesji',
  'stage_srp_info': 'Rozpoczynanie logowania',
  'stage_srp_proof': 'Weryfikacja logowania',
  'stage_totp': 'Weryfikacja kodu uwierzytelniającego',
  'stage_directory': 'Wczytywanie serwerów',
  'stage_certificate': 'Żądanie autoryzacji VPN',
  'stage_secure_storage': 'Zapisywanie konfiguracji',
  'stage_session_revoke': 'Unieważnianie sesji',
  'expired':
      'Żądanie weryfikacji lub wprowadzenia danych wygasło. Rozpocznij konfigurację ponownie.',
  'underlay_changed':
      'Środowisko sieciowe się zmieniło. Rozpocznij konfigurację ponownie.',
  'remote_unknown': 'Nie można potwierdzić unieważnienia zdalnej sesji.',
  'cancelled_remote_unknown':
      'Anulowano na tym urządzeniu. Nie można potwierdzić unieważnienia zdalnej sesji.',
  'privacy':
      'Hasło służy tylko do logowania. Sesje i klucze są chronione na tym urządzeniu.',
  'empty': 'Zaloguj się lub kontynuuj jako gość, aby dodać konto Proton.',
};
