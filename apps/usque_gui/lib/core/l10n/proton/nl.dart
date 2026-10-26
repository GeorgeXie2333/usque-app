const kProtonNl = <String, String>{
  'fallback_account':
      'Proton is niet bereikbaar. Selecteer een bestaand WARP-account en probeer het opnieuw.',
  'fallback_failed':
      'Verbinding via WARP mislukt. Controleer het geselecteerde WARP-account en het netwerk en probeer opnieuw.',
  'fallback_starting': 'Verbinden via WARP…',
  'fallback_active':
      'Deze accountbewerking verloopt via WARP, inclusief verificatie.',
  'connection_api':
      'Een Proton-API-aanvraag is mislukt. Probeer opnieuw of controleer het Proton-account bij Ketenproxy.',
  'connection_authentication':
      'Proton-autorisatie mislukt. Open Ketenproxy en meld u opnieuw aan bij Proton.',
  'connection_certificate':
      'Proton heeft het verbindingscertificaat geweigerd en het herstel is mislukt. Maak opnieuw verbinding of meld u opnieuw aan bij Proton.',
  'connection_restricted':
      'De Proton-server is tijdelijk beperkt. Probeer opnieuw of kies een andere server.',
  'connection_sessions':
      'De limiet voor gelijktijdige Proton-verbindingen is bereikt. Verbreek een andere Proton-sessie en probeer opnieuw.',
  'connection_key':
      'Proton heeft deze verbindingssleutel in meerdere sessies gevonden. Verbreek de andere sessie en probeer opnieuw.',
  'connection_policy':
      'Proton heeft deze verbinding beperkt op grond van het account- of gebruiksbeleid. Controleer het account of kies een andere server.',
  'connection_server':
      'De Proton-server kon de verbinding niet autoriseren. Probeer een andere server.',
  'connection_protocol':
      'Het verbindingsantwoord van Proton kon niet worden gevalideerd. Probeer opnieuw of kies een andere server.',
  'connection_transport':
      'Proton is niet bereikbaar. Controleer uw netwerk of kies een andere server.',
  'connection_reconnect': 'Proton-autorisatie vernieuwd. Opnieuw verbinden…',
  'connection_review':
      'Open Ketenproxy om het Proton-account en de geselecteerde server te controleren.',
  'connection_retry':
      'Probeer de Proton-verbinding opnieuw of kies een andere server.',
  'title': 'Proton VPN',
  'guest': 'Doorgaan als gast',
  'login': 'Aanmelden bij Proton',
  'username': 'Gebruikersnaam',
  'password': 'Wachtwoord',
  'totp': 'Code uit de authenticator-app',
  'submit': 'Doorgaan',
  'verify': 'Verificatie voltooien',
  'verification_hint': 'Los de puzzel op in het officiële verificatievenster.',
  'choose_node': 'Een gratis server kiezen',
  'working': 'Proton VPN voorbereiden…',
  'waiting_totp': 'Voer de code uit uw authenticator-app in om door te gaan.',
  'waiting_verification': 'Verificatie vereist',
  'waiting_node': 'Kies een server om de configuratie te voltooien.',
  'completed':
      'Account opgeslagen. Selecteer het en pas de wijzigingen toe om Proton VPN te gebruiken.',
  'cancelled': 'Configuratie geannuleerd',
  'failed':
      'Proton-configuratie mislukt. Probeer het opnieuw wanneer u klaar bent.',
  'result_unconfirmed':
      'Het resultaat van de Proton-configuratie is niet bevestigd. Vernieuw de status om door te gaan of annuleer de huidige configuratie voordat u opnieuw begint.',
  'unavailable': 'Proton VPN is niet beschikbaar in deze engine.',
  'bad_credentials':
      'Controleer uw Proton-gebruikersnaam, wachtwoord of authenticatiecode.',
  'rate_limited':
      'Proton heeft deze aanvraag beperkt. Wacht voordat u opnieuw probeert.',
  'network':
      'Proton is niet bereikbaar. Controleer uw netwerk of VPN en probeer opnieuw.',
  'verification_failed':
      'De verificatie kon niet worden voltooid. Start de configuratie opnieuw.',
  'verification_invalid':
      'Proton heeft het verificatieresultaat geweigerd. Start de configuratie opnieuw voor een nieuwe verificatieopdracht.',
  'accounts': 'Proton-accounts',
  'add_account': 'Account toevoegen',
  'account_actions': 'Accountacties',
  'resolve_on_connect':
      'De server en autorisatie worden bij het verbinden gecontroleerd.',
  'account': 'Proton-account',
  'signed_in': 'Aangemeld',
  'session_requires_login': 'Aanmelden vereist voor sessie',
  'selected_server': 'Server',
  'automatic_server': 'Automatisch',
  'automatic_hint': 'Kies bij het verbinden de server met de laagste latentie.',
  'available_servers': 'Beschikbare servers',
  'choose_server': 'Een server kiezen',
  'search_servers': 'Zoeken op naam, land of stad',
  'load_servers': 'Serverlijst laden',
  'change_server': 'Server wijzigen',
  'refresh_servers': 'Lijst vernieuwen',
  'remove_account': 'Account verwijderen',
  'remove_account_confirm':
      'Dit account en de opgeslagen serverkeuze verwijderen? Afmelden trekt ook de opgeslagen sessies van deze app in. Alleen lokaal verwijderen laat externe sessies ongewijzigd. De opgeslagen keuze voor de ketenproxy wordt gewist.',
  'forget_local': 'Alleen lokaal verwijderen',
  'sign_out_remove': 'Afmelden en verwijderen',
  'current_account': 'Verbonden account',
  'saved_account': 'Opgeslagen keuze',
  'next_connection': 'Serverkeuze opgeslagen voor de volgende verbinding.',
  'disconnect_to_change':
      'Verbreek de verbinding van dit account voordat u de server wijzigt of het account verwijdert.',
  'cleanup_pending':
      'Het intrekken van een eerdere sessie kon niet worden bevestigd. Meld u opnieuw af of kies uitdrukkelijk voor alleen lokaal verwijderen.',
  'selected_server_unavailable':
      'De geselecteerde server is niet beschikbaar. Kies een andere server of Automatisch.',
  'close': 'Sluiten',
  'verification_error': 'Verificatiefout',
  'refresh': 'Autorisatie vernieuwen of server wijzigen',
  'logout': 'Afmelden en verwijderen',
  'forget': 'Alleen van dit apparaat verwijderen',
  'forget_confirm':
      'Deze Proton-configuratie en opgeslagen sessie van dit apparaat verwijderen? De externe Proton-sessie wordt hierdoor niet ingetrokken.',
  'removed': 'Het Proton-account is van dit apparaat verwijderd.',
  'needs_login': 'Opnieuw aanmelden',
  'expires': 'Autorisatie verloopt',
  'unsupported_second_factor':
      'Dit account vereist een aanmeldmethode die hier niet wordt ondersteund.',
  'reauthentication_required':
      'Meld u opnieuw aan om deze Proton-sessie te vernieuwen.',
  'profile_in_use':
      'Verbreek de verbinding van dit account voordat u het wijzigt of verwijdert.',
  'session_busy':
      'Er loopt nog een andere Proton-bewerking. Wacht of annuleer deze eerst.',
  'api_rejected':
      'Proton heeft deze aanvraag geweigerd. De onderstaande codes geven het serverantwoord aan.',
  'session_not_allowed':
      'Proton staat deze client niet toe een sessie te maken.',
  'credentialless_disabled':
      'Proton-gasttoegang is momenteel niet beschikbaar.',
  'no_free_nodes':
      'Proton heeft geen beschikbare WireGuard-servers voor dit account teruggestuurd.',
  'invalid_directory':
      'Proton heeft een niet-ondersteunde serverlijst teruggestuurd.',
  'invalid_client_configuration':
      'Proton heeft geen bruikbare WireGuard-poorten teruggestuurd.',
  'stage': 'Mislukte stap',
  'stage_anonymous_session': 'Anonieme sessie maken',
  'stage_credentialless': 'Gastaccount maken',
  'stage_session_scopes': 'Gastautorisatie controleren',
  'stage_protected_user': 'Account controleren',
  'stage_vpn_permissions': 'VPN-toegang controleren',
  'stage_session_refresh': 'Sessie vernieuwen',
  'stage_srp_info': 'Aanmelden starten',
  'stage_srp_proof': 'Aanmelding verifiëren',
  'stage_totp': 'Authenticatiecode verifiëren',
  'stage_directory': 'Servers laden',
  'stage_certificate': 'VPN-autorisatie aanvragen',
  'stage_secure_storage': 'Configuratie opslaan',
  'stage_session_revoke': 'Sessie intrekken',
  'expired':
      'Het verificatie- of invoerverzoek is verlopen. Start de configuratie opnieuw.',
  'underlay_changed':
      'De netwerkomgeving is gewijzigd. Start de configuratie opnieuw.',
  'remote_unknown':
      'Het intrekken van de externe sessie kon niet worden bevestigd.',
  'cancelled_remote_unknown':
      'Geannuleerd op dit apparaat. Het intrekken van de externe sessie kon niet worden bevestigd.',
  'privacy':
      'Uw wachtwoord wordt alleen voor het aanmelden gebruikt. Sessies en sleutels zijn op dit apparaat beveiligd.',
  'empty':
      'Meld u aan of ga door als gast om een Proton-account toe te voegen.',
};
