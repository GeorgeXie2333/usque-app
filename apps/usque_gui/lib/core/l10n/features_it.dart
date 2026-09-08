/// Supplemental feature strings for Italian.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowIt = <String, String>{
  'cc_label': 'Controllo di congestione HTTP/3',
  'cc_help': 'Si applica alla prossima connessione manuale.',
  'cc_upgrade': 'Aggiornamento dell’Engine necessario.',
  'cc_h2': 'HTTP/2 usa il TCP di sistema.',
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
  'home_traffic_idle': 'Inizia dopo la connessione',
  'home_traffic_waiting': 'In attesa di campioni',
  'home_traffic_unavailable': 'Cronologia non disponibile',
  'home_traffic_stale': 'Campioni in ritardo',
  'home_outputs_next': 'Le uscite verranno abilitate dopo la connessione',
  'home_outputs_retry': 'Uscite configurate per il tentativo successivo',
  'connection_protection_group': 'Connessione e protezione',
  'proxy_routing_group': 'Proxy e instradamento',
  'application_group': 'Applicazione',
  'proxy_settings_link': 'Indirizzi dei listener, porte, autenticazione e DNS.',
  'proxy_auth_separate':
      'Le credenziali vengono salvate separatamente con Salva credenziali.',
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
      'È selezionato il DNS di sistema fisico; i controlli del DNS '
      'crittografato non si applicano.',
  'nq_finding_unsupported':
      'Il DNS crittografato non è disponibile in questo Engine; non è '
      'consentito un fallback in chiaro.',
  'nq_finding_dns_custom_valid':
      'La configurazione DNS crittografato personalizzata è valida. Il '
      'fallback in chiaro è disattivato.',
  'nq_finding_stale':
      'La lettura non è aggiornata oppure la rete fisica è cambiata.',
  'nq_finding_rtt_high': 'Il tempo di andata e ritorno misurato è elevato.',
  'nq_finding_healthy':
      'La misurazione locale disponibile rientra nell’intervallo previsto.',
  'nq_finding_loss_high': 'La perdita di pacchetti dell’intervallo è elevata.',
  'nq_finding_queue_pressure':
      'Una coda è sotto pressione o ha registrato scarti durante questa '
      'connessione.',
  'nq_finding_pmtu_degraded':
      'La convalida della MTU del percorso è degradata.',
  'nq_finding_migration_reconnect':
      'La migrazione non è disponibile su questo percorso; un cambio di rete '
      'usa una riconnessione completa.',
  'nq_finding_dns_changed':
      'La modalità DNS salvata differisce dalla connessione in esecuzione.',
  'nq_finding_dns_runtime':
      'Il DNS crittografato è riuscito. Lo stato locale non è una prova '
      'esterna di assenza di fughe.',
  'nq_finding_dns_degraded':
      'Il DNS crittografato è degradato; le query dirette non riuscite non '
      'tornano al DNS di sistema.',
  'nq_finding_probe_unsafe':
      'Sonda saltata: lo stato sicuro richiesto o l’identità salvata non è '
      'disponibile. Un tunnel attivo non viene mai duplicato.',
  'nq_finding_probe_success':
      'La sonda autenticata è completata. Questo non è un test esterno di fuga '
      'di pacchetti.',
  'nq_finding_probe_cancelled': 'Sonda annullata e pulizia richiesta.',
  'nq_finding_probe_timeout':
      'La sonda a tempo non è terminata prima della scadenza.',
  'nq_finding_probe_failed':
      'La sonda autenticata non è riuscita; non è stato tentato un fallback '
      'insicuro.',
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
  'nq_doctor': 'Esegui il dottore di rete',
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
      'Questo Engine non fornisce la qualità di rete. I comandi di connessione '
      'esistenti continuano a funzionare.',
  'nq_empty':
      'Connettersi per vedere le misurazioni. I valori sconosciuti non vengono '
      'mostrati come zero.',
  'nq_stale_help':
      'L’origine ha smesso di aggiornarsi. Queste sono letture precedenti; i '
      'vuoti restano vuoti.',
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
      'L’individuazione del percorso non aumenta la MTU TUN del dispositivo.',
  'nq_migration': 'Migrazione di rete',
  'nq_migration_help':
      'Una connessione, un percorso dati. Solo la stessa famiglia IP; non è a '
      'percorsi multipli.',
  'nq_attempts': 'Tentativi',
  'nq_successes': 'Riusciti',
  'nq_failures': 'Non riusciti',
  'nq_last_duration': 'Ultima durata',
  'nq_direct_dns': 'DNS diretto',
  'nq_system_dns': 'DNS di sistema fisico',
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
      'La famiglia IP attuale non è disponibile; viene usata una riconnessione '
      'completa.',
  'nq_reason_socket_protect_failed':
      'Impossibile preparare un socket candidato protetto.',
  'nq_reason_generation_changed_during_setup':
      'La rete è cambiata di nuovo durante la preparazione.',
  'nq_reason_peer_cid_unavailable':
      'Il peer non ha un identificatore di connessione di riserva.',
  'nq_reason_local_cid_unavailable':
      'Un identificatore di connessione locale non è disponibile.',
  'nq_reason_path_probe_rejected':
      'Impossibile convalidare il percorso candidato.',
  'nq_reason_path_validation_timeout':
      'Convalida del percorso scaduta; la riconnessione è disponibile.',
  'nq_reason_superseded':
      'Un cambiamento di rete più recente ha sostituito questo tentativo.',
  'nq_reason_promotion_failed':
      'Impossibile completare il cambio di percorso in modo sicuro.',
  'nq_reason_connection_closed':
      'La connessione si è chiusa durante la migrazione.',
  'nq_reason_unsupported':
      'La migrazione non è disponibile su questa connessione.',
  'nq_reason_unknown': 'Nessun motivo supportato è disponibile.',
  'nq_dns_custom': 'Resolver crittografato personalizzato',
  'nq_dns_server': 'Nome server TLS',
  'nq_dns_path': 'Percorso HTTPS',
  'nq_dns_port': 'Porta (0 usa il valore predefinito)',
  'nq_dns_bootstrap': 'Indirizzi IP di bootstrap',
  'nq_dns_bootstrap_help':
      'Immettere 1–8 indirizzi IP numerici, uno per riga. Non viene usata la '
      'risoluzione dei nomi host.',
  'nq_dns_no_fallback':
      'Se il DNS diretto crittografato non riesce, la query fallisce. Non c’è '
      'mai un fallback al DNS di sistema o in chiaro.',
  'nq_dns_system_privacy':
      'Il DNS di sistema fisico può esporre i nomi delle query dirette al '
      'provider DNS della rete fisica.',
  'nq_dns_scope':
      'Usato solo per le query dirette selezionate da Geo. Il DNS del tunnel '
      'resta invariato.',
  'nq_dns_no_capability':
      'Questo Engine non può usare il DNS diretto crittografato. Le '
      'impostazioni salvate sono conservate. È possibile scegliere '
      'esplicitamente il DNS di sistema.',
  'nq_dns_invalid_name':
      'Immettere un nome DNS senza spazi, sintassi URL o caratteri jolly.',
  'nq_dns_invalid_path':
      'Usare un percorso che inizia con /, di al massimo 256 caratteri, senza '
      'query, frammento o spazi.',
  'nq_dns_invalid_bootstrap':
      'Usare 1–8 IP unicast univoci; niente indirizzi non specificati, '
      'multicast, broadcast o IPv6 link-local.',
  'nq_dns_invalid_port': 'Immettere 0–65535.',
  'nq_dns_invalid_mode': 'Scegliere una modalità DNS supportata.',
  'nq_doctor_deep_title': 'Eseguire i controlli di rete approfonditi?',
  'nq_doctor_deep_body':
      'I controlli approfonditi possono inviare una query DNS di prova al '
      'resolver configurato e convalidare un percorso QUIC protetto. Durano al '
      'massimo 15 secondi, possono essere annullati, non creano mai un secondo '
      'tunnel con dati e non modificano mai DNS, rotte, profilo o trasporto.',
  'nq_doctor_deep_run': 'Esegui i controlli approfonditi',
  'nq_doctor_evidence':
      'I controlli locali descrivono la configurazione e lo stato osservato. '
      'Non sono una prova esterna dell’assenza di fughe DNS.',
};

const Map<String, String> kWindowsRecoveryIt = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'Impossibile ripristinare completamente lo stato di rete VPN precedente. '
      'Non è stata avviata una nuova connessione VPN. Riprovare la connessione '
      'o ispezionare la diagnostica locale.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows non è riuscito a ripristinare lo stato di rete VPN precedente '
      'dopo tre tentativi automatici. Riprovare quando si è pronti, oppure '
      'ispezionare la diagnostica locale.',
  'WINDOWS_RECOVERY_BLOCKED':
      'La riparazione automatica si è interrotta perché lo stato di rete '
      'Windows precedente non poteva essere verificato in modo sicuro. '
      'Riavviare l’Agent o aggiornare Usque, quindi ispezionare la diagnostica '
      'locale.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Il ripristino della rete Windows sta richiedendo più tempo del '
      'previsto. Non è stata avviata una nuova connessione VPN. Attendere il '
      'completamento del ripristino prima di riprovare.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Lo stato di rete è cambiato o è ancora in uso da un’altra sessione. Il '
      'ripristino automatico è stato interrotto per proteggere la connessione '
      'attiva.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Questo Agent Windows non supporta il ripristino automatico sicuro. '
      'Aggiornare l’applicazione e l’Agent insieme, quindi riprovare.',
};

const String kWindowsAdapterCleanupIt =
    'Impossibile rimuovere l’adapter Wintun precedente o verificarne la '
    'rimozione. Non è stata avviata una nuova connessione VPN.';
