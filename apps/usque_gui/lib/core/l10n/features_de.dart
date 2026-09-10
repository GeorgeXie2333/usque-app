/// Supplemental feature strings for German.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowDe = <String, String>{
  'cc_label': 'HTTP/3-Überlastkontrolle',
  'cc_help': 'Wird bei der nächsten manuellen Verbindung wirksam.',
  'cc_upgrade': 'Ein Update der Engine ist erforderlich.',
  'cc_h2': 'HTTP/2 verwendet das System-TCP.',
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
  'home_traffic_idle': 'Beginnt nach dem Verbinden',
  'home_traffic_waiting': 'Warten auf Messwerte',
  'home_traffic_unavailable': 'Verlauf nicht verfügbar',
  'home_traffic_stale': 'Messwerte verzögert',
  'home_outputs_next': 'Ausgaben werden nach dem Verbinden aktiviert',
  'home_outputs_retry': 'Ausgaben für den nächsten Versuch konfiguriert',
  'connection_protection_group': 'Verbindung & Schutz',
  'proxy_routing_group': 'Proxy & Weiterleitung',
  'application_group': 'Anwendung',
  'proxy_settings_link': 'Listener-Adressen, Ports, Authentifizierung und DNS.',
  'proxy_auth_separate':
      'Anmeldedaten werden separat mit „Anmeldedaten speichern“ gespeichert.',
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
      'Physisches System-DNS ist ausgewählt; Prüfungen für verschlüsseltes DNS '
      'gelten nicht.',
  'nq_finding_unsupported':
      'Verschlüsseltes DNS ist in dieser Engine nicht verfügbar; ein '
      'Klartext-Rückfall ist nicht zulässig.',
  'nq_finding_dns_custom_valid':
      'Die eigene Konfiguration für verschlüsseltes DNS ist gültig. '
      'Klartext-Rückfall ist deaktiviert.',
  'nq_finding_stale':
      'Der Messwert ist veraltet oder das physische Netzwerk hat sich '
      'geändert.',
  'nq_finding_rtt_high': 'Die gemessene Umlaufzeit ist erhöht.',
  'nq_finding_healthy':
      'Die verfügbare lokale Messung liegt im erwarteten Bereich.',
  'nq_finding_loss_high': 'Der Paketverlust im Intervall ist erhöht.',
  'nq_finding_queue_pressure':
      'Eine Warteschlange steht unter Druck oder hat während dieser Verbindung '
      'Verwürfe aufgezeichnet.',
  'nq_finding_pmtu_degraded':
      'Die Validierung der Pfad-MTU ist beeinträchtigt.',
  'nq_finding_migration_reconnect':
      'Migration ist auf diesem Pfad nicht verfügbar; bei einem '
      'Netzwerkwechsel wird vollständig neu verbunden.',
  'nq_finding_dns_changed':
      'Der gespeicherte DNS-Modus unterscheidet sich von der laufenden '
      'Verbindung.',
  'nq_finding_dns_runtime':
      'Verschlüsseltes DNS war erfolgreich. Der lokale Zustand ist kein '
      'externer Nachweis gegen Lecks.',
  'nq_finding_dns_degraded':
      'Verschlüsseltes DNS ist beeinträchtigt; fehlgeschlagene direkte '
      'Abfragen fallen nicht auf System-DNS zurück.',
  'nq_finding_probe_unsafe':
      'Sonde übersprungen: Der erforderliche sichere Zustand oder die '
      'gespeicherte Identität ist nicht verfügbar. Ein aktiver Tunnel wird '
      'niemals dupliziert.',
  'nq_finding_probe_success':
      'Die authentifizierte Sonde wurde abgeschlossen. Dies ist kein externer '
      'Test auf Paketlecks.',
  'nq_finding_probe_cancelled':
      'Sonde abgebrochen und Bereinigung angefordert.',
  'nq_finding_probe_timeout':
      'Die zeitlich begrenzte Sonde wurde nicht vor Ablauf der Frist '
      'abgeschlossen.',
  'nq_finding_probe_failed':
      'Die authentifizierte Sonde ist fehlgeschlagen; es wurde kein unsicherer '
      'Rückfall versucht.',
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
      'Diese Engine liefert keine Netzwerkqualität. Ihre vorhandenen '
      'Verbindungssteuerungen funktionieren weiterhin.',
  'nq_empty':
      'Verbinden Sie sich, um Messwerte zu sehen. Unbekannte Werte werden '
      'nicht als 0 angezeigt.',
  'nq_stale_help':
      'Die Quelle hat die Aktualisierung eingestellt. Dies sind frühere '
      'Messwerte; Lücken bleiben Lücken.',
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
  'nq_pmtu_help': 'Die Pfadermittlung erhöht die TUN-MTU des Geräts nicht.',
  'nq_migration': 'Netzwerkmigration',
  'nq_migration_help':
      'Eine Verbindung, ein Datenpfad. Nur dieselbe IP-Familie; kein '
      'Multipfad.',
  'nq_attempts': 'Versuche',
  'nq_successes': 'Erfolgreich',
  'nq_failures': 'Fehlgeschlagen',
  'nq_last_duration': 'Letzte Dauer',
  'nq_direct_dns': 'Direktes DNS',
  'nq_system_dns': 'Physisches System-DNS',
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
      'Die aktuelle IP-Familie ist nicht verfügbar; es wird vollständig neu '
      'verbunden.',
  'nq_reason_socket_protect_failed':
      'Ein geschützter Kandidaten-Socket konnte nicht vorbereitet werden.',
  'nq_reason_generation_changed_during_setup':
      'Das Netzwerk hat sich während der Vorbereitung erneut geändert.',
  'nq_reason_peer_cid_unavailable':
      'Der Peer hat keine Ersatz-Verbindungskennung.',
  'nq_reason_local_cid_unavailable':
      'Eine lokale Verbindungskennung ist nicht verfügbar.',
  'nq_reason_path_probe_rejected':
      'Der Kandidatenpfad konnte nicht validiert werden.',
  'nq_reason_path_validation_timeout':
      'Die Pfadvalidierung ist abgelaufen; eine Neuverbindung ist möglich.',
  'nq_reason_superseded':
      'Eine neuere Netzwerkänderung hat diesen Versuch ersetzt.',
  'nq_reason_promotion_failed':
      'Der Pfadwechsel konnte nicht sicher abgeschlossen werden.',
  'nq_reason_connection_closed':
      'Die Verbindung wurde während der Migration geschlossen.',
  'nq_reason_unsupported':
      'Migration ist auf dieser Verbindung nicht verfügbar.',
  'nq_reason_unknown': 'Kein unterstützter Grund ist verfügbar.',
  'nq_dns_custom': 'Eigener verschlüsselter Resolver',
  'nq_dns_server': 'TLS-Servername',
  'nq_dns_path': 'HTTPS-Pfad',
  'nq_dns_port': 'Port (0 verwendet den Standard)',
  'nq_dns_bootstrap': 'Bootstrap-IP-Adressen',
  'nq_dns_bootstrap_help':
      'Geben Sie 1–8 numerische IP-Adressen ein, eine pro Zeile. Es wird keine '
      'Hostnamensauflösung verwendet.',
  'nq_dns_no_fallback':
      'Wenn verschlüsseltes direktes DNS fehlschlägt, schlägt die Anfrage '
      'fehl. Es erfolgt kein Rückfall auf System- oder Klartext-DNS.',
  'nq_dns_system_privacy':
      'Physisches System-DNS kann Namen direkter Abfragen gegenüber dem '
      'DNS-Anbieter des physischen Netzwerks offenlegen.',
  'nq_dns_scope':
      'Wird nur für Geo-ausgewählte direkte Abfragen verwendet. Das Tunnel-DNS '
      'bleibt unverändert.',
  'nq_dns_no_capability':
      'Diese Engine kann kein verschlüsseltes direktes DNS verwenden. '
      'Gespeicherte Einstellungen bleiben erhalten. Sie können ausdrücklich '
      'System-DNS wählen.',
  'nq_dns_invalid_name':
      'Geben Sie einen DNS-Namen ohne Leerzeichen, URL-Syntax oder Platzhalter '
      'ein.',
  'nq_dns_invalid_path':
      'Verwenden Sie einen mit / beginnenden Pfad mit höchstens 256 Zeichen, '
      'ohne Query, Fragment oder Leerzeichen.',
  'nq_dns_invalid_bootstrap':
      'Verwenden Sie 1–8 eindeutige Unicast-IPs; keine unspezifizierten, '
      'Multicast-, Broadcast- oder IPv6-Link-Local-Adressen.',
  'nq_dns_invalid_port': 'Geben Sie 0–65535 ein.',
  'nq_dns_invalid_mode': 'Wählen Sie einen unterstützten DNS-Modus.',
  'nq_doctor_deep_title': 'Tiefe Netzwerkprüfungen ausführen?',
  'nq_doctor_deep_body':
      'Tiefe Prüfungen können eine DNS-Testabfrage an Ihren konfigurierten '
      'Resolver senden und einen geschützten QUIC-Pfad validieren. Sie dauern '
      'höchstens 15 Sekunden, können abgebrochen werden, erzeugen niemals '
      'einen zweiten datentragenden Tunnel und ändern niemals DNS, Route, '
      'Profil oder Transport.',
  'nq_doctor_deep_run': 'Tiefe Prüfungen ausführen',
  'nq_doctor_evidence':
      'Lokale Prüfungen beschreiben Konfiguration und beobachteten Zustand. '
      'Sie sind kein externer Nachweis, dass es keine DNS-Lecks gibt.',
};

const Map<String, String> kWindowsRecoveryDe = <String, String>{
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
      'Die automatische Reparatur wurde gestoppt, weil der vorherige '
      'Windows-Netzwerkzustand nicht sicher überprüft werden konnte. Starten '
      'Sie den Agent neu oder aktualisieren Sie Usque, und prüfen Sie '
      'anschließend die lokale Diagnose.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'Die Wiederherstellung des Windows-Netzwerks dauert länger als erwartet. '
      'Es wurde keine neue VPN-Verbindung gestartet. Warten Sie, bis die '
      'Wiederherstellung abgeschlossen ist, bevor Sie es erneut versuchen.',
  'WINDOWS_RECOVERY_CONFLICT':
      'Der Netzwerkzustand hat sich geändert oder wird noch von einer anderen '
      'Sitzung verwendet. Die automatische Wiederherstellung wurde gestoppt, '
      'um die aktive Verbindung zu schützen.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Dieser Windows-Agent unterstützt keine sichere automatische '
      'Wiederherstellung. Aktualisieren Sie die Anwendung und den Agent '
      'gemeinsam, und versuchen Sie es anschließend erneut.',
};

const String kWindowsAdapterCleanupDe =
    'Der vorherige Wintun-Adapter konnte nicht entfernt werden, oder die '
    'Entfernung konnte nicht bestätigt werden. Es wurde keine neue '
    'VPN-Verbindung gestartet.';

const Map<String, String> kL4De = <String, String>{
  'l4_quic_not_ready': 'Warten auf eine bereite QUIC-Sitzung',
  'l4_unsupported_packets':
      'Nicht unterstützte oder fehlerhafte Pakete abgelehnt',
  'l4_budget_rejections': 'Abgelehnte Ressourcenzulassungen',
  'l4_not_applicable': 'Nicht zutreffend (L4)',
  'l4_mode': 'L4 (experimentell)',
  'l4_transport_hint': 'Nur TCP; TUN-DNS über TCP. Auto enthält kein L4.',
  'l4_explanation':
      'Nur TCP über HTTP/3. Unterstützt VPN/TUN, SOCKS5 und HTTP; TUN-DNS wird in TCP umgewandelt. Auto wählt L4 nie. Anderes UDP, Remote-Ping, IP-Fragmente und Erweiterungsköpfe sind nicht unterstützt; manche Apps funktionieren möglicherweise nicht.',
  'l4_unsupported':
      'Diese Engine hat keine vollständige L4-Unterstützung erklärt. L4 kann nicht aktiviert werden.',
  'l4_sni_identity':
      'Schreibgeschützt: aus der geladenen Kontoidentität abgeleitet. Die vorhandene CONNECT-IP-SNI bleibt erhalten.',
  'l4_edge_requires_l4':
      'Am Rand aufgelöstes DNS erfordert L4. Wählen Sie vor dem Wechsel zu Auto, H3 oder H2 einen anderen Proxy-DNS-Modus.',
  'proxy_dns_edge_resolved': 'Cloudflare-Rand (nur L4; keine lokale Abfrage)',
  'l4_verified': 'L4 CONNECT bestätigt',
  'l4_unverified': 'QUIC bereit; L4 CONNECT noch nicht bestätigt',
  'l4_status_unknown': 'L4-Prüfstatus unbekannt',
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
      'Starten Sie die Engine neu oder aktualisieren Sie sie, um Netzwerkeinstellungen zu speichern.',
  'settings_save_failed':
      'Einstellungen konnten nicht gespeichert werden. Ihre Änderungen bleiben erhalten.',
  'settings_reconnect': 'Erneut verbinden',
};
