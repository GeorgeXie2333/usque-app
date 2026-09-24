/// Supplemental feature strings for Spanish.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowEs = <String, String>{
  'local_proxy_settings': 'Configuración del proxy local',
  'proxy_switches_hint':
      'Los interruptores se guardan automáticamente. Aplica las ediciones de escucha y DNS con Aplicar cambios.',
  'cc_label': 'Control de congestión HTTP/3',
  'cc_help': 'Se aplica en la próxima conexión manual.',
  'cc_upgrade': 'Actualiza Usque desde Ajustes para usar esta opción.',
  'cc_h2': 'Esta opción solo afecta a las conexiones HTTP/3.',
  'cc_saved': 'Guardado',
  'cc_pending': 'Pendiente de la próxima conexión manual.',
  'save_changes': 'Aplicar cambios',
  'saving_changes': 'Aplicando cambios…',
  'unsaved_changes': 'Cambios sin aplicar',
  'changes_applied': 'Cambios aplicados',
  'changes_apply_hint':
      'Las ediciones solo surten efecto después de aplicarlas.',
  'changes_failed':
      'No se pudieron aplicar los cambios. Revise los valores guardados e '
      'inténtelo de nuevo.',
  'form_errors': 'Revise los campos resaltados antes de aplicar los cambios.',
  'discard_changes_title': '¿Descartar los cambios sin aplicar?',
  'discard_changes_body':
      'Sus ediciones no se han aplicado. Siga editando para guardarlas o '
      'descártelas para salir.',
  'keep_editing': 'Seguir editando',
  'discard_changes': 'Descartar cambios',
  'invalid_port': 'Introduzca un puerto entre 1 y 65535.',
  'listener_exposure':
      'Las direcciones de escucha permiten el acceso desde la red local',
  'invalid_ipv4':
      'Introduzca una dirección IPv4 válida, por ejemplo 127.0.0.1.',
  'invalid_ipv6': 'Introduzca una dirección IPv6 válida, por ejemplo ::1.',
  'output_running': 'En ejecución',
  'output_waiting': 'Activada · no en ejecución',
  'output_disabled': 'Desactivada',
  'output_starting': 'Iniciando',
  'output_stopping': 'Deteniendo',
  'output_reconnecting': 'Reconectando',
  'output_degraded': 'Limitada',
  'output_error': 'Fallido',
  'output_unknown': 'Estado no disponible',
  'shared_network_scope': 'Los ajustes de red los comparten todas las cuentas.',
  'connection_details': 'Detalles de la conexión',
  'home_overview': 'Resumen de la conexión',
  'home_exit_region': 'Región de salida',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Tráfico',
  'home_traffic_window': 'Últimos 60 segundos',
  'home_traffic_idle': 'El tráfico aparece al conectar',
  'home_traffic_waiting': 'Esperando datos de tráfico',
  'home_traffic_unavailable': 'Sin historial de tráfico',
  'home_traffic_stale': 'Actualización del tráfico retrasada',
  'home_outputs_next': 'Disponible al conectar',
  'home_outputs_retry': 'VPN y proxies para la próxima conexión',
  'connection_protection_group': 'Conexión y protección',
  'proxy_routing_group': 'Proxy y enrutamiento',
  'application_group': 'Aplicación',
  'proxy_settings_link':
      'Direcciones de escucha, puertos, autenticación y DNS.',
  'proxy_auth_separate':
      'Pulsa Guardar usuario y contraseña abajo para aplicar estos cambios.',
  'reset_draft_hint':
      'Los valores predeterminados se cargarán en este formulario. Aplique '
      'los cambios para que surtan efecto.',
};

const Map<String, String> kNetworkQualityEs = <String, String>{
  'nq_range': 'Rango',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Tiempo de ida y vuelta',
  'diag_check_quality_packet_loss': 'Pérdida de paquetes',
  'diag_check_quality_queue_pressure': 'Presión de cola',
  'diag_check_quality_pmtu': 'MTU de la ruta',
  'diag_check_transport_migration_capability': 'Migración de la misma familia',
  'diag_check_dns_direct_encrypted_configuration':
      'Configuración de DNS directo',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Estado de ejecución del DNS directo',
  'diag_check_dns_direct_encrypted_reachability': 'Alcance del DNS cifrado',
  'diag_check_transport_h3_path_validation_probe':
      'Protocolo de enlace QUIC aislado',
  'nq_finding_unavailable':
      'Esta medición no está disponible en el estado actual.',
  'nq_finding_invalid_configuration':
      'La configuración DNS personalizada no es válida.',
  'nq_finding_dns_system':
      'Se usa el DNS de la red actual; no corresponde comprobar DNS cifrado.',
  'nq_finding_unsupported':
      'Actualiza Usque para usar DNS cifrado. Las consultas no pasarán a DNS sin cifrar.',
  'nq_finding_dns_custom_valid':
      'La configuración DNS cifrada personalizada es válida. El recurso a '
      'texto plano está desactivado.',
  'nq_finding_stale': 'La lectura está desactualizada o la red física cambió.',
  'nq_finding_rtt_high': 'El tiempo de ida y vuelta medido es elevado.',
  'nq_finding_healthy':
      'Las mediciones disponibles de la conexión están dentro de los valores normales.',
  'nq_finding_loss_high': 'La pérdida de paquetes del intervalo es elevada.',
  'nq_finding_queue_pressure':
      'Hay tráfico pendiente de envío o se descartaron datos durante esta conexión.',
  'nq_finding_pmtu_degraded':
      'Usque no pudo confirmar un tamaño de paquete adecuado para esta conexión.',
  'nq_finding_migration_reconnect':
      'Esta conexión debe restablecerse al cambiar de red.',
  'nq_finding_dns_changed':
      'El modo DNS guardado difiere del de la conexión en ejecución.',
  'nq_finding_dns_runtime': 'El DNS cifrado funciona correctamente.',
  'nq_finding_dns_degraded':
      'Hay problemas con el DNS cifrado. Las consultas fallidas no usarán el DNS sin cifrar de la red.',
  'nq_finding_probe_unsafe':
      'Esta medición no está disponible en el estado actual.',
  'nq_finding_probe_success': 'Esta comprobación se superó.',
  'nq_finding_probe_cancelled': 'Esta comprobación se canceló.',
  'nq_finding_probe_timeout':
      'La comprobación de diagnóstico agotó el tiempo de espera',
  'nq_finding_probe_failed': 'Esta comprobación falló.',
  'diag_fix_nq_profile':
      'Revise los campos DNS personalizados y el nombre del certificado. No '
      'desactive la verificación TLS.',
  'diag_fix_nq_retry':
      'Espere a que la red se estabilice y, a continuación, reintente.',
  'diag_fix_nq_network':
      'Compruebe la conectividad local y compare una muestra reciente antes '
      'de cambiar los ajustes.',
  'diag_fix_nq_reconnect': 'Reconecte para aplicar la configuración guardada.',
  'nav_network_quality': 'Calidad',
  'network_quality': 'Calidad de red',
  'nq_subtitle': 'Lea la conexión, no solo la velocidad.',
  'nq_local_only': 'Solo mediciones locales. No se carga nada.',
  'nq_doctor': 'Ejecutar el diagnóstico de red',
  'nq_doctor_help':
      'Las comprobaciones estándar solo leen el estado local. No abren '
      'conexiones externas ni cambian sus ajustes.',
  'nq_live': 'En vivo',
  'nq_stale': 'Lecturas desactualizadas',
  'nq_updated': 'Última muestra',
  'nq_seconds': '{count} s atrás',
  'nq_good': 'Buena',
  'nq_fair': 'Regular',
  'nq_poor': 'Mala',
  'nq_limited': 'Datos limitados',
  'nq_disconnected': 'Desconectado',
  'nq_connecting': 'Conectando',
  'nq_connected': 'Conectado',
  'nq_unavailable': 'No disponible',
  'nq_not_ready': 'No listo',
  'nq_unsupported': 'No compatible',
  'nq_capability_missing':
      'Esta versión no muestra la calidad de conexión. Puedes seguir conectando y desconectando. Busca actualizaciones en Ajustes.',
  'nq_empty': 'Conéctate para ver las mediciones.',
  'nq_stale_help':
      'Las actualizaciones se han detenido. Se muestran las últimas lecturas.',
  'nq_rtt': 'Tiempo de ida y vuelta',
  'nq_latest': 'Más reciente',
  'nq_smoothed': 'Suavizado',
  'nq_minimum': 'Mínimo',
  'nq_h2_ping': 'PING de protocolo HTTP/2',
  'nq_h3_rtt': 'Medición de ruta QUIC',
  'nq_throughput': 'Rendimiento',
  'nq_download': 'Descarga',
  'nq_upload': 'Carga',
  'nq_one_second': '1 segundo',
  'nq_five_seconds': 'Media de 5 segundos',
  'nq_loss': 'Pérdida de paquetes',
  'nq_loss_h2': 'HTTP/2 no expone una pérdida de paquetes comparable.',
  'nq_loss_interval':
      'Medido en el último intervalo; no es la pérdida acumulada.',
  'nq_congestion': 'Congestión',
  'nq_cwnd': 'Ventana de congestión',
  'nq_in_flight': 'Bytes en tránsito',
  'nq_send_rate': 'Tasa de entrega',
  'nq_h2_window': 'Ventanas de recepción HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Conexión',
  'nq_stalls': 'Bloqueos de capacidad',
  'nq_pmtu': 'MTU de la ruta',
  'nq_outer_pmtu': 'Límite de carga útil UDP externa',
  'nq_inner_payload': 'Límite de carga útil CONNECT-IP',
  'nq_pmtu_help':
      'Es el tamaño de paquete que admite la ruta de red. Usque lo comprueba automáticamente para reducir pérdidas. La comprobación no aumenta la MTU de VPN configurada en Ajustes avanzados de red.',
  'nq_migration': 'Migración de red',
  'nq_migration_help':
      'Usque intenta mantener la conexión al cambiar de red, por ejemplo de Wi-Fi a datos móviles. Ambas deben usar la misma versión IP, IPv4 o IPv6. Solo se usa una red cada vez; no se suman sus velocidades.',
  'nq_attempts': 'Intentos',
  'nq_successes': 'Correctos',
  'nq_failures': 'Fallidos',
  'nq_last_duration': 'Última duración',
  'nq_direct_dns': 'DNS directo',
  'nq_system_dns': 'DNS de la red actual',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Listo',
  'nq_degraded': 'Degradado',
  'nq_timeouts': 'Tiempos de espera',
  'nq_last_rtt': 'Último RTT',
  'nq_dns_redacted':
      'Los nombres del resolvedor y las direcciones bootstrap se muestran '
      'solo en Ajustes.',
  'nq_queues': 'Presión de cola',
  'nq_queue_details': 'Colas de bajo nivel',
  'nq_queue_empty': 'Aún no hay mediciones de cola.',
  'nq_current_capacity': 'Actual / capacidad',
  'nq_high_water': 'Marca máxima',
  'nq_drops': 'Descartes',
  'nq_oldest': 'Elemento más antiguo',
  'nq_tunToTransport': 'Dispositivo → transporte',
  'nq_proxyToTransport': 'Proxy → transporte',
  'nq_transportOutgoing': 'Salida de transporte',
  'nq_h3DatagramSend': 'Datagramas QUIC',
  'nq_h3WireSend': 'Salida UDP',
  'nq_transportToTun': 'Transporte → dispositivo',
  'nq_transportToProxy': 'Transporte → proxy',
  'nq_directDns': 'Solicitudes DNS directas',
  'nq_unknown_queue': 'Otra cola',
  'nq_trends': 'Últimos 60 segundos',
  'nq_samples': 'muestras',
  'nq_pause': 'Pausar gráficos',
  'nq_resume': 'Reanudar gráficos',
  'nq_paused': 'Gráficos en pausa',
  'nq_gaps': 'Las muestras faltantes se muestran como huecos.',
  'nq_phase_idle': 'Inactivo',
  'nq_phase_preparing_socket': 'Preparando la ruta',
  'nq_phase_probing': 'Sondeando',
  'nq_phase_validated': 'Validada',
  'nq_phase_promoting': 'Cambiando de ruta',
  'nq_phase_stable': 'Estable',
  'nq_phase_aborted': 'Abortada',
  'nq_phase_revalidating': 'Revalidando',
  'nq_phase_degraded': 'Degradado',
  'nq_phase_unknown': 'No listo',
  'nq_phase_unsupported': 'No compatible',
  'nq_reason_family_unavailable':
      'La nueva red no permite usar la misma versión IP. Debes volver a conectar.',
  'nq_reason_socket_protect_failed':
      'Usque no pudo usar la nueva red con seguridad. Si la conexión no se recupera, vuelve a conectar manualmente.',
  'nq_reason_generation_changed_during_setup':
      'La red volvió a cambiar durante la preparación.',
  'nq_reason_peer_cid_unavailable':
      'El servidor no pudo mantener la conexión en la nueva red. Si no se recupera, vuelve a conectar manualmente.',
  'nq_reason_local_cid_unavailable':
      'Usque no pudo mantener la conexión en la nueva red. Si no se recupera, vuelve a conectar manualmente.',
  'nq_reason_path_probe_rejected':
      'La nueva red no superó la comprobación. Comprueba que tenga acceso a Internet.',
  'nq_reason_path_validation_timeout':
      'La nueva red no respondió a tiempo. Revísala y vuelve a conectar si es necesario.',
  'nq_reason_superseded':
      'La red volvió a cambiar antes de completar la transición.',
  'nq_reason_promotion_failed':
      'Usque no pudo cambiar de red con seguridad. Si la conexión no se recupera, vuelve a conectar manualmente.',
  'nq_reason_connection_closed':
      'La conexión se cerró al cambiar de red. Vuelve a conectar.',
  'nq_reason_unsupported': 'La migración no está disponible en esta conexión.',
  'nq_reason_unknown': 'No hay un motivo compatible disponible.',
  'nq_dns_custom': 'Resolvedor cifrado personalizado',
  'nq_dns_server': 'Dominio del servidor DNS',
  'nq_dns_path': 'Ruta HTTPS',
  'nq_dns_port': 'Puerto (0 usa el valor predeterminado)',
  'nq_dns_bootstrap': 'Direcciones IP del servidor DNS',
  'nq_dns_bootstrap_help':
      'Introduce de 1 a 8 IP del proveedor DNS, una por línea; por ejemplo, 1.1.1.1. Usque conecta directamente a estas direcciones sin consultar antes el nombre del servidor.',
  'nq_dns_no_fallback':
      'Si el DNS directo cifrado falla, la consulta falla. Nunca se recurre '
      'al DNS del sistema ni al DNS en texto plano.',
  'nq_dns_system_privacy':
      'El proveedor DNS de tu red puede ver los dominios solicitados por el tráfico directo.',
  'nq_dns_scope':
      'Se usa para el tráfico que coincida con las reglas de países directos. No cambia el DNS del tráfico VPN.',
  'nq_dns_no_capability':
      'Actualiza Usque para usar DNS cifrado en conexiones directas. Se conservan tus ajustes. Puedes elegir DNS de la red actual si aceptas sus implicaciones de privacidad.',
  'nq_dns_invalid_name':
      'Introduce un dominio como dns.example.com, sin https://, puerto ni espacios.',
  'nq_dns_invalid_path':
      'Introduce una ruta como /dns-query, de hasta 256 caracteres. Quita los espacios y las partes que empiecen por ? o #.',
  'nq_dns_invalid_bootstrap':
      'Introduce entre 1 y 8 direcciones IP del servidor.',
  'nq_dns_invalid_port':
      'Introduce un puerto de 1 a 65535, o 0 para usar el predeterminado.',
  'nq_dns_invalid_mode': 'Elija un modo DNS compatible.',
  'nq_doctor_deep_title': '¿Ejecutar comprobaciones profundas de red?',
  'nq_doctor_deep_body':
      'Las comprobaciones pueden enviar tráfico de prueba. Duran hasta 15 segundos y se pueden cancelar. La configuración de conexión no cambiará.',
  'nq_doctor_deep_run': 'Ejecutar comprobaciones profundas',
  'nq_doctor_evidence':
      'Estas comprobaciones no permiten confirmar si hay fugas de DNS.',
};

const Map<String, String> kWindowsRecoveryEs = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Es necesario actualizar juntos los componentes de conexión de Usque. Busca actualizaciones en Ajustes. No se ha iniciado una nueva conexión VPN.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'La limpieza de la conexión VPN anterior no ha terminado. Cierra Usque por completo y ábrelo de nuevo. Si sigue fallando, abre Diagnóstico.',
  'WINDOWS_RECOVERY_FAILED':
      'No se pudo restaurar por completo el estado de red VPN anterior. No '
      'se inició una conexión VPN nueva. Reintente la conexión o revise el '
      'diagnóstico local.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows no pudo restaurar el estado de red VPN anterior tras tres '
      'intentos automáticos. Reintente cuando esté listo o revise el '
      'diagnóstico local.',
  'WINDOWS_RECOVERY_BLOCKED':
      'Se detuvo la reparación automática al no poder confirmar que fuera seguro restaurar los ajustes VPN anteriores. Busca actualizaciones en Ajustes; si persiste, exporta un paquete desde Diagnóstico.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'La recuperación de red de Windows está tardando más de lo esperado. '
      'No se inició una conexión VPN nueva. Espere a que termine la '
      'recuperación antes de reintentar.',
  'WINDOWS_RECOVERY_CONFLICT':
      'El estado de la red cambió o sigue en uso por otra sesión. Se '
      'detuvo la recuperación automática para proteger la conexión activa.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Esta instalación no puede restaurar automáticamente los ajustes VPN anteriores. Actualiza Usque desde Ajustes y vuelve a intentarlo.',
};

const String kWindowsAdapterCleanupEs =
    'No se pudo eliminar el adaptador virtual de la conexión anterior o confirmar su eliminación. No se ha iniciado una nueva conexión VPN.';

const Map<String, String> kL4Es = <String, String>{
  'l4_quic_not_ready': 'Preparando conexión L4',
  'l4_unsupported_packets': 'Paquetes no compatibles o malformados rechazados',
  'l4_budget_rejections': 'Admisiones de recursos rechazadas',
  'l4_not_applicable': 'No aplicable (L4)',
  'l4_mode': 'L4 (en fase experimental)',
  'l4_transport_hint':
      'Solo TCP. Las apps que necesitan UDP pueden no funcionar. El modo automático excluye L4.',
  'l4_explanation':
      'L4 transporta TCP mediante HTTP/3 y funciona con VPN y proxies SOCKS5 y HTTP. Convierte las consultas DNS de la VPN a TCP. Las aplicaciones que necesitan otros flujos UDP, Ping remoto, fragmentos IP o cabeceras de extensión pueden no funcionar. Selecciona L4 manualmente; el modo automático no lo elige.',
  'l4_unsupported':
      'L4 no está disponible en esta versión de Usque. Busca actualizaciones en Ajustes.',
  'l4_sni_identity':
      'La cuenta lo configura automáticamente. Se conserva el nombre del servidor de los demás modos de conexión.',
  'l4_edge_requires_l4':
      'El DNS resuelto en el borde requiere L4. Elija otro modo de DNS del proxy antes de pasar a Auto, H3 o H2.',
  'proxy_dns_edge_resolved':
      'Borde de Cloudflare (solo L4; sin consulta local)',
  'l4_verified': 'L4 ha establecido una conexión de aplicación',
  'l4_unverified':
      'Servidor conectado; aún no se ha confirmado una conexión de aplicación',
  'l4_status_unknown': 'No se puede confirmar la conexión de las aplicaciones',
  'l4_sessions': 'Sesiones / vaciado',
  'l4_flows': 'Flujos activos / en espera',
  'l4_connect': 'CONNECT aciertos / fallos / tiempos de espera',
  'l4_buffers': 'Presupuesto de búfer de aplicación usado (bytes)',
  'l4_backpressure': 'Contrapresión de envío / recepción',
  'l4_tun_flows': 'TUN TCP / semiabierto',
  'l4_udp': 'Paquetes UDP rechazados',
  'l4_dns': 'Conversiones DNS aciertos / fallos / tiempos de espera',
  'l4_migration':
      'Flujos conservados por migración / terminados por reconstrucción',
  'l4_na':
      'Control de direcciones CONNECT-IP, colas DATAGRAM, MTU de carga interior y tiempo de espera UDP: no aplicable en L4.',
};

const Map<String, String> kNetworkSettingsEs = <String, String>{
  'settings_applying': 'Guardado, aplicando',
  'settings_applied': 'Guardado y aplicado',
  'settings_deferred': 'Guardado; se aplica en la siguiente conexión manual',
  'settings_failed': 'Guardado, no se pudo aplicar',
  'settings_unknown': 'Resultado aún no confirmado',
  'settings_saved': 'Guardado',
  'settings_unsupported':
      'Cierra Usque por completo, ábrelo y guarda de nuevo. Si sigue fallando, busca actualizaciones en Ajustes.',
  'settings_save_failed':
      'No se pudieron guardar los ajustes. Se conservan sus cambios.',
  'settings_reconnect': 'Reconectar',
};

const Map<String, String> kChainEs = <String, String>{
  'duplicate_directive': 'Esta directiva solo puede aparecer una vez.',
  'mixed_protocols':
      'Todos los extremos remote deben usar el mismo transporte TCP o UDP.',
  'conflicting_protocol':
      'El remote entra en conflicto con el transporte global.',
  'too_many_endpoints': 'Usa como máximo 16 extremos remote.',
  'conflicting_authentication':
      'CLIENT_CERT entra en conflicto con el certificado en línea o el modo de autenticación.',
  'serialized_size_limit':
      'El registro cifrado superaría el límite de almacenamiento.',
  'multi_endpoint_unavailable':
      'Actualiza el motor para usar configuraciones con varios extremos.',
  'candidates': 'Extremos de inicio',
  'random_order':
      'Prueba los extremos en un orden aleatorio nuevo en cada conexión.',
  'file_order': 'Prueba los extremos en el orden del archivo.',
  'attempting': 'Probando extremo',
  'actual_endpoint': 'Extremo conectado',
  'attempt_failures': 'Intentos fallidos',
  'failure_transport': 'transporte cerrado',
  'failure_authentication': 'autenticación',
  'failure_certificate': 'certificado',
  'failure_configuration': 'configuración',
  'failure_address_changed': 'dirección cambiada',
  'failure_protocol': 'protocolo',
  'failure_cleanup': 'limpieza',
  'failure_reason': 'Fallo: {reason}.',
  'manage': 'Gestionar',
  'dns_fallback': 'DNS del túnel (OpenVPN puede negociar el DNS)',
  'dns_unavailable_title': 'No hay DNS por esta salida',
  'dns_unavailable':
      'Ningún servidor DNS es accesible por esta salida. Usa direcciones IP o elige otra salida con DNS accesible.',
  'authentication_failed':
      'Error de autenticación. Actualiza las credenciales antes de volver a conectar.',
  'profile_limit':
      'La biblioteca de configuraciones está llena (128 perfiles).',
  'metadata_limit':
      'Los metadatos de la biblioteca de configuraciones están llenos.',
  'title': 'Proxy en cadena',
  'subtitle': 'Elige una salida a la que se llega a través de WARP.',
  'source': 'Origen de la salida',
  'enable': 'Activar proxy en cadena',
  'import_file': 'Importar archivo',
  'paste': 'Pegar configuración',
  'profiles': 'Configuraciones guardadas',
  'empty': 'Importa una configuración para elegir una salida.',
  'empty_hint_openvpn':
      'Importa un archivo .ovpn o pega su texto. Se admiten extremos TCP y UDP, certificados en línea y usuario/contraseña.',
  'empty_hint_wireguard':
      'Importa un archivo .conf o pega su texto. Se admite una sección [Interface] y una [Peer].',
  'import_limits':
      'Las configuraciones deben ser texto UTF-8 de hasta 128 KiB. Si el dispositivo no tiene selector de archivos, pega el texto.',
  'enable_to_choose':
      'Activa el proxy en cadena para elegir una configuración.',
  'select_required': 'Elige una configuración guardada para aplicarla.',
  'pending_disable': 'Pendiente: desactivar el proxy en cadena',
  'apply_reconnect': 'Aplicar y reconectar',
  'requires_connect_ip': 'Requiere CONNECT-IP',
  'menu': 'Acciones de la configuración',
  'preview': 'Comprobar configuración',
  'save_import': 'Guardar configuración',
  'name': 'Nombre',
  'configuration': 'Texto de la configuración',
  'file_loaded': 'Configuración cargada desde el archivo ({lines} líneas).',
  'username': 'Nombre de usuario',
  'password': 'Contraseña',
  'key_password': 'Contraseña de la clave privada',
  'show_password': 'Mostrar contraseña',
  'hide_password': 'Ocultar contraseña',
  'credentials': 'Actualizar credenciales',
  'rename': 'Cambiar nombre',
  'delete': 'Eliminar',
  'cancel': 'Cancelar',
  'save': 'Guardar',
  'apply': 'Aplicar cambios',
  'clear': 'Borrar selección',
  'current': 'Conexión actual',
  'saved': 'Selección guardada',
  'draft': 'Selección pendiente',
  'disconnected': 'Sin conexión',
  'disabled': 'No activado',
  'enabled_idle': 'Activado · sin conexión',
  'disconnecting': 'Desconectando',
  'file_read_failed': 'No se pudo leer el archivo de configuración.',
  'file_encoding_invalid': 'El archivo de configuración debe ser texto UTF-8.',
  'file_busy': 'Ya hay un selector de archivos abierto.',
  'connected': 'Conectado',
  'connecting': 'Conectando',
  'error': 'Error de conexión',
  'no_selection': 'Ninguna configuración seleccionada',
  'l4': 'Esta configuración requiere el modo CONNECT-IP.',
  'switch_mode': 'Cambiar a CONNECT-IP y aplicar',
  'unsupported': 'Este motor no admite este origen de salida.',
  'scope':
      'Las reglas directas explícitas siguen vigentes. El resto del tráfico usa la salida elegida.',
  'allowed': 'Destinos permitidos',
  'dns': 'DNS',
  'addresses': 'Direcciones del túnel',
  'address_family': 'Familia de direcciones',
  'transport': 'Transporte',
  'endpoint': 'Servidor',
  'restricted':
      'Los destinos fuera de AllowedIPs se bloquean en la ruta del proxy.',
  'delete_confirm':
      '¿Eliminar esta configuración guardada? El archivo importado original no cambia.',
  'profile_in_use':
      'Elige otra configuración o borra la selección guardada antes de eliminar esta.',
  'stale_revision':
      'La configuración cambió. Actualiza la lista e inténtalo de nuevo.',
  'secure_storage_failed':
      'No se pudo leer ni guardar la configuración cifrada.',
  'invalid_configuration':
      'La configuración no es válida o contiene opciones no admitidas.',
  'looks_like_wireguard':
      'Parece una configuración de WireGuard. Cambia el origen de la salida a WireGuard.',
  'looks_like_openvpn':
      'Parece una configuración de OpenVPN. Cambia el origen de la salida a OpenVPN.',
  'error_location': '{message} ({field}, línea {line})',
  'error_field': '{message} ({field})',
  'file_unavailable':
      'No hay selector de archivos. Pega el texto de la configuración.',
  'invalid_size_or_encoding':
      'Usa una configuración UTF-8 de 128 KiB como máximo.',
  'unsupported_directive': 'Esta directiva de OpenVPN no está admitida.',
  'unsupported_or_duplicate_field':
      'Este campo no está admitido o está duplicado.',
  'unsupported_or_duplicate_section': 'Usa una sección Interface y una Peer.',
  'missing_field': 'Falta un campo obligatorio.',
  'invalid_name':
      'Usa un nombre de 1 a 64 caracteres sin caracteres de control.',
  'invalid_key': 'La clave debe ser una clave Base64 válida de 32 bytes.',
  'checking': 'Comprobando la configuración…',
  'changed': 'Cambios guardados',
};
