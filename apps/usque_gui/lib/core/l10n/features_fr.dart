/// Supplemental feature strings for French.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowFr = <String, String>{
  'local_proxy_settings': 'Paramètres du proxy local',
  'proxy_switches_hint':
      'Les interrupteurs sont enregistrés automatiquement. Validez les modifications d’écoute et DNS avec Appliquer les modifications.',
  'cc_label': 'Contrôle de congestion HTTP/3',
  'cc_help': 'S’applique à votre prochaine connexion manuelle.',
  'cc_upgrade':
      'Mettez Usque à jour dans Paramètres pour utiliser cette option.',
  'cc_h2': 'Cette option ne concerne que les connexions HTTP/3.',
  'cc_saved': 'Enregistré',
  'cc_pending': 'En attente de la prochaine connexion manuelle.',
  'save_changes': 'Appliquer les modifications',
  'saving_changes': 'Application des modifications…',
  'unsaved_changes': 'Modifications non appliquées',
  'changes_applied': 'Modifications appliquées',
  'changes_apply_hint':
      'Les modifications ne prennent effet qu’après les avoir appliquées.',
  'changes_failed':
      'Impossible d’appliquer les modifications. Vérifiez les valeurs '
      'enregistrées, puis réessayez.',
  'form_errors':
      'Vérifiez les champs mis en évidence avant d’appliquer les '
      'modifications.',
  'discard_changes_title': 'Abandonner les modifications non appliquées ?',
  'discard_changes_body':
      'Vos modifications n’ont pas été appliquées. Continuez à modifier pour '
      'les enregistrer, ou abandonnez-les pour quitter.',
  'keep_editing': 'Continuer la modification',
  'discard_changes': 'Abandonner les modifications',
  'invalid_port': 'Saisissez un port compris entre 1 et 65535.',
  'listener_exposure':
      'Les adresses des écouteurs autorisent l’accès depuis le réseau local',
  'invalid_ipv4': 'Saisissez une adresse IPv4 valide, par exemple 127.0.0.1.',
  'invalid_ipv6': 'Saisissez une adresse IPv6 valide, par exemple ::1.',
  'output_running': 'En cours',
  'output_waiting': 'Activé · non démarré',
  'output_disabled': 'Désactivé',
  'output_starting': 'Démarrage',
  'output_stopping': 'Arrêt',
  'output_reconnecting': 'Reconnexion',
  'output_degraded': 'Limité',
  'output_error': 'Erreur',
  'output_unknown': 'État indisponible',
  'shared_network_scope':
      'Les paramètres réseau sont partagés par tous les comptes.',
  'connection_details': 'Détails de la connexion',
  'home_overview': 'Aperçu de la connexion',
  'home_exit_region': 'Région de sortie',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Trafic',
  'home_traffic_window': '60 dernières secondes',
  'home_traffic_idle': 'Le trafic s’affiche après connexion',
  'home_traffic_waiting': 'En attente des données de trafic',
  'home_traffic_unavailable': 'Aucun historique de trafic',
  'home_traffic_stale': 'Mise à jour du trafic retardée',
  'home_outputs_next': 'Disponible après connexion',
  'home_outputs_retry': 'VPN et proxys de la prochaine connexion',
  'connection_protection_group': 'Connexion et protection',
  'proxy_routing_group': 'Proxy et routage',
  'application_group': 'L’application',
  'proxy_settings_link':
      'Adresses des écouteurs, ports, authentification et DNS.',
  'proxy_auth_separate':
      'Utilisez Enregistrer le nom et le mot de passe ci-dessous pour appliquer ces modifications.',
  'reset_draft_hint':
      'Les valeurs par défaut seront chargées dans ce formulaire. Appliquez '
      'les modifications pour qu’elles prennent effet.',
};

const Map<String, String> kNetworkQualityFr = <String, String>{
  'nq_range': 'Plage',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Temps d’aller-retour',
  'diag_check_quality_packet_loss': 'Perte de paquets',
  'diag_check_quality_queue_pressure': 'Pression des files',
  'diag_check_quality_pmtu': 'MTU du chemin',
  'diag_check_transport_migration_capability': 'Migration dans la même famille',
  'diag_check_dns_direct_encrypted_configuration':
      'Configuration du DNS direct',
  'diag_check_dns_direct_encrypted_runtime_state': 'Exécution du DNS direct',
  'diag_check_dns_direct_encrypted_reachability':
      'Accessibilité du DNS chiffré',
  'diag_check_transport_h3_path_validation_probe':
      'Poignée de main QUIC isolée',
  'nq_finding_unavailable':
      'Cette mesure n’est pas disponible dans l’état actuel.',
  'nq_finding_invalid_configuration':
      'La configuration DNS personnalisée n’est pas valide.',
  'nq_finding_dns_system':
      'Le DNS du réseau actuel est utilisé ; les contrôles du DNS chiffré ne s’appliquent pas.',
  'nq_finding_unsupported':
      'Mettez Usque à jour pour utiliser un DNS chiffré. Les requêtes ne passeront pas à un DNS non chiffré.',
  'nq_finding_dns_custom_valid':
      'La configuration DNS chiffré personnalisée est valide. Le repli en '
      'clair est désactivé.',
  'nq_finding_stale': 'La lecture est périmée ou le réseau physique a changé.',
  'nq_finding_rtt_high': 'Le temps d’aller-retour mesuré est élevé.',
  'nq_finding_healthy':
      'Les mesures de connexion disponibles sont dans les valeurs normales.',
  'nq_finding_loss_high': 'La perte de paquets de l’intervalle est élevée.',
  'nq_finding_queue_pressure':
      'Du trafic attend son envoi ou des données ont été abandonnées pendant cette connexion.',
  'nq_finding_pmtu_degraded':
      'Usque n’a pas pu confirmer une taille de paquet adaptée à cette connexion.',
  'nq_finding_migration_reconnect':
      'Cette connexion doit être rétablie lorsque vous changez de réseau.',
  'nq_finding_dns_changed':
      'Le mode DNS enregistré diffère de la connexion en cours.',
  'nq_finding_dns_runtime': 'Le DNS chiffré fonctionne.',
  'nq_finding_dns_degraded':
      'Le DNS chiffré rencontre des problèmes. Les requêtes en échec ne seront pas envoyées au DNS non chiffré du réseau.',
  'nq_finding_probe_unsafe':
      'Cette mesure n’est pas disponible dans l’état actuel.',
  'nq_finding_probe_success': 'Ce contrôle a réussi.',
  'nq_finding_probe_cancelled': 'Ce contrôle a été annulé.',
  'nq_finding_probe_timeout': 'Contrôle de diagnostic expiré',
  'nq_finding_probe_failed': 'Ce contrôle a échoué.',
  'diag_fix_nq_profile':
      'Examinez les champs DNS personnalisés et le nom du certificat. Ne '
      'désactivez pas la vérification TLS.',
  'diag_fix_nq_retry': 'Attendez un réseau stable, puis réessayez.',
  'diag_fix_nq_network':
      'Vérifiez la connectivité locale et comparez un nouvel échantillon avant '
      'de modifier les paramètres.',
  'diag_fix_nq_reconnect':
      'Reconnectez-vous pour appliquer la configuration enregistrée.',
  'nav_network_quality': 'Qualité',
  'network_quality': 'Qualité du réseau',
  'nq_subtitle': 'Observez la connexion, pas seulement le débit.',
  'nq_local_only': 'Mesures locales uniquement. Rien n’est envoyé.',
  'nq_doctor': 'Lancer le diagnostic réseau',
  'nq_doctor_help':
      'Les contrôles standard ne lisent que l’état local. Ils n’ouvrent pas de '
      'connexions externes et ne modifient pas vos paramètres.',
  'nq_live': 'En direct',
  'nq_stale': 'Lectures périmées',
  'nq_updated': 'Dernier échantillon',
  'nq_seconds': 'il y a {count} s',
  'nq_good': 'Bon',
  'nq_fair': 'Moyen',
  'nq_poor': 'Faible',
  'nq_limited': 'Données limitées',
  'nq_disconnected': 'Déconnecté',
  'nq_connecting': 'Connexion',
  'nq_connected': 'Connecté',
  'nq_unavailable': 'Non disponible',
  'nq_not_ready': 'Pas prêt',
  'nq_unsupported': 'Non pris en charge',
  'nq_capability_missing':
      'Cette version ne montre pas la qualité de connexion. Vous pouvez toujours vous connecter et vous déconnecter. Recherchez une mise à jour dans Paramètres.',
  'nq_empty': 'Connectez-vous pour voir les mesures.',
  'nq_stale_help':
      'Les mises à jour sont interrompues. Les dernières mesures sont affichées.',
  'nq_rtt': 'Temps d’aller-retour',
  'nq_latest': 'Plus récent',
  'nq_smoothed': 'Lissé',
  'nq_minimum': 'Valeur minimale',
  'nq_h2_ping': 'PING du protocole HTTP/2',
  'nq_h3_rtt': 'Mesure de chemin QUIC',
  'nq_throughput': 'Débit',
  'nq_download': 'Téléchargement',
  'nq_upload': 'Envoi',
  'nq_one_second': '1 seconde',
  'nq_five_seconds': 'Moyenne sur 5 secondes',
  'nq_loss': 'Perte de paquets',
  'nq_loss_h2': 'HTTP/2 n’expose pas de perte de paquets comparable.',
  'nq_loss_interval':
      'Mesuré sur le dernier intervalle ; pas une perte cumulée.',
  'nq_congestion': 'Encombrement',
  'nq_cwnd': 'Fenêtre de congestion',
  'nq_in_flight': 'Bytes en transit',
  'nq_send_rate': 'Débit de livraison',
  'nq_h2_window': 'Fenêtres de réception HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Connexion',
  'nq_stalls': 'Blocages de capacité',
  'nq_pmtu': 'MTU du chemin',
  'nq_outer_pmtu': 'Limite de charge utile UDP externe',
  'nq_inner_payload': 'Limite de charge utile CONNECT-IP',
  'nq_pmtu_help':
      'Il s’agit de la taille des paquets que le chemin réseau peut transporter. Usque la vérifie automatiquement pour réduire les pertes. Ce contrôle n’augmente pas la MTU du VPN définie dans Paramètres réseau avancés.',
  'nq_migration': 'Migration réseau',
  'nq_migration_help':
      'Usque essaie de maintenir la connexion lors d’un changement de réseau, par exemple du Wi-Fi aux données mobiles. Les deux réseaux doivent utiliser la même version IP, IPv4 ou IPv6. Un seul réseau transporte le trafic à la fois ; leurs débits ne s’additionnent pas.',
  'nq_attempts': 'Tentatives',
  'nq_successes': 'Réussies',
  'nq_failures': 'Échouées',
  'nq_last_duration': 'Dernière durée',
  'nq_direct_dns': 'DNS direct',
  'nq_system_dns': 'DNS du réseau actuel',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Prêt',
  'nq_degraded': 'Dégradé',
  'nq_timeouts': 'Délais dépassés',
  'nq_last_rtt': 'Dernier RTT',
  'nq_dns_redacted':
      'Les noms des résolveurs et les adresses de bootstrap ne sont affichés '
      'que dans Paramètres.',
  'nq_queues': 'Pression des files',
  'nq_queue_details': 'Files de bas niveau',
  'nq_queue_empty': 'Pas encore de mesures de file.',
  'nq_current_capacity': 'Actuel / capacité',
  'nq_high_water': 'Niveau maximal',
  'nq_drops': 'Pertes',
  'nq_oldest': 'Élément le plus ancien',
  'nq_tunToTransport': 'Appareil → transport',
  'nq_proxyToTransport': 'Proxy → couche de transport',
  'nq_transportOutgoing': 'Sortie du transport',
  'nq_h3DatagramSend': 'Datagrammes QUIC',
  'nq_h3WireSend': 'Sortie UDP',
  'nq_transportToTun': 'Transport → appareil',
  'nq_transportToProxy': 'Couche de transport → proxy',
  'nq_directDns': 'Requêtes DNS directes',
  'nq_unknown_queue': 'Autre file',
  'nq_trends': '60 dernières secondes',
  'nq_samples': 'échantillons',
  'nq_pause': 'Mettre les graphiques en pause',
  'nq_resume': 'Reprendre les graphiques',
  'nq_paused': 'Graphiques en pause',
  'nq_gaps': 'Les échantillons manquants sont des trous.',
  'nq_phase_idle': 'Inactif',
  'nq_phase_preparing_socket': 'Préparation du chemin',
  'nq_phase_probing': 'Sondage',
  'nq_phase_validated': 'Validé',
  'nq_phase_promoting': 'Changement de chemin',
  'nq_phase_stable': 'Stabilisé',
  'nq_phase_aborted': 'Interrompu',
  'nq_phase_revalidating': 'Revalidation',
  'nq_phase_degraded': 'Dégradé',
  'nq_phase_unknown': 'Pas prêt',
  'nq_phase_unsupported': 'Non pris en charge',
  'nq_reason_family_unavailable':
      'Le nouveau réseau ne peut pas utiliser la même version IP. Une reconnexion est nécessaire.',
  'nq_reason_socket_protect_failed':
      'Usque n’a pas pu utiliser le nouveau réseau en sécurité. Si la connexion ne revient pas, reconnectez-vous manuellement.',
  'nq_reason_generation_changed_during_setup':
      'Le réseau a de nouveau changé pendant la préparation.',
  'nq_reason_peer_cid_unavailable':
      'Le serveur n’a pas pu maintenir la connexion sur le nouveau réseau. Si elle ne revient pas, reconnectez-vous manuellement.',
  'nq_reason_local_cid_unavailable':
      'Usque n’a pas pu maintenir la connexion sur le nouveau réseau. Si elle ne revient pas, reconnectez-vous manuellement.',
  'nq_reason_path_probe_rejected':
      'Le nouveau réseau n’a pas réussi le contrôle de connexion. Vérifiez son accès à Internet.',
  'nq_reason_path_validation_timeout':
      'Le nouveau réseau n’a pas répondu à temps. Vérifiez-le et reconnectez-vous si nécessaire.',
  'nq_reason_superseded':
      'Le réseau a encore changé avant la fin du basculement.',
  'nq_reason_promotion_failed':
      'Usque n’a pas pu terminer le changement de réseau en sécurité. Si la connexion ne revient pas, reconnectez-vous manuellement.',
  'nq_reason_connection_closed':
      'La connexion s’est fermée pendant le changement de réseau. Reconnectez-vous.',
  'nq_reason_unsupported': 'La migration est indisponible sur cette connexion.',
  'nq_reason_unknown': 'Aucune raison prise en charge n’est disponible.',
  'nq_dns_custom': 'Résolveur chiffré personnalisé',
  'nq_dns_server': 'Domaine du serveur DNS',
  'nq_dns_path': 'Chemin HTTPS',
  'nq_dns_port': 'Port (0 utilise la valeur par défaut)',
  'nq_dns_bootstrap': 'Adresses IP du serveur DNS',
  'nq_dns_bootstrap_help':
      'Saisissez 1 à 8 IP fournies par votre service DNS, une par ligne, par exemple 1.1.1.1. Usque s’y connecte directement sans rechercher d’abord le nom du serveur.',
  'nq_dns_no_fallback':
      'Si le DNS direct chiffré échoue, la requête échoue. Il n’y a jamais de '
      'repli vers le DNS système ou en clair.',
  'nq_dns_system_privacy':
      'Le fournisseur DNS du réseau actuel peut voir les domaines demandés par le trafic direct.',
  'nq_dns_scope':
      'S’applique au trafic correspondant aux règles de pays en connexion directe. Le DNS du trafic VPN reste inchangé.',
  'nq_dns_no_capability':
      'Mettez Usque à jour pour utiliser le DNS chiffré des connexions directes. Vos réglages sont conservés. Vous pouvez choisir DNS du réseau actuel si vous acceptez les conséquences pour la confidentialité.',
  'nq_dns_invalid_name':
      'Saisissez un domaine comme dns.example.com, sans https://, port ni espaces.',
  'nq_dns_invalid_path':
      'Saisissez un chemin comme /dns-query, de 256 caractères maximum. Retirez les espaces et les parties commençant par ? ou #.',
  'nq_dns_invalid_bootstrap': 'Saisissez entre 1 et 8 adresses IP du serveur.',
  'nq_dns_invalid_port':
      'Saisissez un port de 1 à 65535, ou 0 pour la valeur par défaut.',
  'nq_dns_invalid_mode': 'Choisissez un mode DNS pris en charge.',
  'nq_doctor_deep_title': 'Exécuter les contrôles réseau approfondis ?',
  'nq_doctor_deep_body':
      'Les vérifications peuvent envoyer du trafic de test. Elles durent au maximum 15 secondes et peuvent être annulées. Vos paramètres de connexion restent inchangés.',
  'nq_doctor_deep_run': 'Exécuter les contrôles approfondis',
  'nq_doctor_evidence':
      'Ces vérifications ne permettent pas de confirmer la présence de fuites DNS.',
};

const Map<String, String> kWindowsRecoveryFr = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Les composants de connexion d’Usque doivent être mis à jour ensemble. Recherchez une mise à jour dans Paramètres. Aucune nouvelle connexion VPN n’a été lancée.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'Le nettoyage de la connexion VPN précédente n’est pas terminé. Quittez complètement Usque puis rouvrez-le. Si le problème persiste, ouvrez Diagnostics.',
  'WINDOWS_RECOVERY_FAILED':
      'L’état réseau VPN précédent n’a pas pu être entièrement restauré. '
      'Aucune nouvelle connexion VPN n’a été démarrée. Réessayez la connexion '
      'ou inspectez les diagnostics locaux.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows n’a pas pu restaurer l’état réseau VPN précédent après trois '
      'tentatives automatiques. Réessayez lorsque vous serez prêt, ou '
      'inspectez les diagnostics locaux.',
  'WINDOWS_RECOVERY_BLOCKED':
      'La réparation automatique s’est arrêtée car la restauration sûre des anciens paramètres VPN n’a pas pu être confirmée. Recherchez une mise à jour dans Paramètres ; si le problème persiste, exportez un paquet depuis Diagnostics.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'La récupération réseau Windows prend plus de temps que prévu. Aucune '
      'nouvelle connexion VPN n’a été démarrée. Attendez la fin de la '
      'récupération avant de réessayer.',
  'WINDOWS_RECOVERY_CONFLICT':
      'L’état du réseau a changé ou est encore utilisé par une autre session. '
      'La récupération automatique a été arrêtée pour protéger la connexion '
      'active.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Cette installation ne peut pas restaurer automatiquement les anciens paramètres VPN. Mettez Usque à jour dans Paramètres avant de réessayer.',
};

const String kWindowsAdapterCleanupFr =
    'L’adaptateur réseau virtuel de la connexion précédente n’a pas pu être supprimé, ou sa suppression n’a pas été confirmée. Aucune nouvelle connexion VPN n’a été lancée.';

const Map<String, String> kL4Fr = <String, String>{
  'l4_quic_not_ready': 'Préparation de la connexion L4',
  'l4_unsupported_packets': 'Paquets non pris en charge ou mal formés rejetés',
  'l4_budget_rejections': 'Admissions de ressources refusées',
  'l4_not_applicable': 'Sans objet (L4)',
  'l4_mode': 'L4 (expérimental)',
  'l4_transport_hint':
      'TCP uniquement. Les applications nécessitant UDP peuvent ne pas fonctionner. Le mode automatique exclut L4.',
  'l4_explanation':
      'L4 transporte le trafic TCP via HTTP/3 et fonctionne avec le VPN et les proxys SOCKS5 et HTTP. Les requêtes DNS du VPN sont converties en TCP. Les applications ayant besoin d’autres flux UDP, de Ping distant, de fragments IP ou d’en-têtes d’extension peuvent ne pas fonctionner. Choisissez L4 manuellement ; le mode automatique ne le sélectionne pas.',
  'l4_unsupported':
      'L4 n’est pas disponible dans cette version d’Usque. Recherchez une mise à jour dans Paramètres.',
  'l4_sni_identity':
      'Défini automatiquement par le compte. Le nom de serveur des autres modes de connexion est conservé.',
  'l4_edge_requires_l4':
      'Le DNS résolu en bordure exige L4. Choisissez un autre mode DNS proxy avant de passer à Auto, H3 ou H2.',
  'proxy_dns_edge_resolved':
      'Bordure Cloudflare (L4 uniquement ; pas de requête locale)',
  'l4_verified': 'L4 a réussi à établir une connexion d’application',
  'l4_unverified':
      'Serveur connecté ; aucune connexion d’application encore confirmée',
  'l4_status_unknown': 'État de connexion des applications indisponible',
  'l4_sessions': 'Sessions / vidage',
  'l4_flows': 'Flux actifs / en attente',
  'l4_connect': 'CONNECT réussites / échecs / délais dépassés',
  'l4_buffers': 'Budget de tampon applicatif utilisé (octets)',
  'l4_backpressure': 'Contre-pression d’envoi / de réception',
  'l4_tun_flows': 'TUN TCP / semi-ouvert',
  'l4_udp': 'Paquets UDP rejetés',
  'l4_dns': 'Conversions DNS réussites / échecs / délais dépassés',
  'l4_migration': 'Flux conservés par migration / terminés par reconstruction',
  'l4_na':
      'Contrôle d’adresse CONNECT-IP, files DATAGRAM, MTU de charge utile interne et délai UDP : sans objet en L4.',
};

const Map<String, String> kNetworkSettingsFr = <String, String>{
  'settings_applying': 'Enregistré, application en cours',
  'settings_applied': 'Enregistré et appliqué',
  'settings_deferred':
      'Enregistré, prend effet à la prochaine connexion manuelle',
  'settings_failed': 'Enregistré, échec de l’application',
  'settings_unknown': 'Résultat pas encore confirmé',
  'settings_saved': 'Enregistré',
  'settings_unsupported':
      'Quittez complètement Usque, rouvrez-le puis enregistrez à nouveau. Si cela échoue, recherchez une mise à jour dans Paramètres.',
  'settings_save_failed':
      'Les paramètres n’ont pas pu être enregistrés. Vos modifications sont conservées.',
  'settings_reconnect': 'Reconnecter',
};

const Map<String, String> kChainFr = <String, String>{
  'duplicate_directive': 'Cette directive ne peut apparaître qu’une fois.',
  'mixed_protocols':
      'Tous les points remote doivent utiliser le même transport TCP ou UDP.',
  'conflicting_protocol':
      'Le remote entre en conflit avec le transport global.',
  'too_many_endpoints': 'Utilisez au plus 16 points remote.',
  'conflicting_authentication':
      'CLIENT_CERT entre en conflit avec le certificat intégré ou le mode d’authentification.',
  'serialized_size_limit':
      'L’enregistrement chiffré dépasserait la limite de stockage.',
  'multi_endpoint_unavailable':
      'Mettez le moteur à jour pour utiliser des configurations à plusieurs points.',
  'candidates': 'Points de démarrage',
  'random_order':
      'Les points sont essayés dans un nouvel ordre aléatoire à chaque connexion.',
  'file_order': 'Les points sont essayés dans l’ordre du fichier.',
  'attempting': 'Point en cours d’essai',
  'actual_endpoint': 'Point connecté',
  'attempt_failures': 'Essais échoués',
  'failure_transport': 'transport fermé',
  'failure_authentication': 'authentification',
  'failure_certificate': 'certificat',
  'failure_configuration': 'paramétrage',
  'failure_address_changed': 'adresse modifiée',
  'failure_protocol': 'protocole',
  'failure_cleanup': 'nettoyage',
  'failure_reason': 'Échec : {reason}.',
  'manage': 'Gérer',
  'dns_fallback': 'DNS du tunnel (OpenVPN peut négocier le DNS)',
  'dns_unavailable_title': 'Pas de DNS par cette sortie',
  'dns_unavailable':
      'Aucun serveur DNS n’est joignable par cette sortie. Utilisez des adresses IP ou réimportez une configuration avec un DNS joignable.',
  'authentication_failed':
      'Échec de l’authentification. Mettez à jour les identifiants avant de vous reconnecter.',
  'profile_limit':
      'La bibliothèque de configurations est pleine (128 profils).',
  'metadata_limit':
      'Les métadonnées de la bibliothèque de configurations sont pleines.',
  'title': 'Proxy en chaîne',
  'subtitle': 'Choisissez une sortie atteinte via WARP.',
  'source': 'Source de sortie',
  'enable': 'Activer le proxy en chaîne',
  'import_file': 'Importer un fichier',
  'paste': 'Coller la configuration',
  'profiles': 'Configurations enregistrées',
  'empty': 'Importez une configuration pour choisir une sortie.',
  'empty_hint_openvpn':
      'Importez un fichier .ovpn ou collez son texte. Les points TCP et UDP, les certificats intégrés et l’identifiant avec mot de passe sont pris en charge.',
  'empty_hint_wireguard':
      'Importez un fichier .conf ou collez son texte. Une section [Interface] et une section [Peer] sont prises en charge.',
  'import_limits':
      'Les configurations doivent être du texte UTF-8 d’au plus 128 KiB. Sans sélecteur de fichiers, collez le texte.',
  'enable_to_choose':
      'Activez le proxy en chaîne pour choisir une configuration.',
  'select_required':
      'Choisissez une configuration enregistrée avant de l’appliquer.',
  'pending_disable': 'En attente : désactiver le proxy en chaîne',
  'apply_reconnect': 'Appliquer et reconnecter',
  'requires_connect_ip': 'Nécessite CONNECT-IP',
  'menu': 'Actions de la configuration',
  'preview': 'Vérifier la configuration',
  'save_import': 'Enregistrer la configuration',
  'name': 'Nom',
  'configuration': 'Texte de la configuration',
  'file_loaded': 'Configuration chargée depuis le fichier ({lines} lignes).',
  'username': 'Nom d’utilisateur',
  'password': 'Mot de passe',
  'key_password': 'Mot de passe de la clé privée',
  'show_password': 'Afficher le mot de passe',
  'hide_password': 'Masquer le mot de passe',
  'credentials': 'Mettre à jour les identifiants',
  'rename': 'Renommer',
  'delete': 'Supprimer',
  'cancel': 'Annuler',
  'save': 'Enregistrer',
  'apply': 'Appliquer les modifications',
  'clear': 'Effacer la sélection',
  'current': 'Connexion actuelle',
  'saved': 'Sélection enregistrée',
  'draft': 'Sélection en attente',
  'disconnected': 'Non connecté',
  'disabled': 'Non activé',
  'enabled_idle': 'Activé · non connecté',
  'disconnecting': 'Déconnexion',
  'file_read_failed': 'Impossible de lire le fichier de configuration.',
  'file_encoding_invalid':
      'Le fichier de configuration doit être du texte UTF-8.',
  'file_busy': 'Un sélecteur de fichiers est déjà ouvert.',
  'connected': 'Connecté',
  'connecting': 'Connexion en cours',
  'error': 'Échec de la connexion',
  'no_selection': 'Aucune configuration sélectionnée',
  'l4': 'Cette configuration nécessite le mode CONNECT-IP.',
  'switch_mode': 'Passer à CONNECT-IP et appliquer',
  'unsupported': 'Ce moteur ne prend pas en charge cette source de sortie.',
  'scope':
      'Les règles directes explicites restent en vigueur. Le reste du trafic utilise la sortie choisie.',
  'allowed': 'Destinations autorisées',
  'dns': 'DNS',
  'addresses': 'Adresses du tunnel',
  'address_family': 'Famille d’adresses',
  'transport': 'Acheminement',
  'endpoint': 'Serveur',
  'restricted':
      'Les destinations hors de AllowedIPs sont bloquées sur le chemin du proxy.',
  'delete_confirm':
      'Supprimer cette configuration enregistrée ? Le fichier importé d’origine reste inchangé.',
  'profile_in_use':
      'Choisissez une autre configuration ou effacez la sélection enregistrée avant de supprimer celle-ci.',
  'stale_revision':
      'La configuration a changé. Actualisez la liste et réessayez.',
  'secure_storage_failed':
      'La configuration chiffrée n’a pas pu être lue ni enregistrée.',
  'invalid_configuration':
      'La configuration est invalide ou contient des options non prises en charge.',
  'looks_like_wireguard':
      'Cela ressemble à une configuration WireGuard. Passez la source de sortie à WireGuard (Custom).',
  'looks_like_openvpn':
      'Cela ressemble à une configuration OpenVPN. Passez la source de sortie à OpenVPN (Custom).',
  'error_location': '{message} ({field}, ligne {line})',
  'error_field': '{message} ({field})',
  'file_unavailable':
      'Aucun sélecteur de fichiers n’est disponible. Collez le texte de la configuration.',
  'invalid_size_or_encoding':
      'Utilisez une configuration UTF-8 d’au plus 128 KiB.',
  'unsupported_directive': 'Cette directive OpenVPN n’est pas prise en charge.',
  'unsupported_or_duplicate_field':
      'Ce champ n’est pas pris en charge ou est en double.',
  'unsupported_or_duplicate_section':
      'Utilisez une seule section Interface et une seule section Peer.',
  'missing_field': 'Un champ obligatoire est absent.',
  'invalid_key': 'La clé doit être une clé Base64 valide de 32 octets.',
  'checking': 'Vérification de la configuration…',
  'changed': 'Modifications enregistrées',
};
