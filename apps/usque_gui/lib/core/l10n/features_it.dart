/// Supplemental feature strings for Italian.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowIt = <String, String>{
  'local_proxy_settings': 'Impostazioni proxy locale',
  'proxy_switches_hint':
      'Le modifiche agli interruttori si salvano automaticamente. Conferma le modifiche di ascolto e DNS con Applica modifiche.',
  'cc_label': 'Controllo di congestione HTTP/3',
  'cc_help': 'Si applica alla prossima connessione manuale.',
  'cc_upgrade': 'Aggiorna Usque in Impostazioni per usare questa opzione.',
  'cc_h2': 'Questa opzione riguarda solo le connessioni HTTP/3.',
  'cc_saved': 'Salvato',
  'cc_pending': 'In attesa della prossima connessione manuale.',
  'save_changes': 'Applica le modifiche',
  'saving_changes': 'Applicazione delle modifiche…',
  'unsaved_changes': 'Modifiche non applicate',
  'changes_applied': 'Modifiche applicate',
  'changes_apply_hint':
      'Le modifiche hanno effetto solo dopo averle applicate.',
  'changes_failed':
      'Impossibile applicare le modifiche. Controllare i valori salvati e '
      'riprovare.',
  'form_errors':
      'Controllare i campi evidenziati prima di applicare le modifiche.',
  'discard_changes_title': 'Ignorare le modifiche non applicate?',
  'discard_changes_body':
      'Le modifiche non sono state applicate. Continuare a modificare per '
      'salvarle, oppure ignorarle per uscire.',
  'keep_editing': 'Continua a modificare',
  'discard_changes': 'Ignora le modifiche',
  'invalid_port': 'Immettere una porta da 1 a 65535.',
  'listener_exposure':
      'Gli indirizzi dei listener consentono l’accesso dalla rete locale',
  'invalid_ipv4': 'Immettere un indirizzo IPv4 valido, ad esempio 127.0.0.1.',
  'invalid_ipv6': 'Immettere un indirizzo IPv6 valido, ad esempio ::1.',
  'output_running': 'In esecuzione',
  'output_waiting': 'Abilitato · non in esecuzione',
  'output_disabled': 'Disattivato',
  'output_starting': 'Avvio',
  'output_stopping': 'Arresto',
  'output_reconnecting': 'Riconnessione',
  'output_degraded': 'Limitato',
  'output_error': 'Errore',
  'output_unknown': 'Stato non disponibile',
  'shared_network_scope':
      'Le impostazioni di rete sono condivise da tutti gli account.',
  'connection_details': 'Dettagli della connessione',
  'home_overview': 'Panoramica della connessione',
  'home_exit_region': 'Regione di uscita',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Traffico',
  'home_traffic_window': 'Ultimi 60 secondi',
  'home_traffic_idle': 'Il traffico appare dopo la connessione',
  'home_traffic_waiting': 'In attesa dei dati di traffico',
  'home_traffic_unavailable': 'Cronologia del traffico non disponibile',
  'home_traffic_stale': 'Aggiornamenti del traffico in ritardo',
  'home_outputs_next': 'Disponibile dopo la connessione',
  'home_outputs_retry': 'VPN e proxy della prossima connessione',
  'connection_protection_group': 'Connessione e protezione',
  'proxy_routing_group': 'Proxy e instradamento',
  'application_group': 'Applicazione',
  'proxy_settings_link': 'Indirizzi dei listener, porte, autenticazione e DNS.',
  'proxy_auth_separate':
      'Usa Salva nome utente e password qui sotto per applicare queste modifiche.',
  'reset_draft_hint':
      'I valori predefiniti verranno caricati in questo modulo. Applicare le '
      'modifiche perché abbiano effetto.',
};

const Map<String, String> kNetworkQualityIt = <String, String>{
  'nq_range': 'Intervallo',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Tempo di andata e ritorno',
  'diag_check_quality_packet_loss': 'Perdita di pacchetti',
  'diag_check_quality_queue_pressure': 'Pressione delle code',
  'diag_check_quality_pmtu': 'MTU del percorso',
  'diag_check_transport_migration_capability':
      'Migrazione nella stessa famiglia',
  'diag_check_dns_direct_encrypted_configuration': 'Configurazione DNS diretto',
  'diag_check_dns_direct_encrypted_runtime_state': 'Esecuzione del DNS diretto',
  'diag_check_dns_direct_encrypted_reachability':
      'Raggiungibilità del DNS crittografato',
  'diag_check_transport_h3_path_validation_probe': 'Handshake QUIC isolato',
  'nq_finding_unavailable':
      'Questa misurazione non è disponibile nello stato attuale.',
  'nq_finding_invalid_configuration':
      'La configurazione DNS personalizzata non è valida.',
  'nq_finding_dns_system':
      'Si usa il DNS della rete attuale; i controlli del DNS cifrato non si applicano.',
  'nq_finding_unsupported':
      'Aggiorna Usque per usare DNS cifrato. Le richieste non passeranno a DNS non cifrato.',
  'nq_finding_dns_custom_valid':
      'La configurazione DNS crittografato personalizzata è valida. Il '
      'fallback in chiaro è disattivato.',
  'nq_finding_stale':
      'La lettura non è aggiornata oppure la rete fisica è cambiata.',
  'nq_finding_rtt_high': 'Il tempo di andata e ritorno misurato è elevato.',
  'nq_finding_healthy':
      'Le misurazioni disponibili della connessione sono nella norma.',
  'nq_finding_loss_high': 'La perdita di pacchetti dell’intervallo è elevata.',
  'nq_finding_queue_pressure':
      'C’è traffico in attesa di invio oppure dati scartati durante questa connessione.',
  'nq_finding_pmtu_degraded':
      'Usque non ha potuto confermare una dimensione di pacchetto adatta alla connessione.',
  'nq_finding_migration_reconnect':
      'Questa connessione deve essere ristabilita quando cambi rete.',
  'nq_finding_dns_changed':
      'La modalità DNS salvata differisce dalla connessione in esecuzione.',
  'nq_finding_dns_runtime': 'Il DNS crittografato funziona.',
  'nq_finding_dns_degraded':
      'Il DNS cifrato ha problemi. Le richieste non riuscite non useranno il DNS non cifrato della rete.',
  'nq_finding_probe_unsafe':
      'Questa misurazione non è disponibile nello stato attuale.',
  'nq_finding_probe_success': 'Questo controllo ha avuto esito positivo.',
  'nq_finding_probe_cancelled': 'Questo controllo è stato annullato.',
  'nq_finding_probe_timeout': 'Timeout del controllo diagnostico',
  'nq_finding_probe_failed': 'Questo controllo non è riuscito.',
  'diag_fix_nq_profile':
      'Esamina i campi DNS personalizzati e il nome del certificato. Non '
      'disattivare la verifica TLS.',
  'diag_fix_nq_retry': 'Attendi una rete stabile, poi riprova.',
  'diag_fix_nq_network':
      'Controlla la connettività locale e confronta un nuovo campione prima di '
      'modificare le impostazioni.',
  'diag_fix_nq_reconnect':
      'Riconnettersi per applicare la configurazione salvata.',
  'nav_network_quality': 'Qualità',
  'network_quality': 'Qualità di rete',
  'nq_subtitle': 'Osserva la connessione, non solo la velocità.',
  'nq_local_only': 'Solo misurazioni locali. Niente viene caricato.',
  'nq_doctor': 'Esegui Controllo di rete',
  'nq_doctor_help':
      'I controlli standard leggono solo lo stato locale. Non aprono '
      'connessioni esterne né modificano le impostazioni.',
  'nq_live': 'In diretta',
  'nq_stale': 'Letture non aggiornate',
  'nq_updated': 'Ultimo campione',
  'nq_seconds': '{count} s fa',
  'nq_good': 'Buona',
  'nq_fair': 'Discreta',
  'nq_poor': 'Scarsa',
  'nq_limited': 'Dati limitati',
  'nq_disconnected': 'Disconnesso',
  'nq_connecting': 'Connessione',
  'nq_connected': 'Connesso',
  'nq_unavailable': 'Non disponibile',
  'nq_not_ready': 'Non pronto',
  'nq_unsupported': 'Non supportato',
  'nq_capability_missing':
      'Questa versione non mostra la qualità della connessione. Puoi ancora connetterti e disconnetterti. Cerca aggiornamenti in Impostazioni.',
  'nq_empty': 'Connettiti per vedere le misurazioni.',
  'nq_stale_help':
      'Gli aggiornamenti sono sospesi. Sono mostrate le ultime letture.',
  'nq_rtt': 'Tempo di andata e ritorno',
  'nq_latest': 'Più recente',
  'nq_smoothed': 'Smussato',
  'nq_minimum': 'Minimo',
  'nq_h2_ping': 'PING del protocollo HTTP/2',
  'nq_h3_rtt': 'Misurazione del percorso QUIC',
  'nq_throughput': 'Velocità effettiva',
  'nq_download': 'Scaricamento',
  'nq_upload': 'Caricamento',
  'nq_one_second': '1 secondo',
  'nq_five_seconds': 'Media su 5 secondi',
  'nq_loss': 'Perdita di pacchetti',
  'nq_loss_h2': 'HTTP/2 non espone una perdita di pacchetti comparabile.',
  'nq_loss_interval':
      'Misurato sull’ultimo intervallo; non è la perdita complessiva.',
  'nq_congestion': 'Congestione',
  'nq_cwnd': 'Finestra di congestione',
  'nq_in_flight': 'Bytes in transito',
  'nq_send_rate': 'Velocità di consegna',
  'nq_h2_window': 'Finestre di ricezione HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Connessione',
  'nq_stalls': 'Stalli di capacità',
  'nq_pmtu': 'MTU del percorso',
  'nq_outer_pmtu': 'Limite del payload UDP esterno',
  'nq_inner_payload': 'Limite del payload CONNECT-IP',
  'nq_pmtu_help':
      'È la dimensione di pacchetto trasportabile dal percorso di rete. Usque la verifica automaticamente per ridurre le perdite. Il controllo non aumenta la MTU VPN definita in Impostazioni di rete avanzate.',
  'nq_migration': 'Migrazione di rete',
  'nq_migration_help':
      'Usque prova a mantenere la connessione quando cambi rete, per esempio dal Wi-Fi ai dati mobili. Le due reti devono usare la stessa versione IP, IPv4 o IPv6. Si usa una rete alla volta; le velocità non si sommano.',
  'nq_attempts': 'Tentativi',
  'nq_successes': 'Riusciti',
  'nq_failures': 'Non riusciti',
  'nq_last_duration': 'Ultima durata',
  'nq_direct_dns': 'DNS diretto',
  'nq_system_dns': 'DNS della rete attuale',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Pronto',
  'nq_degraded': 'Degradato',
  'nq_timeouts': 'Timeout scaduti',
  'nq_last_rtt': 'Ultimo RTT',
  'nq_dns_redacted':
      'I nomi dei resolver e gli indirizzi di bootstrap sono mostrati solo in '
      'Impostazioni.',
  'nq_queues': 'Pressione delle code',
  'nq_queue_details': 'Code di basso livello',
  'nq_queue_empty': 'Nessuna misurazione delle code per ora.',
  'nq_current_capacity': 'Attuale / capacità',
  'nq_high_water': 'Livello massimo',
  'nq_drops': 'Scarti',
  'nq_oldest': 'Voce più vecchia',
  'nq_tunToTransport': 'Dispositivo → trasporto',
  'nq_proxyToTransport': 'Proxy → trasporto',
  'nq_transportOutgoing': 'Uscita del trasporto',
  'nq_h3DatagramSend': 'Datagrammi QUIC',
  'nq_h3WireSend': 'Uscita UDP',
  'nq_transportToTun': 'Trasporto → dispositivo',
  'nq_transportToProxy': 'Trasporto → proxy',
  'nq_directDns': 'Richieste DNS dirette',
  'nq_unknown_queue': 'Altra coda',
  'nq_trends': 'Ultimi 60 secondi',
  'nq_samples': 'campioni',
  'nq_pause': 'Metti in pausa i grafici',
  'nq_resume': 'Riprendi i grafici',
  'nq_paused': 'Grafici in pausa',
  'nq_gaps': 'I campioni mancanti sono vuoti.',
  'nq_phase_idle': 'Inattivo',
  'nq_phase_preparing_socket': 'Preparazione del percorso',
  'nq_phase_probing': 'Sondaggio',
  'nq_phase_validated': 'Convalidato',
  'nq_phase_promoting': 'Cambio di percorso',
  'nq_phase_stable': 'Stabile',
  'nq_phase_aborted': 'Interrotto',
  'nq_phase_revalidating': 'Rivalidazione',
  'nq_phase_degraded': 'Degradato',
  'nq_phase_unknown': 'Non pronto',
  'nq_phase_unsupported': 'Non supportato',
  'nq_reason_family_unavailable':
      'La nuova rete non può usare la stessa versione IP. Occorre riconnettersi.',
  'nq_reason_socket_protect_failed':
      'Usque non ha potuto usare la nuova rete in sicurezza. Se la connessione non torna, riconnettiti manualmente.',
  'nq_reason_generation_changed_during_setup':
      'La rete è cambiata di nuovo durante la preparazione.',
  'nq_reason_peer_cid_unavailable':
      'Il server non ha mantenuto la connessione sulla nuova rete. Se necessario, riconnettiti manualmente.',
  'nq_reason_local_cid_unavailable':
      'Usque non ha mantenuto la connessione sulla nuova rete. Se necessario, riconnettiti manualmente.',
  'nq_reason_path_probe_rejected':
      'La nuova rete non ha superato il controllo. Verifica l’accesso a Internet.',
  'nq_reason_path_validation_timeout':
      'La nuova rete non ha risposto in tempo. Controllala e riconnettiti se necessario.',
  'nq_reason_superseded':
      'La rete è cambiata di nuovo prima del completamento del passaggio.',
  'nq_reason_promotion_failed':
      'Usque non ha completato il cambio di rete in sicurezza. Se la connessione non torna, riconnettiti manualmente.',
  'nq_reason_connection_closed':
      'La connessione si è chiusa durante il cambio di rete. Riconnettiti.',
  'nq_reason_unsupported':
      'La migrazione non è disponibile su questa connessione.',
  'nq_reason_unknown': 'Nessun motivo supportato è disponibile.',
  'nq_dns_custom': 'Resolver crittografato personalizzato',
  'nq_dns_server': 'Dominio del server DNS',
  'nq_dns_path': 'Percorso HTTPS',
  'nq_dns_port': 'Porta (0 usa il valore predefinito)',
  'nq_dns_bootstrap': 'Indirizzi IP del server DNS',
  'nq_dns_bootstrap_help':
      'Inserisci da 1 a 8 IP del fornitore DNS, uno per riga, come 1.1.1.1. Usque si collega direttamente a questi indirizzi senza cercare prima il nome del server.',
  'nq_dns_no_fallback':
      'Se il DNS diretto crittografato non riesce, la query fallisce. Non c’è '
      'mai un fallback al DNS di sistema o in chiaro.',
  'nq_dns_system_privacy':
      'Il fornitore DNS della rete attuale può vedere i domini richiesti dal traffico diretto.',
  'nq_dns_scope':
      'Usato per il traffico che corrisponde alle regole dei paesi diretti. Il DNS del traffico VPN resta invariato.',
  'nq_dns_no_capability':
      'Aggiorna Usque per usare DNS cifrato nelle connessioni dirette. Le impostazioni vengono conservate. Puoi scegliere DNS della rete attuale se accetti le conseguenze per la privacy.',
  'nq_dns_invalid_name':
      'Inserisci un dominio come dns.example.com, senza https://, porta o spazi.',
  'nq_dns_invalid_path':
      'Inserisci un percorso come /dns-query, massimo 256 caratteri. Elimina spazi e parti che iniziano con ? o #.',
  'nq_dns_invalid_bootstrap': 'Inserisci da 1 a 8 indirizzi IP del server.',
  'nq_dns_invalid_port':
      'Inserisci una porta da 1 a 65535 o 0 per il valore predefinito.',
  'nq_dns_invalid_mode': 'Scegliere una modalità DNS supportata.',
  'nq_doctor_deep_title': 'Eseguire i controlli di rete approfonditi?',
  'nq_doctor_deep_body':
      'I controlli possono inviare traffico di prova. Durano fino a 15 secondi e possono essere annullati. Le impostazioni di connessione non cambieranno.',
  'nq_doctor_deep_run': 'Esegui i controlli approfonditi',
  'nq_doctor_evidence':
      'Questi controlli non possono confermare se si verificano perdite DNS.',
};

const Map<String, String> kWindowsRecoveryIt = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'I componenti di connessione di Usque devono essere aggiornati insieme. Cerca aggiornamenti in Impostazioni. Non è stata avviata una nuova connessione VPN.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'La pulizia della connessione VPN precedente non è terminata. Esci completamente da Usque e riaprilo. Se il problema persiste, apri Diagnostica.',
  'WINDOWS_RECOVERY_FAILED':
      'Impossibile ripristinare completamente lo stato di rete VPN precedente. '
      'Non è stata avviata una nuova connessione VPN. Riprovare la connessione '
      'o ispezionare la diagnostica locale.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows non è riuscito a ripristinare lo stato di rete VPN precedente '
      'dopo tre tentativi automatici. Riprovare quando si è pronti, oppure '
      'ispezionare la diagnostica locale.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Il ripristino automatico si è fermato perché non è stato possibile confermare la sicurezza del ripristino delle precedenti impostazioni VPN. Cerca aggiornamenti in Impostazioni; se persiste, esporta un pacchetto da Diagnostica.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Il ripristino della rete Windows sta richiedendo più tempo del '
      'previsto. Non è stata avviata una nuova connessione VPN. Attendere il '
      'completamento del ripristino prima di riprovare.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Lo stato di rete è cambiato o è ancora in uso da un’altra sessione. Il '
      'ripristino automatico è stato interrotto per proteggere la connessione '
      'attiva.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Questa installazione non può ripristinare automaticamente le precedenti impostazioni VPN. Aggiorna Usque in Impostazioni prima di riprovare.',
};

const String kWindowsAdapterCleanupIt =
    'Non è stato possibile rimuovere l’adattatore di rete virtuale precedente o confermarne la rimozione. Non è stata avviata una nuova connessione VPN.';

const Map<String, String> kL4It = <String, String>{
  'l4_quic_not_ready': 'Preparazione della connessione L4',
  'l4_unsupported_packets': 'Pacchetti non supportati o non validi rifiutati',
  'l4_budget_rejections': 'Ammissioni di risorse rifiutate',
  'l4_not_applicable': 'Non applicabile (L4)',
  'l4_mode': 'L4 (sperimentale)',
  'l4_transport_hint':
      'Solo TCP. Le app che richiedono UDP potrebbero non funzionare. La modalità automatica esclude L4.',
  'l4_explanation':
      'L4 trasporta TCP tramite HTTP/3 e funziona con VPN e proxy SOCKS5 e HTTP. Le richieste DNS della VPN vengono convertite in TCP. App che richiedono altro traffico UDP, Ping remoto, frammenti IP o intestazioni di estensione potrebbero non funzionare. Scegli L4 manualmente: la modalità automatica non lo seleziona.',
  'l4_unsupported':
      'L4 non è disponibile in questa versione di Usque. Cerca aggiornamenti in Impostazioni.',
  'l4_sni_identity':
      'Impostato automaticamente dall’account. Il nome del server delle altre modalità resta invariato.',
  'l4_edge_requires_l4':
      'Il DNS risolto all’edge richiede L4. Seleziona un altro modo DNS del proxy prima di passare ad Auto, H3 o H2.',
  'proxy_dns_edge_resolved':
      'Edge Cloudflare (solo L4; nessuna ricerca locale)',
  'l4_verified': 'L4 ha stabilito una connessione di un’app',
  'l4_unverified':
      'Server connesso; connessione delle app non ancora confermata',
  'l4_status_unknown': 'Stato di connessione delle app non disponibile',
  'l4_sessions': 'Sessioni / svuotamento',
  'l4_flows': 'Flussi attivi / in attesa',
  'l4_connect': 'CONNECT successi / errori / timeout',
  'l4_buffers': 'Budget buffer applicazione usato (byte)',
  'l4_backpressure': 'Contropressione invio / ricezione',
  'l4_tun_flows': 'TUN TCP / semiaperto',
  'l4_udp': 'Pacchetti UDP rifiutati',
  'l4_dns': 'Conversioni DNS successi / errori / timeout',
  'l4_migration':
      'Flussi conservati dalla migrazione / terminati dalla ricostruzione',
  'l4_na':
      'Controllo indirizzi CONNECT-IP, code DATAGRAM, MTU del payload interno e timeout UDP: non applicabile in L4.',
};

const Map<String, String> kNetworkSettingsIt = <String, String>{
  'settings_applying': 'Salvato, applicazione in corso',
  'settings_applied': 'Salvato e applicato',
  'settings_deferred': 'Salvato; ha effetto alla prossima connessione manuale',
  'settings_failed': 'Salvato, applicazione non riuscita',
  'settings_unknown': 'Risultato non ancora confermato',
  'settings_saved': 'Salvato',
  'settings_unsupported':
      'Esci completamente da Usque, riaprilo e salva di nuovo. Se non funziona, cerca aggiornamenti in Impostazioni.',
  'settings_save_failed':
      'Impossibile salvare le impostazioni. Le modifiche sono state conservate.',
  'settings_reconnect': 'Riconnetti',
};

const Map<String, String> kChainIt = <String, String>{
  'duplicate_directive': 'Questa direttiva può comparire una sola volta.',
  'mixed_protocols':
      'Tutti gli endpoint remote devono usare lo stesso trasporto TCP o UDP.',
  'conflicting_protocol': 'Il remote è in conflitto con il trasporto globale.',
  'too_many_endpoints': 'Usa al massimo 16 endpoint remote.',
  'conflicting_authentication':
      'CLIENT_CERT è in conflitto con il certificato inline o con la modalità di autenticazione.',
  'serialized_size_limit':
      'Il record cifrato supererebbe il limite di archiviazione.',
  'multi_endpoint_unavailable':
      'Aggiorna il motore per usare configurazioni con più endpoint.',
  'candidates': 'Endpoint di avvio',
  'random_order':
      'Gli endpoint vengono provati in un nuovo ordine casuale a ogni connessione.',
  'file_order': 'Gli endpoint vengono provati nell’ordine del file.',
  'attempting': 'Endpoint in prova',
  'actual_endpoint': 'Endpoint connesso',
  'attempt_failures': 'Tentativi non riusciti',
  'failure_transport': 'trasporto chiuso',
  'failure_authentication': 'autenticazione',
  'failure_certificate': 'certificato',
  'failure_configuration': 'configurazione',
  'failure_address_changed': 'indirizzo cambiato',
  'failure_protocol': 'protocollo',
  'failure_cleanup': 'pulizia',
  'failure_reason': 'Errore: {reason}.',
  'manage': 'Gestisci',
  'dns_fallback': 'DNS del tunnel (OpenVPN può negoziare il DNS)',
  'dns_unavailable_title': 'Nessun DNS tramite questa uscita',
  'dns_unavailable':
      'Nessun server DNS è raggiungibile tramite questa uscita. Usa indirizzi IP oppure importa di nuovo una configurazione con DNS raggiungibile.',
  'authentication_failed':
      'Autenticazione non riuscita. Aggiorna le credenziali prima di riconnetterti.',
  'profile_limit': 'La raccolta di configurazioni è piena (128 profili).',
  'metadata_limit': 'I metadati della raccolta di configurazioni sono pieni.',
  'title': 'Proxy a catena',
  'subtitle': 'Scegli un’uscita raggiunta tramite WARP.',
  'source': 'Origine dell’uscita',
  'enable': 'Attiva proxy a catena',
  'import_file': 'Importa file',
  'paste': 'Incolla configurazione',
  'profiles': 'Configurazioni salvate',
  'empty': 'Importa una configurazione per scegliere un’uscita.',
  'empty_hint_openvpn':
      'Importa un file .ovpn o incolla il testo. Sono supportati endpoint TCP e UDP, certificati inline e nome utente/password.',
  'empty_hint_wireguard':
      'Importa un file .conf o incolla il testo. Sono supportate una sezione [Interface] e una [Peer].',
  'import_limits':
      'Le configurazioni devono essere testo UTF-8 fino a 128 KiB. Senza selettore di file, incolla il testo.',
  'enable_to_choose':
      'Attiva il proxy a catena per scegliere una configurazione.',
  'select_required': 'Seleziona una configurazione salvata da applicare.',
  'pending_disable': 'In attesa: disattiva il proxy a catena',
  'apply_reconnect': 'Applica e riconnetti',
  'requires_connect_ip': 'Richiede CONNECT-IP',
  'menu': 'Azioni della configurazione',
  'preview': 'Controlla configurazione',
  'save_import': 'Salva configurazione',
  'name': 'Nome',
  'configuration': 'Testo della configurazione',
  'file_loaded': 'Configurazione caricata dal file ({lines} righe).',
  'username': 'Nome utente',
  'password': 'Parola d’ordine',
  'key_password': 'Parola d’ordine della chiave privata',
  'show_password': 'Mostra la parola d’ordine',
  'hide_password': 'Nascondi la parola d’ordine',
  'credentials': 'Aggiorna credenziali',
  'rename': 'Rinomina',
  'delete': 'Elimina',
  'cancel': 'Annulla',
  'save': 'Salva',
  'apply': 'Applica modifiche',
  'clear': 'Cancella selezione',
  'current': 'Connessione attuale',
  'saved': 'Selezione salvata',
  'draft': 'Selezione in attesa',
  'disconnected': 'Non connesso',
  'disabled': 'Non attivo',
  'enabled_idle': 'Attivo · non connesso',
  'disconnecting': 'Disconnessione in corso',
  'file_read_failed': 'Impossibile leggere il file di configurazione.',
  'file_encoding_invalid': 'Il file di configurazione deve essere testo UTF-8.',
  'file_busy': 'Un selettore di file è già aperto.',
  'connected': 'Connesso',
  'connecting': 'Connessione in corso',
  'error': 'Connessione non riuscita',
  'no_selection': 'Nessuna configurazione selezionata',
  'l4': 'Questa configurazione richiede la modalità CONNECT-IP.',
  'switch_mode': 'Passa a CONNECT-IP e applica',
  'unsupported': 'Questo motore non supporta questa origine di uscita.',
  'scope':
      'Le regole dirette esplicite restano attive. Il resto del traffico usa l’uscita scelta.',
  'allowed': 'Destinazioni consentite',
  'dns': 'DNS',
  'addresses': 'Indirizzi del tunnel',
  'address_family': 'Famiglia di indirizzi',
  'transport': 'Trasporto',
  'endpoint': 'Nodo server',
  'restricted':
      'Le destinazioni fuori da AllowedIPs sono bloccate sul percorso del proxy.',
  'delete_confirm':
      'Eliminare questa configurazione salvata? Il file importato originale resta invariato.',
  'profile_in_use':
      'Scegli un’altra configurazione o cancella la selezione salvata prima di eliminare questa.',
  'stale_revision':
      'La configurazione è cambiata. Aggiorna l’elenco e riprova.',
  'secure_storage_failed':
      'Impossibile leggere o salvare la configurazione cifrata.',
  'invalid_configuration':
      'La configurazione non è valida o contiene opzioni non supportate.',
  'looks_like_wireguard':
      'Sembra una configurazione WireGuard. Imposta l’origine dell’uscita su WireGuard.',
  'looks_like_openvpn':
      'Sembra una configurazione OpenVPN. Imposta l’origine dell’uscita su OpenVPN.',
  'error_location': '{message} ({field}, riga {line})',
  'error_field': '{message} ({field})',
  'file_unavailable':
      'Nessun selettore di file disponibile. Incolla il testo della configurazione.',
  'invalid_size_or_encoding':
      'Usa una configurazione UTF-8 di al massimo 128 KiB.',
  'unsupported_directive': 'Questa direttiva OpenVPN non è supportata.',
  'unsupported_or_duplicate_field':
      'Questo campo non è supportato oppure è duplicato.',
  'unsupported_or_duplicate_section':
      'Usa una sola sezione Interface e una sola sezione Peer.',
  'missing_field': 'Manca un campo obbligatorio.',
  'invalid_name': 'Usa un nome di 1-64 caratteri senza caratteri di controllo.',
  'invalid_key': 'La chiave deve essere una chiave Base64 valida di 32 byte.',
  'checking': 'Controllo della configurazione…',
  'changed': 'Modifiche salvate',
};
