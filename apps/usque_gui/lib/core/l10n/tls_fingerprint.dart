const kTlsFingerprintCatalogs = <String, Map<String, String>>{
  'en': {
    'tls_fingerprint': 'TLS fingerprint',
    'tls_fingerprint_help': 'Applies to H2, H3 and L4 on the next connection.',
    'tls_fingerprint_unsupported':
        'TLS fingerprint selection is unavailable in this version. Update Usque to change it.',
  },
  'zh_CN': {
    'tls_fingerprint': 'TLS 指纹',
    'tls_fingerprint_help': '适用于 H2、H3 和 L4，下次连接时生效。',
    'tls_fingerprint_unsupported': '此版本不支持选择 TLS 指纹，请更新 Usque 后修改。',
  },
  'zh_TW': {
    'tls_fingerprint': 'TLS 指紋',
    'tls_fingerprint_help': '適用於 H2、H3 和 L4，下次連線時生效。',
    'tls_fingerprint_unsupported': '此版本不支援選擇 TLS 指紋，請更新 Usque 後修改。',
  },
  'zh_HK': {
    'tls_fingerprint': 'TLS 指紋',
    'tls_fingerprint_help': '適用於 H2、H3 和 L4，下次連線時生效。',
    'tls_fingerprint_unsupported': '此版本不支援選擇 TLS 指紋，請更新 Usque 後修改。',
  },
  'ja': {
    'tls_fingerprint': 'TLS フィンガープリント',
    'tls_fingerprint_help': '次回の接続から H2、H3、L4 に適用されます。',
    'tls_fingerprint_unsupported':
        'このバージョンでは TLS フィンガープリントを選択できません。変更するには Usque を更新してください。',
  },
  'ko': {
    'tls_fingerprint': 'TLS 지문',
    'tls_fingerprint_help': '다음 연결부터 H2, H3 및 L4에 적용됩니다.',
    'tls_fingerprint_unsupported':
        '이 버전에서는 TLS 지문을 선택할 수 없습니다. 변경하려면 Usque를 업데이트하세요.',
  },
  'es': {
    'tls_fingerprint': 'Huella TLS',
    'tls_fingerprint_help': 'Se aplica a H2, H3 y L4 en la próxima conexión.',
    'tls_fingerprint_unsupported':
        'Esta versión no permite elegir la huella TLS. Actualiza Usque para cambiarla.',
  },
  'pt': {
    'tls_fingerprint': 'Impressão digital TLS',
    'tls_fingerprint_help': 'Aplica-se a H2, H3 e L4 na próxima conexão.',
    'tls_fingerprint_unsupported':
        'Esta versão não permite escolher a impressão digital TLS. Atualize o Usque para alterá-la.',
  },
  'fr': {
    'tls_fingerprint': 'Empreinte TLS',
    'tls_fingerprint_help':
        'S’applique à H2, H3 et L4 à la prochaine connexion.',
    'tls_fingerprint_unsupported':
        'Cette version ne permet pas de choisir l’empreinte TLS. Mettez Usque à jour pour la modifier.',
  },
  'nl': {
    'tls_fingerprint': 'TLS-vingerafdruk',
    'tls_fingerprint_help':
        'Geldt voor H2, H3 en L4 bij de volgende verbinding.',
    'tls_fingerprint_unsupported':
        'Deze versie ondersteunt geen TLS-vingerafdrukselectie. Werk Usque bij om deze te wijzigen.',
  },
  'tr': {
    'tls_fingerprint': 'TLS parmak izi',
    'tls_fingerprint_help': 'Sonraki bağlantıda H2, H3 ve L4 için uygulanır.',
    'tls_fingerprint_unsupported':
        'Bu sürümde TLS parmak izi seçilemiyor. Değiştirmek için Usque uygulamasını güncelleyin.',
  },
  'ru': {
    'tls_fingerprint': 'Отпечаток TLS',
    'tls_fingerprint_help':
        'Применяется к H2, H3 и L4 при следующем подключении.',
    'tls_fingerprint_unsupported':
        'В этой версии нельзя выбрать отпечаток TLS. Для изменения обновите Usque.',
  },
  'fa': {
    'tls_fingerprint': 'اثر انگشت TLS',
    'tls_fingerprint_help': 'در اتصال بعدی برای H2، H3 و L4 اعمال می‌شود.',
    'tls_fingerprint_unsupported':
        'انتخاب اثر انگشت TLS در این نسخه در دسترس نیست. برای تغییر آن Usque را به‌روز کنید.',
  },
  'ar': {
    'tls_fingerprint': 'بصمة TLS',
    'tls_fingerprint_help': 'تُطبّق على H2 وH3 وL4 عند الاتصال التالي.',
    'tls_fingerprint_unsupported':
        'اختيار بصمة TLS غير متاح في هذا الإصدار. حدّث Usque لتغييرها.',
  },
  'de': {
    'tls_fingerprint': 'TLS-Fingerabdruck',
    'tls_fingerprint_help':
        'Gilt ab der nächsten Verbindung für H2, H3 und L4.',
    'tls_fingerprint_unsupported':
        'Diese Version unterstützt keine TLS-Fingerabdruckauswahl. Aktualisiere Usque, um sie zu ändern.',
  },
  'id': {
    'tls_fingerprint': 'Sidik jari TLS',
    'tls_fingerprint_help':
        'Berlaku untuk H2, H3, dan L4 pada koneksi berikutnya.',
    'tls_fingerprint_unsupported':
        'Pemilihan sidik jari TLS tidak tersedia pada versi ini. Perbarui Usque untuk mengubahnya.',
  },
  'it': {
    'tls_fingerprint': 'Impronta TLS',
    'tls_fingerprint_help':
        'Si applica a H2, H3 e L4 alla prossima connessione.',
    'tls_fingerprint_unsupported':
        'Questa versione non consente di scegliere l’impronta TLS. Aggiorna Usque per modificarla.',
  },
  'pl': {
    'tls_fingerprint': 'Odcisk TLS',
    'tls_fingerprint_help': 'Dotyczy H2, H3 i L4 przy następnym połączeniu.',
    'tls_fingerprint_unsupported':
        'Ta wersja nie obsługuje wyboru odcisku TLS. Zaktualizuj Usque, aby go zmienić.',
  },
  'th': {
    'tls_fingerprint': 'ลายนิ้วมือ TLS',
    'tls_fingerprint_help': 'ใช้กับ H2, H3 และ L4 ในการเชื่อมต่อครั้งถัดไป',
    'tls_fingerprint_unsupported':
        'เวอร์ชันนี้ไม่รองรับการเลือกลายนิ้วมือ TLS โปรดอัปเดต Usque เพื่อเปลี่ยนค่า',
  },
  'uk': {
    'tls_fingerprint': 'Відбиток TLS',
    'tls_fingerprint_help':
        'Застосовується до H2, H3 і L4 під час наступного підключення.',
    'tls_fingerprint_unsupported':
        'Ця версія не підтримує вибір відбитка TLS. Оновіть Usque, щоб змінити його.',
  },
  'vi': {
    'tls_fingerprint': 'Dấu vân tay TLS',
    'tls_fingerprint_help': 'Áp dụng cho H2, H3 và L4 ở lần kết nối tiếp theo.',
    'tls_fingerprint_unsupported':
        'Phiên bản này không hỗ trợ chọn dấu vân tay TLS. Hãy cập nhật Usque để thay đổi.',
  },
};
