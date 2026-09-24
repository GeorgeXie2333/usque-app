/// Supplemental feature strings for Dutch.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowNl = <String, String>{
  'local_proxy_settings': 'Lokale proxyinstellingen',
  'proxy_switches_hint':
      'Schakelaars worden automatisch opgeslagen. Bevestig wijzigingen aan luisteradressen en DNS met Wijzigingen toepassen.',
  'cc_label': 'HTTP/3-congestiecontrole',
  'cc_help': 'Wordt bij de volgende handmatige verbinding toegepast.',
  'cc_upgrade': 'Werk Usque bij via Instellingen om deze optie te gebruiken.',
  'cc_h2': 'Deze optie geldt alleen voor HTTP/3-verbindingen.',
  'cc_saved': 'Opgeslagen',
  'cc_pending': 'Wacht op de volgende handmatige verbinding.',
  'save_changes': 'Wijzigingen toepassen',
  'saving_changes': 'Wijzigingen worden toegepast…',
  'unsaved_changes': 'Niet-toegepaste wijzigingen',
  'changes_applied': 'Wijzigingen toegepast',
  'changes_apply_hint': 'Bewerkingen worden pas van kracht nadat u ze toepast.',
  'changes_failed':
      'Wijzigingen konden niet worden toegepast. Controleer de opgeslagen '
      'waarden en probeer het opnieuw.',
  'form_errors':
      'Controleer de gemarkeerde velden voordat u de wijzigingen toepast.',
  'discard_changes_title': 'Niet-toegepaste wijzigingen verwerpen?',
  'discard_changes_body':
      'Uw bewerkingen zijn nog niet toegepast. Blijf bewerken om ze op te '
      'slaan, of verwerp ze om te vertrekken.',
  'keep_editing': 'Blijven bewerken',
  'discard_changes': 'Wijzigingen verwerpen',
  'invalid_port': 'Voer een poort van 1 tot 65535 in.',
  'listener_exposure':
      'Listeneradressen staan toegang vanaf het lokale netwerk toe',
  'invalid_ipv4': 'Voer een geldig IPv4-adres in, bijvoorbeeld 127.0.0.1.',
  'invalid_ipv6': 'Voer een geldig IPv6-adres in, bijvoorbeeld ::1.',
  'output_running': 'Bezig',
  'output_waiting': 'Ingeschakeld · niet actief',
  'output_disabled': 'Uitgeschakeld',
  'output_starting': 'Bezig met starten',
  'output_stopping': 'Bezig met stoppen',
  'output_reconnecting': 'Opnieuw verbinden',
  'output_degraded': 'Beperkt',
  'output_error': 'Fout',
  'output_unknown': 'Status niet beschikbaar',
  'shared_network_scope':
      'Netwerkinstellingen worden door alle accounts gedeeld.',
  'connection_details': 'Verbindingsgegevens',
  'home_overview': 'Verbindingsoverzicht',
  'home_exit_region': 'Uitgangsregio',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Verkeer',
  'home_traffic_window': 'Laatste 60 seconden',
  'home_traffic_idle': 'Verkeer verschijnt na verbinden',
  'home_traffic_waiting': 'Wachten op verkeersgegevens',
  'home_traffic_unavailable': 'Geen verkeersgeschiedenis beschikbaar',
  'home_traffic_stale': 'Verkeersupdates vertraagd',
  'home_outputs_next': 'Beschikbaar na verbinden',
  'home_outputs_retry': 'VPN en proxy’s voor de volgende verbinding',
  'connection_protection_group': 'Verbinding en bescherming',
  'proxy_routing_group': 'Proxy en routering',
  'application_group': 'Applicatie',
  'proxy_settings_link': 'Listeneradressen, poorten, authenticatie en DNS.',
  'proxy_auth_separate':
      'Gebruik hieronder Gebruikersnaam en wachtwoord opslaan om deze wijzigingen toe te passen.',
  'reset_draft_hint':
      'Standaardwaarden worden in dit formulier geladen. Pas de wijzigingen '
      'toe om ze door te voeren.',
};

const Map<String, String> kNetworkQualityNl = <String, String>{
  'nq_range': 'Bereik',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Retourtijd',
  'diag_check_quality_packet_loss': 'Pakketverlies',
  'diag_check_quality_queue_pressure': 'Wachtrijdruk',
  'diag_check_quality_pmtu': 'Pad-MTU',
  'diag_check_transport_migration_capability':
      'Migratie binnen dezelfde familie',
  'diag_check_dns_direct_encrypted_configuration':
      'Configuratie van directe DNS',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Uitvoeringsstatus van directe DNS',
  'diag_check_dns_direct_encrypted_reachability':
      'Bereikbaarheid van versleutelde DNS',
  'diag_check_transport_h3_path_validation_probe': 'Geïsoleerde QUIC-handshake',
  'nq_finding_unavailable':
      'Deze meting is in de huidige status niet beschikbaar.',
  'nq_finding_invalid_configuration':
      'De aangepaste DNS-configuratie is ongeldig.',
  'nq_finding_dns_system':
      'DNS van het huidige netwerk wordt gebruikt; controles voor versleutelde DNS zijn niet van toepassing.',
  'nq_finding_unsupported':
      'Werk Usque bij voor versleutelde DNS. Aanvragen schakelen niet over op onversleutelde DNS.',
  'nq_finding_dns_custom_valid':
      'De aangepaste configuratie voor versleutelde DNS is geldig. Terugval '
      'naar platte tekst is uitgeschakeld.',
  'nq_finding_stale':
      'De meting is verouderd of het fysieke netwerk is gewijzigd.',
  'nq_finding_rtt_high': 'De gemeten retourtijd is verhoogd.',
  'nq_finding_healthy':
      'De beschikbare verbindingsmetingen liggen binnen het normale bereik.',
  'nq_finding_loss_high': 'Het pakketverlies in het interval is verhoogd.',
  'nq_finding_queue_pressure':
      'Verkeer wacht op verzending of er zijn gegevens weggegooid tijdens deze verbinding.',
  'nq_finding_pmtu_degraded':
      'Usque kon geen geschikte pakketgrootte voor deze verbinding bevestigen.',
  'nq_finding_migration_reconnect':
      'Bij een netwerkwissel moet deze verbinding opnieuw worden opgezet.',
  'nq_finding_dns_changed':
      'De opgeslagen DNS-modus verschilt van de actieve verbinding.',
  'nq_finding_dns_runtime': 'Versleutelde DNS werkt.',
  'nq_finding_dns_degraded':
      'Versleutelde DNS heeft problemen. Mislukte aanvragen gebruiken niet de onversleutelde DNS van het netwerk.',
  'nq_finding_probe_unsafe':
      'Deze meting is in de huidige status niet beschikbaar.',
  'nq_finding_probe_success': 'Deze controle is geslaagd.',
  'nq_finding_probe_cancelled': 'Deze controle is geannuleerd.',
  'nq_finding_probe_timeout': 'Time-out bij diagnostische controle',
  'nq_finding_probe_failed': 'Deze controle is mislukt.',
  'diag_fix_nq_profile':
      'Controleer de aangepaste DNS-velden en de certificaatnaam. Schakel '
      'TLS-verificatie niet uit.',
  'diag_fix_nq_retry':
      'Wacht op een stabiel netwerk en probeer het daarna opnieuw.',
  'diag_fix_nq_network':
      'Controleer de lokale connectiviteit en vergelijk een nieuwe meting '
      'voordat u instellingen wijzigt.',
  'diag_fix_nq_reconnect':
      'Maak opnieuw verbinding om de opgeslagen configuratie toe te passen.',
  'nav_network_quality': 'Kwaliteit',
  'network_quality': 'Netwerkkwaliteit',
  'nq_subtitle': 'Bekijk de verbinding, niet alleen de snelheid.',
  'nq_local_only': 'Alleen lokale metingen. Er wordt niets geüpload.',
  'nq_doctor': 'Netwerkcontrole uitvoeren',
  'nq_doctor_help':
      'Standaardcontroles lezen alleen de lokale status. Ze openen geen '
      'externe verbindingen en wijzigen uw instellingen niet.',
  'nq_live': 'Actueel',
  'nq_stale': 'Verouderde metingen',
  'nq_updated': 'Laatste meting',
  'nq_seconds': '{count} s geleden',
  'nq_good': 'Goed',
  'nq_fair': 'Matig',
  'nq_poor': 'Slecht',
  'nq_limited': 'Beperkte gegevens',
  'nq_disconnected': 'Niet verbonden',
  'nq_connecting': 'Verbinden',
  'nq_connected': 'Verbonden',
  'nq_unavailable': 'Niet beschikbaar',
  'nq_not_ready': 'Niet gereed',
  'nq_unsupported': 'Niet ondersteund',
  'nq_capability_missing':
      'Deze versie toont geen verbindingskwaliteit. Verbinden en verbreken blijven mogelijk. Controleer op updates bij Instellingen.',
  'nq_empty': 'Maak verbinding om metingen te bekijken.',
  'nq_stale_help':
      'Updates zijn onderbroken. De laatste metingen worden getoond.',
  'nq_rtt': 'Retourtijd',
  'nq_latest': 'Nieuwste',
  'nq_smoothed': 'Afgevlakt',
  'nq_minimum': 'Minimumwaarde',
  'nq_h2_ping': 'HTTP/2-protocol-PING',
  'nq_h3_rtt': 'QUIC-padmeting',
  'nq_throughput': 'Doorvoer',
  'nq_download': 'Downloaden',
  'nq_upload': 'Uploaden',
  'nq_one_second': '1 seconde',
  'nq_five_seconds': 'Gemiddelde over 5 seconden',
  'nq_loss': 'Pakketverlies',
  'nq_loss_h2': 'HTTP/2 biedt geen vergelijkbaar pakketverlies.',
  'nq_loss_interval':
      'Gemeten over het laatste interval; geen cumulatief verlies.',
  'nq_congestion': 'Congestie',
  'nq_cwnd': 'Congestievenster',
  'nq_in_flight': 'Bytes onderweg',
  'nq_send_rate': 'Afleversnelheid',
  'nq_h2_window': 'HTTP/2-ontvangstvensters',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Verbinding',
  'nq_stalls': 'Capaciteitsstagnaties',
  'nq_pmtu': 'Pad-MTU',
  'nq_outer_pmtu': 'Buitenste UDP-payloadlimiet',
  'nq_inner_payload': 'CONNECT-IP-payloadlimiet',
  'nq_pmtu_help':
      'Dit is de pakketgrootte die het netwerkpad aankan. Usque controleert die automatisch om pakketverlies te beperken. Dit verhoogt niet de VPN-MTU in Geavanceerde netwerkinstellingen.',
  'nq_migration': 'Netwerkmigratie',
  'nq_migration_help':
      'Usque probeert de verbinding te behouden bij een wissel, bijvoorbeeld van wifi naar mobiele data. Beide netwerken moeten dezelfde IP-versie gebruiken: IPv4 of IPv6. Er wordt één netwerk tegelijk gebruikt; de snelheden worden niet opgeteld.',
  'nq_attempts': 'Pogingen',
  'nq_successes': 'Geslaagd',
  'nq_failures': 'Mislukt',
  'nq_last_duration': 'Laatste duur',
  'nq_direct_dns': 'Directe DNS',
  'nq_system_dns': 'DNS van het huidige netwerk',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Gereed',
  'nq_degraded': 'Gedegradeerd',
  'nq_timeouts': 'Time-outs',
  'nq_last_rtt': 'Laatste RTT',
  'nq_dns_redacted':
      'Resolver-namen en bootstrap-adressen worden alleen in Instellingen '
      'getoond.',
  'nq_queues': 'Wachtrijdruk',
  'nq_queue_details': 'Wachtrijen op laag niveau',
  'nq_queue_empty': 'Nog geen wachtrijmetingen.',
  'nq_current_capacity': 'Huidig / capaciteit',
  'nq_high_water': 'Hoogwaterlijn',
  'nq_drops': 'Verliezen',
  'nq_oldest': 'Oudste item',
  'nq_tunToTransport': 'Apparaat → transportlaag',
  'nq_proxyToTransport': 'Proxy → transportlaag',
  'nq_transportOutgoing': 'Transport uitgaand',
  'nq_h3DatagramSend': 'QUIC-datagrammen',
  'nq_h3WireSend': 'UDP-uitvoer',
  'nq_transportToTun': 'Transportlaag → apparaat',
  'nq_transportToProxy': 'Transportlaag → proxy',
  'nq_directDns': 'Directe DNS-verzoeken',
  'nq_unknown_queue': 'Andere wachtrij',
  'nq_trends': 'Laatste 60 seconden',
  'nq_samples': 'metingen',
  'nq_pause': 'Grafieken pauzeren',
  'nq_resume': 'Grafieken hervatten',
  'nq_paused': 'Grafieken gepauzeerd',
  'nq_gaps': 'Ontbrekende metingen zijn hiaten.',
  'nq_phase_idle': 'Inactief',
  'nq_phase_preparing_socket': 'Pad voorbereiden',
  'nq_phase_probing': 'Aan het toetsen',
  'nq_phase_validated': 'Gevalideerd',
  'nq_phase_promoting': 'Pad wisselen',
  'nq_phase_stable': 'Stabiel',
  'nq_phase_aborted': 'Afgebroken',
  'nq_phase_revalidating': 'Opnieuw valideren',
  'nq_phase_degraded': 'Gedegradeerd',
  'nq_phase_unknown': 'Niet gereed',
  'nq_phase_unsupported': 'Niet ondersteund',
  'nq_reason_family_unavailable':
      'Het nieuwe netwerk kan niet dezelfde IP-versie gebruiken. Opnieuw verbinden is nodig.',
  'nq_reason_socket_protect_failed':
      'Usque kon het nieuwe netwerk niet veilig gebruiken. Verbind handmatig opnieuw als de verbinding niet herstelt.',
  'nq_reason_generation_changed_during_setup':
      'Het netwerk is tijdens de voorbereiding opnieuw gewijzigd.',
  'nq_reason_peer_cid_unavailable':
      'De server kon de verbinding op het nieuwe netwerk niet behouden. Verbind zo nodig handmatig opnieuw.',
  'nq_reason_local_cid_unavailable':
      'Usque kon de verbinding op het nieuwe netwerk niet behouden. Verbind zo nodig handmatig opnieuw.',
  'nq_reason_path_probe_rejected':
      'Het nieuwe netwerk slaagde niet voor de verbindingscontrole. Controleer de internettoegang.',
  'nq_reason_path_validation_timeout':
      'Het nieuwe netwerk reageerde niet op tijd. Controleer het en verbind zo nodig opnieuw.',
  'nq_reason_superseded':
      'Het netwerk veranderde opnieuw voordat de wissel klaar was.',
  'nq_reason_promotion_failed':
      'Usque kon de netwerkwissel niet veilig voltooien. Verbind handmatig opnieuw als de verbinding niet herstelt.',
  'nq_reason_connection_closed':
      'De verbinding is gesloten tijdens de netwerkwissel. Verbind opnieuw.',
  'nq_reason_unsupported': 'Migratie is op deze verbinding niet beschikbaar.',
  'nq_reason_unknown': 'Er is geen ondersteunde reden beschikbaar.',
  'nq_dns_custom': 'Aangepaste versleutelde resolver',
  'nq_dns_server': 'Domeinnaam van DNS-server',
  'nq_dns_path': 'HTTPS-pad',
  'nq_dns_port': 'Poort (0 gebruikt de standaardwaarde)',
  'nq_dns_bootstrap': 'IP-adressen van DNS-server',
  'nq_dns_bootstrap_help':
      'Voer 1–8 IP-adressen van je DNS-aanbieder in, één per regel, zoals 1.1.1.1. Usque verbindt rechtstreeks met deze adressen zonder eerst de servernaam op te zoeken.',
  'nq_dns_no_fallback':
      'Als versleutelde directe DNS mislukt, mislukt de query. Er wordt nooit '
      'teruggevallen op systeem- of platte DNS.',
  'nq_dns_system_privacy':
      'De DNS-aanbieder van je huidige netwerk kan domeinen van direct verkeer zien.',
  'nq_dns_scope':
      'Voor verkeer dat overeenkomt met je regels voor directe landen. DNS voor VPN-verkeer blijft gelijk.',
  'nq_dns_no_capability':
      'Werk Usque bij voor versleutelde DNS bij direct verkeer. Je instellingen blijven bewaard. Je kunt DNS van het huidige netwerk kiezen als je de privacygevolgen accepteert.',
  'nq_dns_invalid_name':
      'Voer een domein zoals dns.example.com in, zonder https://, poort of spaties.',
  'nq_dns_invalid_path':
      'Voer een pad zoals /dns-query in, maximaal 256 tekens. Verwijder spaties en delen vanaf ? of #.',
  'nq_dns_invalid_bootstrap': 'Voer 1–8 IP-adressen van de server in.',
  'nq_dns_invalid_port':
      'Voer een poort van 1 tot 65535 in, of 0 voor de standaardwaarde.',
  'nq_dns_invalid_mode': 'Kies een ondersteunde DNS-modus.',
  'nq_doctor_deep_title': 'Diepgaande netwerkcontroles uitvoeren?',
  'nq_doctor_deep_body':
      'Controles kunnen testverkeer verzenden. Ze duren maximaal 15 seconden en kunnen worden geannuleerd. Je verbindingsinstellingen blijven ongewijzigd.',
  'nq_doctor_deep_run': 'Diepgaande controles uitvoeren',
  'nq_doctor_evidence':
      'Deze controles kunnen niet vaststellen of DNS-lekken optreden.',
};

const Map<String, String> kWindowsRecoveryNl = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'De verbindingsonderdelen van Usque moeten samen worden bijgewerkt. Controleer op updates bij Instellingen. Er is geen nieuwe VPN-verbinding gestart.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'De vorige VPN-verbinding is nog niet opgeruimd. Sluit Usque volledig en open het opnieuw. Open Diagnostiek als het probleem blijft.',
  'WINDOWS_RECOVERY_FAILED':
      'De vorige VPN-netwerkstatus kon niet volledig worden hersteld. Er is '
      'geen nieuwe VPN-verbinding gestart. Probeer de verbinding opnieuw of '
      'bekijk de lokale diagnostiek.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows kon de vorige VPN-netwerkstatus na drie automatische pogingen '
      'niet herstellen. Probeer het opnieuw wanneer u klaar bent, of bekijk de '
      'lokale diagnostiek.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Automatisch herstel is gestopt omdat niet kon worden bevestigd dat de vorige VPN-instellingen veilig hersteld konden worden. Controleer op updates bij Instellingen; exporteer bij aanhoudende problemen een diagnosepakket.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Het herstel van het Windows-netwerk duurt langer dan verwacht. Er is '
      'geen nieuwe VPN-verbinding gestart. Wacht tot het herstel is voltooid '
      'voordat u het opnieuw probeert.',
  'WINDOWS_RECOVERY_CONFLICT':
      'De netwerkstatus is gewijzigd of wordt nog gebruikt door een andere '
      'sessie. Het automatische herstel is gestopt om de actieve verbinding te '
      'beschermen.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Deze installatie kan de vorige VPN-instellingen niet automatisch herstellen. Werk Usque bij via Instellingen en probeer opnieuw.',
};

const String kWindowsAdapterCleanupNl =
    'De virtuele netwerkadapter van de vorige verbinding kon niet worden verwijderd, of de verwijdering kon niet worden bevestigd. Er is geen nieuwe VPN-verbinding gestart.';

const Map<String, String> kL4Nl = <String, String>{
  'l4_quic_not_ready': 'L4-verbinding voorbereiden',
  'l4_unsupported_packets':
      'Niet-ondersteunde of ongeldige pakketten geweigerd',
  'l4_budget_rejections': 'Afgewezen resourcetoelatingen',
  'l4_not_applicable': 'Niet van toepassing (L4)',
  'l4_mode': 'L4 (experimenteel)',
  'l4_transport_hint':
      'Alleen TCP. Apps die UDP nodig hebben werken mogelijk niet. De automatische modus kiest geen L4.',
  'l4_explanation':
      'L4 vervoert TCP via HTTP/3 en werkt met VPN, SOCKS5- en HTTP-proxy’s. DNS-aanvragen van de VPN worden omgezet naar TCP. Apps die ander UDP-verkeer, externe Ping, IP-fragmenten of uitbreidingsheaders nodig hebben, werken mogelijk niet. Kies L4 handmatig; Automatisch kiest het niet.',
  'l4_unsupported':
      'L4 is niet beschikbaar in deze Usque-versie. Controleer op updates bij Instellingen.',
  'l4_sni_identity':
      'Automatisch ingesteld door je account. De servernaam voor andere verbindingsmodi blijft bewaard.',
  'l4_edge_requires_l4':
      'Aan de rand omgezette DNS vereist L4. Kies een andere proxy-DNS-modus voordat u naar Auto, H3 of H2 schakelt.',
  'proxy_dns_edge_resolved':
      'Cloudflare-rand (alleen L4; geen lokale opzoeking)',
  'l4_verified': 'L4 heeft een appverbinding gemaakt',
  'l4_unverified': 'Server verbonden; appverbinding nog niet bevestigd',
  'l4_status_unknown': 'Status van appverbinding niet beschikbaar',
  'l4_sessions': 'Sessies / leegloop',
  'l4_flows': 'Actieve / wachtende streams',
  'l4_connect': 'CONNECT geslaagd / mislukt / time-out',
  'l4_buffers': 'Gebruikt applicatiebufferbudget (bytes)',
  'l4_backpressure': 'Verzend- / ontvangsttegendruk',
  'l4_tun_flows': 'TUN TCP / halfopen',
  'l4_udp': 'Geweigerde UDP-pakketten',
  'l4_dns': 'DNS-omzettingen geslaagd / mislukt / time-out',
  'l4_migration': 'Streams behouden door migratie / beëindigd door herbouw',
  'l4_na':
      'CONNECT-IP-adresbeheer, DATAGRAM-wachtrijen, binnenste payload-MTU en UDP-time-out: niet van toepassing in L4.',
};

const Map<String, String> kNetworkSettingsNl = <String, String>{
  'settings_applying': 'Opgeslagen, wordt toegepast',
  'settings_applied': 'Opgeslagen en toegepast',
  'settings_deferred':
      'Opgeslagen, gaat in bij de volgende handmatige verbinding',
  'settings_failed': 'Opgeslagen, toepassen mislukt',
  'settings_unknown': 'Resultaat nog niet bevestigd',
  'settings_saved': 'Opgeslagen',
  'settings_unsupported':
      'Sluit Usque volledig, open het opnieuw en sla nogmaals op. Controleer bij problemen op updates in Instellingen.',
  'settings_save_failed':
      'Instellingen konden niet worden opgeslagen. Uw wijzigingen blijven behouden.',
  'settings_reconnect': 'Opnieuw verbinden',
};

const Map<String, String> kChainNl = <String, String>{
  'duplicate_directive': 'Deze richtlijn mag maar één keer voorkomen.',
  'mixed_protocols':
      'Alle remote-eindpunten moeten hetzelfde TCP- of UDP-transport gebruiken.',
  'conflicting_protocol':
      'De remote-waarde botst met de globale transportinstelling.',
  'too_many_endpoints': 'Gebruik hoogstens 16 remote-eindpunten.',
  'conflicting_authentication':
      'CLIENT_CERT botst met het ingesloten certificaat of de authenticatiemodus.',
  'serialized_size_limit':
      'De versleutelde record zou de opslaglimiet overschrijden.',
  'multi_endpoint_unavailable':
      'Werk de engine bij om configuraties met meerdere eindpunten te gebruiken.',
  'candidates': 'Starteindpunten',
  'random_order':
      'Eindpunten worden bij elke verbinding in een nieuwe willekeurige volgorde geprobeerd.',
  'file_order': 'Eindpunten worden in de volgorde van het bestand geprobeerd.',
  'attempting': 'Eindpunt wordt geprobeerd',
  'actual_endpoint': 'Verbonden eindpunt',
  'attempt_failures': 'Mislukte pogingen',
  'failure_transport': 'transport gesloten',
  'failure_authentication': 'authenticatie',
  'failure_certificate': 'certificaat',
  'failure_configuration': 'configuratie',
  'failure_address_changed': 'adres gewijzigd',
  'failure_protocol': 'protocolfout',
  'failure_cleanup': 'opschoning',
  'failure_reason': 'Fout: {reason}.',
  'manage': 'Beheren',
  'dns_fallback': 'Tunnel-DNS (OpenVPN kan DNS onderhandelen)',
  'dns_unavailable_title': 'Geen DNS via deze uitgang',
  'dns_unavailable':
      'Er is geen DNS-server bereikbaar via deze uitgang. Gebruik IP-adressen of kies een andere uitgang met bereikbare DNS.',
  'authentication_failed':
      'Authenticatie mislukt. Werk de aanmeldgegevens bij voordat u opnieuw verbindt.',
  'profile_limit': 'De configuratiebibliotheek is vol (128 profielen).',
  'metadata_limit': 'De metadata van de configuratiebibliotheek is vol.',
  'title': 'Ketenproxy',
  'subtitle': 'Kies een uitgang die via WARP wordt bereikt.',
  'source': 'Uitgangsbron',
  'enable': 'Ketenproxy inschakelen',
  'import_file': 'Bestand importeren',
  'paste': 'Configuratie plakken',
  'profiles': 'Opgeslagen configuraties',
  'empty': 'Importeer een configuratie om een uitgang te kiezen.',
  'empty_hint_openvpn':
      'Importeer een .ovpn-bestand of plak de tekst. TCP- en UDP-eindpunten, ingesloten certificaten en gebruikersnaam/wachtwoord worden ondersteund.',
  'empty_hint_wireguard':
      'Importeer een .conf-bestand of plak de tekst. Eén sectie [Interface] en één sectie [Peer] worden ondersteund.',
  'import_limits':
      'Configuraties moeten UTF-8-tekst van maximaal 128 KiB zijn. Zonder bestandskeuze kunt u de tekst plakken.',
  'enable_to_choose': 'Schakel de ketenproxy in om een configuratie te kiezen.',
  'select_required': 'Selecteer een opgeslagen configuratie om toe te passen.',
  'pending_disable': 'In afwachting: ketenproxy uitschakelen',
  'apply_reconnect': 'Toepassen en opnieuw verbinden',
  'requires_connect_ip': 'Vereist CONNECT-IP',
  'menu': 'Configuratieacties',
  'preview': 'Configuratie controleren',
  'save_import': 'Configuratie opslaan',
  'name': 'Naam',
  'configuration': 'Configuratietekst',
  'file_loaded': 'Configuratie geladen uit bestand ({lines} regels).',
  'username': 'Gebruikersnaam',
  'password': 'Wachtwoord',
  'key_password': 'Wachtwoord van de privésleutel',
  'show_password': 'Wachtwoord tonen',
  'hide_password': 'Wachtwoord verbergen',
  'credentials': 'Aanmeldgegevens bijwerken',
  'rename': 'Hernoemen',
  'delete': 'Verwijderen',
  'cancel': 'Annuleren',
  'save': 'Opslaan',
  'apply': 'Wijzigingen toepassen',
  'clear': 'Selectie wissen',
  'current': 'Huidige verbinding',
  'saved': 'Opgeslagen selectie',
  'draft': 'Selectie in afwachting',
  'disconnected': 'Niet verbonden',
  'disabled': 'Niet ingeschakeld',
  'enabled_idle': 'Ingeschakeld · niet verbonden',
  'disconnecting': 'Verbinding wordt verbroken',
  'file_read_failed': 'Het configuratiebestand kon niet worden gelezen.',
  'file_encoding_invalid': 'Het configuratiebestand moet UTF-8-tekst zijn.',
  'file_busy': 'Er is al een bestandskeuze geopend.',
  'connected': 'Verbonden',
  'connecting': 'Verbinden',
  'error': 'Verbinding mislukt',
  'no_selection': 'Geen configuratie geselecteerd',
  'l4': 'Deze configuratie vereist de modus CONNECT-IP.',
  'switch_mode': 'Overschakelen naar CONNECT-IP en toepassen',
  'unsupported': 'Deze engine ondersteunt deze uitgangsbron niet.',
  'scope':
      'Bestaande expliciete directe regels blijven gelden. Overig verkeer gebruikt de gekozen uitgang.',
  'allowed': 'Toegestane bestemmingen',
  'dns': 'DNS',
  'addresses': 'Tunneladressen',
  'address_family': 'Adresfamilie',
  'transport': 'Overdracht',
  'endpoint': 'Doelserver',
  'restricted':
      'Bestemmingen buiten AllowedIPs worden op het proxypad geblokkeerd.',
  'delete_confirm':
      'Deze opgeslagen configuratie verwijderen? Het oorspronkelijk geïmporteerde bestand blijft ongewijzigd.',
  'profile_in_use':
      'Kies een andere configuratie of wis de opgeslagen selectie voordat u deze configuratie verwijdert.',
  'stale_revision':
      'De configuratie is gewijzigd. Vernieuw de lijst en probeer het opnieuw.',
  'secure_storage_failed':
      'De versleutelde configuratie kon niet worden gelezen of opgeslagen.',
  'invalid_configuration':
      'De configuratie is ongeldig of bevat niet-ondersteunde opties.',
  'looks_like_wireguard':
      'Dit lijkt een WireGuard-configuratie. Zet de uitgangsbron op WireGuard.',
  'looks_like_openvpn':
      'Dit lijkt een OpenVPN-configuratie. Zet de uitgangsbron op OpenVPN.',
  'error_location': '{message} ({field}, regel {line})',
  'error_field': '{message} ({field})',
  'file_unavailable':
      'Er is geen bestandskeuze beschikbaar. Plak de configuratietekst.',
  'invalid_size_or_encoding':
      'Gebruik een UTF-8-configuratie van maximaal 128 KiB.',
  'unsupported_directive': 'Deze OpenVPN-richtlijn wordt niet ondersteund.',
  'unsupported_or_duplicate_field':
      'Dit veld wordt niet ondersteund of komt dubbel voor.',
  'unsupported_or_duplicate_section':
      'Gebruik één sectie Interface en één sectie Peer.',
  'missing_field': 'Een verplicht veld ontbreekt.',
  'invalid_name':
      'Gebruik een naam van 1 tot 64 tekens zonder besturingstekens.',
  'invalid_key':
      'De sleutel moet een geldige Base64-sleutel van 32 bytes zijn.',
  'checking': 'Configuratie wordt gecontroleerd…',
  'changed': 'Wijzigingen opgeslagen',
};
