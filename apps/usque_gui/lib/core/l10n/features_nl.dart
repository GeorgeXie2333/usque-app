/// Supplemental feature strings for Dutch.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowNl = <String, String>{
  'cc_label': 'HTTP/3-congestiecontrole',
  'cc_help': 'Wordt bij de volgende handmatige verbinding toegepast.',
  'cc_upgrade': 'Update van de Engine vereist.',
  'cc_h2': 'HTTP/2 gebruikt systeem-TCP.',
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
  'home_traffic_idle': 'Start na het verbinden',
  'home_traffic_waiting': 'Wachten op metingen',
  'home_traffic_unavailable': 'Geschiedenis niet beschikbaar',
  'home_traffic_stale': 'Metingen vertraagd',
  'home_outputs_next': 'Uitgangen worden na het verbinden ingeschakeld',
  'home_outputs_retry': 'Uitgangen geconfigureerd voor de volgende poging',
  'connection_protection_group': 'Verbinding en bescherming',
  'proxy_routing_group': 'Proxy en routering',
  'application_group': 'Applicatie',
  'proxy_settings_link': 'Listeneradressen, poorten, authenticatie en DNS.',
  'proxy_auth_separate':
      'Inloggegevens worden afzonderlijk opgeslagen met Inloggegevens opslaan.',
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
      'Fysieke systeem-DNS is geselecteerd; controles voor versleutelde DNS '
      'zijn niet van toepassing.',
  'nq_finding_unsupported':
      'Versleutelde DNS is in deze Engine niet beschikbaar; terugval naar '
      'platte tekst is niet toegestaan.',
  'nq_finding_dns_custom_valid':
      'De aangepaste configuratie voor versleutelde DNS is geldig. Terugval '
      'naar platte tekst is uitgeschakeld.',
  'nq_finding_stale':
      'De meting is verouderd of het fysieke netwerk is gewijzigd.',
  'nq_finding_rtt_high': 'De gemeten retourtijd is verhoogd.',
  'nq_finding_healthy':
      'De beschikbare lokale meting ligt binnen het verwachte bereik.',
  'nq_finding_loss_high': 'Het pakketverlies in het interval is verhoogd.',
  'nq_finding_queue_pressure':
      'Een wachtrij staat onder druk of heeft tijdens deze verbinding '
      'verliezen geregistreerd.',
  'nq_finding_pmtu_degraded': 'De validatie van de pad-MTU is verslechterd.',
  'nq_finding_migration_reconnect':
      'Migratie is op dit pad niet beschikbaar; bij een netwerkwijziging wordt '
      'volledig opnieuw verbonden.',
  'nq_finding_dns_changed':
      'De opgeslagen DNS-modus verschilt van de actieve verbinding.',
  'nq_finding_dns_runtime':
      'Versleutelde DNS is geslaagd. De lokale status is geen extern bewijs '
      'tegen lekken.',
  'nq_finding_dns_degraded':
      'Versleutelde DNS is verslechterd; mislukte directe query’s vallen niet '
      'terug op systeem-DNS.',
  'nq_finding_probe_unsafe':
      'Probe overgeslagen: de vereiste veilige status of opgeslagen identiteit '
      'is niet beschikbaar. Een actieve tunnel wordt nooit gedupliceerd.',
  'nq_finding_probe_success':
      'De geverifieerde probe is voltooid. Dit is geen externe test op '
      'pakketlekken.',
  'nq_finding_probe_cancelled': 'Probe geannuleerd en opschoning aangevraagd.',
  'nq_finding_probe_timeout':
      'De begrensde probe is niet vóór de deadline voltooid.',
  'nq_finding_probe_failed':
      'De geverifieerde probe is mislukt; er is geen onveilige terugval '
      'geprobeerd.',
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
      'Deze Engine biedt geen netwerkkwaliteit. De bestaande '
      'verbindingsbediening blijft werken.',
  'nq_empty':
      'Maak verbinding om metingen te zien. Onbekende waarden worden niet als '
      'nul weergegeven.',
  'nq_stale_help':
      'De bron is gestopt met bijwerken. Dit zijn eerdere metingen; hiaten '
      'blijven hiaten.',
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
  'nq_pmtu_help': 'Padontdekking verhoogt de TUN-MTU van het apparaat niet.',
  'nq_migration': 'Netwerkmigratie',
  'nq_migration_help':
      'Één verbinding, één gegevenspad. Alleen dezelfde IP-adresfamilie; geen '
      'meerdere paden.',
  'nq_attempts': 'Pogingen',
  'nq_successes': 'Geslaagd',
  'nq_failures': 'Mislukt',
  'nq_last_duration': 'Laatste duur',
  'nq_direct_dns': 'Directe DNS',
  'nq_system_dns': 'Fysieke systeem-DNS',
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
      'De huidige IP-adresfamilie is niet beschikbaar; er wordt volledig '
      'opnieuw verbonden.',
  'nq_reason_socket_protect_failed':
      'Een beveiligde kandidaat-socket kon niet worden voorbereid.',
  'nq_reason_generation_changed_during_setup':
      'Het netwerk is tijdens de voorbereiding opnieuw gewijzigd.',
  'nq_reason_peer_cid_unavailable':
      'De peer heeft geen extra verbindingsidentificatie.',
  'nq_reason_local_cid_unavailable':
      'Een lokale verbindingsidentificatie is niet beschikbaar.',
  'nq_reason_path_probe_rejected':
      'Het kandidaatpad kon niet worden gevalideerd.',
  'nq_reason_path_validation_timeout':
      'De padvalidatie is verlopen; opnieuw verbinden is beschikbaar.',
  'nq_reason_superseded':
      'Een nieuwere netwerkwijziging heeft deze poging vervangen.',
  'nq_reason_promotion_failed':
      'Het padwisselen kon niet veilig worden voltooid.',
  'nq_reason_connection_closed':
      'De verbinding is tijdens de migratie gesloten.',
  'nq_reason_unsupported': 'Migratie is op deze verbinding niet beschikbaar.',
  'nq_reason_unknown': 'Er is geen ondersteunde reden beschikbaar.',
  'nq_dns_custom': 'Aangepaste versleutelde resolver',
  'nq_dns_server': 'TLS-servernaam',
  'nq_dns_path': 'HTTPS-pad',
  'nq_dns_port': 'Poort (0 gebruikt de standaardwaarde)',
  'nq_dns_bootstrap': 'Bootstrap-IP-adressen',
  'nq_dns_bootstrap_help':
      'Voer 1–8 numerieke IP-adressen in, één per regel. Er wordt geen '
      'hostnaamopzoeking gebruikt.',
  'nq_dns_no_fallback':
      'Als versleutelde directe DNS mislukt, mislukt de query. Er wordt nooit '
      'teruggevallen op systeem- of platte DNS.',
  'nq_dns_system_privacy':
      'Fysieke systeem-DNS kan namen van directe query’s blootstellen aan de '
      'DNS-provider van het fysieke netwerk.',
  'nq_dns_scope':
      'Alleen gebruikt voor door Geo geselecteerde directe query’s. Tunnel-DNS '
      'blijft ongewijzigd.',
  'nq_dns_no_capability':
      'Deze Engine kan geen versleutelde directe DNS gebruiken. Opgeslagen '
      'instellingen blijven behouden. U kunt expliciet systeem-DNS kiezen.',
  'nq_dns_invalid_name':
      'Voer een DNS-naam in zonder spaties, URL-syntaxis of jokertekens.',
  'nq_dns_invalid_path':
      'Gebruik een pad dat met / begint, tot 256 tekens, zonder query, '
      'fragment of witruimte.',
  'nq_dns_invalid_bootstrap':
      'Gebruik 1–8 unieke unicast-IP’s; geen ongespecificeerde, multicast-, '
      'broadcast- of IPv6-link-local-adressen.',
  'nq_dns_invalid_port': 'Voer 0–65535 in.',
  'nq_dns_invalid_mode': 'Kies een ondersteunde DNS-modus.',
  'nq_doctor_deep_title': 'Diepgaande netwerkcontroles uitvoeren?',
  'nq_doctor_deep_body':
      'Diepgaande controles kunnen een DNS-testquery naar uw geconfigureerde '
      'resolver sturen en een beveiligd QUIC-pad valideren. Ze duren maximaal '
      '15 seconden, kunnen worden geannuleerd, maken nooit een tweede '
      'datadragende tunnel aan en wijzigen nooit uw DNS, route, profiel of '
      'transport.',
  'nq_doctor_deep_run': 'Diepgaande controles uitvoeren',
  'nq_doctor_evidence':
      'Lokale controles beschrijven de configuratie en de waargenomen status. '
      'Ze zijn geen extern bewijs van nul DNS-lekken.',
};

const Map<String, String> kWindowsRecoveryNl = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'De vorige VPN-netwerkstatus kon niet volledig worden hersteld. Er is '
      'geen nieuwe VPN-verbinding gestart. Probeer de verbinding opnieuw of '
      'bekijk de lokale diagnostiek.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows kon de vorige VPN-netwerkstatus na drie automatische pogingen '
      'niet herstellen. Probeer het opnieuw wanneer u klaar bent, of bekijk de '
      'lokale diagnostiek.',
  'WINDOWS_RECOVERY_BLOCKED':
      'De automatische reparatie is gestopt omdat de vorige '
      'Windows-netwerkstatus niet veilig kon worden geverifieerd. Start de '
      'Agent opnieuw of werk Usque bij, en bekijk daarna de lokale '
      'diagnostiek.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Het herstel van het Windows-netwerk duurt langer dan verwacht. Er is '
      'geen nieuwe VPN-verbinding gestart. Wacht tot het herstel is voltooid '
      'voordat u het opnieuw probeert.',
  'WINDOWS_RECOVERY_CONFLICT':
      'De netwerkstatus is gewijzigd of wordt nog gebruikt door een andere '
      'sessie. Het automatische herstel is gestopt om de actieve verbinding te '
      'beschermen.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Deze Windows-Agent ondersteunt geen veilig automatisch herstel. Werk de '
      'toepassing en de Agent samen bij, en probeer het daarna opnieuw.',
};

const String kWindowsAdapterCleanupNl =
    'De vorige Wintun-adapter kon niet worden verwijderd, of de verwijdering '
    'kon niet worden geverifieerd. Er is geen nieuwe VPN-verbinding gestart.';

const Map<String, String> kL4Nl = <String, String>{
  'l4_quic_not_ready': 'Wachten op een gerede QUIC-sessie',
  'l4_unsupported_packets':
      'Niet-ondersteunde of ongeldige pakketten geweigerd',
  'l4_budget_rejections': 'Afgewezen resourcetoelatingen',
  'l4_not_applicable': 'Niet van toepassing (L4)',
  'l4_mode': 'L4 (experimenteel)',
  'l4_explanation':
      'Alleen TCP over HTTP/3. Ondersteunt VPN/TUN, SOCKS5 en HTTP; TUN-DNS wordt naar TCP omgezet. Auto kiest nooit L4. Overige UDP, externe ping, IP-fragmenten en extensiekoppen worden niet ondersteund; sommige apps werken mogelijk niet.',
  'l4_unsupported':
      'Deze engine heeft geen volledige L4-ondersteuning aangegeven. L4 kan niet worden ingeschakeld.',
  'l4_sni_identity':
      'Alleen-lezen: afgeleid van de geladen accountidentiteit. De bestaande CONNECT-IP-SNI blijft behouden.',
  'l4_edge_requires_l4':
      'Aan de rand omgezette DNS vereist L4. Kies een andere proxy-DNS-modus voordat u naar Auto, H3 of H2 schakelt.',
  'proxy_dns_edge_resolved':
      'Cloudflare-rand (alleen L4; geen lokale opzoeking)',
  'l4_verified': 'L4 CONNECT geverifieerd',
  'l4_unverified': 'QUIC gereed; L4 CONNECT nog niet geverifieerd',
  'l4_status_unknown': 'L4-verificatiestatus onbekend',
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
      'Start de Engine opnieuw of werk deze bij om netwerkinstellingen op te slaan.',
  'settings_save_failed':
      'Instellingen konden niet worden opgeslagen. Uw wijzigingen blijven behouden.',
  'settings_reconnect': 'Opnieuw verbinden',
};
