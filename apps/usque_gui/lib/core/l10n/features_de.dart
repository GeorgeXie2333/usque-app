/// Supplemental feature strings for German.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowDe = <String, String>{
  'local_proxy_settings': 'Lokale Proxy-Einstellungen',
  'proxy_switches_hint':
      'Schalter werden automatisch gespeichert. Änderungen an Listenern und DNS mit Änderungen anwenden übernehmen.',
  'cc_label': 'HTTP/3-Überlastkontrolle',
  'cc_help': 'Wird bei der nächsten manuellen Verbindung wirksam.',
  'cc_upgrade':
      'Aktualisiere Usque unter Einstellungen, um diese Option zu nutzen.',
  'cc_h2': 'Diese Option betrifft nur HTTP/3-Verbindungen.',
  'cc_saved': 'Gespeichert',
  'cc_pending': 'Wartet auf die nächste manuelle Verbindung.',
  'save_changes': 'Änderungen anwenden',
  'saving_changes': 'Änderungen werden angewendet…',
  'unsaved_changes': 'Nicht angewendete Änderungen',
  'changes_applied': 'Änderungen angewendet',
  'changes_apply_hint':
      'Bearbeitungen werden erst wirksam, nachdem Sie sie anwenden.',
  'changes_failed':
      'Änderungen konnten nicht angewendet werden. Prüfen Sie die '
      'gespeicherten Werte und versuchen Sie es erneut.',
  'form_errors':
      'Prüfen Sie die hervorgehobenen Felder, bevor Sie die Änderungen '
      'anwenden.',
  'discard_changes_title': 'Nicht angewendete Änderungen verwerfen?',
  'discard_changes_body':
      'Ihre Bearbeitungen wurden noch nicht angewendet. Bearbeiten Sie weiter, '
      'um sie zu speichern, oder verwerfen Sie sie, um die Seite zu verlassen.',
  'keep_editing': 'Weiter bearbeiten',
  'discard_changes': 'Änderungen verwerfen',
  'invalid_port': 'Geben Sie einen Port von 1 bis 65535 ein.',
  'listener_exposure':
      'Listener-Adressen erlauben Zugriff aus dem lokalen Netzwerk',
  'invalid_ipv4':
      'Geben Sie eine gültige IPv4-Adresse ein, zum Beispiel 127.0.0.1.',
  'invalid_ipv6': 'Geben Sie eine gültige IPv6-Adresse ein, zum Beispiel ::1.',
  'output_running': 'Läuft',
  'output_waiting': 'Aktiviert · läuft nicht',
  'output_disabled': 'Deaktiviert',
  'output_starting': 'Startet',
  'output_stopping': 'Stoppt',
  'output_reconnecting': 'Neuverbindung',
  'output_degraded': 'Eingeschränkt',
  'output_error': 'Fehler',
  'output_unknown': 'Status nicht verfügbar',
  'shared_network_scope':
      'Netzwerkeinstellungen werden von allen Konten gemeinsam genutzt.',
  'connection_details': 'Verbindungsdetails',
  'home_overview': 'Verbindungsübersicht',
  'home_exit_region': 'Ausgangsregion',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Datenverkehr',
  'home_traffic_window': 'Letzte 60 Sekunden',
  'home_traffic_idle': 'Verkehr wird nach dem Verbinden angezeigt',
  'home_traffic_waiting': 'Warten auf Verkehrsdaten',
  'home_traffic_unavailable': 'Kein Verkehrsverlauf verfügbar',
  'home_traffic_stale': 'Verkehrsdaten werden verzögert aktualisiert',
  'home_outputs_next': 'Nach dem Verbinden verfügbar',
  'home_outputs_retry': 'VPN und Proxys für die nächste Verbindung',
  'connection_protection_group': 'Verbindung & Schutz',
  'proxy_routing_group': 'Proxy & Weiterleitung',
  'application_group': 'Anwendung',
  'proxy_settings_link': 'Listener-Adressen, Ports, Authentifizierung und DNS.',
  'proxy_auth_separate':
      'Klicke unten auf Benutzername und Passwort speichern, um diese Änderungen zu übernehmen.',
  'reset_draft_hint':
      'Standardwerte werden in dieses Formular geladen. Wenden Sie die '
      'Änderungen an, damit sie wirksam werden.',
};

const Map<String, String> kNetworkQualityDe = <String, String>{
  'nq_range': 'Bereich',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Umlaufzeit',
  'diag_check_quality_packet_loss': 'Paketverlust',
  'diag_check_quality_queue_pressure': 'Warteschlangendruck',
  'diag_check_quality_pmtu': 'Pfad-MTU',
  'diag_check_transport_migration_capability': 'Migration in derselben Familie',
  'diag_check_dns_direct_encrypted_configuration':
      'Konfiguration des direkten DNS',
  'diag_check_dns_direct_encrypted_runtime_state': 'Laufzeit des direkten DNS',
  'diag_check_dns_direct_encrypted_reachability':
      'Erreichbarkeit von verschlüsseltem DNS',
  'diag_check_transport_h3_path_validation_probe': 'Isolierter QUIC-Handshake',
  'nq_finding_unavailable':
      'Diese Messung ist im aktuellen Zustand nicht verfügbar.',
  'nq_finding_invalid_configuration':
      'Die eigene DNS-Konfiguration ist ungültig.',
  'nq_finding_dns_system':
      'DNS des aktuellen Netzwerks wird verwendet; Prüfungen für verschlüsseltes DNS entfallen.',
  'nq_finding_unsupported':
      'Aktualisiere Usque für verschlüsseltes DNS. Anfragen wechseln nicht zu unverschlüsseltem DNS.',
  'nq_finding_dns_custom_valid':
      'Die eigene Konfiguration für verschlüsseltes DNS ist gültig. '
      'Klartext-Rückfall ist deaktiviert.',
  'nq_finding_stale':
      'Der Messwert ist veraltet oder das physische Netzwerk hat sich '
      'geändert.',
  'nq_finding_rtt_high': 'Die gemessene Umlaufzeit ist erhöht.',
  'nq_finding_healthy':
      'Die verfügbaren Verbindungsmesswerte liegen im normalen Bereich.',
  'nq_finding_loss_high': 'Der Paketverlust im Intervall ist erhöht.',
  'nq_finding_queue_pressure':
      'Daten warten auf den Versand oder wurden während dieser Verbindung verworfen.',
  'nq_finding_pmtu_degraded':
      'Usque konnte keine geeignete Paketgröße für diese Verbindung bestätigen.',
  'nq_finding_migration_reconnect':
      'Beim Netzwerkwechsel muss diese Verbindung neu aufgebaut werden.',
  'nq_finding_dns_changed':
      'Der gespeicherte DNS-Modus unterscheidet sich von der laufenden '
      'Verbindung.',
  'nq_finding_dns_runtime': 'Verschlüsseltes DNS funktioniert.',
  'nq_finding_dns_degraded':
      'Verschlüsseltes DNS hat Probleme. Fehlgeschlagene Anfragen verwenden nicht das unverschlüsselte DNS des Netzwerks.',
  'nq_finding_probe_unsafe':
      'Diese Messung ist im aktuellen Zustand nicht verfügbar.',
  'nq_finding_probe_success': 'Diese Prüfung wurde bestanden.',
  'nq_finding_probe_cancelled': 'Diese Prüfung wurde abgebrochen.',
  'nq_finding_probe_timeout': 'Zeitüberschreitung bei der Diagnoseprüfung',
  'nq_finding_probe_failed': 'Diese Prüfung ist fehlgeschlagen.',
  'diag_fix_nq_profile':
      'Prüfen Sie die eigenen DNS-Felder und den Zertifikatsnamen. '
      'Deaktivieren Sie die TLS-Prüfung nicht.',
  'diag_fix_nq_retry':
      'Warten Sie auf ein stabiles Netzwerk und versuchen Sie es dann erneut.',
  'diag_fix_nq_network':
      'Prüfen Sie die lokale Konnektivität und vergleichen Sie eine neue '
      'Messung, bevor Sie Einstellungen ändern.',
  'diag_fix_nq_reconnect':
      'Verbinden Sie erneut, um die gespeicherte Konfiguration anzuwenden.',
  'nav_network_quality': 'Qualität',
  'network_quality': 'Netzwerkqualität',
  'nq_subtitle': 'Die Verbindung beurteilen, nicht nur die Geschwindigkeit.',
  'nq_local_only': 'Nur lokale Messungen. Es wird nichts hochgeladen.',
  'nq_doctor': 'Netzwerkdiagnose starten',
  'nq_doctor_help':
      'Standardprüfungen lesen nur den lokalen Zustand. Sie öffnen keine '
      'externen Verbindungen und ändern Ihre Einstellungen nicht.',
  'nq_live': 'Echtzeit',
  'nq_stale': 'Veraltete Messwerte',
  'nq_updated': 'Letzte Messung',
  'nq_seconds': '{count} s her',
  'nq_good': 'Gut',
  'nq_fair': 'Mittel',
  'nq_poor': 'Schlecht',
  'nq_limited': 'Begrenzte Daten',
  'nq_disconnected': 'Getrennt',
  'nq_connecting': 'Verbinden',
  'nq_connected': 'Verbunden',
  'nq_unavailable': 'Nicht verfügbar',
  'nq_not_ready': 'Nicht bereit',
  'nq_unsupported': 'Nicht unterstützt',
  'nq_capability_missing':
      'Diese Version zeigt keine Verbindungsqualität. Verbinden und Trennen bleiben möglich. Suche unter Einstellungen nach Updates.',
  'nq_empty': 'Verbinde dich, um Messwerte zu sehen.',
  'nq_stale_help':
      'Aktualisierungen wurden angehalten. Die letzten Messwerte werden angezeigt.',
  'nq_rtt': 'Umlaufzeit',
  'nq_latest': 'Aktuell',
  'nq_smoothed': 'Geglättet',
  'nq_minimum': 'Minimalwert',
  'nq_h2_ping': 'HTTP/2-Protokoll-PING',
  'nq_h3_rtt': 'QUIC-Pfadmessung',
  'nq_throughput': 'Durchsatz',
  'nq_download': 'Herunterladen',
  'nq_upload': 'Hochladen',
  'nq_one_second': '1 Sekunde',
  'nq_five_seconds': '5-Sekunden-Mittelwert',
  'nq_loss': 'Paketverlust',
  'nq_loss_h2': 'HTTP/2 stellt keinen vergleichbaren Paketverlust bereit.',
  'nq_loss_interval': 'Über das letzte Intervall gemessen; kein Gesamtverlust.',
  'nq_congestion': 'Überlastung',
  'nq_cwnd': 'Überlastfenster',
  'nq_in_flight': 'Bytes in Übertragung',
  'nq_send_rate': 'Zustellrate',
  'nq_h2_window': 'HTTP/2-Empfangsfenster',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Verbindung',
  'nq_stalls': 'Kapazitätsengpässe',
  'nq_pmtu': 'Pfad-MTU',
  'nq_outer_pmtu': 'Äußeres UDP-Nutzlastlimit',
  'nq_inner_payload': 'CONNECT-IP-Nutzlastlimit',
  'nq_pmtu_help':
      'Dies ist die Paketgröße, die der Netzwerkpfad transportieren kann. Usque prüft sie automatisch, um Paketverluste zu reduzieren. Die Prüfung erhöht nicht die VPN-MTU aus Erweiterte Netzwerkeinstellungen.',
  'nq_migration': 'Netzwerkmigration',
  'nq_migration_help':
      'Usque versucht die Verbindung beim Wechsel etwa von WLAN zu mobilen Daten zu erhalten. Beide Netzwerke müssen dieselbe IP-Version nutzen, IPv4 oder IPv6. Es wird jeweils nur ein Netzwerk verwendet; Geschwindigkeiten werden nicht addiert.',
  'nq_attempts': 'Versuche',
  'nq_successes': 'Erfolgreich',
  'nq_failures': 'Fehlgeschlagen',
  'nq_last_duration': 'Letzte Dauer',
  'nq_direct_dns': 'Direktes DNS',
  'nq_system_dns': 'DNS des aktuellen Netzwerks',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Bereit',
  'nq_degraded': 'Beeinträchtigt',
  'nq_timeouts': 'Zeitüberschreitungen',
  'nq_last_rtt': 'Letzte RTT',
  'nq_dns_redacted':
      'Resolver-Namen und Bootstrap-Adressen werden nur in den Einstellungen '
      'angezeigt.',
  'nq_queues': 'Warteschlangendruck',
  'nq_queue_details': 'Warteschlangen auf niedriger Ebene',
  'nq_queue_empty': 'Noch keine Warteschlangenmessungen.',
  'nq_current_capacity': 'Aktuell / Kapazität',
  'nq_high_water': 'Höchststand',
  'nq_drops': 'Verwürfe',
  'nq_oldest': 'Ältester Eintrag',
  'nq_tunToTransport': 'Gerät → Transportschicht',
  'nq_proxyToTransport': 'Proxy → Transportschicht',
  'nq_transportOutgoing': 'Transport ausgehend',
  'nq_h3DatagramSend': 'QUIC-Datagramme',
  'nq_h3WireSend': 'UDP-Ausgabe',
  'nq_transportToTun': 'Transportschicht → Gerät',
  'nq_transportToProxy': 'Transportschicht → Proxy',
  'nq_directDns': 'Direkte DNS-Anfragen',
  'nq_unknown_queue': 'Andere Warteschlange',
  'nq_trends': 'Letzte 60 Sekunden',
  'nq_samples': 'Messwerte',
  'nq_pause': 'Diagramme anhalten',
  'nq_resume': 'Diagramme fortsetzen',
  'nq_paused': 'Diagramme angehalten',
  'nq_gaps': 'Fehlende Messwerte sind Lücken.',
  'nq_phase_idle': 'Leerlauf',
  'nq_phase_preparing_socket': 'Pfad wird vorbereitet',
  'nq_phase_probing': 'Sondierung',
  'nq_phase_validated': 'Validiert',
  'nq_phase_promoting': 'Pfadwechsel',
  'nq_phase_stable': 'Stabil',
  'nq_phase_aborted': 'Abgebrochen',
  'nq_phase_revalidating': 'Neuvalidierung',
  'nq_phase_degraded': 'Beeinträchtigt',
  'nq_phase_unknown': 'Nicht bereit',
  'nq_phase_unsupported': 'Nicht unterstützt',
  'nq_reason_family_unavailable':
      'Das neue Netzwerk kann dieselbe IP-Version nicht nutzen. Eine neue Verbindung ist erforderlich.',
  'nq_reason_socket_protect_failed':
      'Usque konnte das neue Netzwerk nicht sicher nutzen. Verbinde dich manuell erneut, falls die Verbindung nicht zurückkehrt.',
  'nq_reason_generation_changed_during_setup':
      'Das Netzwerk hat sich während der Vorbereitung erneut geändert.',
  'nq_reason_peer_cid_unavailable':
      'Der Server konnte die Verbindung im neuen Netzwerk nicht erhalten. Verbinde dich bei Bedarf manuell erneut.',
  'nq_reason_local_cid_unavailable':
      'Usque konnte die Verbindung im neuen Netzwerk nicht erhalten. Verbinde dich bei Bedarf manuell erneut.',
  'nq_reason_path_probe_rejected':
      'Das neue Netzwerk hat die Verbindungsprüfung nicht bestanden. Prüfe seinen Internetzugang.',
  'nq_reason_path_validation_timeout':
      'Das neue Netzwerk hat nicht rechtzeitig geantwortet. Prüfe es und verbinde dich bei Bedarf erneut.',
  'nq_reason_superseded':
      'Das Netzwerk änderte sich erneut, bevor der Wechsel abgeschlossen war.',
  'nq_reason_promotion_failed':
      'Usque konnte den Netzwerkwechsel nicht sicher abschließen. Verbinde dich bei Bedarf manuell erneut.',
  'nq_reason_connection_closed':
      'Die Verbindung wurde beim Netzwerkwechsel geschlossen. Verbinde dich erneut.',
  'nq_reason_unsupported':
      'Migration ist auf dieser Verbindung nicht verfügbar.',
  'nq_reason_unknown': 'Kein unterstützter Grund ist verfügbar.',
  'nq_dns_custom': 'Eigener verschlüsselter Resolver',
  'nq_dns_server': 'Domain des DNS-Servers',
  'nq_dns_path': 'HTTPS-Pfad',
  'nq_dns_port': 'Port (0 verwendet den Standard)',
  'nq_dns_bootstrap': 'IP-Adressen des DNS-Servers',
  'nq_dns_bootstrap_help':
      'Gib 1–8 IP-Adressen deines DNS-Anbieters ein, eine pro Zeile, etwa 1.1.1.1. Usque verbindet sich direkt damit, ohne vorher den Servernamen aufzulösen.',
  'nq_dns_no_fallback':
      'Wenn verschlüsseltes direktes DNS fehlschlägt, schlägt die Anfrage '
      'fehl. Es erfolgt kein Rückfall auf System- oder Klartext-DNS.',
  'nq_dns_system_privacy':
      'Der DNS-Anbieter des aktuellen Netzwerks kann Domains von Direktverbindungen sehen.',
  'nq_dns_scope':
      'Gilt für Verkehr passend zu den Direktverbindungsregeln nach Land. Das DNS für VPN-Verkehr bleibt unverändert.',
  'nq_dns_no_capability':
      'Aktualisiere Usque für verschlüsseltes DNS bei Direktverbindungen. Einstellungen bleiben erhalten. Du kannst DNS des aktuellen Netzwerks wählen, wenn du die Datenschutzfolgen akzeptierst.',
  'nq_dns_invalid_name':
      'Gib eine Domain wie dns.example.com ohne https://, Port oder Leerzeichen ein.',
  'nq_dns_invalid_path':
      'Gib einen Pfad wie /dns-query mit höchstens 256 Zeichen ein. Entferne Leerzeichen und Teile ab ? oder #.',
  'nq_dns_invalid_bootstrap': 'Gib 1–8 Server-IP-Adressen ein.',
  'nq_dns_invalid_port':
      'Gib einen Port von 1 bis 65535 oder 0 für den Standardwert ein.',
  'nq_dns_invalid_mode': 'Wählen Sie einen unterstützten DNS-Modus.',
  'nq_doctor_deep_title': 'Tiefe Netzwerkprüfungen ausführen?',
  'nq_doctor_deep_body':
      'Prüfungen können Testdaten senden. Sie dauern bis zu 15 Sekunden und lassen sich abbrechen. Deine Verbindungseinstellungen bleiben unverändert.',
  'nq_doctor_deep_run': 'Tiefe Prüfungen ausführen',
  'nq_doctor_evidence':
      'Diese Prüfungen können nicht feststellen, ob DNS-Lecks auftreten.',
};

const Map<String, String> kWindowsRecoveryDe = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Usques Verbindungskomponenten müssen gemeinsam aktualisiert werden. Suche unter Einstellungen nach Updates. Es wurde keine neue VPN-Verbindung gestartet.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'Die vorherige VPN-Verbindung ist noch nicht vollständig bereinigt. Beende Usque vollständig und öffne es erneut. Hilft das nicht, öffne Diagnose.',
  'WINDOWS_RECOVERY_FAILED':
      'Der vorherige VPN-Netzwerkzustand konnte nicht vollständig '
      'wiederhergestellt werden. Es wurde keine neue VPN-Verbindung gestartet. '
      'Versuchen Sie die Verbindung erneut oder prüfen Sie die lokale '
      'Diagnose.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows konnte den vorherigen VPN-Netzwerkzustand nach drei '
      'automatischen Versuchen nicht wiederherstellen. Versuchen Sie es '
      'erneut, wenn Sie bereit sind, oder prüfen Sie die lokale Diagnose.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Die automatische Reparatur wurde gestoppt, weil die sichere Wiederherstellung der vorherigen VPN-Einstellungen nicht bestätigt werden konnte. Suche unter Einstellungen nach Updates; exportiere bei anhaltenden Problemen ein Diagnosepaket.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Die Wiederherstellung des Windows-Netzwerks dauert länger als erwartet. '
      'Es wurde keine neue VPN-Verbindung gestartet. Warten Sie, bis die '
      'Wiederherstellung abgeschlossen ist, bevor Sie es erneut versuchen.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Der Netzwerkzustand hat sich geändert oder wird noch von einer anderen '
      'Sitzung verwendet. Die automatische Wiederherstellung wurde gestoppt, '
      'um die aktive Verbindung zu schützen.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Diese Installation kann die vorherigen VPN-Einstellungen nicht automatisch wiederherstellen. Aktualisiere Usque unter Einstellungen und versuche es erneut.',
};

const String kWindowsAdapterCleanupDe =
    'Der virtuelle Netzwerkadapter der vorherigen Verbindung konnte nicht entfernt oder seine Entfernung nicht bestätigt werden. Es wurde keine neue VPN-Verbindung gestartet.';

const Map<String, String> kL4De = <String, String>{
  'l4_quic_not_ready': 'L4-Verbindung wird vorbereitet',
  'l4_unsupported_packets':
      'Nicht unterstützte oder fehlerhafte Pakete abgelehnt',
  'l4_budget_rejections': 'Abgelehnte Ressourcenzulassungen',
  'l4_not_applicable': 'Nicht zutreffend (L4)',
  'l4_mode': 'L4 (experimentell)',
  'l4_transport_hint':
      'Nur TCP. Apps, die UDP benötigen, funktionieren möglicherweise nicht. Der automatische Modus wählt kein L4.',
  'l4_explanation':
      'L4 überträgt TCP über HTTP/3 und funktioniert mit VPN, SOCKS5- und HTTP-Proxys. DNS-Anfragen des VPN werden in TCP umgewandelt. Apps mit anderem UDP-Verkehr, entferntem Ping, IP-Fragmenten oder Erweiterungsheadern funktionieren möglicherweise nicht. Wähle L4 manuell; Automatisch wählt es nicht.',
  'l4_unsupported':
      'Diese Usque-Version unterstützt L4 nicht. Suche unter Einstellungen nach Updates.',
  'l4_sni_identity':
      'Wird vom Konto automatisch festgelegt. Der Servername anderer Verbindungsmodi bleibt erhalten.',
  'l4_edge_requires_l4':
      'Am Rand aufgelöstes DNS erfordert L4. Wählen Sie vor dem Wechsel zu Auto, H3 oder H2 einen anderen Proxy-DNS-Modus.',
  'proxy_dns_edge_resolved': 'Cloudflare-Rand (nur L4; keine lokale Abfrage)',
  'l4_verified': 'L4 hat eine App-Verbindung hergestellt',
  'l4_unverified': 'Server verbunden; App-Verbindung noch nicht bestätigt',
  'l4_status_unknown': 'Status der App-Verbindung nicht verfügbar',
  'l4_sessions': 'Sitzungen / Abbau',
  'l4_flows': 'Aktive / wartende Streams',
  'l4_connect': 'CONNECT Erfolg / Fehler / Zeitüberschreitung',
  'l4_buffers': 'Verbrauchtes Anwendungspufferbudget (Byte)',
  'l4_backpressure': 'Sende- / Empfangsgegendruck',
  'l4_tun_flows': 'TUN-TCP / halboffen',
  'l4_udp': 'Abgelehnte UDP-Pakete',
  'l4_dns': 'DNS-Umwandlungen Erfolg / Fehler / Zeitüberschreitung',
  'l4_migration':
      'Durch Migration erhaltene / durch Neuaufbau beendete Streams',
  'l4_na':
      'CONNECT-IP-Adresssteuerung, DATAGRAM-Warteschlangen, inneres Nutzlast-MTU und UDP-Timeout: in L4 nicht zutreffend.',
};

const Map<String, String> kNetworkSettingsDe = <String, String>{
  'settings_applying': 'Gespeichert, wird angewendet',
  'settings_applied': 'Gespeichert und angewendet',
  'settings_deferred':
      'Gespeichert, gilt bei der nächsten manuellen Verbindung',
  'settings_failed': 'Gespeichert, Anwendung fehlgeschlagen',
  'settings_unknown': 'Ergebnis noch nicht bestätigt',
  'settings_saved': 'Gespeichert',
  'settings_unsupported':
      'Beende Usque vollständig, öffne es erneut und speichere nochmals. Suche bei weiteren Fehlern unter Einstellungen nach Updates.',
  'settings_save_failed':
      'Einstellungen konnten nicht gespeichert werden. Ihre Änderungen bleiben erhalten.',
  'settings_reconnect': 'Erneut verbinden',
};

const Map<String, String> kChainDe = <String, String>{
  'duplicate_directive': 'Diese Direktive darf nur einmal vorkommen.',
  'mixed_protocols':
      'Alle remote-Endpunkte müssen denselben TCP- oder UDP-Transport verwenden.',
  'conflicting_protocol':
      'Die remote-Angabe widerspricht der globalen Transporteinstellung.',
  'too_many_endpoints': 'Verwenden Sie höchstens 16 remote-Endpunkte.',
  'conflicting_authentication':
      'CLIENT_CERT widerspricht dem eingebetteten Zertifikat oder dem Authentifizierungsmodus.',
  'serialized_size_limit':
      'Der verschlüsselte Datensatz würde die Speichergrenze überschreiten.',
  'multi_endpoint_unavailable':
      'Aktualisieren Sie die Engine, um Konfigurationen mit mehreren Endpunkten zu verwenden.',
  'candidates': 'Start-Endpunkte',
  'random_order':
      'Endpunkte werden bei jeder Verbindung in einer neuen Zufallsreihenfolge versucht.',
  'file_order': 'Endpunkte werden in der Dateireihenfolge versucht.',
  'attempting': 'Endpunkt wird versucht',
  'actual_endpoint': 'Verbundener Endpunkt',
  'attempt_failures': 'Fehlgeschlagene Versuche',
  'failure_transport': 'Transport getrennt',
  'failure_authentication': 'Authentifizierung',
  'failure_certificate': 'Zertifikat',
  'failure_configuration': 'Konfiguration',
  'failure_address_changed': 'Adresse geändert',
  'failure_protocol': 'Protokoll',
  'failure_cleanup': 'Aufräumen',
  'failure_reason': 'Fehlerursache: {reason}.',
  'manage': 'Verwalten',
  'dns_fallback': 'Tunnel-DNS (OpenVPN kann DNS aushandeln)',
  'dns_unavailable_title': 'Kein DNS über diesen Ausgang',
  'dns_unavailable':
      'Über diesen Ausgang ist kein DNS-Server erreichbar. Verwenden Sie IP-Adressen oder wählen Sie einen anderen Ausgang mit erreichbarem DNS.',
  'authentication_failed':
      'Authentifizierung fehlgeschlagen. Aktualisieren Sie die Anmeldedaten, bevor Sie sich erneut verbinden.',
  'profile_limit': 'Die Konfigurationsbibliothek ist voll (128 Profile).',
  'metadata_limit': 'Die Metadaten der Konfigurationsbibliothek sind voll.',
  'title': 'Kettenproxy',
  'subtitle': 'Wählen Sie einen über WARP erreichten Ausgang.',
  'source': 'Ausgangsart',
  'enable': 'Kettenproxy aktivieren',
  'import_file': 'Datei importieren',
  'paste': 'Konfiguration einfügen',
  'profiles': 'Gespeicherte Konfigurationen',
  'empty': 'Importieren Sie eine Konfiguration, um einen Ausgang zu wählen.',
  'empty_hint_openvpn':
      'Importieren Sie eine .ovpn-Datei oder fügen Sie ihren Text ein. TCP- und UDP-Endpunkte, eingebettete Zertifikate sowie Benutzername und Passwort werden unterstützt.',
  'empty_hint_wireguard':
      'Importieren Sie eine .conf-Datei oder fügen Sie ihren Text ein. Je ein Abschnitt [Interface] und [Peer] wird unterstützt.',
  'import_limits':
      'Konfigurationen müssen UTF-8-Text bis 128 KiB sein. Geräte ohne Dateiauswahl können den Text einfügen.',
  'enable_to_choose':
      'Aktivieren Sie den Kettenproxy, um eine Konfiguration zu wählen.',
  'select_required':
      'Wählen Sie eine gespeicherte Konfiguration aus, bevor Sie sie anwenden.',
  'pending_disable': 'Ausstehend: Kettenproxy deaktivieren',
  'apply_reconnect': 'Anwenden und neu verbinden',
  'requires_connect_ip': 'Erfordert CONNECT-IP',
  'menu': 'Konfigurationsaktionen',
  'preview': 'Konfiguration prüfen',
  'save_import': 'Konfiguration speichern',
  'name': 'Bezeichnung',
  'configuration': 'Konfigurationstext',
  'file_loaded': 'Konfiguration aus Datei geladen ({lines} Zeilen).',
  'username': 'Benutzername',
  'password': 'Passwort',
  'key_password': 'Passwort des privaten Schlüssels',
  'show_password': 'Passwort anzeigen',
  'hide_password': 'Passwort verbergen',
  'credentials': 'Anmeldedaten aktualisieren',
  'rename': 'Umbenennen',
  'delete': 'Löschen',
  'cancel': 'Abbrechen',
  'save': 'Speichern',
  'apply': 'Änderungen anwenden',
  'clear': 'Auswahl aufheben',
  'current': 'Aktuelle Verbindung',
  'saved': 'Gespeicherte Auswahl',
  'draft': 'Ausstehende Auswahl',
  'disconnected': 'Nicht verbunden',
  'disabled': 'Nicht aktiviert',
  'enabled_idle': 'Aktiviert · nicht verbunden',
  'disconnecting': 'Verbindung wird getrennt',
  'file_read_failed': 'Die Konfigurationsdatei konnte nicht gelesen werden.',
  'file_encoding_invalid': 'Die Konfigurationsdatei muss UTF-8-Text sein.',
  'file_busy': 'Eine Dateiauswahl ist bereits geöffnet.',
  'connected': 'Verbunden',
  'connecting': 'Verbindung wird hergestellt',
  'error': 'Verbindung fehlgeschlagen',
  'no_selection': 'Keine Konfiguration ausgewählt',
  'l4': 'Diese Konfiguration erfordert den CONNECT-IP-Modus.',
  'switch_mode': 'Zu CONNECT-IP wechseln und anwenden',
  'unsupported': 'Diese Engine unterstützt diese Ausgangsart nicht.',
  'scope':
      'Bestehende ausdrückliche Direktregeln bleiben gültig. Übriger Datenverkehr nutzt den gewählten Ausgang.',
  'allowed': 'Zulässige Ziele',
  'dns': 'DNS',
  'addresses': 'Tunneladressen',
  'address_family': 'Adressfamilie',
  'transport': 'Übertragung',
  'endpoint': 'Zielserver',
  'restricted':
      'Ziele außerhalb von AllowedIPs werden auf dem Proxy-Pfad blockiert.',
  'delete_confirm':
      'Diese gespeicherte Konfiguration löschen? Die ursprünglich importierte Datei bleibt unverändert.',
  'profile_in_use':
      'Wählen Sie eine andere Konfiguration oder heben Sie die gespeicherte Auswahl auf, bevor Sie diese Konfiguration löschen.',
  'stale_revision':
      'Die Konfiguration hat sich geändert. Aktualisieren Sie die Liste und versuchen Sie es erneut.',
  'secure_storage_failed':
      'Die verschlüsselte Konfiguration konnte nicht gelesen oder gespeichert werden.',
  'invalid_configuration':
      'Die Konfiguration ist ungültig oder enthält nicht unterstützte Optionen.',
  'looks_like_wireguard':
      'Das sieht nach einer WireGuard-Konfiguration aus. Stellen Sie die Ausgangsart auf WireGuard um.',
  'looks_like_openvpn':
      'Das sieht nach einer OpenVPN-Konfiguration aus. Stellen Sie die Ausgangsart auf OpenVPN um.',
  'error_location': '{message} ({field}, Zeile {line})',
  'error_field': '{message} ({field})',
  'file_unavailable':
      'Keine Dateiauswahl verfügbar. Fügen Sie den Konfigurationstext ein.',
  'invalid_size_or_encoding':
      'Verwenden Sie eine UTF-8-Konfiguration von höchstens 128 KiB.',
  'unsupported_directive': 'Diese OpenVPN-Direktive wird nicht unterstützt.',
  'unsupported_or_duplicate_field':
      'Dieses Feld wird nicht unterstützt oder ist doppelt.',
  'unsupported_or_duplicate_section':
      'Verwenden Sie je einen Abschnitt Interface und Peer.',
  'missing_field': 'Ein erforderliches Feld fehlt.',
  'invalid_name':
      'Verwenden Sie einen Namen mit 1 bis 64 Zeichen ohne Steuerzeichen.',
  'invalid_key':
      'Der Schlüssel muss ein gültiger 32-Byte-Base64-Schlüssel sein.',
  'checking': 'Konfiguration wird geprüft…',
  'changed': 'Änderungen gespeichert',
};
