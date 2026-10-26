const kProtonEs = <String, String>{
  'fallback_account':
      'No se pudo acceder a Proton. Selecciona una cuenta WARP existente y vuelve a intentarlo.',
  'fallback_failed':
      'No se pudo conectar a través de WARP. Comprueba la cuenta WARP seleccionada y la red, y vuelve a intentarlo.',
  'fallback_starting': 'Conectando a través de WARP…',
  'fallback_active':
      'Esta operación de cuenta usa WARP, incluida la verificación.',
  'connection_api':
      'Ha fallado una solicitud a la API de Proton. Reinténtalo o revisa la cuenta Proton en Proxy en cadena.',
  'connection_authentication':
      'La autorización de Proton ha fallado. Abre Proxy en cadena e inicia sesión de nuevo en Proton.',
  'connection_certificate':
      'Proton ha rechazado el certificado de conexión y la recuperación ha fallado. Vuelve a conectar o inicia sesión de nuevo en Proton.',
  'connection_restricted':
      'El servidor Proton está restringido temporalmente. Reinténtalo o elige otro servidor.',
  'connection_sessions':
      'Se ha alcanzado el límite de conexiones simultáneas de Proton. Desconecta otra sesión Proton y reinténtalo.',
  'connection_key':
      'Proton ha detectado esta clave de conexión en varias sesiones. Desconecta la otra sesión y reinténtalo.',
  'connection_policy':
      'Proton ha restringido esta conexión según su política de cuentas o uso. Revisa la cuenta o elige otro servidor.',
  'connection_server':
      'El servidor Proton no ha podido autorizar la conexión. Prueba otro servidor.',
  'connection_protocol':
      'No se ha podido validar la respuesta de conexión de Proton. Reinténtalo o elige otro servidor.',
  'connection_transport':
      'No se pudo acceder a Proton. Comprueba la red o elige otro servidor.',
  'connection_reconnect':
      'La autorización de Proton se ha renovado. Reconectando…',
  'connection_review':
      'Abre Proxy en cadena para revisar la cuenta Proton y el servidor seleccionado.',
  'connection_retry': 'Reintenta la conexión Proton o elige otro servidor.',
  'title': 'Proton VPN',
  'guest': 'Continuar como invitado',
  'login': 'Iniciar sesión en Proton',
  'username': 'Nombre de usuario',
  'password': 'Contraseña',
  'totp': 'Código de la aplicación de autenticación',
  'submit': 'Continuar',
  'verify': 'Completar la verificación',
  'verification_hint':
      'Resuelve el rompecabezas en la ventana oficial de verificación.',
  'choose_node': 'Elegir un servidor gratuito',
  'working': 'Preparando Proton VPN…',
  'waiting_totp':
      'Introduce el código de tu aplicación de autenticación para continuar.',
  'waiting_verification': 'Verificación necesaria',
  'waiting_node': 'Elige un servidor para finalizar la configuración.',
  'completed':
      'Cuenta guardada. Selecciónala y aplica los cambios para usar Proton VPN.',
  'cancelled': 'Configuración cancelada',
  'failed':
      'La configuración de Proton ha fallado. Vuelve a intentarlo cuando quieras.',
  'result_unconfirmed':
      'El resultado de la configuración de Proton no está confirmado. Actualiza el estado para continuar o cancela la configuración actual antes de empezar de nuevo.',
  'unavailable': 'Proton VPN no está disponible en este motor.',
  'bad_credentials':
      'Comprueba tu nombre de usuario Proton, contraseña o código de autenticación.',
  'rate_limited':
      'Proton ha limitado esta solicitud. Espera antes de volver a intentarlo.',
  'network':
      'No se pudo acceder a Proton. Comprueba la red o la VPN y reinténtalo.',
  'verification_failed':
      'No se ha podido completar la verificación. Inicia la configuración de nuevo.',
  'verification_invalid':
      'Proton ha rechazado el resultado de la verificación. Inicia la configuración de nuevo para obtener un nuevo desafío.',
  'accounts': 'Cuentas Proton',
  'add_account': 'Añadir cuenta',
  'account_actions': 'Acciones de la cuenta',
  'resolve_on_connect':
      'El servidor y la autorización se comprueban al conectar.',
  'account': 'Cuenta Proton',
  'signed_in': 'Sesión iniciada',
  'session_requires_login': 'La sesión requiere autenticación',
  'selected_server': 'Servidor',
  'automatic_server': 'Automático',
  'automatic_hint': 'Elegir el servidor con menor latencia al conectar.',
  'available_servers': 'Servidores disponibles',
  'choose_server': 'Elegir un servidor',
  'search_servers': 'Buscar por nombre, país o ciudad',
  'load_servers': 'Cargar lista de servidores',
  'change_server': 'Cambiar servidor',
  'refresh_servers': 'Actualizar lista',
  'remove_account': 'Eliminar cuenta',
  'remove_account_confirm':
      '¿Eliminar esta cuenta y su selección de servidor guardada? Cerrar sesión también revoca las sesiones guardadas por esta aplicación. Olvidar localmente no cambia las sesiones remotas. Se borrará la selección guardada del proxy en cadena.',
  'forget_local': 'Olvidar localmente',
  'sign_out_remove': 'Cerrar sesión y eliminar',
  'current_account': 'Cuenta conectada',
  'saved_account': 'Selección guardada',
  'next_connection': 'Selección de servidor guardada para la próxima conexión.',
  'disconnect_to_change':
      'Desconecta esta cuenta antes de cambiar su servidor o eliminarla.',
  'cleanup_pending':
      'No se pudo confirmar la revocación de una sesión anterior. Cierra sesión de nuevo o elige explícitamente olvidar la cuenta localmente.',
  'selected_server_unavailable':
      'El servidor seleccionado no está disponible. Elige otro servidor o Automático.',
  'close': 'Cerrar',
  'verification_error': 'Error de verificación',
  'refresh': 'Renovar autorización o cambiar servidor',
  'logout': 'Cerrar sesión y eliminar',
  'forget': 'Olvidar solo en este dispositivo',
  'forget_confirm':
      '¿Eliminar esta configuración Proton y su sesión guardada de este dispositivo? Esto no revoca la sesión Proton remota.',
  'removed': 'La cuenta Proton se ha eliminado de este dispositivo.',
  'needs_login': 'Iniciar sesión de nuevo',
  'expires': 'Caducidad de la autorización',
  'unsupported_second_factor':
      'Esta cuenta requiere un método de inicio de sesión no compatible aquí.',
  'reauthentication_required':
      'Inicia sesión de nuevo para renovar esta sesión Proton.',
  'profile_in_use': 'Desconecta esta cuenta antes de modificarla o eliminarla.',
  'session_busy':
      'Hay otra operación Proton en curso. Espera o cancélala primero.',
  'api_rejected':
      'Proton ha rechazado esta solicitud. Los códigos siguientes identifican la respuesta del servidor.',
  'session_not_allowed': 'Proton no permite que este cliente cree una sesión.',
  'credentialless_disabled':
      'El acceso de invitado a Proton no está disponible actualmente.',
  'no_free_nodes':
      'Proton no ha devuelto servidores WireGuard disponibles para esta cuenta.',
  'invalid_directory':
      'Proton ha devuelto una lista de servidores no compatible.',
  'invalid_client_configuration':
      'Proton no ha devuelto puertos WireGuard utilizables.',
  'stage': 'Paso fallido',
  'stage_anonymous_session': 'Crear una sesión anónima',
  'stage_credentialless': 'Crear una cuenta de invitado',
  'stage_session_scopes': 'Comprobar la autorización del invitado',
  'stage_protected_user': 'Comprobar la cuenta',
  'stage_vpn_permissions': 'Comprobar el acceso VPN',
  'stage_session_refresh': 'Renovar la sesión',
  'stage_srp_info': 'Iniciar la autenticación',
  'stage_srp_proof': 'Verificar la autenticación',
  'stage_totp': 'Verificar el código de autenticación',
  'stage_directory': 'Cargar servidores',
  'stage_certificate': 'Solicitar autorización VPN',
  'stage_secure_storage': 'Guardar la configuración',
  'stage_session_revoke': 'Revocar la sesión',
  'expired':
      'La solicitud de verificación o de entrada ha caducado. Inicia la configuración de nuevo.',
  'underlay_changed':
      'El entorno de red ha cambiado. Inicia la configuración de nuevo.',
  'remote_unknown': 'No se pudo confirmar la revocación de la sesión remota.',
  'cancelled_remote_unknown':
      'Cancelado en este dispositivo. No se pudo confirmar la revocación de la sesión remota.',
  'privacy':
      'Tu contraseña solo se usa para iniciar sesión. Las sesiones y las claves están protegidas en este dispositivo.',
  'empty':
      'Inicia sesión o continúa como invitado para añadir una cuenta Proton.',
};
