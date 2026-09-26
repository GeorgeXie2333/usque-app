/// Supplemental feature strings for Portuguese (Brazil).
/// Not a full catalog: do not define app_version.
const Map<String, String> kUiWorkflowPt = <String, String>{
  'local_proxy_settings': 'Configurações do proxy local',
  'proxy_switches_hint':
      'As alterações dos interruptores são salvas automaticamente. Use Aplicar alterações para editar a escuta e o DNS.',
  'cc_label': 'Controle de congestionamento HTTP/3',
  'cc_help': 'Vale na próxima conexão manual.',
  'cc_upgrade': 'Atualize o Usque em Configurações para usar esta opção.',
  'cc_h2': 'Esta opção afeta apenas conexões HTTP/3.',
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
  'home_traffic_idle': 'O tráfego aparece após conectar',
  'home_traffic_waiting': 'Aguardando dados de tráfego',
  'home_traffic_unavailable': 'Sem histórico de tráfego',
  'home_traffic_stale': 'Atualização do tráfego atrasada',
  'home_outputs_next': 'Disponível após conectar',
  'home_outputs_retry': 'VPN e proxies da próxima conexão',
  'connection_protection_group': 'Conexão e proteção',
  'proxy_routing_group': 'Proxy e roteamento',
  'application_group': 'Aplicativo',
  'proxy_settings_link': 'Endereços do ouvinte, portas, autenticação e DNS.',
  'proxy_auth_separate':
      'Use Salvar usuário e senha abaixo para aplicar estas alterações.',
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
      'Usando o DNS da rede atual; as verificações de DNS criptografado não se aplicam.',
  'nq_finding_unsupported':
      'Atualize o Usque para usar DNS criptografado. As consultas não mudarão para DNS sem criptografia.',
  'nq_finding_dns_custom_valid':
      'A configuração DNS criptografada personalizada é válida. O fallback '
      'em texto simples está desabilitado.',
  'nq_finding_stale': 'A leitura está desatualizada ou a rede física mudou.',
  'nq_finding_rtt_high': 'O tempo de ida e volta medido está elevado.',
  'nq_finding_healthy':
      'As medições disponíveis da conexão estão dentro da faixa normal.',
  'nq_finding_loss_high': 'A perda de pacotes do intervalo está elevada.',
  'nq_finding_queue_pressure':
      'Há tráfego aguardando envio ou dados foram descartados nesta conexão.',
  'nq_finding_pmtu_degraded':
      'O Usque não confirmou um tamanho de pacote adequado para esta conexão.',
  'nq_finding_migration_reconnect':
      'Esta conexão precisa ser restabelecida ao mudar de rede.',
  'nq_finding_dns_changed':
      'O modo DNS salvo difere do da conexão em execução.',
  'nq_finding_dns_runtime': 'O DNS criptografado está funcionando.',
  'nq_finding_dns_degraded':
      'O DNS criptografado está com problemas. Consultas que falharem não usarão o DNS sem criptografia da rede.',
  'nq_finding_probe_unsafe':
      'Esta medição não está disponível no estado atual.',
  'nq_finding_probe_success': 'Esta verificação passou.',
  'nq_finding_probe_cancelled': 'Esta verificação foi cancelada.',
  'nq_finding_probe_timeout':
      'Tempo limite da verificação diagnóstica excedido',
  'nq_finding_probe_failed': 'Esta verificação falhou.',
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
      'Esta versão não mostra a qualidade da conexão. Conectar e desconectar continuam disponíveis. Procure atualizações em Configurações.',
  'nq_empty': 'Conecte-se para ver as medições.',
  'nq_stale_help':
      'As atualizações foram pausadas. Exibindo as últimas leituras.',
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
      'É o tamanho de pacote que o caminho de rede comporta. O Usque verifica automaticamente para reduzir perdas. A verificação não aumenta a MTU da VPN definida em Configurações avançadas de rede.',
  'nq_migration': 'Migração de rede',
  'nq_migration_help':
      'O Usque tenta manter a conexão ao trocar de rede, como de Wi-Fi para dados móveis. Ambas precisam usar a mesma versão IP, IPv4 ou IPv6. Só uma rede transporta tráfego por vez; as velocidades não são somadas.',
  'nq_attempts': 'Tentativas',
  'nq_successes': 'Bem-sucedidas',
  'nq_failures': 'Com falha',
  'nq_last_duration': 'Última duração',
  'nq_direct_dns': 'DNS direto',
  'nq_system_dns': 'DNS da rede atual',
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
      'A nova rede não pode usar a mesma versão IP. É necessário reconectar.',
  'nq_reason_socket_protect_failed':
      'O Usque não pôde usar a nova rede com segurança. Se a conexão não voltar, reconecte manualmente.',
  'nq_reason_generation_changed_during_setup':
      'A rede mudou novamente durante a preparação.',
  'nq_reason_peer_cid_unavailable':
      'O servidor não manteve a conexão na nova rede. Se ela não voltar, reconecte manualmente.',
  'nq_reason_local_cid_unavailable':
      'O Usque não manteve a conexão na nova rede. Se ela não voltar, reconecte manualmente.',
  'nq_reason_path_probe_rejected':
      'A nova rede não passou na verificação. Confira se ela tem acesso à Internet.',
  'nq_reason_path_validation_timeout':
      'A nova rede não respondeu a tempo. Verifique-a e reconecte se necessário.',
  'nq_reason_superseded': 'A rede mudou novamente antes do fim da troca.',
  'nq_reason_promotion_failed':
      'O Usque não concluiu a troca de rede com segurança. Se a conexão não voltar, reconecte manualmente.',
  'nq_reason_connection_closed':
      'A conexão foi encerrada durante a troca de rede. Reconecte para continuar.',
  'nq_reason_unsupported': 'A migração não está disponível nesta conexão.',
  'nq_reason_unknown': 'Nenhum motivo compatível está disponível.',
  'nq_dns_custom': 'Resolvedor criptografado personalizado',
  'nq_dns_server': 'Domínio do servidor DNS',
  'nq_dns_path': 'Caminho HTTPS',
  'nq_dns_port': 'Porta (0 usa o padrão)',
  'nq_dns_bootstrap': 'Endereços IP do servidor DNS',
  'nq_dns_bootstrap_help':
      'Informe de 1 a 8 IPs fornecidos pelo serviço DNS, um por linha, como 1.1.1.1. O Usque conecta diretamente a esses endereços sem consultar primeiro o nome do servidor.',
  'nq_dns_no_fallback':
      'Se o DNS direto criptografado falhar, a consulta falha. Nunca há '
      'fallback para o DNS do sistema ou em texto simples.',
  'nq_dns_system_privacy':
      'O provedor DNS da rede atual pode ver os domínios consultados pelo tráfego direto.',
  'nq_dns_scope':
      'Usado no tráfego correspondente às regras de países diretos. O DNS do tráfego VPN não muda.',
  'nq_dns_no_capability':
      'Atualize o Usque para usar DNS criptografado nas conexões diretas. Suas configurações são mantidas. Você pode escolher DNS da rede atual se aceitar o impacto na privacidade.',
  'nq_dns_invalid_name':
      'Informe um domínio como dns.example.com, sem https://, porta ou espaços.',
  'nq_dns_invalid_path':
      'Informe um caminho como /dns-query, com até 256 caracteres. Remova espaços e partes iniciadas por ? ou #.',
  'nq_dns_invalid_bootstrap': 'Informe de 1 a 8 endereços IP do servidor.',
  'nq_dns_invalid_port':
      'Informe uma porta de 1 a 65535 ou 0 para usar o padrão.',
  'nq_dns_invalid_mode': 'Escolha um modo DNS compatível.',
  'nq_doctor_deep_title': 'Executar verificações aprofundadas de rede?',
  'nq_doctor_deep_body':
      'As verificações podem enviar tráfego de teste. Duram até 15 segundos e podem ser canceladas. As configurações de conexão não serão alteradas.',
  'nq_doctor_deep_run': 'Executar verificações aprofundadas',
  'nq_doctor_evidence':
      'Estas verificações não confirmam se há vazamentos de DNS.',
};

const Map<String, String> kWindowsRecoveryPt = <String, String>{
  'WINDOWS_DEVICE_REUSE_UNSUPPORTED':
      'Os componentes de conexão do Usque precisam ser atualizados juntos. Procure atualizações em Configurações. Nenhuma nova conexão VPN foi iniciada.',
  'WINDOWS_DEVICE_RECOVERY_REQUIRED':
      'A limpeza da conexão VPN anterior não terminou. Feche o Usque por completo e abra novamente. Se continuar falhando, abra Diagnósticos.',
  'WINDOWS_RECOVERY_FAILED':
      'Não foi possível restaurar por completo o estado de rede VPN '
      'anterior. Nenhuma nova conexão VPN foi iniciada. Tente conectar '
      'novamente ou inspecione os diagnósticos locais.',
  'WINDOWS_RECOVERY_EXHAUSTED':
      'O Windows não conseguiu restaurar o estado de rede VPN anterior '
      'após três tentativas automáticas. Tente novamente quando estiver '
      'pronto ou inspecione os diagnósticos locais.',
  'WINDOWS_RECOVERY_BLOCKED':
      'O reparo automático parou porque não foi possível confirmar uma restauração segura das configurações VPN anteriores. Procure atualizações em Configurações; se persistir, exporte um pacote em Diagnósticos.',
  'WINDOWS_RECOVERY_TIMEOUT':
      'A recuperação de rede do Windows está demorando mais do que o '
      'esperado. Nenhuma nova conexão VPN foi iniciada. Aguarde o término '
      'da recuperação antes de tentar novamente.',
  'WINDOWS_RECOVERY_CONFLICT':
      'O estado da rede mudou ou ainda está em uso por outra sessão. A '
      'recuperação automática foi interrompida para proteger a conexão '
      'ativa.',
  'WINDOWS_RECOVERY_UNSUPPORTED':
      'Esta instalação não restaura automaticamente as configurações VPN anteriores. Atualize o Usque em Configurações antes de tentar novamente.',
};

const String kWindowsAdapterCleanupPt =
    'O adaptador virtual da conexão anterior não pôde ser removido ou sua remoção não foi confirmada. Nenhuma nova conexão VPN foi iniciada.';

const Map<String, String> kL4Pt = <String, String>{
  'l4_quic_not_ready': 'Preparando conexão L4',
  'l4_unsupported_packets': 'Pacotes incompatíveis ou malformados rejeitados',
  'l4_budget_rejections': 'Admissões de recurso rejeitadas',
  'l4_not_applicable': 'Não aplicável (L4)',
  'l4_mode': 'L4 (em fase experimental)',
  'l4_transport_hint':
      'Somente TCP. Apps que precisam de UDP podem não funcionar. O modo automático não inclui L4.',
  'l4_explanation':
      'L4 transporta TCP por HTTP/3 e funciona com VPN e proxies SOCKS5 e HTTP. Consultas DNS da VPN são convertidas para TCP. Aplicativos que exigem outro tráfego UDP, Ping remoto, fragmentos IP ou cabeçalhos de extensão podem não funcionar. Escolha L4 manualmente; o modo automático não o seleciona.',
  'l4_unsupported':
      'L4 não está disponível nesta versão do Usque. Procure atualizações em Configurações.',
  'l4_sni_identity':
      'Definido automaticamente pela conta. O nome do servidor dos outros modos de conexão é mantido.',
  'l4_edge_requires_l4':
      'O DNS resolvido na borda exige L4. Selecione outro modo de DNS do proxy antes de mudar para Auto, H3 ou H2.',
  'proxy_dns_edge_resolved':
      'Borda Cloudflare (somente L4; sem consulta local)',
  'l4_verified': 'L4 já estabeleceu uma conexão de aplicativo',
  'l4_unverified':
      'Servidor conectado; conexão de aplicativo ainda não confirmada',
  'l4_status_unknown':
      'Não é possível confirmar o status da conexão dos aplicativos',
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
      'Feche o Usque por completo, abra novamente e tente salvar. Se continuar falhando, procure atualizações em Configurações.',
  'settings_save_failed':
      'Não foi possível salvar as configurações. Suas edições foram mantidas.',
  'settings_reconnect': 'Reconectar',
};

const Map<String, String> kChainPt = <String, String>{
  "batch_title": "Importar configurações",
  "batch_counts":
      "Prontas: {ready} · Incompletas: {pending} · Falhas: {failed} · Salvas: {saved}",
  "batch_ready": "Pronta para importar",
  "batch_pending": "Preencha o nome ou as credenciais",
  "batch_saved": "Importada",
  "batch_close": "Fechar",
  "batch_import": "Importar itens válidos ({count})",
  "batch_checking": "Verificando {done} de {total}",
  "batch_saving": "Salvando configurações…",
  "batch_uncertain":
      "O salvamento foi interrompido. Feche e confira a biblioteca antes de importar novamente; alguns itens podem já estar salvos.",
  "file_count_limit": "Selecione no máximo 128 arquivos por vez.",
  'duplicate_directive': 'Esta diretiva só pode aparecer uma vez.',
  'mixed_protocols':
      'Todos os endpoints remote devem usar o mesmo transporte TCP ou UDP.',
  'conflicting_protocol': 'O remote entra em conflito com o transporte global.',
  'too_many_endpoints': 'Use no máximo 16 endpoints remote.',
  'conflicting_authentication':
      'CLIENT_CERT entra em conflito com o certificado embutido ou com o modo de autenticação.',
  'serialized_size_limit':
      'O registro criptografado excederia o limite de armazenamento.',
  'multi_endpoint_unavailable':
      'Atualize o mecanismo para usar configurações com vários endpoints.',
  'candidates': 'Endpoints de início',
  'random_order':
      'Tenta os endpoints em uma nova ordem aleatória a cada conexão.',
  'file_order': 'Tenta os endpoints na ordem do arquivo.',
  'attempting': 'Tentando endpoint',
  'actual_endpoint': 'Endpoint conectado',
  'attempt_failures': 'Tentativas com falha',
  'failure_transport': 'transporte encerrado',
  'failure_authentication': 'autenticação',
  'failure_certificate': 'certificado',
  'failure_configuration': 'configuração',
  'failure_address_changed': 'endereço alterado',
  'failure_protocol': 'protocolo',
  'failure_cleanup': 'limpeza',
  'failure_reason': 'Falha: {reason}.',
  'manage': 'Gerenciar',
  'dns_fallback': 'DNS do túnel (o OpenVPN pode negociar o DNS)',
  'dns_unavailable_title': 'Sem DNS por esta saída',
  'dns_unavailable':
      'Nenhum servidor DNS é acessível por esta saída. Use endereços IP ou escolha outra saída com DNS acessível.',
  'authentication_failed':
      'Falha na autenticação. Atualize as credenciais antes de conectar outra vez.',
  'profile_limit': 'A biblioteca de configurações está cheia (128 perfis).',
  'metadata_limit': 'Os metadados da biblioteca de configurações estão cheios.',
  'title': 'Proxy em cadeia',
  'subtitle': 'Escolha uma saída alcançada pelo WARP.',
  'source': 'Origem da saída',
  'enable': 'Ativar proxy em cadeia',
  'import_file': 'Importar arquivo',
  'paste': 'Colar configuração',
  'profiles': 'Configurações salvas',
  'empty': 'Importe uma configuração para escolher uma saída.',
  'empty_hint_openvpn':
      'Importe um arquivo .ovpn ou cole o texto. Endpoints TCP e UDP, certificados embutidos e usuário/senha são aceitos.',
  'empty_hint_wireguard':
      'Importe um arquivo .conf ou cole o texto. Uma seção [Interface] e uma [Peer] são aceitas.',
  'import_limits':
      'As configurações devem ser texto UTF-8 de até 128 KiB. Em aparelhos sem seletor de arquivos, cole o texto.',
  'enable_to_choose': 'Ative o proxy em cadeia para escolher uma configuração.',
  'select_required': 'Selecione uma configuração salva para aplicar.',
  'pending_disable': 'Pendente: desativar o proxy em cadeia',
  'apply_reconnect': 'Aplicar e reconectar',
  'requires_connect_ip': 'Exige CONNECT-IP',
  'menu': 'Ações da configuração',
  'preview': 'Verificar configuração',
  'save_import': 'Salvar configuração',
  'name': 'Nome',
  'configuration': 'Texto da configuração',
  'file_loaded': 'Configuração carregada do arquivo ({lines} linhas).',
  'username': 'Nome de usuário',
  'password': 'Senha',
  'key_password': 'Senha da chave privada',
  'show_password': 'Mostrar senha',
  'hide_password': 'Ocultar senha',
  'credentials': 'Atualizar credenciais',
  'rename': 'Renomear',
  'delete': 'Excluir',
  'cancel': 'Cancelar',
  'save': 'Salvar',
  'apply': 'Aplicar alterações',
  'clear': 'Limpar seleção',
  'current': 'Conexão atual',
  'saved': 'Seleção salva',
  'draft': 'Seleção pendente',
  'disconnected': 'Sem conexão',
  'disabled': 'Não ativado',
  'enabled_idle': 'Ativado · sem conexão',
  'disconnecting': 'Desconectando',
  'file_read_failed': 'Não foi possível ler o arquivo de configuração.',
  'file_encoding_invalid': 'O arquivo de configuração deve ser texto UTF-8.',
  'file_busy': 'Um seletor de arquivos já está aberto.',
  'connected': 'Conectado',
  'connecting': 'Conectando',
  'error': 'Falha na conexão',
  'no_selection': 'Nenhuma configuração selecionada',
  'l4': 'Esta configuração exige o modo CONNECT-IP.',
  'switch_mode': 'Mudar para CONNECT-IP e aplicar',
  'unsupported': 'Este mecanismo não aceita esta origem de saída.',
  'scope':
      'As regras diretas explícitas continuam valendo. O restante do tráfego usa a saída escolhida.',
  'allowed': 'Destinos permitidos',
  'dns': 'DNS',
  'addresses': 'Endereços do túnel',
  'address_family': 'Família de endereços',
  'transport': 'Transporte',
  'endpoint': 'Servidor',
  'restricted':
      'Destinos fora de AllowedIPs são bloqueados no caminho do proxy.',
  'delete_confirm':
      'Excluir esta configuração salva? O arquivo importado original não muda.',
  'profile_in_use':
      'Escolha outra configuração ou limpe a seleção salva antes de excluir esta.',
  'stale_revision': 'A configuração mudou. Atualize a lista e tente de novo.',
  'secure_storage_failed':
      'Não foi possível ler nem salvar a configuração criptografada.',
  'invalid_configuration':
      'A configuração é inválida ou contém opções não aceitas.',
  'looks_like_wireguard':
      'Isto parece uma configuração WireGuard. Mude a origem da saída para WireGuard.',
  'looks_like_openvpn':
      'Isto parece uma configuração OpenVPN. Mude a origem da saída para OpenVPN.',
  'error_location': '{message} ({field}, linha {line})',
  'error_field': '{message} ({field})',
  'file_unavailable':
      'Não há seletor de arquivos. Cole o texto da configuração.',
  'invalid_size_or_encoding':
      'Use uma configuração UTF-8 de no máximo 128 KiB.',
  'unsupported_directive': 'Esta diretiva OpenVPN não é aceita.',
  'unsupported_or_duplicate_field':
      'Este campo não é aceito ou está duplicado.',
  'unsupported_or_duplicate_section': 'Use uma seção Interface e uma Peer.',
  'missing_field': 'Falta um campo obrigatório.',
  'invalid_name':
      'Use um nome com 1 a 64 caracteres, sem caracteres de controle.',
  'invalid_key': 'A chave deve ser uma chave Base64 válida de 32 bytes.',
  'checking': 'Verificando a configuração…',
  'changed': 'Alterações salvas',
};
