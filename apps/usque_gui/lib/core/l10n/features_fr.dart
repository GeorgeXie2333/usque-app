/// Supplemental feature strings for French.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowFr = <String, String>{
  'cc_label': 'Contrôle de congestion HTTP/3',
  'cc_help': 'S’applique à votre prochaine connexion manuelle.',
  'cc_upgrade': 'Mise à jour de l’Engine requise.',
  'cc_h2': 'HTTP/2 utilise le TCP du système.',
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
  'home_traffic_idle': 'Démarre après la connexion',
  'home_traffic_waiting': 'En attente d’échantillons',
  'home_traffic_unavailable': 'Historique indisponible',
  'home_traffic_stale': 'Échantillons retardés',
  'home_outputs_next': 'Les sorties seront activées après la connexion',
  'home_outputs_retry': 'Sorties configurées pour la prochaine tentative',
  'connection_protection_group': 'Connexion et protection',
  'proxy_routing_group': 'Proxy et routage',
  'application_group': 'L’application',
  'proxy_settings_link':
      'Adresses des écouteurs, ports, authentification et DNS.',
  'proxy_auth_separate':
      'Les identifiants sont enregistrés séparément avec Enregistrer les '
      'identifiants.',
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
      'Le DNS du système physique est sélectionné ; les contrôles de DNS '
      'chiffré ne s’appliquent pas.',
  'nq_finding_unsupported':
      'Le DNS chiffré est indisponible dans cet Engine ; aucun repli en clair '
      'n’est autorisé.',
  'nq_finding_dns_custom_valid':
      'La configuration DNS chiffré personnalisée est valide. Le repli en '
      'clair est désactivé.',
  'nq_finding_stale': 'La lecture est périmée ou le réseau physique a changé.',
  'nq_finding_rtt_high': 'Le temps d’aller-retour mesuré est élevé.',
  'nq_finding_healthy':
      'La mesure locale disponible se situe dans la plage attendue.',
  'nq_finding_loss_high': 'La perte de paquets de l’intervalle est élevée.',
  'nq_finding_queue_pressure':
      'Une file est sous pression ou a enregistré des pertes pendant cette '
      'connexion.',
  'nq_finding_pmtu_degraded': 'La validation de la MTU du chemin est dégradée.',
  'nq_finding_migration_reconnect':
      'La migration est indisponible sur ce chemin ; un changement de réseau '
      'utilise une reconnexion complète.',
  'nq_finding_dns_changed':
      'Le mode DNS enregistré diffère de la connexion en cours.',
  'nq_finding_dns_runtime':
      'Le DNS chiffré a réussi. L’état local n’est pas une preuve externe '
      'd’absence de fuites.',
  'nq_finding_dns_degraded':
      'Le DNS chiffré est dégradé ; les requêtes directes en échec ne '
      'basculent pas vers le DNS système.',
  'nq_finding_probe_unsafe':
      'Sonde ignorée : l’état sûr requis ou l’identité enregistrée est '
      'indisponible. Un tunnel actif n’est jamais dupliqué.',
  'nq_finding_probe_success':
      'La sonde authentifiée est terminée. Ce n’est pas un test externe de '
      'fuites de paquets.',
  'nq_finding_probe_cancelled': 'Sonde annulée et nettoyage demandé.',
  'nq_finding_probe_timeout':
      'La sonde bornée n’a pas terminé avant l’échéance.',
  'nq_finding_probe_failed':
      'La sonde authentifiée a échoué ; aucun repli non sécurisé n’a été '
      'tenté.',
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
      'Cet Engine ne fournit pas la qualité réseau. Vos commandes de connexion '
      'existantes fonctionnent toujours.',
  'nq_empty':
      'Connectez-vous pour voir les mesures. Les valeurs inconnues ne sont pas '
      'affichées comme zéro.',
  'nq_stale_help':
      'La source a cessé de se mettre à jour. Ce sont d’anciennes lectures ; '
      'les trous restent des trous.',
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
      'La découverte du chemin n’augmente pas la MTU TUN de l’appareil.',
  'nq_migration': 'Migration réseau',
  'nq_migration_help':
      'Une connexion, un chemin de données. Même famille d’adresses IP '
      'uniquement ; pas de chemins multiples.',
  'nq_attempts': 'Tentatives',
  'nq_successes': 'Réussies',
  'nq_failures': 'Échouées',
  'nq_last_duration': 'Dernière durée',
  'nq_direct_dns': 'DNS direct',
  'nq_system_dns': 'DNS du système physique',
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
      'La famille d’adresses IP actuelle est indisponible ; une reconnexion '
      'complète est utilisée.',
  'nq_reason_socket_protect_failed':
      'Un socket candidat protégé n’a pas pu être préparé.',
  'nq_reason_generation_changed_during_setup':
      'Le réseau a de nouveau changé pendant la préparation.',
  'nq_reason_peer_cid_unavailable':
      'Le pair n’a pas d’identifiant de connexion de réserve.',
  'nq_reason_local_cid_unavailable':
      'Un identifiant de connexion local est indisponible.',
  'nq_reason_path_probe_rejected': 'Le chemin candidat n’a pas pu être validé.',
  'nq_reason_path_validation_timeout':
      'La validation du chemin a expiré ; la reconnexion est disponible.',
  'nq_reason_superseded':
      'Un changement réseau plus récent a remplacé cette tentative.',
  'nq_reason_promotion_failed':
      'Le changement de chemin n’a pas pu être terminé en toute sécurité.',
  'nq_reason_connection_closed':
      'La connexion s’est fermée pendant la migration.',
  'nq_reason_unsupported': 'La migration est indisponible sur cette connexion.',
  'nq_reason_unknown': 'Aucune raison prise en charge n’est disponible.',
  'nq_dns_custom': 'Résolveur chiffré personnalisé',
  'nq_dns_server': 'Nom de serveur TLS',
  'nq_dns_path': 'Chemin HTTPS',
  'nq_dns_port': 'Port (0 utilise la valeur par défaut)',
  'nq_dns_bootstrap': 'Adresses IP de bootstrap',
  'nq_dns_bootstrap_help':
      'Saisissez 1–8 adresses IP numériques, une par ligne. Aucune résolution '
      'de nom d’hôte n’est utilisée.',
  'nq_dns_no_fallback':
      'Si le DNS direct chiffré échoue, la requête échoue. Il n’y a jamais de '
      'repli vers le DNS système ou en clair.',
  'nq_dns_system_privacy':
      'Le DNS du système physique peut exposer les noms des requêtes directes '
      'au fournisseur DNS du réseau physique.',
  'nq_dns_scope':
      'Utilisé uniquement pour les requêtes directes sélectionnées par Geo. Le '
      'DNS du tunnel est inchangé.',
  'nq_dns_no_capability':
      'Cet Engine ne peut pas utiliser le DNS direct chiffré. Les paramètres '
      'enregistrés sont conservés. Vous pouvez choisir explicitement le DNS '
      'système.',
  'nq_dns_invalid_name':
      'Saisissez un nom DNS sans espaces, syntaxe d’URL ni jokers.',
  'nq_dns_invalid_path':
      'Utilisez un chemin commençant par /, de 256 caractères au plus, sans '
      'requête, fragment ni espace.',
  'nq_dns_invalid_bootstrap':
      'Utilisez 1–8 IP unicast uniques ; pas d’adresse non spécifiée, '
      'multicast, broadcast ou IPv6 link-local.',
  'nq_dns_invalid_port': 'Saisissez 0–65535.',
  'nq_dns_invalid_mode': 'Choisissez un mode DNS pris en charge.',
  'nq_doctor_deep_title': 'Exécuter les contrôles réseau approfondis ?',
  'nq_doctor_deep_body':
      'Les contrôles approfondis peuvent envoyer une requête DNS de test à '
      'votre résolveur configuré et valider un chemin QUIC protégé. Ils durent '
      'au plus 15 secondes, peuvent être annulés, ne créent jamais un second '
      'tunnel porteur de données et ne modifient jamais votre DNS, vos routes, '
      'votre profil ou le transport.',
  'nq_doctor_deep_run': 'Exécuter les contrôles approfondis',
  'nq_doctor_evidence':
      'Les contrôles locaux décrivent la configuration et l’état observé. Ils '
      'ne constituent pas une preuve externe de l’absence de fuites DNS.',
};

const Map<String, String> kWindowsRecoveryFr = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'L’état réseau VPN précédent n’a pas pu être entièrement restauré. '
      'Aucune nouvelle connexion VPN n’a été démarrée. Réessayez la connexion '
      'ou inspectez les diagnostics locaux.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows n’a pas pu restaurer l’état réseau VPN précédent après trois '
      'tentatives automatiques. Réessayez lorsque vous serez prêt, ou '
      'inspectez les diagnostics locaux.',
  'WINDOWS_RECOVERY_BLOCKED':
      'La réparation automatique s’est arrêtée car l’état réseau Windows '
      'précédent n’a pas pu être vérifié en toute sécurité. Redémarrez l’Agent '
      'ou mettez Usque à jour, puis inspectez les diagnostics locaux.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'La récupération réseau Windows prend plus de temps que prévu. Aucune '
      'nouvelle connexion VPN n’a été démarrée. Attendez la fin de la '
      'récupération avant de réessayer.',
  'WINDOWS_RECOVERY_CONFLICT':
      'L’état du réseau a changé ou est encore utilisé par une autre session. '
      'La récupération automatique a été arrêtée pour protéger la connexion '
      'active.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Cet Agent Windows ne prend pas en charge la récupération automatique '
      'sécurisée. Mettez à jour l’application et l’Agent ensemble, puis '
      'réessayez.',
};

const String kWindowsAdapterCleanupFr =
    'L’adaptateur Wintun précédent n’a pas pu être retiré, ou son retrait n’a '
    'pas pu être vérifié. Aucune nouvelle connexion VPN n’a démarré.';
