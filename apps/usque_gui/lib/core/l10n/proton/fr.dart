const kProtonFr = <String, String>{
  'fallback_account':
      'Proton est inaccessible. Sélectionnez un compte WARP existant et réessayez.',
  'fallback_failed':
      'Connexion via WARP impossible. Vérifiez le compte WARP sélectionné et le réseau, puis réessayez.',
  'fallback_starting': 'Connexion via WARP…',
  'fallback_active':
      'Cette opération de compte passe par WARP, y compris la vérification.',
  'connection_api':
      'Une requête à l’API Proton a échoué. Réessayez ou vérifiez le compte Proton dans Proxy en chaîne.',
  'connection_authentication':
      'L’autorisation Proton a échoué. Ouvrez Proxy en chaîne et reconnectez-vous à votre compte Proton.',
  'connection_certificate':
      'Proton a refusé le certificat de connexion et la récupération a échoué. Relancez la connexion ou reconnectez-vous à votre compte Proton.',
  'connection_restricted':
      'Le serveur Proton est temporairement restreint. Réessayez ou choisissez un autre serveur.',
  'connection_sessions':
      'La limite de connexions simultanées de Proton est atteinte. Déconnectez une autre session Proton et réessayez.',
  'connection_key':
      'Proton a détecté cette clé de connexion dans plusieurs sessions. Déconnectez l’autre session et réessayez.',
  'connection_policy':
      'Proton a restreint cette connexion selon sa politique de compte ou d’utilisation. Vérifiez le compte ou choisissez un autre serveur.',
  'connection_server':
      'Le serveur Proton n’a pas pu autoriser la connexion. Essayez un autre serveur.',
  'connection_protocol':
      'La réponse de connexion de Proton n’a pas pu être validée. Réessayez ou choisissez un autre serveur.',
  'connection_transport':
      'Proton est inaccessible. Vérifiez votre réseau ou choisissez un autre serveur.',
  'connection_reconnect':
      'L’autorisation Proton a été renouvelée. Reconnexion…',
  'connection_review':
      'Ouvrez Proxy en chaîne pour vérifier le compte Proton et le serveur sélectionné.',
  'connection_retry':
      'Réessayez la connexion Proton ou choisissez un autre serveur.',
  'title': 'Proton VPN',
  'guest': 'Continuer en tant qu’invité',
  'login': 'Se connecter à Proton',
  'username': 'Nom d’utilisateur',
  'password': 'Mot de passe',
  'totp': 'Code de l’application d’authentification',
  'submit': 'Continuer',
  'verify': 'Terminer la vérification',
  'verification_hint':
      'Résolvez le puzzle dans la fenêtre de vérification officielle.',
  'choose_node': 'Choisir un serveur gratuit',
  'working': 'Préparation de Proton VPN…',
  'waiting_totp':
      'Saisissez le code de votre application d’authentification pour continuer.',
  'waiting_verification': 'Vérification requise',
  'waiting_node': 'Choisissez un serveur pour terminer la configuration.',
  'completed':
      'Compte enregistré. Sélectionnez-le et appliquez les modifications pour utiliser Proton VPN.',
  'cancelled': 'Configuration annulée',
  'failed':
      'La configuration de Proton a échoué. Réessayez quand vous le souhaitez.',
  'result_unconfirmed':
      'Le résultat de la configuration de Proton n’est pas confirmé. Actualisez l’état pour continuer ou annulez la configuration en cours avant de recommencer.',
  'unavailable': 'Proton VPN n’est pas disponible dans ce moteur.',
  'bad_credentials':
      'Vérifiez votre nom d’utilisateur Proton, votre mot de passe ou votre code d’authentification.',
  'rate_limited':
      'Proton a limité cette requête. Patientez avant de réessayer.',
  'network':
      'Proton est inaccessible. Vérifiez votre réseau ou votre VPN, puis réessayez.',
  'verification_failed':
      'La vérification n’a pas pu être terminée. Recommencez la configuration.',
  'verification_invalid':
      'Proton a refusé le résultat de la vérification. Recommencez la configuration pour obtenir un nouveau défi.',
  'accounts': 'Comptes Proton',
  'add_account': 'Ajouter un compte',
  'account_actions': 'Actions du compte',
  'resolve_on_connect':
      'Le serveur et l’autorisation sont vérifiés lors de la connexion.',
  'account': 'Compte Proton',
  'signed_in': 'Authentifié',
  'session_requires_login': 'Authentification requise pour la session',
  'selected_server': 'Serveur',
  'automatic_server': 'Automatique',
  'automatic_hint':
      'Choisir le serveur à la latence la plus faible à la connexion.',
  'available_servers': 'Serveurs disponibles',
  'choose_server': 'Choisir un serveur',
  'search_servers': 'Rechercher par nom, pays ou ville',
  'load_servers': 'Charger la liste des serveurs',
  'change_server': 'Changer de serveur',
  'refresh_servers': 'Actualiser la liste',
  'remove_account': 'Supprimer le compte',
  'remove_account_confirm':
      'Supprimer ce compte et son choix de serveur enregistré ? La déconnexion révoque aussi les sessions enregistrées par cette application. L’oubli local ne modifie pas les sessions distantes. Le choix enregistré du proxy en chaîne sera effacé.',
  'forget_local': 'Oublier localement',
  'sign_out_remove': 'Se déconnecter et supprimer',
  'current_account': 'Compte connecté',
  'saved_account': 'Choix enregistré',
  'next_connection': 'Choix du serveur enregistré pour la prochaine connexion.',
  'disconnect_to_change':
      'Déconnectez ce compte avant de changer son serveur ou de le supprimer.',
  'cleanup_pending':
      'La révocation d’une session précédente n’a pas pu être confirmée. Déconnectez-vous à nouveau ou choisissez explicitement d’oublier le compte localement.',
  'selected_server_unavailable':
      'Le serveur sélectionné est indisponible. Choisissez un autre serveur ou Automatique.',
  'close': 'Fermer',
  'verification_error': 'Erreur de vérification',
  'refresh': 'Renouveler l’autorisation ou changer de serveur',
  'logout': 'Se déconnecter et supprimer',
  'forget': 'Oublier sur cet appareil uniquement',
  'forget_confirm':
      'Supprimer cette configuration Proton et sa session enregistrée de cet appareil ? Cela ne révoque pas la session Proton distante.',
  'removed': 'Le compte Proton a été supprimé de cet appareil.',
  'needs_login': 'S’authentifier à nouveau',
  'expires': 'Expiration de l’autorisation',
  'unsupported_second_factor':
      'Ce compte nécessite une méthode d’authentification non prise en charge ici.',
  'reauthentication_required':
      'Authentifiez-vous à nouveau pour renouveler cette session Proton.',
  'profile_in_use':
      'Déconnectez ce compte avant de le modifier ou de le supprimer.',
  'session_busy':
      'Une autre opération Proton est en cours. Patientez ou annulez-la d’abord.',
  'api_rejected':
      'Proton a refusé cette requête. Les codes ci-dessous identifient la réponse du serveur.',
  'session_not_allowed': 'Proton n’autorise pas ce client à créer une session.',
  'credentialless_disabled':
      'L’accès invité à Proton est actuellement indisponible.',
  'no_free_nodes':
      'Proton n’a renvoyé aucun serveur WireGuard disponible pour ce compte.',
  'invalid_directory':
      'Proton a renvoyé une liste de serveurs non prise en charge.',
  'invalid_client_configuration':
      'Proton n’a renvoyé aucun port WireGuard utilisable.',
  'stage': 'Étape en échec',
  'stage_anonymous_session': 'Créer une session anonyme',
  'stage_credentialless': 'Créer un compte invité',
  'stage_session_scopes': 'Vérifier l’autorisation de l’invité',
  'stage_protected_user': 'Vérifier le compte',
  'stage_vpn_permissions': 'Vérifier l’accès VPN',
  'stage_session_refresh': 'Renouveler la session',
  'stage_srp_info': 'Démarrer l’authentification',
  'stage_srp_proof': 'Vérifier l’authentification',
  'stage_totp': 'Vérifier le code d’authentification',
  'stage_directory': 'Charger les serveurs',
  'stage_certificate': 'Demander l’autorisation VPN',
  'stage_secure_storage': 'Enregistrer la configuration',
  'stage_session_revoke': 'Révoquer la session',
  'expired':
      'La demande de vérification ou de saisie a expiré. Recommencez la configuration.',
  'underlay_changed':
      'Le contexte réseau a changé. Recommencez la configuration.',
  'remote_unknown':
      'La révocation de la session distante n’a pas pu être confirmée.',
  'cancelled_remote_unknown':
      'Annulé sur cet appareil. La révocation de la session distante n’a pas pu être confirmée.',
  'privacy':
      'Votre mot de passe sert uniquement à l’authentification. Les sessions et les clés sont protégées sur cet appareil.',
  'empty':
      'Authentifiez-vous ou continuez en tant qu’invité pour ajouter un compte Proton.',
};
