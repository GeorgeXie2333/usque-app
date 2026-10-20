const kLinuxDesktopCatalogs = <String, Map<String, String>>{
  'en': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Linux network setup failed. Export diagnostics for the failed stage.",
    'linux_platform_failed':
        "Linux network setup failed. Export diagnostics for the failed stage.",
    'linux_service_dependencies':
        "Install systemd, nftables and libsecret-1 using your system package manager, then retry. Your login session also needs a Secret Service keyring.",
    'linux_service_title': 'Background service',
    'linux_service_installed':
        'Installed. Restart Usque after installing or repairing the service.',
    'linux_service_install_hint':
        'Verify the AppImage signature before approving administrator access. Installing the service enables VPN support but does not connect a VPN.',
    'linux_service_reinstall':
        'Install or reinstall the matching Usque package to restore the background service.',
    'linux_service_install': 'Install service',
    'linux_service_repair': 'Repair service',
    'linux_keyring_unavailable':
        'Install or unlock a system keyring that supports Secret Service, then retry. Credentials are never saved in plain text.',
    'linux_output_unavailable':
        'This output is unavailable in the installed Linux Engine.',
    'linux_tray_unavailable':
        'A desktop tray host is unavailable. Closing the window exits Usque.',
  },
  'zh_CN': {
    'diag_fail_LINUX_PLATFORM_FAILED': "Linux 网络初始化失败，请导出诊断查看失败阶段。",
    'linux_platform_failed': "Linux 网络初始化失败，请导出诊断查看失败阶段。",
    'linux_service_dependencies':
        "请使用系统软件包管理器安装 systemd、nftables 和 libsecret-1，然后重试。登录会话还需要支持 Secret Service 的钥匙串。",
    'linux_service_title': '后台服务',
    'linux_service_installed': '已安装。安装或修复服务后，请重新启动 Usque。',
    'linux_service_install_hint':
        '授予管理员权限前，请验证 AppImage 签名。安装服务可提供 VPN 支持，但不会连接 VPN。',
    'linux_service_reinstall': '请安装或重新安装匹配版本的 Usque 软件包以恢复后台服务。',
    'linux_service_install': '安装服务',
    'linux_service_repair': '修复服务',
    'linux_keyring_unavailable':
        '请安装或解锁支持 Secret Service 的系统钥匙串，然后重试。凭据不会以明文保存。',
    'linux_output_unavailable': '当前安装的 Linux 引擎不支持此输出。',
    'linux_tray_unavailable': '桌面托盘服务不可用。关闭窗口将退出 Usque。',
  },
  'zh_TW': {
    'diag_fail_LINUX_PLATFORM_FAILED': "Linux 網路初始化失敗，請匯出診斷查看失敗階段。",
    'linux_platform_failed': "Linux 網路初始化失敗，請匯出診斷查看失敗階段。",
    'linux_service_dependencies':
        "請使用系統套件管理員安裝 systemd、nftables 和 libsecret-1，然後重試。登入工作階段還需要支援 Secret Service 的鑰匙圈。",
    'linux_service_title': '背景服務',
    'linux_service_installed': '已安裝。安裝或修復服務後，請重新啟動 Usque。',
    'linux_service_install_hint':
        '授予管理員權限前，請驗證 AppImage 簽章。安裝服務可提供 VPN 支援，但不會連線 VPN。',
    'linux_service_reinstall': '請安裝或重新安裝相符版本的 Usque 套件以還原背景服務。',
    'linux_service_install': '安裝服務',
    'linux_service_repair': '修復服務',
    'linux_keyring_unavailable':
        '請安裝或解鎖支援 Secret Service 的系統鑰匙圈，然後重試。憑證不會以明文儲存。',
    'linux_output_unavailable': '目前安裝的 Linux 引擎不支援此輸出。',
    'linux_tray_unavailable': '桌面系統匣服務無法使用。關閉視窗將結束 Usque。',
  },
  'zh_HK': {
    'diag_fail_LINUX_PLATFORM_FAILED': "Linux 網絡初始化失敗，請匯出診斷查看失敗階段。",
    'linux_platform_failed': "Linux 網絡初始化失敗，請匯出診斷查看失敗階段。",
    'linux_service_dependencies':
        "請使用系統套件管理員安裝 systemd、nftables 和 libsecret-1，然後重試。登入工作階段還需要支援 Secret Service 的鑰匙圈。",
    'linux_service_title': '背景服務',
    'linux_service_installed': '已安裝。安裝或修復服務後，請重新啟動 Usque。',
    'linux_service_install_hint':
        '授予管理員權限前，請驗證 AppImage 簽署。安裝服務可提供 VPN 支援，但不會連線 VPN。',
    'linux_service_reinstall': '請安裝或重新安裝相符版本的 Usque 套件以還原背景服務。',
    'linux_service_install': '安裝服務',
    'linux_service_repair': '修復服務',
    'linux_keyring_unavailable':
        '請安裝或解鎖支援 Secret Service 的系統鑰匙圈，然後重試。憑證不會以明文儲存。',
    'linux_output_unavailable': '目前安裝的 Linux 引擎不支援此輸出。',
    'linux_tray_unavailable': '桌面系統匣服務無法使用。關閉視窗將結束 Usque。',
  },
  'ja': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Linux のネットワーク設定に失敗しました。診断をエクスポートして失敗した段階を確認してください。",
    'linux_platform_failed':
        "Linux のネットワーク設定に失敗しました。診断をエクスポートして失敗した段階を確認してください。",
    'linux_service_dependencies':
        "システムのパッケージマネージャーで systemd、nftables、libsecret-1 をインストールして再試行してください。ログインセッションには Secret Service 対応のキーリングも必要です。",
    'linux_service_title': 'バックグラウンドサービス',
    'linux_service_installed': 'インストール済み。サービスのインストールや修復後は Usque を再起動してください。',
    'linux_service_install_hint':
        '管理者権限を許可する前に AppImage の署名を検証してください。サービスのインストールで VPN が利用可能になりますが、VPN には接続しません。',
    'linux_service_reinstall':
        '対応する Usque パッケージをインストールまたは再インストールして、サービスを復元してください。',
    'linux_service_install': 'サービスをインストール',
    'linux_service_repair': 'サービスを修復',
    'linux_keyring_unavailable':
        'Secret Service 対応のシステムキーリングをインストールするか、ロックを解除して再試行してください。認証情報は平文では保存されません。',
    'linux_output_unavailable': 'インストールされている Linux エンジンでは、この出力は利用できません。',
    'linux_tray_unavailable': 'デスクトップのトレイが利用できません。ウィンドウを閉じると Usque が終了します。',
  },
  'ko': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Linux 네트워크 설정에 실패했습니다. 진단을 내보내 실패한 단계를 확인하세요.",
    'linux_platform_failed': "Linux 네트워크 설정에 실패했습니다. 진단을 내보내 실패한 단계를 확인하세요.",
    'linux_service_dependencies':
        "시스템 패키지 관리자로 systemd, nftables, libsecret-1을 설치한 후 다시 시도하세요. 로그인 세션에는 Secret Service 키링도 필요합니다.",
    'linux_service_title': '백그라운드 서비스',
    'linux_service_installed': '설치됨. 서비스를 설치하거나 복구한 후 Usque를 다시 시작하세요.',
    'linux_service_install_hint':
        '관리자 권한을 허용하기 전에 AppImage 서명을 확인하세요. 서비스 설치는 VPN 지원을 활성화하지만 VPN에 연결하지는 않습니다.',
    'linux_service_reinstall': '일치하는 Usque 패키지를 설치하거나 다시 설치하여 서비스를 복원하세요.',
    'linux_service_install': '서비스 설치',
    'linux_service_repair': '서비스 복구',
    'linux_keyring_unavailable':
        'Secret Service를 지원하는 시스템 키링을 설치하거나 잠금 해제한 후 다시 시도하세요. 자격 증명은 일반 텍스트로 저장되지 않습니다.',
    'linux_output_unavailable': '설치된 Linux 엔진에서는 이 출력을 사용할 수 없습니다.',
    'linux_tray_unavailable': '데스크톱 트레이를 사용할 수 없습니다. 창을 닫으면 Usque가 종료됩니다.',
  },
  'es': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Falló la configuración de red de Linux. Exporta el diagnóstico para ver la etapa del fallo.",
    'linux_platform_failed':
        "Falló la configuración de red de Linux. Exporta el diagnóstico para ver la etapa del fallo.",
    'linux_service_dependencies':
        "Instala systemd, nftables y libsecret-1 con el gestor de paquetes del sistema y vuelve a intentarlo. Tu sesión también necesita un llavero Secret Service.",
    'linux_service_title': 'Servicio en segundo plano',
    'linux_service_installed':
        'Instalado. Reinicia Usque después de instalar o reparar el servicio.',
    'linux_service_install_hint':
        'Verifica la firma de AppImage antes de conceder acceso de administrador. Instalar el servicio habilita VPN, pero no lo conecta.',
    'linux_service_reinstall':
        'Instala o reinstala el paquete de Usque correspondiente para restaurar el servicio.',
    'linux_service_install': 'Instalar servicio',
    'linux_service_repair': 'Reparar servicio',
    'linux_keyring_unavailable':
        'Instala o desbloquea un llavero del sistema compatible con Secret Service y vuelve a intentarlo. Las credenciales nunca se guardan en texto sin cifrar.',
    'linux_output_unavailable':
        'Esta salida no está disponible en el motor de Linux instalado.',
    'linux_tray_unavailable':
        'La bandeja del sistema no está disponible. Cerrar la ventana sale de Usque.',
  },
  'pt': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "A configuração de rede do Linux falhou. Exporte o diagnóstico para ver a etapa da falha.",
    'linux_platform_failed':
        "A configuração de rede do Linux falhou. Exporte o diagnóstico para ver a etapa da falha.",
    'linux_service_dependencies':
        "Instale systemd, nftables e libsecret-1 com o gerenciador de pacotes do sistema e tente novamente. Sua sessão também precisa de um chaveiro Secret Service.",
    'linux_service_title': 'Serviço em segundo plano',
    'linux_service_installed':
        'Instalado. Reinicie o Usque após instalar ou reparar o serviço.',
    'linux_service_install_hint':
        'Verifique a assinatura do AppImage antes de autorizar acesso de administrador. Instalar o serviço habilita VPN, mas não conecta a VPN.',
    'linux_service_reinstall':
        'Instale ou reinstale o pacote Usque correspondente para restaurar o serviço.',
    'linux_service_install': 'Instalar serviço',
    'linux_service_repair': 'Reparar serviço',
    'linux_keyring_unavailable':
        'Instale ou desbloqueie um chaveiro do sistema compatível com Secret Service e tente novamente. As credenciais nunca são salvas em texto simples.',
    'linux_output_unavailable':
        'Esta saída não está disponível no motor Linux instalado.',
    'linux_tray_unavailable':
        'A bandeja do sistema não está disponível. Fechar a janela encerra o Usque.',
  },
  'fr': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "La configuration réseau de Linux a échoué. Exportez le diagnostic pour identifier la phase en échec.",
    'linux_platform_failed':
        "La configuration réseau de Linux a échoué. Exportez le diagnostic pour identifier la phase en échec.",
    'linux_service_dependencies':
        "Installez systemd, nftables et libsecret-1 avec le gestionnaire de paquets du système, puis réessayez. Votre session nécessite aussi un trousseau Secret Service.",
    'linux_service_title': 'Service en arrière-plan',
    'linux_service_installed':
        'Installé. Redémarrez Usque après installation ou réparation du service.',
    'linux_service_install_hint':
        'Vérifiez la signature AppImage avant de donner les droits administrateur. Le service permet le VPN sans établir de connexion.',
    'linux_service_reinstall':
        'Installez ou réinstallez le paquet Usque correspondant pour restaurer le service.',
    'linux_service_install': 'Installer le service',
    'linux_service_repair': 'Réparer le service',
    'linux_keyring_unavailable':
        'Installez ou déverrouillez un trousseau système compatible avec Secret Service, puis réessayez. Les identifiants ne sont jamais enregistrés en clair.',
    'linux_output_unavailable':
        'Cette sortie est indisponible dans le moteur Linux installé.',
    'linux_tray_unavailable':
        'La zone de notification est indisponible. Fermer la fenêtre quitte Usque.',
  },
  'nl': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "De Linux-netwerkinstelling is mislukt. Exporteer de diagnose om de mislukte stap te bekijken.",
    'linux_platform_failed':
        "De Linux-netwerkinstelling is mislukt. Exporteer de diagnose om de mislukte stap te bekijken.",
    'linux_service_dependencies':
        "Installeer systemd, nftables en libsecret-1 via de pakketbeheerder en probeer opnieuw. Je sessie heeft ook een Secret Service-sleutelbos nodig.",
    'linux_service_title': 'Achtergrondservice',
    'linux_service_installed':
        'Geïnstalleerd. Herstart Usque na installatie of reparatie van de service.',
    'linux_service_install_hint':
        'Controleer de AppImage-handtekening voordat je beheerderstoegang toestaat. De service maakt VPN mogelijk zonder verbinding te maken.',
    'linux_service_reinstall':
        'Installeer het bijbehorende Usque-pakket opnieuw om de service te herstellen.',
    'linux_service_install': 'Service installeren',
    'linux_service_repair': 'Service repareren',
    'linux_keyring_unavailable':
        'Installeer of ontgrendel een systeemsleutelbos met ondersteuning voor Secret Service en probeer het opnieuw. Inloggegevens worden nooit als platte tekst opgeslagen.',
    'linux_output_unavailable':
        'Deze uitvoer is niet beschikbaar in de geïnstalleerde Linux-engine.',
    'linux_tray_unavailable':
        'Het systeemvak is niet beschikbaar. Het sluiten van het venster beëindigt Usque.',
  },
  'tr': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Linux ağ kurulumu başarısız oldu. Başarısız aşamayı görmek için tanılamayı dışa aktarın.",
    'linux_platform_failed':
        "Linux ağ kurulumu başarısız oldu. Başarısız aşamayı görmek için tanılamayı dışa aktarın.",
    'linux_service_dependencies':
        "Sistem paket yöneticisiyle systemd, nftables ve libsecret-1 yükleyip yeniden deneyin. Oturumunuz ayrıca Secret Service anahtarlığı gerektirir.",
    'linux_service_title': 'Arka plan hizmeti',
    'linux_service_installed':
        'Kurulu. Hizmeti kurduktan veya onardıktan sonra Usque uygulamasını yeniden başlatın.',
    'linux_service_install_hint':
        'Yönetici erişimine izin vermeden önce AppImage imzasını doğrulayın. Hizmet kurulumu VPN desteğini etkinleştirir ancak VPN bağlantısı başlatmaz.',
    'linux_service_reinstall':
        'Hizmeti geri yüklemek için eşleşen Usque paketini kurun veya yeniden kurun.',
    'linux_service_install': 'Hizmeti kur',
    'linux_service_repair': 'Hizmeti onar',
    'linux_keyring_unavailable':
        'Secret Service destekleyen bir sistem anahtarlığı kurun veya kilidini açın, ardından yeniden deneyin. Kimlik bilgileri asla düz metin olarak kaydedilmez.',
    'linux_output_unavailable':
        'Bu çıkış kurulu Linux motorunda kullanılamıyor.',
    'linux_tray_unavailable':
        'Sistem tepsisi kullanılamıyor. Pencereyi kapatmak Usque uygulamasından çıkar.',
  },
  'ru': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Не удалось настроить сеть Linux. Экспортируйте диагностику, чтобы узнать этап сбоя.",
    'linux_platform_failed':
        "Не удалось настроить сеть Linux. Экспортируйте диагностику, чтобы узнать этап сбоя.",
    'linux_service_dependencies':
        "Установите systemd, nftables и libsecret-1 через системный менеджер пакетов и повторите попытку. Для сеанса также нужно хранилище ключей Secret Service.",
    'linux_service_title': 'Фоновая служба',
    'linux_service_installed':
        'Установлена. После установки или восстановления службы перезапустите Usque.',
    'linux_service_install_hint':
        'Проверьте подпись AppImage перед предоставлением прав администратора. Установка службы добавляет поддержку VPN, но не подключает VPN.',
    'linux_service_reinstall':
        'Установите или переустановите соответствующий пакет Usque для восстановления службы.',
    'linux_service_install': 'Установить службу',
    'linux_service_repair': 'Восстановить службу',
    'linux_keyring_unavailable':
        'Установите или разблокируйте системное хранилище ключей с поддержкой Secret Service и повторите попытку. Учётные данные не сохраняются открытым текстом.',
    'linux_output_unavailable':
        'Этот выход недоступен в установленном движке Linux.',
    'linux_tray_unavailable':
        'Системный трей недоступен. Закрытие окна завершит Usque.',
  },
  'fa': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "راه‌اندازی شبکه Linux ناموفق بود. برای دیدن مرحله خطا، اطلاعات عیب‌یابی را صادر کنید.",
    'linux_platform_failed':
        "راه‌اندازی شبکه Linux ناموفق بود. برای دیدن مرحله خطا، اطلاعات عیب‌یابی را صادر کنید.",
    'linux_service_dependencies':
        "systemd، nftables و libsecret-1 را با مدیر بستهٔ سیستم نصب کنید و دوباره تلاش کنید. نشست ورود شما به دسته‌کلید Secret Service نیز نیاز دارد.",
    'linux_service_title': 'سرویس پس‌زمینه',
    'linux_service_installed':
        'نصب شده است. پس از نصب یا ترمیم سرویس، Usque را دوباره اجرا کنید.',
    'linux_service_install_hint':
        'پیش از تأیید دسترسی مدیر، امضای AppImage را بررسی کنید. نصب سرویس پشتیبانی VPN را فعال می‌کند ولی به VPN متصل نمی‌شود.',
    'linux_service_reinstall':
        'برای بازیابی سرویس، بستهٔ هم‌نسخهٔ Usque را نصب یا دوباره نصب کنید.',
    'linux_service_install': 'نصب سرویس',
    'linux_service_repair': 'ترمیم سرویس',
    'linux_keyring_unavailable':
        'کیف کلید سیستم با پشتیبانی از Secret Service را نصب یا باز کنید و دوباره تلاش کنید. اطلاعات ورود به‌صورت متن ساده ذخیره نمی‌شوند.',
    'linux_output_unavailable':
        'این خروجی در موتور نصب‌شدهٔ لینوکس در دسترس نیست.',
    'linux_tray_unavailable':
        'سینی سیستم در دسترس نیست. بستن پنجره، Usque را می‌بندد.',
  },
  'ar': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "فشل إعداد شبكة Linux. صدّر بيانات التشخيص لمعرفة المرحلة التي فشلت.",
    'linux_platform_failed':
        "فشل إعداد شبكة Linux. صدّر بيانات التشخيص لمعرفة المرحلة التي فشلت.",
    'linux_service_dependencies':
        "ثبّت systemd وnftables وlibsecret-1 باستخدام مدير حزم النظام، ثم أعد المحاولة. تحتاج جلسة تسجيل الدخول أيضًا إلى مخزن مفاتيح يدعم Secret Service.",
    'linux_service_title': 'خدمة الخلفية',
    'linux_service_installed':
        'مثبّتة. أعد تشغيل Usque بعد تثبيت الخدمة أو إصلاحها.',
    'linux_service_install_hint':
        'تحقق من توقيع AppImage قبل منح صلاحيات المسؤول. يتيح تثبيت الخدمة دعم VPN لكنه لا يتصل به.',
    'linux_service_reinstall':
        'ثبّت حزمة Usque المطابقة أو أعد تثبيتها لاستعادة الخدمة.',
    'linux_service_install': 'تثبيت الخدمة',
    'linux_service_repair': 'إصلاح الخدمة',
    'linux_keyring_unavailable':
        'ثبّت مخزن مفاتيح للنظام يدعم Secret Service أو افتح قفله، ثم أعد المحاولة. لا تُحفظ بيانات الاعتماد كنص عادي.',
    'linux_output_unavailable': 'هذا المخرج غير متاح في محرك Linux المثبّت.',
    'linux_tray_unavailable':
        'علبة النظام غير متاحة. إغلاق النافذة ينهي Usque.',
  },
  'de': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Die Linux-Netzwerkeinrichtung ist fehlgeschlagen. Exportieren Sie die Diagnose, um den fehlgeschlagenen Schritt zu sehen.",
    'linux_platform_failed':
        "Die Linux-Netzwerkeinrichtung ist fehlgeschlagen. Exportieren Sie die Diagnose, um den fehlgeschlagenen Schritt zu sehen.",
    'linux_service_dependencies':
        "Installiere systemd, nftables und libsecret-1 mit der Paketverwaltung und versuche es erneut. Deine Sitzung benötigt außerdem einen Secret-Service-Schlüsselbund.",
    'linux_service_title': 'Hintergrunddienst',
    'linux_service_installed':
        'Installiert. Starte Usque nach der Installation oder Reparatur des Dienstes neu.',
    'linux_service_install_hint':
        'Prüfe die AppImage-Signatur, bevor du Administratorrechte erlaubst. Die Dienstinstallation ermöglicht VPN, stellt aber keine Verbindung her.',
    'linux_service_reinstall':
        'Installiere das passende Usque-Paket erneut, um den Dienst wiederherzustellen.',
    'linux_service_install': 'Dienst installieren',
    'linux_service_repair': 'Dienst reparieren',
    'linux_keyring_unavailable':
        'Installiere oder entsperre einen Systemschlüsselbund mit Secret-Service-Unterstützung und versuche es erneut. Zugangsdaten werden niemals im Klartext gespeichert.',
    'linux_output_unavailable':
        'Dieser Ausgang ist in der installierten Linux-Engine nicht verfügbar.',
    'linux_tray_unavailable':
        'Die Systemleiste ist nicht verfügbar. Das Schließen des Fensters beendet Usque.',
  },
  'id': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Penyiapan jaringan Linux gagal. Ekspor diagnostik untuk melihat tahap yang gagal.",
    'linux_platform_failed':
        "Penyiapan jaringan Linux gagal. Ekspor diagnostik untuk melihat tahap yang gagal.",
    'linux_service_dependencies':
        "Instal systemd, nftables, dan libsecret-1 melalui pengelola paket sistem, lalu coba lagi. Sesi login juga memerlukan keyring Secret Service.",
    'linux_service_title': 'Layanan latar belakang',
    'linux_service_installed':
        'Terpasang. Mulai ulang Usque setelah memasang atau memperbaiki layanan.',
    'linux_service_install_hint':
        'Verifikasi tanda tangan AppImage sebelum menyetujui akses administrator. Pemasangan layanan mengaktifkan dukungan VPN tanpa menghubungkan VPN.',
    'linux_service_reinstall':
        'Pasang atau pasang ulang paket Usque yang sesuai untuk memulihkan layanan.',
    'linux_service_install': 'Pasang layanan',
    'linux_service_repair': 'Perbaiki layanan',
    'linux_keyring_unavailable':
        'Pasang atau buka kunci keyring sistem yang mendukung Secret Service, lalu coba lagi. Kredensial tidak pernah disimpan sebagai teks biasa.',
    'linux_output_unavailable':
        'Keluaran ini tidak tersedia di mesin Linux yang terpasang.',
    'linux_tray_unavailable':
        'Baki sistem tidak tersedia. Menutup jendela akan keluar dari Usque.',
  },
  'it': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Configurazione di rete Linux non riuscita. Esporta la diagnostica per individuare la fase non riuscita.",
    'linux_platform_failed':
        "Configurazione di rete Linux non riuscita. Esporta la diagnostica per individuare la fase non riuscita.",
    'linux_service_dependencies':
        "Installa systemd, nftables e libsecret-1 con il gestore pacchetti di sistema, poi riprova. La sessione richiede anche un portachiavi Secret Service.",
    'linux_service_title': 'Servizio in background',
    'linux_service_installed':
        'Installato. Riavvia Usque dopo aver installato o riparato il servizio.',
    'linux_service_install_hint':
        'Verifica la firma AppImage prima di autorizzare i privilegi amministrativi. Il servizio abilita il supporto VPN senza connettersi.',
    'linux_service_reinstall':
        'Installa o reinstalla il pacchetto Usque corrispondente per ripristinare il servizio.',
    'linux_service_install': 'Installa servizio',
    'linux_service_repair': 'Ripara servizio',
    'linux_keyring_unavailable':
        'Installa o sblocca un portachiavi di sistema compatibile con Secret Service, poi riprova. Le credenziali non vengono mai salvate in chiaro.',
    'linux_output_unavailable':
        'Questa uscita non è disponibile nel motore Linux installato.',
    'linux_tray_unavailable':
        'Il vassoio di sistema non è disponibile. Chiudere la finestra termina Usque.',
  },
  'pl': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Konfiguracja sieci Linux nie powiodła się. Wyeksportuj diagnostykę, aby sprawdzić etap błędu.",
    'linux_platform_failed':
        "Konfiguracja sieci Linux nie powiodła się. Wyeksportuj diagnostykę, aby sprawdzić etap błędu.",
    'linux_service_dependencies':
        "Zainstaluj systemd, nftables i libsecret-1 za pomocą systemowego menedżera pakietów i spróbuj ponownie. Sesja wymaga też magazynu kluczy Secret Service.",
    'linux_service_title': 'Usługa w tle',
    'linux_service_installed':
        'Zainstalowana. Uruchom ponownie Usque po instalacji lub naprawie usługi.',
    'linux_service_install_hint':
        'Sprawdź podpis AppImage przed przyznaniem praw administratora. Instalacja usługi umożliwia VPN, ale nie nawiązuje połączenia.',
    'linux_service_reinstall':
        'Zainstaluj lub ponownie zainstaluj odpowiedni pakiet Usque, aby przywrócić usługę.',
    'linux_service_install': 'Zainstaluj usługę',
    'linux_service_repair': 'Napraw usługę',
    'linux_keyring_unavailable':
        'Zainstaluj lub odblokuj systemowy magazyn kluczy obsługujący Secret Service i spróbuj ponownie. Dane uwierzytelniające nigdy nie są zapisywane jawnym tekstem.',
    'linux_output_unavailable':
        'To wyjście jest niedostępne w zainstalowanym silniku Linux.',
    'linux_tray_unavailable':
        'Zasobnik systemowy jest niedostępny. Zamknięcie okna zakończy Usque.',
  },
  'th': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "ตั้งค่าเครือข่าย Linux ไม่สำเร็จ ส่งออกข้อมูลวินิจฉัยเพื่อดูขั้นตอนที่ล้มเหลว",
    'linux_platform_failed':
        "ตั้งค่าเครือข่าย Linux ไม่สำเร็จ ส่งออกข้อมูลวินิจฉัยเพื่อดูขั้นตอนที่ล้มเหลว",
    'linux_service_dependencies':
        "ติดตั้ง systemd, nftables และ libsecret-1 ผ่านตัวจัดการแพ็กเกจของระบบ แล้วลองอีกครั้ง เซสชันเข้าสู่ระบบต้องมีพวงกุญแจที่รองรับ Secret Service ด้วย",
    'linux_service_title': 'บริการเบื้องหลัง',
    'linux_service_installed':
        'ติดตั้งแล้ว เริ่ม Usque ใหม่หลังจากติดตั้งหรือซ่อมแซมบริการ',
    'linux_service_install_hint':
        'ตรวจสอบลายเซ็น AppImage ก่อนอนุญาตสิทธิ์ผู้ดูแลระบบ การติดตั้งบริการเปิดใช้การรองรับ VPN แต่ไม่เชื่อมต่อ VPN',
    'linux_service_reinstall':
        'ติดตั้งหรือติดตั้งแพ็กเกจ Usque รุ่นที่ตรงกันอีกครั้งเพื่อกู้คืนบริการ',
    'linux_service_install': 'ติดตั้งบริการ',
    'linux_service_repair': 'ซ่อมแซมบริการ',
    'linux_keyring_unavailable':
        'ติดตั้งหรือปลดล็อกพวงกุญแจระบบที่รองรับ Secret Service แล้วลองอีกครั้ง ข้อมูลรับรองจะไม่ถูกบันทึกเป็นข้อความธรรมดา',
    'linux_output_unavailable':
        'เอาต์พุตนี้ไม่พร้อมใช้งานในเอนจิน Linux ที่ติดตั้ง',
    'linux_tray_unavailable':
        'ถาดระบบไม่พร้อมใช้งาน การปิดหน้าต่างจะออกจาก Usque',
  },
  'uk': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Не вдалося налаштувати мережу Linux. Експортуйте діагностику, щоб визначити етап збою.",
    'linux_platform_failed':
        "Не вдалося налаштувати мережу Linux. Експортуйте діагностику, щоб визначити етап збою.",
    'linux_service_dependencies':
        "Установіть systemd, nftables і libsecret-1 через системний менеджер пакунків і повторіть спробу. Сеанс також потребує сховища ключів Secret Service.",
    'linux_service_title': 'Фонова служба',
    'linux_service_installed':
        'Установлено. Перезапустіть Usque після встановлення або відновлення служби.',
    'linux_service_install_hint':
        'Перевірте підпис AppImage перед наданням прав адміністратора. Установлення служби додає підтримку VPN, але не підключає VPN.',
    'linux_service_reinstall':
        'Установіть або перевстановіть відповідний пакет Usque для відновлення служби.',
    'linux_service_install': 'Установити службу',
    'linux_service_repair': 'Відновити службу',
    'linux_keyring_unavailable':
        'Установіть або розблокуйте системне сховище ключів із підтримкою Secret Service і повторіть спробу. Облікові дані ніколи не зберігаються відкритим текстом.',
    'linux_output_unavailable':
        'Цей вихід недоступний у встановленому рушії Linux.',
    'linux_tray_unavailable':
        'Системний трей недоступний. Закриття вікна завершить Usque.',
  },
  'vi': {
    'diag_fail_LINUX_PLATFORM_FAILED':
        "Thiết lập mạng Linux thất bại. Xuất chẩn đoán để xem bước gặp lỗi.",
    'linux_platform_failed':
        "Thiết lập mạng Linux thất bại. Xuất chẩn đoán để xem bước gặp lỗi.",
    'linux_service_dependencies':
        "Cài systemd, nftables và libsecret-1 bằng trình quản lý gói hệ thống rồi thử lại. Phiên đăng nhập cũng cần kho khóa hỗ trợ Secret Service.",
    'linux_service_title': 'Dịch vụ nền',
    'linux_service_installed':
        'Đã cài đặt. Khởi động lại Usque sau khi cài đặt hoặc sửa dịch vụ.',
    'linux_service_install_hint':
        'Xác minh chữ ký AppImage trước khi cấp quyền quản trị. Cài dịch vụ cho phép hỗ trợ VPN nhưng không kết nối VPN.',
    'linux_service_reinstall':
        'Cài đặt hoặc cài lại gói Usque phù hợp để khôi phục dịch vụ.',
    'linux_service_install': 'Cài dịch vụ',
    'linux_service_repair': 'Sửa dịch vụ',
    'linux_keyring_unavailable':
        'Cài đặt hoặc mở khóa kho khóa hệ thống hỗ trợ Secret Service, rồi thử lại. Thông tin đăng nhập không bao giờ được lưu dưới dạng văn bản thuần.',
    'linux_output_unavailable':
        'Đầu ra này không khả dụng trong bộ máy Linux đã cài đặt.',
    'linux_tray_unavailable':
        'Khay hệ thống không khả dụng. Đóng cửa sổ sẽ thoát Usque.',
  },
};
