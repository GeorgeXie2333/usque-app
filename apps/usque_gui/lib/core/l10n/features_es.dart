/// Supplemental feature strings for Spanish.
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowEs = <String, String>{
  'cc_label': 'Control de congestión HTTP/3',
  'cc_help': 'Se aplica en la próxima conexión manual.',
  'cc_upgrade': 'Se requiere una actualización del Engine.',
  'cc_h2': 'HTTP/2 usa el TCP del sistema.',
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
  'home_traffic_idle': 'Comienza después de conectar',
  'home_traffic_waiting': 'Esperando muestras',
  'home_traffic_unavailable': 'Historial no disponible',
  'home_traffic_stale': 'Muestras retrasadas',
  'home_outputs_next': 'Salidas que se activarán al conectar',
  'home_outputs_retry': 'Salidas configuradas para el próximo intento',
  'connection_protection_group': 'Conexión y protección',
  'proxy_routing_group': 'Proxy y enrutamiento',
  'application_group': 'Aplicación',
  'proxy_settings_link':
      'Direcciones de escucha, puertos, autenticación y DNS.',
  'proxy_auth_separate':
      'Las credenciales se guardan por separado con Guardar credenciales.',
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
      'Está seleccionado el DNS del sistema físico; las comprobaciones de '
      'DNS cifrado no aplican.',
  'nq_finding_unsupported':
      'El DNS cifrado no está disponible en este Engine; no se permite el '
      'recurso a texto plano.',
  'nq_finding_dns_custom_valid':
      'La configuración DNS cifrada personalizada es válida. El recurso a '
      'texto plano está desactivado.',
  'nq_finding_stale': 'La lectura está desactualizada o la red física cambió.',
  'nq_finding_rtt_high': 'El tiempo de ida y vuelta medido es elevado.',
  'nq_finding_healthy':
      'La medición local disponible está dentro del rango esperado.',
  'nq_finding_loss_high': 'La pérdida de paquetes del intervalo es elevada.',
  'nq_finding_queue_pressure':
      'Una cola está bajo presión o ha registrado descartes durante esta '
      'conexión.',
  'nq_finding_pmtu_degraded':
      'La validación de la MTU de la ruta está degradada.',
  'nq_finding_migration_reconnect':
      'La migración no está disponible en esta ruta; un cambio de red usa '
      'una reconexión completa.',
  'nq_finding_dns_changed':
      'El modo DNS guardado difiere del de la conexión en ejecución.',
  'nq_finding_dns_runtime':
      'El DNS cifrado se completó correctamente. El estado local no '
      'demuestra la ausencia de fugas externas.',
  'nq_finding_dns_degraded':
      'El DNS cifrado está degradado; las consultas directas fallidas no '
      'recurren al DNS del sistema.',
  'nq_finding_probe_unsafe':
      'Sonda omitida: falta el estado seguro requerido o la identidad '
      'guardada. Nunca se duplica un túnel activo.',
  'nq_finding_probe_success':
      'La sonda autenticada se completó. Esto no es una prueba externa de '
      'fugas de paquetes.',
  'nq_finding_probe_cancelled': 'Sonda cancelada y limpieza solicitada.',
  'nq_finding_probe_timeout': 'La sonda acotada no terminó antes de su plazo.',
  'nq_finding_probe_failed':
      'La sonda autenticada falló; no se intentó un recurso inseguro.',
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
      'Este Engine no ofrece calidad de red. Los controles de conexión '
      'actuales siguen funcionando.',
  'nq_empty':
      'Conecte para ver las mediciones. Los valores desconocidos no se '
      'muestran como cero.',
  'nq_stale_help':
      'El origen dejó de actualizarse. Estas son lecturas anteriores; los '
      'huecos se conservan como huecos.',
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
      'El descubrimiento de ruta no aumenta la MTU TUN del dispositivo.',
  'nq_migration': 'Migración de red',
  'nq_migration_help':
      'Una conexión, una ruta de datos. Solo la misma familia IP; no es '
      'múltiples rutas.',
  'nq_attempts': 'Intentos',
  'nq_successes': 'Correctos',
  'nq_failures': 'Fallidos',
  'nq_last_duration': 'Última duración',
  'nq_direct_dns': 'DNS directo',
  'nq_system_dns': 'DNS del sistema físico',
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
      'La familia IP actual no está disponible; se usa una reconexión '
      'completa.',
  'nq_reason_socket_protect_failed':
      'No se pudo preparar un socket candidato protegido.',
  'nq_reason_generation_changed_during_setup':
      'La red volvió a cambiar durante la preparación.',
  'nq_reason_peer_cid_unavailable':
      'El par no tiene un identificador de conexión de reserva.',
  'nq_reason_local_cid_unavailable':
      'No hay un identificador de conexión local disponible.',
  'nq_reason_path_probe_rejected': 'No se pudo validar la ruta candidata.',
  'nq_reason_path_validation_timeout':
      'La validación de la ruta agotó el tiempo de espera; la reconexión '
      'está disponible.',
  'nq_reason_superseded':
      'Un cambio de red más reciente reemplazó este intento.',
  'nq_reason_promotion_failed':
      'No se pudo completar el cambio de ruta de forma segura.',
  'nq_reason_connection_closed': 'La conexión se cerró durante la migración.',
  'nq_reason_unsupported': 'La migración no está disponible en esta conexión.',
  'nq_reason_unknown': 'No hay un motivo compatible disponible.',
  'nq_dns_custom': 'Resolvedor cifrado personalizado',
  'nq_dns_server': 'Nombre del servidor TLS',
  'nq_dns_path': 'Ruta HTTPS',
  'nq_dns_port': 'Puerto (0 usa el valor predeterminado)',
  'nq_dns_bootstrap': 'Direcciones IP bootstrap',
  'nq_dns_bootstrap_help':
      'Introduzca de 1 a 8 direcciones IP numéricas, una por línea. No se '
      'usa resolución de nombres.',
  'nq_dns_no_fallback':
      'Si el DNS directo cifrado falla, la consulta falla. Nunca se recurre '
      'al DNS del sistema ni al DNS en texto plano.',
  'nq_dns_system_privacy':
      'El DNS del sistema físico puede exponer los nombres de las consultas '
      'directas al proveedor DNS de la red física.',
  'nq_dns_scope':
      'Se usa solo para consultas directas seleccionadas por Geo. El DNS '
      'del túnel no cambia.',
  'nq_dns_no_capability':
      'Este Engine no puede usar DNS directo cifrado. Se conservan los '
      'ajustes guardados. Puede elegir explícitamente el DNS del sistema.',
  'nq_dns_invalid_name':
      'Introduzca un nombre DNS sin espacios, sintaxis de URL ni comodines.',
  'nq_dns_invalid_path':
      'Use una ruta de hasta 256 caracteres que empiece por /, sin '
      'consulta, fragmento ni espacios en blanco.',
  'nq_dns_invalid_bootstrap':
      'Use de 1 a 8 IP unicast únicas; no se permiten direcciones no '
      'especificadas, multicast, broadcast ni IPv6 de enlace local.',
  'nq_dns_invalid_port': 'Introduzca un valor entre 0 y 65535.',
  'nq_dns_invalid_mode': 'Elija un modo DNS compatible.',
  'nq_doctor_deep_title': '¿Ejecutar comprobaciones profundas de red?',
  'nq_doctor_deep_body':
      'Las comprobaciones profundas pueden enviar una consulta DNS de '
      'prueba al resolvedor configurado y validar una ruta QUIC protegida. '
      'Duran 15 segundos como máximo, se pueden cancelar, nunca crean un '
      'segundo túnel de datos y nunca cambian el DNS, las rutas, el perfil '
      'ni el transporte.',
  'nq_doctor_deep_run': 'Ejecutar comprobaciones profundas',
  'nq_doctor_evidence':
      'Las comprobaciones locales describen la configuración y el estado '
      'observado. No son una prueba externa de que no haya fugas de DNS.',
};

const Map<String, String> kWindowsRecoveryEs = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'No se pudo restaurar por completo el estado de red VPN anterior. No '
      'se inició una conexión VPN nueva. Reintente la conexión o revise el '
      'diagnóstico local.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'Windows no pudo restaurar el estado de red VPN anterior tras tres '
      'intentos automáticos. Reintente cuando esté listo o revise el '
      'diagnóstico local.',
  'WINDOWS_RECOVERY_BLOCKED':
      'La reparación automática se detuvo porque no se pudo verificar de '
      'forma segura el estado de red de Windows anterior. Reinicie el '
      'Agent o actualice Usque y, a continuación, revise el diagnóstico '
      'local.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'La recuperación de red de Windows está tardando más de lo esperado. '
      'No se inició una conexión VPN nueva. Espere a que termine la '
      'recuperación antes de reintentar.',
  'WINDOWS_RECOVERY_CONFLICT':
      'El estado de la red cambió o sigue en uso por otra sesión. Se '
      'detuvo la recuperación automática para proteger la conexión activa.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Este Agent de Windows no admite la recuperación automática segura. '
      'Actualice la aplicación y el Agent a la vez y, a continuación, '
      'reintente.',
};

const String kWindowsAdapterCleanupEs =
    'No se pudo quitar el adaptador Wintun anterior o no se pudo verificar '
    'su eliminación. No se ha iniciado una conexión VPN nueva.';
