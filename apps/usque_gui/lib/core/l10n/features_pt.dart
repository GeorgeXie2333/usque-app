/// Supplemental feature strings for Portuguese (Brazil).
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowPt = <String, String>{
  'cc_label': 'Controle de congestionamento HTTP/3',
  'cc_help': 'Vale na próxima conexão manual.',
  'cc_upgrade': 'É necessária uma atualização do Engine.',
  'cc_h2': 'O HTTP/2 usa o TCP do sistema.',
  'cc_saved': 'Salvo',
  'cc_pending': 'Pendente da próxima conexão manual.',
  'save_changes': 'Aplicar alterações',
  'saving_changes': 'Aplicando alterações…',
  'unsaved_changes': 'Alterações não aplicadas',
  'changes_applied': 'Alterações aplicadas',
  'changes_apply_hint':
      'As edições só passam a valer depois que você as aplicar.',
  'changes_failed':
      'Não foi possível aplicar as alterações. Revise os valores salvos e '
      'tente novamente.',
  'form_errors':
      'Verifique os campos destacados antes de aplicar as alterações.',
  'discard_changes_title': 'Descartar alterações não aplicadas?',
  'discard_changes_body':
      'Suas edições ainda não foram aplicadas. Continue editando para '
      'salvá-las ou descarte-as para sair.',
  'keep_editing': 'Continuar editando',
  'discard_changes': 'Descartar alterações',
  'invalid_port': 'Insira uma porta de 1 a 65535.',
  'listener_exposure':
      'Os endereços do ouvinte permitem acesso pela rede local',
  'invalid_ipv4': 'Insira um endereço IPv4 válido, por exemplo 127.0.0.1.',
  'invalid_ipv6': 'Insira um endereço IPv6 válido, por exemplo ::1.',
  'output_running': 'Em execução',
  'output_waiting': 'Habilitada · não em execução',
  'output_disabled': 'Desabilitada',
  'output_starting': 'Iniciando',
  'output_stopping': 'Parando',
  'output_reconnecting': 'Reconectando',
  'output_degraded': 'Limitada',
  'output_error': 'Com falha',
  'output_unknown': 'Status indisponível',
  'shared_network_scope':
      'As configurações de rede são compartilhadas por todas as contas.',
  'connection_details': 'Detalhes da conexão',
  'home_overview': 'Visão geral da conexão',
  'home_exit_region': 'Região de saída',
  'home_kill_switch': 'Kill Switch',
  'home_traffic': 'Tráfego',
  'home_traffic_window': 'Últimos 60 segundos',
  'home_traffic_idle': 'Começa depois de conectar',
  'home_traffic_waiting': 'Aguardando amostras',
  'home_traffic_unavailable': 'Histórico indisponível',
  'home_traffic_stale': 'Amostras atrasadas',
  'home_outputs_next': 'Saídas habilitadas após conectar',
  'home_outputs_retry': 'Saídas configuradas para a próxima tentativa',
  'connection_protection_group': 'Conexão e proteção',
  'proxy_routing_group': 'Proxy e roteamento',
  'application_group': 'Aplicativo',
  'proxy_settings_link': 'Endereços do ouvinte, portas, autenticação e DNS.',
  'proxy_auth_separate':
      'As credenciais são salvas separadamente com Salvar credenciais.',
  'reset_draft_hint':
      'Os padrões serão carregados neste formulário. Aplique as alterações '
      'para que passem a valer.',
};

const Map<String, String> kNetworkQualityPt = <String, String>{
  'nq_range': 'Intervalo',
  'nq_bytes': 'Bytes',
  'diag_check_quality_rtt': 'Tempo de ida e volta',
  'diag_check_quality_packet_loss': 'Perda de pacotes',
  'diag_check_quality_queue_pressure': 'Pressão da fila',
  'diag_check_quality_pmtu': 'MTU do caminho',
  'diag_check_transport_migration_capability': 'Migração da mesma família',
  'diag_check_dns_direct_encrypted_configuration': 'Configuração de DNS direto',
  'diag_check_dns_direct_encrypted_runtime_state':
      'Estado de execução do DNS direto',
  'diag_check_dns_direct_encrypted_reachability':
      'Alcance do DNS criptografado',
  'diag_check_transport_h3_path_validation_probe': 'Handshake QUIC isolado',
  'nq_finding_unavailable': 'Esta medição não está disponível no estado atual.',
  'nq_finding_invalid_configuration':
      'A configuração DNS personalizada é inválida.',
  'nq_finding_dns_system':
      'O DNS do sistema físico está selecionado; as verificações de DNS '
      'criptografado não se aplicam.',
  'nq_finding_unsupported':
      'O DNS criptografado está indisponível neste Engine; nenhum fallback '
      'em texto simples é permitido.',
  'nq_finding_dns_custom_valid':
      'A configuração DNS criptografada personalizada é válida. O fallback '
      'em texto simples está desabilitado.',
  'nq_finding_stale': 'A leitura está desatualizada ou a rede física mudou.',
  'nq_finding_rtt_high': 'O tempo de ida e volta medido está elevado.',
  'nq_finding_healthy':
      'A medição local disponível está dentro do intervalo esperado.',
  'nq_finding_loss_high': 'A perda de pacotes do intervalo está elevada.',
  'nq_finding_queue_pressure':
      'Uma fila está sob pressão ou registrou descartes durante esta '
      'conexão.',
  'nq_finding_pmtu_degraded': 'A validação da MTU do caminho está degradada.',
  'nq_finding_migration_reconnect':
      'A migração não está disponível neste caminho; uma mudança de rede '
      'usa reconexão completa.',
  'nq_finding_dns_changed':
      'O modo DNS salvo difere do da conexão em execução.',
  'nq_finding_dns_runtime':
      'O DNS criptografado foi bem-sucedido. O estado local não prova a '
      'ausência de vazamentos externos.',
  'nq_finding_dns_degraded':
      'O DNS criptografado está degradado; as consultas diretas com falha '
      'não recorrem ao DNS do sistema.',
  'nq_finding_probe_unsafe':
      'Sonda ignorada: o estado seguro necessário ou a identidade salva '
      'está indisponível. Um túnel ativo nunca é duplicado.',
  'nq_finding_probe_success':
      'A sonda autenticada foi concluída. Isto não é um teste externo de '
      'vazamento de pacotes.',
  'nq_finding_probe_cancelled': 'Sonda cancelada e limpeza solicitada.',
  'nq_finding_probe_timeout': 'A sonda limitada não terminou antes do prazo.',
  'nq_finding_probe_failed':
      'A sonda autenticada falhou; nenhum fallback inseguro foi tentado.',
  'diag_fix_nq_profile':
      'Revise os campos DNS personalizados e o nome do certificado. Não '
      'desabilite a verificação TLS.',
  'diag_fix_nq_retry': 'Aguarde uma rede estável e, então, tente novamente.',
  'diag_fix_nq_network':
      'Verifique a conectividade local e compare uma amostra recente antes '
      'de alterar as configurações.',
  'diag_fix_nq_reconnect': 'Reconecte para aplicar a configuração salva.',
  'nav_network_quality': 'Qualidade',
  'network_quality': 'Qualidade da rede',
  'nq_subtitle': 'Leia a conexão, não só a velocidade.',
  'nq_local_only': 'Somente medições locais. Nada é enviado.',
  'nq_doctor': 'Executar o diagnóstico de rede',
  'nq_doctor_help':
      'As verificações padrão leem somente o estado local. Elas não abrem '
      'conexões externas nem alteram suas configurações.',
  'nq_live': 'Ao vivo',
  'nq_stale': 'Leituras desatualizadas',
  'nq_updated': 'Última amostra',
  'nq_seconds': '{count} s atrás',
  'nq_good': 'Boa',
  'nq_fair': 'Razoável',
  'nq_poor': 'Ruim',
  'nq_limited': 'Dados limitados',
  'nq_disconnected': 'Desconectado',
  'nq_connecting': 'Conectando',
  'nq_connected': 'Conectado',
  'nq_unavailable': 'Não disponível',
  'nq_not_ready': 'Não pronto',
  'nq_unsupported': 'Não suportado',
  'nq_capability_missing':
      'Este Engine não fornece qualidade de rede. Os controles de conexão '
      'existentes continuam funcionando.',
  'nq_empty':
      'Conecte para ver as medições. Valores desconhecidos não são '
      'mostrados como zero.',
  'nq_stale_help':
      'A origem parou de atualizar. Estas são leituras anteriores; as '
      'lacunas permanecem lacunas.',
  'nq_rtt': 'Tempo de ida e volta',
  'nq_latest': 'Mais recente',
  'nq_smoothed': 'Suavizado',
  'nq_minimum': 'Mínimo',
  'nq_h2_ping': 'PING de protocolo HTTP/2',
  'nq_h3_rtt': 'Medição de caminho QUIC',
  'nq_throughput': 'Vazão',
  'nq_download': 'Recebimento',
  'nq_upload': 'Envio',
  'nq_one_second': '1 segundo',
  'nq_five_seconds': 'Média de 5 segundos',
  'nq_loss': 'Perda de pacotes',
  'nq_loss_h2': 'O HTTP/2 não expõe uma perda de pacotes comparável.',
  'nq_loss_interval': 'Medido no último intervalo; não é a perda acumulada.',
  'nq_congestion': 'Congestionamento',
  'nq_cwnd': 'Janela de congestionamento',
  'nq_in_flight': 'Bytes em trânsito',
  'nq_send_rate': 'Taxa de entrega',
  'nq_h2_window': 'Janelas de recebimento HTTP/2',
  'nq_stream_window': 'Stream',
  'nq_connection_window': 'Conexão',
  'nq_stalls': 'Interrupções de capacidade',
  'nq_pmtu': 'MTU do caminho',
  'nq_outer_pmtu': 'Limite de carga útil UDP externa',
  'nq_inner_payload': 'Limite de carga útil CONNECT-IP',
  'nq_pmtu_help':
      'A descoberta de caminho não aumenta a MTU TUN do dispositivo.',
  'nq_migration': 'Migração de rede',
  'nq_migration_help':
      'Uma conexão, um caminho de dados. Somente a mesma família IP; não é '
      'múltiplos caminhos.',
  'nq_attempts': 'Tentativas',
  'nq_successes': 'Bem-sucedidas',
  'nq_failures': 'Com falha',
  'nq_last_duration': 'Última duração',
  'nq_direct_dns': 'DNS direto',
  'nq_system_dns': 'DNS do sistema físico',
  'nq_doh': 'DNS over HTTPS',
  'nq_dot': 'DNS over TLS',
  'nq_ready': 'Pronto',
  'nq_degraded': 'Degradado',
  'nq_timeouts': 'Tempos limite',
  'nq_last_rtt': 'Último RTT',
  'nq_dns_redacted':
      'Os nomes do resolvedor e os endereços bootstrap são mostrados '
      'somente em Configurações.',
  'nq_queues': 'Pressão da fila',
  'nq_queue_details': 'Filas de baixo nível',
  'nq_queue_empty': 'Ainda não há medições de fila.',
  'nq_current_capacity': 'Atual / capacidade',
  'nq_high_water': 'Marca máxima',
  'nq_drops': 'Descartes',
  'nq_oldest': 'Item mais antigo',
  'nq_tunToTransport': 'Dispositivo → transporte',
  'nq_proxyToTransport': 'Proxy → transporte',
  'nq_transportOutgoing': 'Saída do transporte',
  'nq_h3DatagramSend': 'Datagramas QUIC',
  'nq_h3WireSend': 'Saída UDP',
  'nq_transportToTun': 'Transporte → dispositivo',
  'nq_transportToProxy': 'Transporte → proxy',
  'nq_directDns': 'Solicitações DNS diretas',
  'nq_unknown_queue': 'Outra fila',
  'nq_trends': 'Últimos 60 segundos',
  'nq_samples': 'amostras',
  'nq_pause': 'Pausar gráficos',
  'nq_resume': 'Retomar gráficos',
  'nq_paused': 'Gráficos pausados',
  'nq_gaps': 'As amostras ausentes aparecem como lacunas.',
  'nq_phase_idle': 'Ocioso',
  'nq_phase_preparing_socket': 'Preparando o caminho',
  'nq_phase_probing': 'Sondando',
  'nq_phase_validated': 'Validado',
  'nq_phase_promoting': 'Trocando de caminho',
  'nq_phase_stable': 'Estável',
  'nq_phase_aborted': 'Interrompido',
  'nq_phase_revalidating': 'Revalidando',
  'nq_phase_degraded': 'Degradado',
  'nq_phase_unknown': 'Não pronto',
  'nq_phase_unsupported': 'Não suportado',
  'nq_reason_family_unavailable':
      'A família IP atual está indisponível; será usada uma reconexão '
      'completa.',
  'nq_reason_socket_protect_failed':
      'Não foi possível preparar um soquete candidato protegido.',
  'nq_reason_generation_changed_during_setup':
      'A rede mudou novamente durante a preparação.',
  'nq_reason_peer_cid_unavailable':
      'O par não tem um identificador de conexão reserva.',
  'nq_reason_local_cid_unavailable':
      'Não há um identificador de conexão local disponível.',
  'nq_reason_path_probe_rejected':
      'Não foi possível validar o caminho candidato.',
  'nq_reason_path_validation_timeout':
      'A validação do caminho excedeu o tempo limite; a reconexão está '
      'disponível.',
  'nq_reason_superseded':
      'Uma mudança de rede mais recente substituiu esta tentativa.',
  'nq_reason_promotion_failed':
      'Não foi possível concluir a troca de caminho com segurança.',
  'nq_reason_connection_closed': 'A conexão foi fechada durante a migração.',
  'nq_reason_unsupported': 'A migração não está disponível nesta conexão.',
  'nq_reason_unknown': 'Nenhum motivo compatível está disponível.',
  'nq_dns_custom': 'Resolvedor criptografado personalizado',
  'nq_dns_server': 'Nome do servidor TLS',
  'nq_dns_path': 'Caminho HTTPS',
  'nq_dns_port': 'Porta (0 usa o padrão)',
  'nq_dns_bootstrap': 'Endereços IP bootstrap',
  'nq_dns_bootstrap_help':
      'Insira de 1 a 8 endereços IP numéricos, um por linha. Nenhuma '
      'consulta de nome de host é usada.',
  'nq_dns_no_fallback':
      'Se o DNS direto criptografado falhar, a consulta falha. Nunca há '
      'fallback para o DNS do sistema ou em texto simples.',
  'nq_dns_system_privacy':
      'O DNS do sistema físico pode expor os nomes das consultas diretas '
      'ao provedor de DNS da rede física.',
  'nq_dns_scope':
      'Usado somente para consultas diretas selecionadas por Geo. O DNS '
      'do túnel não é alterado.',
  'nq_dns_no_capability':
      'Este Engine não pode usar DNS direto criptografado. As '
      'configurações salvas são preservadas. Você pode escolher '
      'explicitamente o DNS do sistema.',
  'nq_dns_invalid_name':
      'Insira um nome DNS sem espaços, sintaxe de URL ou curingas.',
  'nq_dns_invalid_path':
      'Use um caminho de até 256 caracteres começando com /, sem consulta, '
      'fragmento ou espaços.',
  'nq_dns_invalid_bootstrap':
      'Use de 1 a 8 IPs unicast exclusivos; sem endereço não especificado, '
      'multicast, broadcast ou IPv6 link-local.',
  'nq_dns_invalid_port': 'Insira um valor de 0 a 65535.',
  'nq_dns_invalid_mode': 'Escolha um modo DNS compatível.',
  'nq_doctor_deep_title': 'Executar verificações aprofundadas de rede?',
  'nq_doctor_deep_body':
      'As verificações aprofundadas podem enviar uma consulta DNS de teste '
      'ao resolvedor configurado e validar um caminho QUIC protegido. '
      'Duram no máximo 15 segundos, podem ser canceladas, nunca criam um '
      'segundo túnel de dados e nunca alteram o DNS, as rotas, o perfil ou '
      'o transporte.',
  'nq_doctor_deep_run': 'Executar verificações aprofundadas',
  'nq_doctor_evidence':
      'As verificações locais descrevem a configuração e o estado '
      'observado. Não são uma prova externa de ausência de vazamentos de '
      'DNS.',
};

const Map<String, String> kWindowsRecoveryPt = <String, String>{
  'WINDOWS_RECOVERY_FAILED':
      'Não foi possível restaurar por completo o estado de rede VPN '
      'anterior. Nenhuma nova conexão VPN foi iniciada. Tente conectar '
      'novamente ou inspecione os diagnósticos locais.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'O Windows não conseguiu restaurar o estado de rede VPN anterior '
      'após três tentativas automáticas. Tente novamente quando estiver '
      'pronto ou inspecione os diagnósticos locais.',
  'WINDOWS_RECOVERY_BLOCKED':
      'O reparo automático parou porque o estado de rede anterior do '
      'Windows não pôde ser verificado com segurança. Reinicie o Agent ou '
      'atualize o Usque e, em seguida, inspecione os diagnósticos locais.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'A recuperação de rede do Windows está demorando mais do que o '
      'esperado. Nenhuma nova conexão VPN foi iniciada. Aguarde o término '
      'da recuperação antes de tentar novamente.',
  'WINDOWS_RECOVERY_CONFLICT':
      'O estado da rede mudou ou ainda está em uso por outra sessão. A '
      'recuperação automática foi interrompida para proteger a conexão '
      'ativa.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Este Agent do Windows não oferece recuperação automática segura. '
      'Atualize o aplicativo e o Agent juntos e, em seguida, tente '
      'novamente.',
};

const String kWindowsAdapterCleanupPt =
    'Não foi possível remover o adaptador Wintun anterior ou a remoção não '
    'pôde ser verificada. Nenhuma nova conexão VPN foi iniciada.';

const Map<String, String> kL4Pt = <String, String>{
  'l4_quic_not_ready': 'Aguardando uma sessão QUIC pronta',
  'l4_unsupported_packets': 'Pacotes incompatíveis ou malformados rejeitados',
  'l4_budget_rejections': 'Admissões de recurso rejeitadas',
  'l4_not_applicable': 'Não aplicável (L4)',
  'l4_mode': 'L4 (em fase experimental)',
  'l4_transport_hint': 'Apenas TCP; o DNS do TUN usa TCP. Auto não inclui L4.',
  'l4_explanation':
      'Somente TCP sobre HTTP/3. Compatível com VPN/TUN, SOCKS5 e HTTP; o DNS do TUN é convertido para TCP. O Auto nunca escolhe L4. Outros UDP, ping remoto, fragmentos IP e cabeçalhos de extensão não são compatíveis; alguns aplicativos podem não funcionar.',
  'l4_unsupported':
      'Este mecanismo não declarou suporte L4 completo. Não é possível ativar o L4.',
  'l4_sni_identity':
      'Somente leitura: derivado da identidade da conta carregada. O SNI CONNECT-IP existente é preservado.',
  'l4_edge_requires_l4':
      'O DNS resolvido na borda exige L4. Selecione outro modo de DNS do proxy antes de mudar para Auto, H3 ou H2.',
  'proxy_dns_edge_resolved':
      'Borda Cloudflare (somente L4; sem consulta local)',
  'l4_verified': 'L4 CONNECT verificado',
  'l4_unverified': 'QUIC pronto; L4 CONNECT ainda não verificado',
  'l4_status_unknown': 'Status de verificação L4 desconhecido',
  'l4_sessions': 'Sessões / esvaziamento',
  'l4_flows': 'Fluxos ativos / em espera',
  'l4_connect': 'CONNECT êxitos / falhas / tempos esgotados',
  'l4_buffers': 'Orçamento de buffer do aplicativo usado (bytes)',
  'l4_backpressure': 'Contrapressão de envio / recebimento',
  'l4_tun_flows': 'TUN TCP / meio aberto',
  'l4_udp': 'Pacotes UDP rejeitados',
  'l4_dns': 'Conversões DNS êxitos / falhas / tempos esgotados',
  'l4_migration':
      'Fluxos preservados pela migração / encerrados pela reconstrução',
  'l4_na':
      'Controle de endereço CONNECT-IP, filas DATAGRAM, MTU da carga interna e tempo limite UDP: não aplicável no L4.',
};

const Map<String, String> kNetworkSettingsPt = <String, String>{
  'settings_applying': 'Salvo, aplicando',
  'settings_applied': 'Salvo e aplicado',
  'settings_deferred': 'Salvo; vale na próxima conexão manual',
  'settings_failed': 'Salvo, falha ao aplicar',
  'settings_unknown': 'Resultado ainda não confirmado',
  'settings_saved': 'Salvo',
  'settings_unsupported':
      'Reinicie ou atualize o Engine para salvar as configurações de rede.',
  'settings_save_failed':
      'Não foi possível salvar as configurações. Suas edições foram mantidas.',
  'settings_reconnect': 'Reconectar',
};
