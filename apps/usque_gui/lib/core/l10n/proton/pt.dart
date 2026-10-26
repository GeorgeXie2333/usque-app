const kProtonPt = <String, String>{
  'fallback_account':
      'Não foi possível acessar o Proton. Selecione uma conta WARP existente e tente novamente.',
  'fallback_failed':
      'Não foi possível conectar pelo WARP. Verifique a conta WARP selecionada e a rede e tente novamente.',
  'fallback_starting': 'Conectando pelo WARP…',
  'fallback_active':
      'Esta operação da conta usa o WARP, incluindo a verificação.',
  'connection_api':
      'Uma solicitação à API do Proton falhou. Tente novamente ou confira a conta Proton em Proxy em cadeia.',
  'connection_authentication':
      'A autorização do Proton falhou. Abra Proxy em cadeia e entre novamente no Proton.',
  'connection_certificate':
      'O Proton rejeitou o certificado de conexão e a recuperação falhou. Reconecte ou entre novamente no Proton.',
  'connection_restricted':
      'O servidor Proton está temporariamente restrito. Tente novamente ou escolha outro servidor.',
  'connection_sessions':
      'O limite de conexões simultâneas do Proton foi atingido. Desconecte outra sessão Proton e tente novamente.',
  'connection_key':
      'O Proton detectou esta chave de conexão em várias sessões. Desconecte a outra sessão e tente novamente.',
  'connection_policy':
      'O Proton restringiu esta conexão conforme sua política de contas ou uso. Verifique a conta ou escolha outro servidor.',
  'connection_server':
      'O servidor Proton não conseguiu autorizar a conexão. Tente outro servidor.',
  'connection_protocol':
      'Não foi possível validar a resposta de conexão do Proton. Tente novamente ou escolha outro servidor.',
  'connection_transport':
      'Não foi possível acessar o Proton. Verifique a rede ou escolha outro servidor.',
  'connection_reconnect': 'A autorização do Proton foi renovada. Reconectando…',
  'connection_review':
      'Abra Proxy em cadeia para conferir a conta Proton e o servidor selecionado.',
  'connection_retry':
      'Tente a conexão Proton novamente ou escolha outro servidor.',
  'title': 'Proton VPN',
  'guest': 'Continuar como convidado',
  'login': 'Entrar no Proton',
  'username': 'Nome de usuário',
  'password': 'Senha',
  'totp': 'Código do aplicativo autenticador',
  'submit': 'Continuar',
  'verify': 'Concluir verificação',
  'verification_hint':
      'Resolva o quebra-cabeça na janela oficial de verificação.',
  'choose_node': 'Escolher um servidor gratuito',
  'working': 'Preparando o Proton VPN…',
  'waiting_totp': 'Digite o código do aplicativo autenticador para continuar.',
  'waiting_verification': 'Verificação necessária',
  'waiting_node': 'Escolha um servidor para concluir a configuração.',
  'completed':
      'Conta salva. Selecione-a e aplique as alterações para usar o Proton VPN.',
  'cancelled': 'Configuração cancelada',
  'failed': 'A configuração do Proton falhou. Tente novamente quando quiser.',
  'result_unconfirmed':
      'O resultado da configuração do Proton não foi confirmado. Atualize o status para continuar ou cancele a configuração atual antes de recomeçar.',
  'unavailable': 'O Proton VPN não está disponível neste mecanismo.',
  'bad_credentials':
      'Verifique seu nome de usuário Proton, senha ou código do autenticador.',
  'rate_limited':
      'O Proton limitou esta solicitação. Aguarde antes de tentar novamente.',
  'network':
      'Não foi possível acessar o Proton. Verifique a rede ou a VPN e tente novamente.',
  'verification_failed':
      'Não foi possível concluir a verificação. Reinicie a configuração.',
  'verification_invalid':
      'O Proton rejeitou o resultado da verificação. Reinicie a configuração para obter um novo desafio.',
  'accounts': 'Contas Proton',
  'add_account': 'Adicionar conta',
  'account_actions': 'Ações da conta',
  'resolve_on_connect':
      'O servidor e a autorização são verificados ao conectar.',
  'account': 'Conta Proton',
  'signed_in': 'Autenticado',
  'session_requires_login': 'A sessão requer autenticação',
  'selected_server': 'Servidor',
  'automatic_server': 'Automático',
  'automatic_hint': 'Escolher o servidor de menor latência ao conectar.',
  'available_servers': 'Servidores disponíveis',
  'choose_server': 'Escolher um servidor',
  'search_servers': 'Buscar por nome, país ou cidade',
  'load_servers': 'Carregar lista de servidores',
  'change_server': 'Trocar servidor',
  'refresh_servers': 'Atualizar lista',
  'remove_account': 'Remover conta',
  'remove_account_confirm':
      'Remover esta conta e sua escolha de servidor salva? Sair também revoga as sessões salvas por este aplicativo. Esquecer localmente mantém as sessões remotas inalteradas. A seleção salva do proxy em cadeia será apagada.',
  'forget_local': 'Esquecer localmente',
  'sign_out_remove': 'Sair e remover',
  'current_account': 'Conta conectada',
  'saved_account': 'Seleção salva',
  'next_connection': 'Escolha de servidor salva para a próxima conexão.',
  'disconnect_to_change':
      'Desconecte esta conta antes de trocar seu servidor ou removê-la.',
  'cleanup_pending':
      'Não foi possível confirmar a revogação de uma sessão anterior. Saia novamente ou escolha explicitamente esquecer a conta localmente.',
  'selected_server_unavailable':
      'O servidor selecionado não está disponível. Escolha outro servidor ou Automático.',
  'close': 'Fechar',
  'verification_error': 'Erro de verificação',
  'refresh': 'Renovar autorização ou trocar servidor',
  'logout': 'Sair e excluir',
  'forget': 'Esquecer apenas neste dispositivo',
  'forget_confirm':
      'Remover esta configuração Proton e sua sessão salva deste dispositivo? Isso não revoga a sessão Proton remota.',
  'removed': 'A conta Proton foi removida deste dispositivo.',
  'needs_login': 'Entrar novamente',
  'expires': 'Validade da autorização',
  'unsupported_second_factor':
      'Esta conta requer um método de autenticação não compatível aqui.',
  'reauthentication_required':
      'Entre novamente para renovar esta sessão Proton.',
  'profile_in_use': 'Desconecte esta conta antes de alterá-la ou removê-la.',
  'session_busy':
      'Outra operação Proton está em andamento. Aguarde ou cancele-a primeiro.',
  'api_rejected':
      'O Proton rejeitou esta solicitação. Os códigos abaixo identificam a resposta do servidor.',
  'session_not_allowed':
      'O Proton não permite que este cliente crie uma sessão.',
  'credentialless_disabled':
      'O acesso de convidado ao Proton está indisponível no momento.',
  'no_free_nodes':
      'O Proton não retornou servidores WireGuard disponíveis para esta conta.',
  'invalid_directory':
      'O Proton retornou uma lista de servidores incompatível.',
  'invalid_client_configuration':
      'O Proton não retornou portas WireGuard utilizáveis.',
  'stage': 'Etapa que falhou',
  'stage_anonymous_session': 'Criar uma sessão anônima',
  'stage_credentialless': 'Criar uma conta de convidado',
  'stage_session_scopes': 'Verificar autorização do convidado',
  'stage_protected_user': 'Verificar a conta',
  'stage_vpn_permissions': 'Verificar acesso à VPN',
  'stage_session_refresh': 'Renovar a sessão',
  'stage_srp_info': 'Iniciar autenticação',
  'stage_srp_proof': 'Verificar autenticação',
  'stage_totp': 'Verificar código do autenticador',
  'stage_directory': 'Carregar servidores',
  'stage_certificate': 'Solicitar autorização VPN',
  'stage_secure_storage': 'Salvar a configuração',
  'stage_session_revoke': 'Revogar a sessão',
  'expired':
      'A solicitação de verificação ou entrada expirou. Reinicie a configuração.',
  'underlay_changed': 'O ambiente de rede mudou. Reinicie a configuração.',
  'remote_unknown': 'Não foi possível confirmar a revogação da sessão remota.',
  'cancelled_remote_unknown':
      'Cancelado neste dispositivo. Não foi possível confirmar a revogação da sessão remota.',
  'privacy':
      'Sua senha é usada apenas para entrar. As sessões e chaves são protegidas neste dispositivo.',
  'empty': 'Entre ou continue como convidado para adicionar uma conta Proton.',
};
