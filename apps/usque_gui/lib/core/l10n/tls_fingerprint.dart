const kTlsFingerprintCatalogs = <String, Map<String, String>>{
  'en': {
    "masque_connections": "MASQUE connections",
    "masque_connections_help":
        "Enter 1–{max}; default 3. Applies to HTTP/2, HTTP/3 and L4. Applying a change reconnects.",
    "masque_connections_unsupported":
        "Update Usque to configure multiple MASQUE connections.",
    'tls_fingerprint': 'TLS fingerprint',
    'tls_fingerprint_help':
        'Applies to HTTP/2, HTTP/3 and L4 on the next connection.',
    'tls_fingerprint_unsupported':
        'TLS fingerprint selection is unavailable in this version. Update Usque to change it.',
  },
  'zh_CN': {
    "masque_connections": "MASQUE 连接数",
    "masque_connections_help":
        "可填写 1–{max}，默认 3。适用于 HTTP/2、HTTP/3 和 L4，应用修改后将重新连接。",
    "masque_connections_unsupported": "请更新 Usque 后配置多条 MASQUE 连接。",
    'tls_fingerprint': 'TLS 指纹',
    'tls_fingerprint_help': '适用于 HTTP/2、HTTP/3 和 L4，下次连接时生效。',
    'tls_fingerprint_unsupported': '此版本不支持选择 TLS 指纹，请更新 Usque 后修改。',
  },
  'zh_TW': {
    "masque_connections": "MASQUE 連線數",
    "masque_connections_help":
        "可填寫 1–{max}，預設 3。適用於 HTTP/2、HTTP/3 和 L4，套用變更後將重新連線。",
    "masque_connections_unsupported": "請更新 Usque 後設定多條 MASQUE 連線。",
    'tls_fingerprint': 'TLS 指紋',
    'tls_fingerprint_help': '適用於 HTTP/2、HTTP/3 和 L4，下次連線時生效。',
    'tls_fingerprint_unsupported': '此版本不支援選擇 TLS 指紋，請更新 Usque 後修改。',
  },
  'zh_HK': {
    "masque_connections": "MASQUE 連線數",
    "masque_connections_help":
        "可輸入 1–{max}，預設 3。適用於 HTTP/2、HTTP/3 和 L4，套用變更後會重新連線。",
    "masque_connections_unsupported": "請更新 Usque 後設定多條 MASQUE 連線。",
    'tls_fingerprint': 'TLS 指紋',
    'tls_fingerprint_help': '適用於 HTTP/2、HTTP/3 和 L4，下次連線時生效。',
    'tls_fingerprint_unsupported': '此版本不支援選擇 TLS 指紋，請更新 Usque 後修改。',
  },
  'ja': {
    "masque_connections": "MASQUE 接続数",
    "masque_connections_help":
        "1～{max} を入力します。既定値は 3 です。HTTP/2、HTTP/3、L4 に適用され、変更の適用時に再接続します。",
    "masque_connections_unsupported": "複数の MASQUE 接続を設定するには Usque を更新してください。",
    'tls_fingerprint': 'TLS フィンガープリント',
    'tls_fingerprint_help': '次回の接続から HTTP/2、HTTP/3、L4 に適用されます。',
    'tls_fingerprint_unsupported':
        'このバージョンでは TLS フィンガープリントを選択できません。変更するには Usque を更新してください。',
  },
  'ko': {
    "masque_connections": "MASQUE 연결 수",
    "masque_connections_help":
        "1부터 {max}까지 입력하세요. 기본값은 3입니다. HTTP/2, HTTP/3, L4에 적용되며 변경 사항을 적용하면 다시 연결합니다.",
    "masque_connections_unsupported": "여러 MASQUE 연결을 설정하려면 Usque를 업데이트하세요.",
    'tls_fingerprint': 'TLS 지문',
    'tls_fingerprint_help': '다음 연결부터 HTTP/2, HTTP/3 및 L4에 적용됩니다.',
    'tls_fingerprint_unsupported':
        '이 버전에서는 TLS 지문을 선택할 수 없습니다. 변경하려면 Usque를 업데이트하세요.',
  },
  'es': {
    "masque_connections": "Conexiones MASQUE",
    "masque_connections_help":
        "Introduce 1–{max}; valor predeterminado: 3. Se aplica a HTTP/2, HTTP/3 y L4. Aplicar el cambio vuelve a conectar.",
    "masque_connections_unsupported":
        "Actualiza Usque para configurar varias conexiones MASQUE.",
    'tls_fingerprint': 'Huella TLS',
    'tls_fingerprint_help':
        'Se aplica a HTTP/2, HTTP/3 y L4 en la próxima conexión.',
    'tls_fingerprint_unsupported':
        'Esta versión no permite elegir la huella TLS. Actualiza Usque para cambiarla.',
  },
  'pt': {
    "masque_connections": "Conexões MASQUE",
    "masque_connections_help":
        "Insira 1–{max}; padrão: 3. Vale para HTTP/2, HTTP/3 e L4. Aplicar a alteração reconecta.",
    "masque_connections_unsupported":
        "Atualize o Usque para configurar várias conexões MASQUE.",
    'tls_fingerprint': 'Impressão digital TLS',
    'tls_fingerprint_help': 'Vale para HTTP/2, HTTP/3 e L4 na próxima conexão.',
    'tls_fingerprint_unsupported':
        'Esta versão não permite escolher a impressão digital TLS. Atualize o Usque para alterá-la.',
  },
  'fr': {
    "masque_connections": "Connexions MASQUE",
    "masque_connections_help":
        "Saisissez 1–{max} ; valeur par défaut : 3. Pour HTTP/2, HTTP/3 et L4. Appliquer la modification reconnecte.",
    "masque_connections_unsupported":
        "Mettez Usque à jour pour configurer plusieurs connexions MASQUE.",
    'tls_fingerprint': 'Empreinte TLS',
    'tls_fingerprint_help':
        'S’applique à HTTP/2, HTTP/3 et L4 à la prochaine connexion.',
    'tls_fingerprint_unsupported':
        'Cette version ne permet pas de choisir l’empreinte TLS. Mettez Usque à jour pour la modifier.',
  },
  'nl': {
    "masque_connections": "MASQUE-verbindingen",
    "masque_connections_help":
        "Voer 1–{max} in; standaard 3. Voor HTTP/2, HTTP/3 en L4. Toepassen maakt opnieuw verbinding.",
    "masque_connections_unsupported":
        "Werk Usque bij om meerdere MASQUE-verbindingen in te stellen.",
    'tls_fingerprint': 'TLS-vingerafdruk',
    'tls_fingerprint_help':
        'Geldt voor HTTP/2, HTTP/3 en L4 bij de volgende verbinding.',
    'tls_fingerprint_unsupported':
        'Deze versie ondersteunt geen TLS-vingerafdrukselectie. Werk Usque bij om deze te wijzigen.',
  },
  'tr': {
    "masque_connections": "MASQUE bağlantı sayısı",
    "masque_connections_help":
        "1–{max} girin; varsayılan 3. HTTP/2, HTTP/3 ve L4 için geçerlidir. Değişikliği uygulamak yeniden bağlanır.",
    "masque_connections_unsupported":
        "Birden fazla MASQUE bağlantısı ayarlamak için Usque’yi güncelleyin.",
    'tls_fingerprint': 'TLS parmak izi',
    'tls_fingerprint_help':
        'Sonraki bağlantıda HTTP/2, HTTP/3 ve L4 için uygulanır.',
    'tls_fingerprint_unsupported':
        'Bu sürümde TLS parmak izi seçilemiyor. Değiştirmek için Usque uygulamasını güncelleyin.',
  },
  'ru': {
    "masque_connections": "Количество соединений MASQUE",
    "masque_connections_help":
        "Введите 1–{max}; по умолчанию 3. Для HTTP/2, HTTP/3 и L4. Применение изменения переподключает.",
    "masque_connections_unsupported":
        "Обновите Usque, чтобы настроить несколько соединений MASQUE.",
    'tls_fingerprint': 'Отпечаток TLS',
    'tls_fingerprint_help':
        'Применяется к HTTP/2, HTTP/3 и L4 при следующем подключении.',
    'tls_fingerprint_unsupported':
        'В этой версии нельзя выбрать отпечаток TLS. Для изменения обновите Usque.',
  },
  'fa': {
    "masque_connections": "تعداد اتصال‌های MASQUE",
    "masque_connections_help":
        "عددی از ۱ تا {max} وارد کنید؛ پیش‌فرض ۳ است. برای HTTP/2، HTTP/3 و L4 کاربرد دارد. اعمال تغییر باعث اتصال دوباره می‌شود.",
    "masque_connections_unsupported":
        "برای تنظیم چند اتصال MASQUE، Usque را به‌روز کنید.",
    'tls_fingerprint': 'اثر انگشت TLS',
    'tls_fingerprint_help':
        'در اتصال بعدی برای HTTP/2، HTTP/3 و L4 اعمال می‌شود.',
    'tls_fingerprint_unsupported':
        'انتخاب اثر انگشت TLS در این نسخه در دسترس نیست. برای تغییر آن Usque را به‌روز کنید.',
  },
  'ar': {
    "masque_connections": "عدد اتصالات MASQUE",
    "masque_connections_help":
        "أدخل من 1 إلى {max}؛ الافتراضي 3. ينطبق على HTTP/2 وHTTP/3 وL4. يؤدي تطبيق التغيير إلى إعادة الاتصال.",
    "masque_connections_unsupported": "حدّث Usque لضبط عدة اتصالات MASQUE.",
    'tls_fingerprint': 'بصمة TLS',
    'tls_fingerprint_help': 'تُطبّق على HTTP/2 وHTTP/3 وL4 عند الاتصال التالي.',
    'tls_fingerprint_unsupported':
        'اختيار بصمة TLS غير متاح في هذا الإصدار. حدّث Usque لتغييرها.',
  },
  'de': {
    "masque_connections": "MASQUE-Verbindungen",
    "masque_connections_help":
        "1–{max} eingeben; Standard: 3. Gilt für HTTP/2, HTTP/3 und L4. Anwenden stellt die Verbindung neu her.",
    "masque_connections_unsupported":
        "Usque aktualisieren, um mehrere MASQUE-Verbindungen einzustellen.",
    'tls_fingerprint': 'TLS-Fingerabdruck',
    'tls_fingerprint_help':
        'Gilt ab der nächsten Verbindung für HTTP/2, HTTP/3 und L4.',
    'tls_fingerprint_unsupported':
        'Diese Version unterstützt keine TLS-Fingerabdruckauswahl. Aktualisiere Usque, um sie zu ändern.',
  },
  'id': {
    "masque_connections": "Jumlah koneksi MASQUE",
    "masque_connections_help":
        "Masukkan 1–{max}; bawaan 3. Berlaku untuk HTTP/2, HTTP/3, dan L4. Menerapkan perubahan akan menyambung ulang.",
    "masque_connections_unsupported":
        "Perbarui Usque untuk mengatur beberapa koneksi MASQUE.",
    'tls_fingerprint': 'Sidik jari TLS',
    'tls_fingerprint_help':
        'Berlaku untuk HTTP/2, HTTP/3, dan L4 pada koneksi berikutnya.',
    'tls_fingerprint_unsupported':
        'Pemilihan sidik jari TLS tidak tersedia pada versi ini. Perbarui Usque untuk mengubahnya.',
  },
  'it': {
    "masque_connections": "Connessioni MASQUE",
    "masque_connections_help":
        "Inserisci 1–{max}; predefinito: 3. Per HTTP/2, HTTP/3 e L4. Applicare la modifica riconnette.",
    "masque_connections_unsupported":
        "Aggiorna Usque per configurare più connessioni MASQUE.",
    'tls_fingerprint': 'Impronta TLS',
    'tls_fingerprint_help':
        'Si applica a HTTP/2, HTTP/3 e L4 alla prossima connessione.',
    'tls_fingerprint_unsupported':
        'Questa versione non consente di scegliere l’impronta TLS. Aggiorna Usque per modificarla.',
  },
  'pl': {
    "masque_connections": "Liczba połączeń MASQUE",
    "masque_connections_help":
        "Wpisz 1–{max}; domyślnie 3. Dla HTTP/2, HTTP/3 i L4. Zastosowanie zmiany łączy ponownie.",
    "masque_connections_unsupported":
        "Zaktualizuj Usque, aby skonfigurować wiele połączeń MASQUE.",
    'tls_fingerprint': 'Odcisk TLS',
    'tls_fingerprint_help':
        'Dotyczy HTTP/2, HTTP/3 i L4 przy następnym połączeniu.',
    'tls_fingerprint_unsupported':
        'Ta wersja nie obsługuje wyboru odcisku TLS. Zaktualizuj Usque, aby go zmienić.',
  },
  'th': {
    "masque_connections": "จำนวนการเชื่อมต่อ MASQUE",
    "masque_connections_help":
        "กรอก 1–{max} ค่าเริ่มต้นคือ 3 ใช้กับ HTTP/2, HTTP/3 และ L4 การใช้ค่าที่เปลี่ยนจะเชื่อมต่อใหม่",
    "masque_connections_unsupported":
        "อัปเดต Usque เพื่อตั้งค่าการเชื่อมต่อ MASQUE หลายรายการ",
    'tls_fingerprint': 'ลายนิ้วมือ TLS',
    'tls_fingerprint_help':
        'ใช้กับ HTTP/2, HTTP/3 และ L4 ในการเชื่อมต่อครั้งถัดไป',
    'tls_fingerprint_unsupported':
        'เวอร์ชันนี้ไม่รองรับการเลือกลายนิ้วมือ TLS โปรดอัปเดต Usque เพื่อเปลี่ยนค่า',
  },
  'uk': {
    "masque_connections": "Кількість з’єднань MASQUE",
    "masque_connections_help":
        "Введіть 1–{max}; типово 3. Для HTTP/2, HTTP/3 і L4. Застосування зміни виконує повторне підключення.",
    "masque_connections_unsupported":
        "Оновіть Usque, щоб налаштувати кілька з’єднань MASQUE.",
    'tls_fingerprint': 'Відбиток TLS',
    'tls_fingerprint_help':
        'Застосовується до HTTP/2, HTTP/3 і L4 під час наступного підключення.',
    'tls_fingerprint_unsupported':
        'Ця версія не підтримує вибір відбитка TLS. Оновіть Usque, щоб змінити його.',
  },
  'vi': {
    "masque_connections": "Số kết nối MASQUE",
    "masque_connections_help":
        "Nhập 1–{max}; mặc định 3. Áp dụng cho HTTP/2, HTTP/3 và L4. Áp dụng thay đổi sẽ kết nối lại.",
    "masque_connections_unsupported":
        "Cập nhật Usque để thiết lập nhiều kết nối MASQUE.",
    'tls_fingerprint': 'Dấu vân tay TLS',
    'tls_fingerprint_help':
        'Áp dụng cho HTTP/2, HTTP/3 và L4 ở lần kết nối tiếp theo.',
    'tls_fingerprint_unsupported':
        'Phiên bản này không hỗ trợ chọn dấu vân tay TLS. Hãy cập nhật Usque để thay đổi.',
  },
};
