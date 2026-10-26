const kProtonIt = <String, String>{
  'fallback_account':
      'Impossibile raggiungere Proton. Seleziona un account WARP esistente e riprova.',
  'fallback_failed':
      'Impossibile connettersi tramite WARP. Controlla l’account WARP selezionato e la rete, poi riprova.',
  'fallback_starting': 'Connessione tramite WARP…',
  'fallback_active':
      'Questa operazione sull’account usa WARP, inclusa la verifica.',
  'connection_api':
      'Una richiesta all’API Proton non è riuscita. Riprova o controlla l’account Proton in Proxy a catena.',
  'connection_authentication':
      'Autorizzazione Proton non riuscita. Apri Proxy a catena e accedi di nuovo a Proton.',
  'connection_certificate':
      'Proton ha rifiutato il certificato di connessione e il ripristino non è riuscito. Riconnettiti o accedi di nuovo a Proton.',
  'connection_restricted':
      'Il server Proton è temporaneamente soggetto a restrizioni. Riprova o scegli un altro server.',
  'connection_sessions':
      'È stato raggiunto il limite di connessioni simultanee di Proton. Disconnetti un’altra sessione Proton e riprova.',
  'connection_key':
      'Proton ha rilevato questa chiave di connessione in più sessioni. Disconnetti l’altra sessione e riprova.',
  'connection_policy':
      'Proton ha limitato questa connessione in base alle proprie regole sull’account o sull’utilizzo. Controlla l’account o scegli un altro server.',
  'connection_server':
      'Il server Proton non ha potuto autorizzare la connessione. Prova un altro server.',
  'connection_protocol':
      'Impossibile convalidare la risposta di connessione di Proton. Riprova o scegli un altro server.',
  'connection_transport':
      'Impossibile raggiungere Proton. Controlla la rete o scegli un altro server.',
  'connection_reconnect': 'Autorizzazione Proton rinnovata. Riconnessione…',
  'connection_review':
      'Apri Proxy a catena per controllare l’account Proton e il server selezionato.',
  'connection_retry': 'Riprova la connessione Proton o scegli un altro server.',
  'title': 'Proton VPN',
  'guest': 'Continua come ospite',
  'login': 'Accedi a Proton',
  'username': 'Nome utente',
  'password': 'Password',
  'totp': 'Codice dell’app di autenticazione',
  'submit': 'Continua',
  'verify': 'Completa la verifica',
  'verification_hint':
      'Risolvi il puzzle nella finestra di verifica ufficiale.',
  'choose_node': 'Scegli un server gratuito',
  'working': 'Preparazione di Proton VPN…',
  'waiting_totp':
      'Inserisci il codice dell’app di autenticazione per continuare.',
  'waiting_verification': 'Verifica richiesta',
  'waiting_node': 'Scegli un server per completare la configurazione.',
  'completed':
      'Account salvato. Selezionalo e applica le modifiche per usare Proton VPN.',
  'cancelled': 'Configurazione annullata',
  'failed': 'Configurazione Proton non riuscita. Riprova quando vuoi.',
  'result_unconfirmed':
      'Il risultato della configurazione Proton non è confermato. Aggiorna lo stato per continuare o annulla la configurazione corrente prima di ricominciare.',
  'unavailable': 'Proton VPN non è disponibile in questo motore.',
  'bad_credentials':
      'Controlla il nome utente Proton, la password o il codice di autenticazione.',
  'rate_limited':
      'Proton ha limitato questa richiesta. Attendi prima di riprovare.',
  'network':
      'Impossibile raggiungere Proton. Controlla la rete o la VPN, poi riprova.',
  'verification_failed':
      'Impossibile completare la verifica. Ricomincia la configurazione.',
  'verification_invalid':
      'Proton ha rifiutato il risultato della verifica. Ricomincia la configurazione per ricevere una nuova verifica.',
  'accounts': 'Account Proton',
  'add_account': 'Aggiungi account',
  'account_actions': 'Azioni dell’account',
  'resolve_on_connect':
      'Il server e l’autorizzazione vengono controllati alla connessione.',
  'account': 'Account Proton',
  'signed_in': 'Accesso effettuato',
  'session_requires_login': 'La sessione richiede l’accesso',
  'selected_server': 'Server',
  'automatic_server': 'Automatico',
  'automatic_hint': 'Scegli il server con la latenza minima alla connessione.',
  'available_servers': 'Server disponibili',
  'choose_server': 'Scegli un server',
  'search_servers': 'Cerca per nome, paese o città',
  'load_servers': 'Carica elenco server',
  'change_server': 'Cambia server',
  'refresh_servers': 'Aggiorna elenco',
  'remove_account': 'Rimuovi account',
  'remove_account_confirm':
      'Rimuovere questo account e la scelta del server salvata? Uscendo dall’account vengono revocate anche le sessioni salvate da questa app. La rimozione locale lascia invariate le sessioni remote. La selezione salvata del proxy a catena verrà cancellata.',
  'forget_local': 'Rimuovi solo localmente',
  'sign_out_remove': 'Esci e rimuovi',
  'current_account': 'Account connesso',
  'saved_account': 'Selezione salvata',
  'next_connection': 'Scelta del server salvata per la prossima connessione.',
  'disconnect_to_change':
      'Disconnetti questo account prima di cambiare server o rimuoverlo.',
  'cleanup_pending':
      'Impossibile confermare la revoca di una sessione precedente. Esci di nuovo dall’account o scegli esplicitamente di rimuoverlo solo localmente.',
  'selected_server_unavailable':
      'Il server selezionato non è disponibile. Scegli un altro server o Automatico.',
  'close': 'Chiudi',
  'verification_error': 'Errore di verifica',
  'refresh': 'Rinnova autorizzazione o cambia server',
  'logout': 'Esci ed elimina',
  'forget': 'Rimuovi solo da questo dispositivo',
  'forget_confirm':
      'Rimuovere questa configurazione Proton e la sessione salvata da questo dispositivo? La sessione Proton remota non verrà revocata.',
  'removed': 'L’account Proton è stato rimosso da questo dispositivo.',
  'needs_login': 'Accedi di nuovo',
  'expires': 'Scadenza dell’autorizzazione',
  'unsupported_second_factor':
      'Questo account richiede un metodo di accesso non supportato qui.',
  'reauthentication_required':
      'Accedi di nuovo per rinnovare questa sessione Proton.',
  'profile_in_use':
      'Disconnetti questo account prima di modificarlo o rimuoverlo.',
  'session_busy':
      'Un’altra operazione Proton è ancora in corso. Attendi o annullala prima.',
  'api_rejected':
      'Proton ha rifiutato questa richiesta. I codici seguenti identificano la risposta del server.',
  'session_not_allowed':
      'Proton non consente a questo client di creare una sessione.',
  'credentialless_disabled':
      'L’accesso ospite a Proton non è attualmente disponibile.',
  'no_free_nodes':
      'Proton non ha restituito server WireGuard disponibili per questo account.',
  'invalid_directory':
      'Proton ha restituito un elenco di server non supportato.',
  'invalid_client_configuration':
      'Proton non ha restituito porte WireGuard utilizzabili.',
  'stage': 'Passaggio non riuscito',
  'stage_anonymous_session': 'Crea una sessione anonima',
  'stage_credentialless': 'Crea un account ospite',
  'stage_session_scopes': 'Controlla l’autorizzazione dell’ospite',
  'stage_protected_user': 'Controlla l’account',
  'stage_vpn_permissions': 'Controlla l’accesso VPN',
  'stage_session_refresh': 'Rinnova la sessione',
  'stage_srp_info': 'Avvia l’accesso',
  'stage_srp_proof': 'Verifica l’accesso',
  'stage_totp': 'Verifica il codice di autenticazione',
  'stage_directory': 'Carica i server',
  'stage_certificate': 'Richiedi l’autorizzazione VPN',
  'stage_secure_storage': 'Salva la configurazione',
  'stage_session_revoke': 'Revoca la sessione',
  'expired':
      'La richiesta di verifica o inserimento è scaduta. Ricomincia la configurazione.',
  'underlay_changed':
      'Il contesto di rete è cambiato. Ricomincia la configurazione.',
  'remote_unknown': 'Impossibile confermare la revoca della sessione remota.',
  'cancelled_remote_unknown':
      'Annullato su questo dispositivo. Impossibile confermare la revoca della sessione remota.',
  'privacy':
      'La password serve solo per l’accesso. Sessioni e chiavi sono protette su questo dispositivo.',
  'empty': 'Accedi o continua come ospite per aggiungere un account Proton.',
};
