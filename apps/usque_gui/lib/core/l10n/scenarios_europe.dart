// Scene translations share the keys and placeholders of kScenarioEn.

const kScenarioJa = <String, String>{
  'scenario_platform_unsupported': 'このシーンには、このデバイスで利用できない設定が含まれています。',
  'scenario_limit': 'シーンまたはシーンライブラリが上限を超えています。ルールを減らすか、不要なシーンを削除してください。',
  'scenario_vpn_permission': "先に切断してこのシーンを選択し、再接続して VPN の使用を許可してください。",
  'scenario_rules_missing': "このシーンを使用する前に、設定で必要な分流ルールデータを用意してください。",
  'profiles': "プロファイル",
  'nav_profiles': "構成",
  'profiles_subtitle': "アカウントとネットワークシーンを選択します。",
  'scenario_scenes': "シーン",
  'scenario_accounts': "アカウント",
  'scenario_create': "新しいシーン",
  'scenario_empty': "最初のシーンを保存",
  'scenario_empty_hint': "現在のネットワーク設定を再利用できるシーンとして保存します。",
  'scenario_name': "シーン名",
  'scenario_binding': "WARP アカウント",
  'scenario_follow_account': "現在のアカウントを使用",
  'scenario_bound_account': "紐付けたアカウント",
  'scenario_use': "シーンを使用",
  'scenario_duplicate': "複製",
  'scenario_update_current': "現在の設定で更新",
  'scenario_save_as': "新しいシーンとして保存",
  'scenario_save': "シーンを保存",
  'scenario_save_use': "保存して使用",
  'scenario_preview': "シーンの変更を確認",
  'scenario_restore': "切り替え前の設定を復元",
  'scenario_selected': "選択済み",
  'scenario_applied': "適用済み",
  'scenario_modified': "変更あり",
  'scenario_binding_modified': "アカウントが紐付けと異なります",
  'scenario_template_updated': "シーンに更新があります",
  'scenario_pending': "保存済み・適用待ち",
  'scenario_failed': "保存済み・適用失敗",
  'scenario_unknown': "結果を確認できません",
  'scenario_custom': "カスタム",
  'scenario_unsupported': "このエンジンはシーンに対応していません。アカウント管理は利用できます。",
  'scenario_save_failed': "操作を完了できませんでした。状態を更新してから再試行してください。",
  'scenario_reference_missing': "このシーンで必要なアカウントまたは出口を利用できません。",
  'scenario_global_hint':
      "Kill Switch、プロキシのアドレスと認証、詳細調整、アプリ別の選択にはデバイス設定を使用します。",
  'scenario_output': "出力",
  'scenario_exit': "出口",
  'scenario_connection': "接続設定",
  'scenario_current': "選択したシーン",
  'scenario_current_account': "選択したアカウント",
  'scenario_running_account': "接続中のアカウント",
  'scenario_no_changes': "選択した設定はすでに一致しています。",
  'scenario_will_reconnect': "エンジンが必要な接続手順で変更を適用します。再接続が必要になる場合があります。",
  'scenario_disconnected_hint': "接続は切断されたままになります。",
  'scenario_delete_title': "シーンを削除しますか？",
  'scenario_delete_body': "保存したシーンを削除します。現在のネットワーク設定は保持されます。",
  'scenario_validation_name': "1～64 文字の名前を入力してください。",
  'scenario_keep_account': "現在のアカウントを保持します。",
  'scenario_account_change': "アカウントの変更",
  'scenario_library_hint': "既存の出口を選択します。インポートとログインはプロキシページで管理してください。",
  'scenario_saved': "シーンを保存しました",
  'scenario_country_hint':
      "国コードをカンマ区切りで入力します（例：CN, JP）。使用前に必要なルールデータを用意してください。",
  'scenario_rules_count': "カスタムルール {count} 件",
  'scenario_confirm_use': "シーンを適用",
  'scenario_invalid_dns': "有効な DNS の IP アドレスを入力してください。",
  'scenario_new_flows': "変更は新しい接続に適用されます。既存の接続は元の経路を使い続ける場合があります。",
  'scenario_not_applied': "シーンは適用されませんでした。現在の設定は変更されていません。",
  'scenario_kill_switch_output':
      "Kill Switch がオンですが、このシーンは {tunnel} をオフにします。切り替え後はシステム全体の保護が停止します。",
  'scenario_settings_invalid': "このシーンに保存された設定は無効になりました。編集して保存し直してください。",
  'protection_handoff_unavailable':
      "この切り替えでは Kill Switch の保護を維持できません。接続を切断してから切り替えてください。",
  'scenario_confirm_before_connect': "前回のシーン切り替えを確認中です。シーンを更新してから接続してください。",
  'scenario_unsaved': 'シーンに未保存の変更があります',
  'scenario_inherited_changes': 'あわせて適用されるその他の保存済み設定',
};

const kScenarioKo = <String, String>{
  'scenario_platform_unsupported': '이 시나리오에는 이 기기에서 사용할 수 없는 설정이 있습니다.',
  'scenario_limit':
      '시나리오 또는 시나리오 라이브러리가 한도를 초과했습니다. 규칙을 줄이거나 사용하지 않는 시나리오를 삭제하세요.',
  'scenario_vpn_permission': "먼저 연결을 해제하고 이 시나리오를 선택한 뒤, 연결하여 VPN 권한을 허용하세요.",
  'scenario_rules_missing': "이 시나리오를 사용하기 전에 설정에서 필요한 라우팅 규칙 데이터를 준비하세요.",
  'profiles': "프로필",
  'nav_profiles': "프로필",
  'profiles_subtitle': "계정과 네트워크 시나리오를 선택하세요.",
  'scenario_scenes': "시나리오",
  'scenario_accounts': "계정",
  'scenario_create': "새 시나리오",
  'scenario_empty': "첫 시나리오 저장",
  'scenario_empty_hint': "현재 네트워크 설정을 다시 사용할 수 있는 시나리오로 저장합니다.",
  'scenario_name': "시나리오 이름",
  'scenario_binding': "WARP 계정",
  'scenario_follow_account': "현재 계정 사용",
  'scenario_bound_account': "연결된 계정",
  'scenario_use': "시나리오 사용",
  'scenario_duplicate': "복제",
  'scenario_update_current': "현재 설정으로 업데이트",
  'scenario_save_as': "새 시나리오로 저장",
  'scenario_save': "시나리오 저장",
  'scenario_save_use': "저장 후 사용",
  'scenario_preview': "시나리오 변경 사항 확인",
  'scenario_restore': "전환 전 설정 복원",
  'scenario_selected': "선택됨",
  'scenario_applied': "적용됨",
  'scenario_modified': "수정됨",
  'scenario_binding_modified': "계정이 연결된 계정과 다릅니다",
  'scenario_template_updated': "시나리오가 업데이트되었습니다",
  'scenario_pending': "저장됨 · 적용 대기 중",
  'scenario_failed': "저장됨 · 적용 실패",
  'scenario_unknown': "결과 미확인",
  'scenario_custom': "사용자 지정",
  'scenario_unsupported': "이 엔진은 시나리오를 지원하지 않습니다. 계정 관리는 사용할 수 있습니다.",
  'scenario_save_failed': "작업을 완료하지 못했습니다. 상태를 새로 고친 후 다시 시도하세요.",
  'scenario_reference_missing': "이 시나리오에 필요한 계정이나 출구를 사용할 수 없습니다.",
  'scenario_global_hint':
      "Kill Switch, 프록시 주소와 인증, 고급 조정 및 앱별 선택은 기기 설정을 사용합니다.",
  'scenario_output': "출력",
  'scenario_exit': "출구",
  'scenario_connection': "연결 환경설정",
  'scenario_current': "선택한 시나리오",
  'scenario_current_account': "선택한 계정",
  'scenario_running_account': "연결 중인 계정",
  'scenario_no_changes': "선택한 설정이 이미 일치합니다.",
  'scenario_will_reconnect':
      "엔진이 필요한 연결 절차에 따라 변경 사항을 적용합니다. 다시 연결해야 할 수 있습니다.",
  'scenario_disconnected_hint': "연결이 해제된 상태를 유지합니다.",
  'scenario_delete_title': "시나리오를 삭제할까요?",
  'scenario_delete_body': "저장된 시나리오를 삭제합니다. 현재 네트워크 설정은 유지됩니다.",
  'scenario_validation_name': "1~64자의 이름을 입력하세요.",
  'scenario_keep_account': "현재 계정을 유지합니다.",
  'scenario_account_change': "계정 변경",
  'scenario_library_hint': "기존 출구를 선택하세요. 가져오기와 로그인은 프록시 페이지에서 관리합니다.",
  'scenario_saved': "시나리오 저장됨",
  'scenario_country_hint':
      "국가 코드를 쉼표로 구분해 입력하세요(예: CN, JP). 사용 전에 필요한 규칙 데이터가 준비되어야 합니다.",
  'scenario_rules_count': "사용자 지정 규칙 {count}개",
  'scenario_confirm_use': "시나리오 적용",
  'scenario_invalid_dns': "올바른 DNS IP 주소를 입력하세요.",
  'scenario_new_flows': "변경 사항은 새 연결에 적용됩니다. 기존 연결은 이전 경로를 계속 사용할 수 있습니다.",
  'scenario_not_applied': "시나리오가 적용되지 않았습니다. 현재 설정은 바뀌지 않았습니다.",
  'scenario_kill_switch_output':
      "Kill Switch가 켜져 있지만 이 시나리오는 {tunnel}을(를) 끕니다. 전환 후에는 시스템 전체 보호가 중지됩니다.",
  'scenario_settings_invalid':
      "이 시나리오에 저장된 설정이 더 이상 유효하지 않습니다. 편집한 뒤 다시 저장하세요.",
  'protection_handoff_unavailable':
      "이번 전환에서는 Kill Switch 보호를 유지할 수 없습니다. 연결을 끊은 뒤 다시 전환하세요.",
  'scenario_confirm_before_connect':
      "이전 시나리오 전환을 아직 확인하는 중입니다. 시나리오를 새로 고친 뒤 다시 연결하세요.",
  'scenario_unsaved': '저장하지 않은 시나리오 변경 사항',
  'scenario_inherited_changes': '함께 적용되는 기타 저장된 설정',
};

const kScenarioEs = <String, String>{
  'scenario_platform_unsupported':
      'Esta escena contiene un ajuste no disponible en este dispositivo.',
  'scenario_limit':
      'La escena o su biblioteca supera el límite. Reduce las reglas o elimina una escena que no uses.',
  'scenario_vpn_permission':
      "Desconecta, selecciona este escenario y vuelve a conectar para conceder el permiso de VPN.",
  'scenario_rules_missing':
      "Prepara los datos de enrutamiento necesarios en Ajustes antes de usar este escenario.",
  'profiles': "Perfiles",
  'nav_profiles': "Perfiles",
  'profiles_subtitle': "Elige tu cuenta y tu escenario de red.",
  'scenario_scenes': "Escenarios",
  'scenario_accounts': "Cuentas",
  'scenario_create': "Nuevo escenario",
  'scenario_empty': "Guarda tu primer escenario",
  'scenario_empty_hint':
      "Guarda los ajustes de red actuales como un escenario reutilizable.",
  'scenario_name': "Nombre del escenario",
  'scenario_binding': "Cuenta de WARP",
  'scenario_follow_account': "Usar la cuenta actual",
  'scenario_bound_account': "Cuenta vinculada",
  'scenario_use': "Usar escenario",
  'scenario_duplicate': "Duplicar",
  'scenario_update_current': "Actualizar con los ajustes actuales",
  'scenario_save_as': "Guardar como nuevo escenario",
  'scenario_save': "Guardar escenario",
  'scenario_save_use': "Guardar y usar",
  'scenario_preview': "Revisar los cambios del escenario",
  'scenario_restore': "Restaurar los ajustes anteriores",
  'scenario_selected': "Seleccionado",
  'scenario_applied': "Aplicado",
  'scenario_modified': "Modificado",
  'scenario_binding_modified': "La cuenta no coincide con la vinculada",
  'scenario_template_updated': "El escenario tiene una actualización",
  'scenario_pending': "Guardado; pendiente de aplicar",
  'scenario_failed': "Guardado; no se pudo aplicar",
  'scenario_unknown': "Resultado sin confirmar",
  'scenario_custom': "Personalizado",
  'scenario_unsupported':
      "Este motor no admite escenarios. Puedes gestionar las cuentas.",
  'scenario_save_failed':
      "No se pudo completar la operación. Actualiza el estado antes de volver a intentarlo.",
  'scenario_reference_missing':
      "Una cuenta o salida necesaria para este escenario no está disponible.",
  'scenario_global_hint':
      "Kill Switch, las direcciones y la autenticación del proxy, los ajustes avanzados y la selección por aplicación usan los ajustes del dispositivo.",
  'scenario_output': "Salidas",
  'scenario_exit': "Salida",
  'scenario_connection': "Preferencias de conexión",
  'scenario_current': "Escenario seleccionado",
  'scenario_current_account': "Cuenta seleccionada",
  'scenario_running_account': "Cuenta conectada",
  'scenario_no_changes': "Los ajustes seleccionados ya coinciden.",
  'scenario_will_reconnect':
      "El motor aplicará los cambios mediante el proceso de conexión necesario. Puede ser necesario reconectar.",
  'scenario_disconnected_hint': "La conexión permanecerá desconectada.",
  'scenario_delete_title': "¿Eliminar escenario?",
  'scenario_delete_body':
      "Se eliminará este escenario guardado. Se conservarán los ajustes de red actuales.",
  'scenario_validation_name': "Introduce un nombre de entre 1 y 64 caracteres.",
  'scenario_keep_account': "Se mantendrá la cuenta actual.",
  'scenario_account_change': "Cambio de cuenta",
  'scenario_library_hint':
      "Elige una salida existente. Gestiona las importaciones y el inicio de sesión en Proxy.",
  'scenario_saved': "Escenario guardado",
  'scenario_country_hint':
      "Códigos de país separados por comas (por ejemplo, CN, JP). Los datos de reglas necesarios deben estar disponibles antes de usarlo.",
  'scenario_rules_count': "{count} reglas personalizadas",
  'scenario_confirm_use': "Aplicar escenario",
  'scenario_invalid_dns': "Introduce direcciones IP de DNS válidas.",
  'scenario_new_flows':
      "Los cambios se aplican a las conexiones nuevas; las existentes pueden conservar sus rutas.",
  'scenario_not_applied':
      "El escenario no se aplicó. Los ajustes actuales no cambiaron.",
  'scenario_kill_switch_output':
      "Kill Switch está activado, pero este escenario desactiva {tunnel}. La protección de todo el sistema se detiene tras el cambio.",
  'scenario_settings_invalid':
      "Los ajustes guardados de este escenario ya no son válidos. Edítalo y vuelve a guardarlo.",
  'protection_handoff_unavailable':
      "No se puede mantener la protección de Kill Switch durante este cambio. Desconéctate y vuelve a cambiar.",
  'scenario_confirm_before_connect':
      "El último cambio de escenario aún se está confirmando. Actualiza Escenarios y vuelve a conectar.",
  'scenario_unsaved': 'Cambios del escenario sin guardar',
  'scenario_inherited_changes':
      'Otros ajustes guardados que también se aplican',
};

const kScenarioPt = <String, String>{
  'scenario_platform_unsupported':
      'Esta cena contém uma definição indisponível neste dispositivo.',
  'scenario_limit':
      'A cena ou a biblioteca de cenas excede o limite. Reduza as regras ou remova uma cena não utilizada.',
  'scenario_vpn_permission':
      "Desligue a ligação, selecione este cenário e volte a ligar para conceder a permissão de VPN.",
  'scenario_rules_missing':
      "Prepare os dados de encaminhamento necessários nas Definições antes de usar este cenário.",
  'profiles': "Perfis",
  'nav_profiles': "Perfis",
  'profiles_subtitle': "Escolha a sua conta e o cenário de rede.",
  'scenario_scenes': "Cenários",
  'scenario_accounts': "Contas",
  'scenario_create': "Novo cenário",
  'scenario_empty': "Guarde o seu primeiro cenário",
  'scenario_empty_hint':
      "Guarde as definições de rede atuais como um cenário reutilizável.",
  'scenario_name': "Nome do cenário",
  'scenario_binding': "Conta WARP",
  'scenario_follow_account': "Usar a conta atual",
  'scenario_bound_account': "Conta associada",
  'scenario_use': "Usar cenário",
  'scenario_duplicate': "Duplicar",
  'scenario_update_current': "Atualizar com as definições atuais",
  'scenario_save_as': "Guardar como novo cenário",
  'scenario_save': "Guardar cenário",
  'scenario_save_use': "Guardar e usar",
  'scenario_preview': "Rever as alterações do cenário",
  'scenario_restore': "Restaurar as definições anteriores",
  'scenario_selected': "Selecionado",
  'scenario_applied': "Aplicado",
  'scenario_modified': "Modificado",
  'scenario_binding_modified': "A conta difere da conta associada",
  'scenario_template_updated': "O cenário tem uma atualização",
  'scenario_pending': "Guardado; a aguardar aplicação",
  'scenario_failed': "Guardado; falha ao aplicar",
  'scenario_unknown': "Resultado não confirmado",
  'scenario_custom': "Personalizado",
  'scenario_unsupported':
      "Este motor não suporta cenários. A gestão de contas está disponível.",
  'scenario_save_failed':
      "Não foi possível concluir a operação. Atualize o estado antes de tentar novamente.",
  'scenario_reference_missing':
      "Uma conta ou saída necessária para este cenário não está disponível.",
  'scenario_global_hint':
      "O Kill Switch, os endereços e a autenticação do proxy, os ajustes avançados e a seleção por aplicação usam as definições do dispositivo.",
  'scenario_output': "Saídas",
  'scenario_exit': "Saída",
  'scenario_connection': "Preferências de ligação",
  'scenario_current': "Cenário selecionado",
  'scenario_current_account': "Conta selecionada",
  'scenario_running_account': "Conta ligada",
  'scenario_no_changes': "As definições selecionadas já coincidem.",
  'scenario_will_reconnect':
      "O motor aplicará estas alterações através do processo de ligação necessário. Pode ser necessário voltar a ligar.",
  'scenario_disconnected_hint': "A ligação permanecerá desligada.",
  'scenario_delete_title': "Eliminar cenário?",
  'scenario_delete_body':
      "Eliminar este cenário guardado. As definições de rede atuais serão mantidas.",
  'scenario_validation_name': "Introduza um nome com 1 a 64 caracteres.",
  'scenario_keep_account': "A conta atual será mantida.",
  'scenario_account_change': "Alteração de conta",
  'scenario_library_hint':
      "Escolha uma saída existente. Gira as importações e o início de sessão em Proxy.",
  'scenario_saved': "Cenário guardado",
  'scenario_country_hint':
      "Códigos de país separados por vírgulas (por exemplo, CN, JP). Os dados de regras necessários devem estar disponíveis antes da utilização.",
  'scenario_rules_count': "{count} regras personalizadas",
  'scenario_confirm_use': "Aplicar cenário",
  'scenario_invalid_dns': "Introduza endereços IP de DNS válidos.",
  'scenario_new_flows':
      "As alterações aplicam-se às novas ligações; as ligações existentes podem manter os seus percursos.",
  'scenario_not_applied':
      "O cenário não foi aplicado. As definições atuais não foram alteradas.",
  'scenario_kill_switch_output':
      "O Kill Switch está ativado, mas este cenário desativa {tunnel}. A proteção de todo o sistema para após a mudança.",
  'scenario_settings_invalid':
      "As definições guardadas deste cenário já não são válidas. Edite-o e guarde-o novamente.",
  'protection_handoff_unavailable':
      "Não é possível manter a proteção do Kill Switch durante esta mudança. Desligue e volte a mudar.",
  'scenario_confirm_before_connect':
      "A última mudança de cenário ainda está a ser confirmada. Atualize Cenários e volte a ligar.",
  'scenario_unsaved': 'Alterações do cenário por guardar',
  'scenario_inherited_changes':
      'Outras definições guardadas que também são aplicadas',
};

const kScenarioFr = <String, String>{
  'scenario_platform_unsupported':
      'Cette scène contient un réglage indisponible sur cet appareil.',
  'scenario_limit':
      'La scène ou la bibliothèque dépasse la limite. Réduisez les règles ou supprimez une scène inutilisée.',
  'scenario_vpn_permission':
      "Déconnectez-vous, sélectionnez ce scénario, puis reconnectez-vous pour autoriser le VPN.",
  'scenario_rules_missing':
      "Préparez les données de routage nécessaires dans les réglages avant d’utiliser ce scénario.",
  'profiles': "Profils",
  'nav_profiles': "Profils",
  'profiles_subtitle': "Choisissez votre compte et votre scénario réseau.",
  'scenario_scenes': "Scénarios",
  'scenario_accounts': "Comptes",
  'scenario_create': "Nouveau scénario",
  'scenario_empty': "Enregistrez votre premier scénario",
  'scenario_empty_hint':
      "Enregistrez les réglages réseau actuels dans un scénario réutilisable.",
  'scenario_name': "Nom du scénario",
  'scenario_binding': "Compte WARP",
  'scenario_follow_account': "Utiliser le compte actuel",
  'scenario_bound_account': "Compte associé",
  'scenario_use': "Utiliser le scénario",
  'scenario_duplicate': "Dupliquer",
  'scenario_update_current': "Actualiser avec les réglages actuels",
  'scenario_save_as': "Enregistrer comme nouveau scénario",
  'scenario_save': "Enregistrer le scénario",
  'scenario_save_use': "Enregistrer et utiliser",
  'scenario_preview': "Vérifier les changements du scénario",
  'scenario_restore': "Restaurer les réglages précédents",
  'scenario_selected': "Sélectionné",
  'scenario_applied': "Appliqué",
  'scenario_modified': "Modifié",
  'scenario_binding_modified': "Le compte diffère du compte associé",
  'scenario_template_updated': "Le scénario a été mis à jour",
  'scenario_pending': "Enregistré ; application en attente",
  'scenario_failed': "Enregistré ; échec de l’application",
  'scenario_unknown': "Résultat non confirmé",
  'scenario_custom': "Personnalisé",
  'scenario_unsupported':
      "Ce moteur ne prend pas en charge les scénarios. La gestion des comptes reste disponible.",
  'scenario_save_failed':
      "L’opération n’a pas pu être terminée. Actualisez l’état avant de réessayer.",
  'scenario_reference_missing':
      "Un compte ou un point de sortie nécessaire à ce scénario est indisponible.",
  'scenario_global_hint':
      "Kill Switch, les adresses et l’authentification du proxy, les réglages avancés et la sélection par application utilisent les réglages de l’appareil.",
  'scenario_output': "Sorties",
  'scenario_exit': "Point de sortie",
  'scenario_connection': "Préférences de connexion",
  'scenario_current': "Scénario sélectionné",
  'scenario_current_account': "Compte sélectionné",
  'scenario_running_account': "Compte connecté",
  'scenario_no_changes': "Les réglages sélectionnés correspondent déjà.",
  'scenario_will_reconnect':
      "Le moteur appliquera les changements selon la procédure de connexion nécessaire. Une reconnexion peut être requise.",
  'scenario_disconnected_hint': "La connexion restera coupée.",
  'scenario_delete_title': "Supprimer le scénario ?",
  'scenario_delete_body':
      "Supprimer ce scénario enregistré. Les réglages réseau actuels seront conservés.",
  'scenario_validation_name': "Saisissez un nom de 1 à 64 caractères.",
  'scenario_keep_account': "Le compte actuel sera conservé.",
  'scenario_account_change': "Changement de compte",
  'scenario_library_hint':
      "Choisissez une sortie existante. Gérez les importations et la connexion au compte dans Proxy.",
  'scenario_saved': "Scénario enregistré",
  'scenario_country_hint':
      "Codes de pays séparés par des virgules (par exemple CN, JP). Les données des règles nécessaires doivent être disponibles avant utilisation.",
  'scenario_rules_count': "{count} règles personnalisées",
  'scenario_confirm_use': "Appliquer le scénario",
  'scenario_invalid_dns': "Saisissez des adresses IP de DNS valides.",
  'scenario_new_flows':
      "Les changements s’appliquent aux nouvelles connexions ; les connexions existantes peuvent conserver leur chemin.",
  'scenario_not_applied':
      "Le scénario n’a pas été appliqué. Les réglages actuels sont inchangés.",
  'scenario_kill_switch_output':
      "Kill Switch est activé, mais ce scénario désactive {tunnel}. La protection de tout le système s’arrête après le changement.",
  'scenario_settings_invalid':
      "Les réglages enregistrés de ce scénario ne sont plus valides. Modifiez-le puis enregistrez-le à nouveau.",
  'protection_handoff_unavailable':
      "La protection Kill Switch ne peut pas être maintenue pendant ce changement. Déconnectez-vous, puis changez à nouveau.",
  'scenario_confirm_before_connect':
      "Le dernier changement de scénario est encore en cours de confirmation. Actualisez Scénarios, puis reconnectez-vous.",
  'scenario_unsaved': 'Modifications du scénario non enregistrées',
  'scenario_inherited_changes':
      'Autres paramètres enregistrés également appliqués',
};

const kScenarioNl = <String, String>{
  'scenario_platform_unsupported':
      'Deze scène bevat een instelling die op dit apparaat niet beschikbaar is.',
  'scenario_limit':
      'De scène of bibliotheek overschrijdt de limiet. Verminder het aantal regels of verwijder een ongebruikte scène.',
  'scenario_vpn_permission':
      "Verbreek de verbinding, selecteer dit scenario en maak daarna verbinding om VPN-toestemming te geven.",
  'scenario_rules_missing':
      "Bereid de vereiste routeringsgegevens voor in Instellingen voordat je dit scenario gebruikt.",
  'profiles': "Profielen",
  'nav_profiles': "Profielen",
  'profiles_subtitle': "Kies je account en netwerkscenario.",
  'scenario_scenes': "Scenario’s",
  'scenario_accounts': "Accounts",
  'scenario_create': "Nieuw scenario",
  'scenario_empty': "Sla je eerste scenario op",
  'scenario_empty_hint':
      "Sla de huidige netwerkinstellingen op als een herbruikbaar scenario.",
  'scenario_name': "Scenarionaam",
  'scenario_binding': "WARP-account",
  'scenario_follow_account': "Huidig account gebruiken",
  'scenario_bound_account': "Gekoppeld account",
  'scenario_use': "Scenario gebruiken",
  'scenario_duplicate': "Dupliceren",
  'scenario_update_current': "Bijwerken met huidige instellingen",
  'scenario_save_as': "Opslaan als nieuw scenario",
  'scenario_save': "Scenario opslaan",
  'scenario_save_use': "Opslaan en gebruiken",
  'scenario_preview': "Scenariowijzigingen controleren",
  'scenario_restore': "Vorige instellingen herstellen",
  'scenario_selected': "Geselecteerd",
  'scenario_applied': "Toegepast",
  'scenario_modified': "Gewijzigd",
  'scenario_binding_modified': "Account wijkt af van het gekoppelde account",
  'scenario_template_updated': "Er is een update voor het scenario",
  'scenario_pending': "Opgeslagen; wacht op toepassing",
  'scenario_failed': "Opgeslagen; toepassen mislukt",
  'scenario_unknown': "Resultaat niet bevestigd",
  'scenario_custom': "Aangepast",
  'scenario_unsupported':
      "Deze engine ondersteunt geen scenario’s. Accountbeheer blijft beschikbaar.",
  'scenario_save_failed':
      "De bewerking kon niet worden voltooid. Vernieuw de status voordat je het opnieuw probeert.",
  'scenario_reference_missing':
      "Een account of uitgang die voor dit scenario nodig is, is niet beschikbaar.",
  'scenario_global_hint':
      "Kill Switch, proxyadressen en authenticatie, geavanceerde afstemming en selectie per app gebruiken de apparaatinstellingen.",
  'scenario_output': "Uitgangen",
  'scenario_exit': "Uitgang",
  'scenario_connection': "Verbindingsvoorkeuren",
  'scenario_current': "Geselecteerd scenario",
  'scenario_current_account': "Geselecteerd account",
  'scenario_running_account': "Verbonden account",
  'scenario_no_changes': "De geselecteerde instellingen komen al overeen.",
  'scenario_will_reconnect':
      "De engine past deze wijzigingen toe via de vereiste verbindingsprocedure. Mogelijk moet de verbinding opnieuw worden gemaakt.",
  'scenario_disconnected_hint': "De verbinding blijft verbroken.",
  'scenario_delete_title': "Scenario verwijderen?",
  'scenario_delete_body':
      "Dit opgeslagen scenario verwijderen. De huidige netwerkinstellingen blijven behouden.",
  'scenario_validation_name': "Voer een naam van 1–64 tekens in.",
  'scenario_keep_account': "Het huidige account blijft behouden.",
  'scenario_account_change': "Accountwijziging",
  'scenario_library_hint':
      "Kies een bestaande uitgang. Beheer imports en aanmeldingen op de pagina Proxy.",
  'scenario_saved': "Scenario opgeslagen",
  'scenario_country_hint':
      "Landcodes, gescheiden door komma’s (bijvoorbeeld CN, JP). De vereiste regelgegevens moeten vóór gebruik beschikbaar zijn.",
  'scenario_rules_count': "{count} aangepaste regels",
  'scenario_confirm_use': "Scenario toepassen",
  'scenario_invalid_dns': "Voer geldige DNS-IP-adressen in.",
  'scenario_new_flows':
      "Wijzigingen gelden voor nieuwe verbindingen; bestaande verbindingen kunnen hun huidige routes behouden.",
  'scenario_not_applied':
      "Het scenario is niet toegepast. De huidige instellingen zijn ongewijzigd.",
  'scenario_kill_switch_output':
      "Kill Switch staat aan, maar dit scenario schakelt {tunnel} uit. De systeembrede bescherming stopt na het wisselen.",
  'scenario_settings_invalid':
      "De opgeslagen instellingen van dit scenario zijn niet meer geldig. Bewerk het en sla het opnieuw op.",
  'protection_handoff_unavailable':
      "De Kill Switch-bescherming kan tijdens deze wissel niet behouden blijven. Verbreek de verbinding en wissel opnieuw.",
  'scenario_confirm_before_connect':
      "De laatste scenariowissel wordt nog bevestigd. Vernieuw Scenario's en maak opnieuw verbinding.",
  'scenario_unsaved': 'Niet-opgeslagen scenariowijzigingen',
  'scenario_inherited_changes':
      'Andere opgeslagen instellingen die ook worden toegepast',
};

const kScenarioTr = <String, String>{
  'scenario_platform_unsupported':
      'Bu sahne, bu cihazda kullanılamayan bir ayar içeriyor.',
  'scenario_limit':
      'Sahne veya sahne kitaplığı sınırı aşıyor. Kuralları azaltın veya kullanılmayan bir sahneyi silin.',
  'scenario_vpn_permission':
      "Bağlantıyı kesin, bu senaryoyu seçin ve VPN iznini vermek için yeniden bağlanın.",
  'scenario_rules_missing':
      "Bu senaryoyu kullanmadan önce Ayarlar’da gerekli yönlendirme verilerini hazırlayın.",
  'profiles': "Profiller",
  'nav_profiles': "Profiller",
  'profiles_subtitle': "Hesabınızı ve ağ senaryonuzu seçin.",
  'scenario_scenes': "Senaryolar",
  'scenario_accounts': "Hesaplar",
  'scenario_create': "Yeni senaryo",
  'scenario_empty': "İlk senaryonuzu kaydedin",
  'scenario_empty_hint':
      "Geçerli ağ ayarlarını yeniden kullanılabilir bir senaryo olarak kaydedin.",
  'scenario_name': "Senaryo adı",
  'scenario_binding': "WARP hesabı",
  'scenario_follow_account': "Geçerli hesabı kullan",
  'scenario_bound_account': "Bağlı hesap",
  'scenario_use': "Senaryoyu kullan",
  'scenario_duplicate': "Çoğalt",
  'scenario_update_current': "Geçerli ayarlarla güncelle",
  'scenario_save_as': "Yeni senaryo olarak kaydet",
  'scenario_save': "Senaryoyu kaydet",
  'scenario_save_use': "Kaydet ve kullan",
  'scenario_preview': "Senaryo değişikliklerini gözden geçir",
  'scenario_restore': "Önceki ayarları geri yükle",
  'scenario_selected': "Seçildi",
  'scenario_applied': "Uygulandı",
  'scenario_modified': "Değiştirildi",
  'scenario_binding_modified': "Hesap, bağlı hesaptan farklı",
  'scenario_template_updated': "Senaryo için güncelleme var",
  'scenario_pending': "Kaydedildi; uygulanması bekleniyor",
  'scenario_failed': "Kaydedildi; uygulanamadı",
  'scenario_unknown': "Sonuç doğrulanmadı",
  'scenario_custom': "Özel",
  'scenario_unsupported':
      "Bu motor senaryoları desteklemiyor. Hesap yönetimi kullanılabilir.",
  'scenario_save_failed':
      "İşlem tamamlanamadı. Yeniden denemeden önce durumu yenileyin.",
  'scenario_reference_missing':
      "Bu senaryonun gerektirdiği bir hesap veya çıkış kullanılamıyor.",
  'scenario_global_hint':
      "Kill Switch, proxy adresleri ve kimlik doğrulama, gelişmiş ayarlar ve uygulama bazlı seçim cihaz ayarlarını kullanır.",
  'scenario_output': "Çıkışlar",
  'scenario_exit': "Çıkış",
  'scenario_connection': "Bağlantı tercihleri",
  'scenario_current': "Seçili senaryo",
  'scenario_current_account': "Seçili hesap",
  'scenario_running_account': "Bağlı olan hesap",
  'scenario_no_changes': "Seçili ayarlar zaten eşleşiyor.",
  'scenario_will_reconnect':
      "Motor, bu değişiklikleri gerekli bağlantı sürecini kullanarak uygulayacak. Yeniden bağlanmak gerekebilir.",
  'scenario_disconnected_hint': "Bağlantı kesik kalacak.",
  'scenario_delete_title': "Senaryo silinsin mi?",
  'scenario_delete_body':
      "Bu kayıtlı senaryo silinecek. Geçerli ağ ayarları korunacak.",
  'scenario_validation_name': "1–64 karakter uzunluğunda bir ad girin.",
  'scenario_keep_account': "Geçerli hesap korunacak.",
  'scenario_account_change': "Hesap değişikliği",
  'scenario_library_hint':
      "Mevcut bir çıkış seçin. İçe aktarma ve oturum açma işlemlerini Proxy sayfasında yönetin.",
  'scenario_saved': "Senaryo kaydedildi",
  'scenario_country_hint':
      "Virgülle ayrılmış ülke kodları (örneğin CN, JP). Gerekli kural verileri kullanımdan önce hazır olmalıdır.",
  'scenario_rules_count': "{count} özel kural",
  'scenario_confirm_use': "Senaryoyu uygula",
  'scenario_invalid_dns': "Geçerli DNS IP adresleri girin.",
  'scenario_new_flows':
      "Değişiklikler yeni bağlantılara uygulanır; mevcut bağlantılar önceki yollarını koruyabilir.",
  'scenario_not_applied': "Senaryo uygulanmadı. Geçerli ayarlar değişmedi.",
  'scenario_kill_switch_output':
      "Kill Switch açık, ancak bu senaryo {tunnel} özelliğini kapatıyor. Geçişten sonra sistem genelindeki koruma durur.",
  'scenario_settings_invalid':
      "Bu senaryonun kayıtlı ayarları artık geçerli değil. Düzenleyip yeniden kaydedin.",
  'protection_handoff_unavailable':
      "Bu geçiş sırasında Kill Switch koruması korunamıyor. Bağlantıyı kesip yeniden geçiş yapın.",
  'scenario_confirm_before_connect':
      "Son senaryo geçişi hâlâ doğrulanıyor. Senaryoları yenileyip yeniden bağlanın.",
  'scenario_unsaved': 'Kaydedilmemiş senaryo değişiklikleri',
  'scenario_inherited_changes': 'Ayrıca uygulanacak diğer kayıtlı ayarlar',
};

const kScenarioRu = <String, String>{
  'scenario_platform_unsupported':
      'Этот сценарий содержит настройку, недоступную на этом устройстве.',
  'scenario_limit':
      'Сценарий или библиотека сценариев превышает лимит. Сократите правила или удалите ненужный сценарий.',
  'scenario_vpn_permission':
      "Отключитесь, выберите этот сценарий и подключитесь снова, чтобы предоставить разрешение VPN.",
  'scenario_rules_missing':
      "Перед использованием этого сценария подготовьте необходимые данные маршрутизации в настройках.",
  'profiles': "Профили",
  'nav_profiles': "Профили",
  'profiles_subtitle': "Выберите аккаунт и сетевой сценарий.",
  'scenario_scenes': "Сценарии",
  'scenario_accounts': "Аккаунты",
  'scenario_create': "Новый сценарий",
  'scenario_empty': "Сохраните первый сценарий",
  'scenario_empty_hint':
      "Сохраните текущие настройки сети как сценарий для повторного использования.",
  'scenario_name': "Название сценария",
  'scenario_binding': "Аккаунт WARP",
  'scenario_follow_account': "Использовать текущий аккаунт",
  'scenario_bound_account': "Привязанный аккаунт",
  'scenario_use': "Использовать сценарий",
  'scenario_duplicate': "Дублировать",
  'scenario_update_current': "Обновить из текущих настроек",
  'scenario_save_as': "Сохранить как новый сценарий",
  'scenario_save': "Сохранить сценарий",
  'scenario_save_use': "Сохранить и использовать",
  'scenario_preview': "Проверить изменения сценария",
  'scenario_restore': "Восстановить предыдущие настройки",
  'scenario_selected': "Выбран",
  'scenario_applied': "Применён",
  'scenario_modified': "Изменён",
  'scenario_binding_modified': "Аккаунт отличается от привязанного",
  'scenario_template_updated': "Сценарий обновлён",
  'scenario_pending': "Сохранён; ожидает применения",
  'scenario_failed': "Сохранён; не удалось применить",
  'scenario_unknown': "Результат не подтверждён",
  'scenario_custom': "Пользовательский",
  'scenario_unsupported':
      "Этот движок не поддерживает сценарии. Управление аккаунтами доступно.",
  'scenario_save_failed':
      "Не удалось завершить операцию. Обновите состояние перед повторной попыткой.",
  'scenario_reference_missing':
      "Необходимый для этого сценария аккаунт или выход недоступен.",
  'scenario_global_hint':
      "Kill Switch, адреса и аутентификация прокси, расширенные параметры и выбор приложений используют настройки устройства.",
  'scenario_output': "Выходы",
  'scenario_exit': "Выход",
  'scenario_connection': "Параметры подключения",
  'scenario_current': "Выбранный сценарий",
  'scenario_current_account': "Выбранный аккаунт",
  'scenario_running_account': "Подключённый аккаунт",
  'scenario_no_changes': "Выбранные настройки уже совпадают.",
  'scenario_will_reconnect':
      "Движок применит изменения с помощью необходимой процедуры подключения. Может потребоваться переподключение.",
  'scenario_disconnected_hint': "Подключение останется отключённым.",
  'scenario_delete_title': "Удалить сценарий?",
  'scenario_delete_body':
      "Удалить этот сохранённый сценарий. Текущие настройки сети сохранятся.",
  'scenario_validation_name': "Введите название длиной от 1 до 64 символов.",
  'scenario_keep_account': "Текущий аккаунт будет сохранён.",
  'scenario_account_change': "Смена аккаунта",
  'scenario_library_hint':
      "Выберите существующий выход. Импорт и вход в аккаунт доступны на странице «Прокси».",
  'scenario_saved': "Сценарий сохранён",
  'scenario_country_hint':
      "Коды стран через запятую (например, CN, JP). Перед использованием должны быть доступны необходимые данные правил.",
  'scenario_rules_count': "Пользовательских правил: {count}",
  'scenario_confirm_use': "Применить сценарий",
  'scenario_invalid_dns': "Введите допустимые IP-адреса DNS.",
  'scenario_new_flows':
      "Изменения применяются к новым подключениям; существующие подключения могут сохранить прежние маршруты.",
  'scenario_not_applied':
      "Сценарий не применён. Текущие настройки не изменились.",
  'scenario_kill_switch_output':
      "Kill Switch включён, но этот сценарий отключает {tunnel}. После переключения защита всей системы прекратится.",
  'scenario_settings_invalid':
      "Сохранённые настройки этого сценария больше недействительны. Измените и сохраните его снова.",
  'protection_handoff_unavailable':
      "Во время этого переключения нельзя сохранить защиту Kill Switch. Отключитесь и переключитесь снова.",
  'scenario_confirm_before_connect':
      "Последнее переключение сценария ещё подтверждается. Обновите сценарии и подключитесь снова.",
  'scenario_unsaved': 'Несохранённые изменения сценария',
  'scenario_inherited_changes':
      'Другие сохранённые настройки, которые также применятся',
};

const kScenarioDe = <String, String>{
  'scenario_platform_unsupported':
      'Diese Szene enthält eine Einstellung, die auf diesem Gerät nicht verfügbar ist.',
  'scenario_limit':
      'Die Szene oder Szenenbibliothek überschreitet das Limit. Reduziere die Regeln oder entferne eine ungenutzte Szene.',
  'scenario_vpn_permission':
      "Verbindung trennen, dieses Szenario auswählen und erneut verbinden, um die VPN-Berechtigung zu erteilen.",
  'scenario_rules_missing':
      "Vor der Verwendung dieses Szenarios die benötigten Routingdaten unter Einstellungen vorbereiten.",
  'profiles': "Profile",
  'nav_profiles': "Profile",
  'profiles_subtitle': "Konto und Netzwerkszenario auswählen.",
  'scenario_scenes': "Szenarien",
  'scenario_accounts': "Konten",
  'scenario_create': "Neues Szenario",
  'scenario_empty': "Erstes Szenario speichern",
  'scenario_empty_hint':
      "Aktuelle Netzwerkeinstellungen als wiederverwendbares Szenario speichern.",
  'scenario_name': "Szenarioname",
  'scenario_binding': "WARP-Konto",
  'scenario_follow_account': "Aktuelles Konto verwenden",
  'scenario_bound_account': "Zugeordnetes Konto",
  'scenario_use': "Szenario verwenden",
  'scenario_duplicate': "Duplizieren",
  'scenario_update_current': "Aus aktuellen Einstellungen aktualisieren",
  'scenario_save_as': "Als neues Szenario speichern",
  'scenario_save': "Szenario speichern",
  'scenario_save_use': "Speichern und verwenden",
  'scenario_preview': "Szenarioänderungen prüfen",
  'scenario_restore': "Vorherige Einstellungen wiederherstellen",
  'scenario_selected': "Ausgewählt",
  'scenario_applied': "Angewendet",
  'scenario_modified': "Geändert",
  'scenario_binding_modified': "Konto weicht von der Zuordnung ab",
  'scenario_template_updated': "Für das Szenario liegt eine Aktualisierung vor",
  'scenario_pending': "Gespeichert; Anwendung ausstehend",
  'scenario_failed': "Gespeichert; Anwendung fehlgeschlagen",
  'scenario_unknown': "Ergebnis nicht bestätigt",
  'scenario_custom': "Benutzerdefiniert",
  'scenario_unsupported':
      "Diese Engine unterstützt keine Szenarien. Die Kontoverwaltung ist verfügbar.",
  'scenario_save_failed':
      "Der Vorgang konnte nicht abgeschlossen werden. Vor einem erneuten Versuch den Status aktualisieren.",
  'scenario_reference_missing':
      "Ein für dieses Szenario benötigtes Konto oder ein Ausgang ist nicht verfügbar.",
  'scenario_global_hint':
      "Kill Switch, Proxyadressen und Authentifizierung, erweiterte Anpassungen und die App-Auswahl verwenden die Geräteeinstellungen.",
  'scenario_output': "Ausgänge",
  'scenario_exit': "Ausgang",
  'scenario_connection': "Verbindungseinstellungen",
  'scenario_current': "Ausgewähltes Szenario",
  'scenario_current_account': "Ausgewähltes Konto",
  'scenario_running_account': "Verbundenes Konto",
  'scenario_no_changes':
      "Die ausgewählten Einstellungen stimmen bereits überein.",
  'scenario_will_reconnect':
      "Die Engine wendet die Änderungen mit dem erforderlichen Verbindungsverfahren an. Eine erneute Verbindung kann nötig sein.",
  'scenario_disconnected_hint': "Die Verbindung bleibt getrennt.",
  'scenario_delete_title': "Szenario löschen?",
  'scenario_delete_body':
      "Dieses gespeicherte Szenario löschen. Die aktuellen Netzwerkeinstellungen bleiben erhalten.",
  'scenario_validation_name': "Einen Namen mit 1–64 Zeichen eingeben.",
  'scenario_keep_account': "Das aktuelle Konto bleibt erhalten.",
  'scenario_account_change': "Kontowechsel",
  'scenario_library_hint':
      "Einen vorhandenen Ausgang auswählen. Importe und Anmeldung werden unter Proxy verwaltet.",
  'scenario_saved': "Szenario gespeichert",
  'scenario_country_hint':
      "Durch Kommas getrennte Ländercodes (zum Beispiel CN, JP). Die benötigten Regeldaten müssen vor der Nutzung verfügbar sein.",
  'scenario_rules_count': "{count} benutzerdefinierte Regeln",
  'scenario_confirm_use': "Szenario anwenden",
  'scenario_invalid_dns': "Gültige DNS-IP-Adressen eingeben.",
  'scenario_new_flows':
      "Änderungen gelten für neue Verbindungen; bestehende Verbindungen können ihre bisherigen Wege beibehalten.",
  'scenario_not_applied':
      "Das Szenario wurde nicht angewendet. Die aktuellen Einstellungen sind unverändert.",
  'scenario_kill_switch_output':
      "Kill Switch ist aktiv, aber dieses Szenario schaltet {tunnel} aus. Nach dem Wechsel endet der systemweite Schutz.",
  'scenario_settings_invalid':
      "Die gespeicherten Einstellungen dieses Szenarios sind nicht mehr gültig. Bearbeiten und speichern Sie es erneut.",
  'protection_handoff_unavailable':
      "Der Kill-Switch-Schutz kann bei diesem Wechsel nicht aufrechterhalten werden. Trennen Sie die Verbindung und wechseln Sie erneut.",
  'scenario_confirm_before_connect':
      "Der letzte Szenariowechsel wird noch bestätigt. Aktualisieren Sie die Szenarien und verbinden Sie sich erneut.",
  'scenario_unsaved': 'Nicht gespeicherte Szenarioänderungen',
  'scenario_inherited_changes':
      'Weitere gespeicherte Einstellungen, die ebenfalls gelten',
};

const kScenarioIt = <String, String>{
  'scenario_platform_unsupported':
      'Questa scena contiene un’impostazione non disponibile su questo dispositivo.',
  'scenario_limit':
      'La scena o la libreria supera il limite. Riduci le regole o elimina una scena inutilizzata.',
  'scenario_vpn_permission':
      "Disconnettiti, seleziona questo scenario e riconnettiti per concedere l’autorizzazione VPN.",
  'scenario_rules_missing':
      "Prepara i dati di instradamento necessari nelle Impostazioni prima di usare questo scenario.",
  'profiles': "Profili",
  'nav_profiles': "Profili",
  'profiles_subtitle': "Scegli l’account e lo scenario di rete.",
  'scenario_scenes': "Scenari",
  'scenario_accounts': "Account",
  'scenario_create': "Nuovo scenario",
  'scenario_empty': "Salva il primo scenario",
  'scenario_empty_hint':
      "Salva le impostazioni di rete attuali come scenario riutilizzabile.",
  'scenario_name': "Nome dello scenario",
  'scenario_binding': "Account WARP",
  'scenario_follow_account': "Usa l’account attuale",
  'scenario_bound_account': "Account associato",
  'scenario_use': "Usa scenario",
  'scenario_duplicate': "Duplica",
  'scenario_update_current': "Aggiorna dalle impostazioni attuali",
  'scenario_save_as': "Salva come nuovo scenario",
  'scenario_save': "Salva scenario",
  'scenario_save_use': "Salva e usa",
  'scenario_preview': "Verifica le modifiche dello scenario",
  'scenario_restore': "Ripristina le impostazioni precedenti",
  'scenario_selected': "Selezionato",
  'scenario_applied': "Applicato",
  'scenario_modified': "Modificato",
  'scenario_binding_modified': "L’account è diverso da quello associato",
  'scenario_template_updated': "Lo scenario ha un aggiornamento",
  'scenario_pending': "Salvato; in attesa di applicazione",
  'scenario_failed': "Salvato; applicazione non riuscita",
  'scenario_unknown': "Risultato non confermato",
  'scenario_custom': "Personalizzato",
  'scenario_unsupported':
      "Questo motore non supporta gli scenari. La gestione degli account è disponibile.",
  'scenario_save_failed':
      "Impossibile completare l’operazione. Aggiorna lo stato prima di riprovare.",
  'scenario_reference_missing':
      "Un account o un’uscita necessari per questo scenario non sono disponibili.",
  'scenario_global_hint':
      "Kill Switch, indirizzi e autenticazione del proxy, impostazioni avanzate e selezione per app usano le impostazioni del dispositivo.",
  'scenario_output': "Uscite",
  'scenario_exit': "Uscita",
  'scenario_connection': "Preferenze di connessione",
  'scenario_current': "Scenario selezionato",
  'scenario_current_account': "Account selezionato",
  'scenario_running_account': "Account connesso",
  'scenario_no_changes': "Le impostazioni selezionate corrispondono già.",
  'scenario_will_reconnect':
      "Il motore applicherà le modifiche tramite la procedura di connessione necessaria. Potrebbe essere necessario riconnettersi.",
  'scenario_disconnected_hint': "La connessione rimarrà disconnessa.",
  'scenario_delete_title': "Eliminare lo scenario?",
  'scenario_delete_body':
      "Elimina questo scenario salvato. Le impostazioni di rete attuali verranno mantenute.",
  'scenario_validation_name': "Inserisci un nome da 1 a 64 caratteri.",
  'scenario_keep_account': "L’account attuale verrà mantenuto.",
  'scenario_account_change': "Cambio di account",
  'scenario_library_hint':
      "Scegli un’uscita esistente. Gestisci importazioni e accesso nella pagina Proxy.",
  'scenario_saved': "Scenario salvato",
  'scenario_country_hint':
      "Codici paese separati da virgole (ad esempio CN, JP). I dati delle regole necessari devono essere disponibili prima dell’uso.",
  'scenario_rules_count': "{count} regole personalizzate",
  'scenario_confirm_use': "Applica scenario",
  'scenario_invalid_dns': "Inserisci indirizzi IP DNS validi.",
  'scenario_new_flows':
      "Le modifiche si applicano alle nuove connessioni; quelle esistenti possono mantenere i percorsi precedenti.",
  'scenario_not_applied':
      "Lo scenario non è stato applicato. Le impostazioni attuali non sono cambiate.",
  'scenario_kill_switch_output':
      "Kill Switch è attivo, ma questo scenario disattiva {tunnel}. Dopo il cambio la protezione dell’intero sistema si interrompe.",
  'scenario_settings_invalid':
      "Le impostazioni salvate di questo scenario non sono più valide. Modificalo e salvalo di nuovo.",
  'protection_handoff_unavailable':
      "Durante questo cambio non è possibile mantenere la protezione Kill Switch. Disconnettiti e cambia di nuovo.",
  'scenario_confirm_before_connect':
      "L’ultimo cambio di scenario è ancora in fase di conferma. Aggiorna Scenari e riconnettiti.",
  'scenario_unsaved': 'Modifiche allo scenario non salvate',
  'scenario_inherited_changes': 'Altre impostazioni salvate applicate insieme',
};

const kScenarioPl = <String, String>{
  'scenario_platform_unsupported':
      'Ta scena zawiera ustawienie niedostępne na tym urządzeniu.',
  'scenario_limit':
      'Scena lub biblioteka scen przekracza limit. Zmniejsz liczbę reguł lub usuń nieużywaną scenę.',
  'scenario_vpn_permission':
      "Rozłącz się, wybierz ten scenariusz i połącz się ponownie, aby przyznać uprawnienie VPN.",
  'scenario_rules_missing':
      "Przed użyciem tego scenariusza przygotuj wymagane dane routingu w Ustawieniach.",
  'profiles': "Profile",
  'nav_profiles': "Profile",
  'profiles_subtitle': "Wybierz konto i scenariusz sieciowy.",
  'scenario_scenes': "Scenariusze",
  'scenario_accounts': "Konta",
  'scenario_create': "Nowy scenariusz",
  'scenario_empty': "Zapisz pierwszy scenariusz",
  'scenario_empty_hint':
      "Zapisz bieżące ustawienia sieci jako scenariusz do ponownego użycia.",
  'scenario_name': "Nazwa scenariusza",
  'scenario_binding': "Konto WARP",
  'scenario_follow_account': "Użyj bieżącego konta",
  'scenario_bound_account': "Powiązane konto",
  'scenario_use': "Użyj scenariusza",
  'scenario_duplicate': "Duplikuj",
  'scenario_update_current': "Zaktualizuj z bieżących ustawień",
  'scenario_save_as': "Zapisz jako nowy scenariusz",
  'scenario_save': "Zapisz scenariusz",
  'scenario_save_use': "Zapisz i użyj",
  'scenario_preview': "Sprawdź zmiany scenariusza",
  'scenario_restore': "Przywróć poprzednie ustawienia",
  'scenario_selected': "Wybrany",
  'scenario_applied': "Zastosowany",
  'scenario_modified': "Zmieniony",
  'scenario_binding_modified': "Konto różni się od powiązanego",
  'scenario_template_updated': "Scenariusz ma aktualizację",
  'scenario_pending': "Zapisano; oczekiwanie na zastosowanie",
  'scenario_failed': "Zapisano; zastosowanie nie powiodło się",
  'scenario_unknown': "Wynik niepotwierdzony",
  'scenario_custom': "Niestandardowy",
  'scenario_unsupported':
      "Ten silnik nie obsługuje scenariuszy. Zarządzanie kontami jest dostępne.",
  'scenario_save_failed':
      "Nie udało się ukończyć operacji. Odśwież stan przed ponowną próbą.",
  'scenario_reference_missing':
      "Konto lub wyjście wymagane przez ten scenariusz jest niedostępne.",
  'scenario_global_hint':
      "Kill Switch, adresy i uwierzytelnianie proxy, ustawienia zaawansowane oraz wybór aplikacji korzystają z ustawień urządzenia.",
  'scenario_output': "Wyjścia",
  'scenario_exit': "Wyjście",
  'scenario_connection': "Preferencje połączenia",
  'scenario_current': "Wybrany scenariusz",
  'scenario_current_account': "Wybrane konto",
  'scenario_running_account': "Połączone konto",
  'scenario_no_changes': "Wybrane ustawienia są już zgodne.",
  'scenario_will_reconnect':
      "Silnik zastosuje zmiany zgodnie z wymaganą procedurą połączenia. Może być konieczne ponowne połączenie.",
  'scenario_disconnected_hint': "Połączenie pozostanie rozłączone.",
  'scenario_delete_title': "Usunąć scenariusz?",
  'scenario_delete_body':
      "Usuń ten zapisany scenariusz. Bieżące ustawienia sieci zostaną zachowane.",
  'scenario_validation_name': "Wpisz nazwę o długości od 1 do 64 znaków.",
  'scenario_keep_account': "Bieżące konto zostanie zachowane.",
  'scenario_account_change': "Zmiana konta",
  'scenario_library_hint':
      "Wybierz istniejące wyjście. Importami i logowaniem zarządza się na stronie Proxy.",
  'scenario_saved': "Scenariusz zapisany",
  'scenario_country_hint':
      "Kody krajów oddzielone przecinkami (na przykład CN, JP). Wymagane dane reguł muszą być dostępne przed użyciem.",
  'scenario_rules_count': "Reguły niestandardowe: {count}",
  'scenario_confirm_use': "Zastosuj scenariusz",
  'scenario_invalid_dns': "Wpisz poprawne adresy IP DNS.",
  'scenario_new_flows':
      "Zmiany dotyczą nowych połączeń; istniejące połączenia mogą zachować dotychczasowe trasy.",
  'scenario_not_applied':
      "Scenariusz nie został zastosowany. Bieżące ustawienia się nie zmieniły.",
  'scenario_kill_switch_output':
      "Kill Switch jest włączony, ale ten scenariusz wyłącza {tunnel}. Po przełączeniu ochrona całego systemu zostanie zatrzymana.",
  'scenario_settings_invalid':
      "Zapisane ustawienia tego scenariusza są już nieprawidłowe. Edytuj go i zapisz ponownie.",
  'protection_handoff_unavailable':
      "Podczas tego przełączenia nie można utrzymać ochrony Kill Switch. Rozłącz się i przełącz ponownie.",
  'scenario_confirm_before_connect':
      "Ostatnie przełączenie scenariusza jest nadal potwierdzane. Odśwież scenariusze i połącz się ponownie.",
  'scenario_unsaved': 'Niezapisane zmiany scenariusza',
  'scenario_inherited_changes':
      'Inne zapisane ustawienia, które również zostaną zastosowane',
};
