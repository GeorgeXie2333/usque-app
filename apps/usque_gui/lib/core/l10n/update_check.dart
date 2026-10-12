// Fixed update error messages; never interpolate backend diagnostics.
const kUpdateCheckCatalogs = <String, Map<String, String>>{
  'en': {
    'update_check_failed':
        "Update check failed. Try again; if it persists, download the latest version from the official GitHub Releases page.",
    'update_check_network':
        "Could not reach GitHub to check for updates, or the request timed out. Check your network and try again.",
    'update_check_rate_limited':
        "GitHub has temporarily limited update requests. Please try again later.",
    'update_check_unavailable':
        "GitHub update information is unavailable. Try again later or open the official Releases page.",
    'update_check_response_too_large':
        "The update response exceeded the safety size limit. Check the official GitHub Releases page for a newer version.",
    'update_check_invalid_response':
        "GitHub returned invalid update information. The update check was stopped. Try again later or open the official Releases page.",
  },
  'zh_CN': {
    'update_check_failed': "检查更新失败。请重试；若仍失败，请前往 GitHub 官方发布页下载最新版本。",
    'update_check_network': "无法连接 GitHub 检查更新，或请求已超时。请检查网络后重试。",
    'update_check_rate_limited': "GitHub 暂时限制了更新请求，请稍后重试。",
    'update_check_unavailable': "暂时无法获取 GitHub 更新信息，请稍后重试或打开官方发布页。",
    'update_check_response_too_large': "更新响应超过安全大小限制。请前往 GitHub 官方发布页查看新版本。",
    'update_check_invalid_response': "GitHub 返回的更新信息无效，已停止检查。请稍后重试或打开官方发布页。",
  },
  'zh_TW': {
    'update_check_failed': "檢查更新失敗。請重試；若仍失敗，請前往 GitHub 官方發佈頁下載最新版本。",
    'update_check_network': "無法連線至 GitHub 檢查更新，或請求已逾時。請檢查網路後重試。",
    'update_check_rate_limited': "GitHub 暫時限制了更新請求，請稍後重試。",
    'update_check_unavailable': "暫時無法取得 GitHub 更新資訊，請稍後重試或開啟官方發佈頁。",
    'update_check_response_too_large': "更新回應超過安全大小限制。請前往 GitHub 官方發佈頁查看新版本。",
    'update_check_invalid_response': "GitHub 傳回的更新資訊無效，已停止檢查。請稍後重試或開啟官方發佈頁。",
  },
  'zh_HK': {
    'update_check_failed': "檢查更新失敗。請重試；若仍失敗，請前往 GitHub 官方發佈頁下載最新版本。",
    'update_check_network': "無法連線至 GitHub 檢查更新，或請求已逾時。請檢查網絡後重試。",
    'update_check_rate_limited': "GitHub 暫時限制了更新請求，請稍後重試。",
    'update_check_unavailable': "暫時無法取得 GitHub 更新資訊，請稍後重試或開啟官方發佈頁。",
    'update_check_response_too_large': "更新回應超過安全大小限制。請前往 GitHub 官方發佈頁查看新版本。",
    'update_check_invalid_response': "GitHub 傳回的更新資訊無效，已停止檢查。請稍後重試或開啟官方發佈頁。",
  },
  'ja': {
    'update_check_failed':
        "更新の確認に失敗しました。再試行し、解決しない場合は GitHub の公式リリースページから最新版をダウンロードしてください。",
    'update_check_network':
        "更新確認のため GitHub に接続できないか、要求がタイムアウトしました。ネットワークを確認して再試行してください。",
    'update_check_rate_limited': "GitHub が更新要求を一時的に制限しています。しばらくしてから再試行してください。",
    'update_check_unavailable':
        "GitHub の更新情報を取得できません。後で再試行するか、公式リリースページを開いてください。",
    'update_check_response_too_large':
        "更新応答が安全上のサイズ制限を超えました。GitHub の公式リリースページで新しいバージョンを確認してください。",
    'update_check_invalid_response':
        "GitHub の更新情報が無効なため、確認を停止しました。後で再試行するか、公式リリースページを開いてください。",
  },
  'ko': {
    'update_check_failed':
        "업데이트 확인에 실패했습니다. 다시 시도하고, 계속 실패하면 GitHub 공식 릴리스 페이지에서 최신 버전을 다운로드하세요.",
    'update_check_network':
        "업데이트 확인을 위해 GitHub에 연결할 수 없거나 요청 시간이 초과되었습니다. 네트워크를 확인하고 다시 시도하세요.",
    'update_check_rate_limited':
        "GitHub에서 업데이트 요청을 일시적으로 제한했습니다. 나중에 다시 시도하세요.",
    'update_check_unavailable':
        "GitHub 업데이트 정보를 가져올 수 없습니다. 나중에 다시 시도하거나 공식 릴리스 페이지를 여세요.",
    'update_check_response_too_large':
        "업데이트 응답이 안전 크기 제한을 초과했습니다. GitHub 공식 릴리스 페이지에서 새 버전을 확인하세요.",
    'update_check_invalid_response':
        "GitHub에서 잘못된 업데이트 정보를 반환하여 확인을 중단했습니다. 나중에 다시 시도하거나 공식 릴리스 페이지를 여세요.",
  },
  'es': {
    'update_check_failed':
        "No se pudo buscar actualizaciones. Reinténtalo; si el problema persiste, descarga la última versión desde la página oficial de versiones de GitHub.",
    'update_check_network':
        "No se pudo conectar con GitHub o la solicitud agotó el tiempo de espera. Comprueba tu red y reinténtalo.",
    'update_check_rate_limited':
        "GitHub ha limitado temporalmente las solicitudes de actualización. Inténtalo más tarde.",
    'update_check_unavailable':
        "La información de actualizaciones de GitHub no está disponible. Inténtalo más tarde o abre la página oficial de versiones.",
    'update_check_response_too_large':
        "La respuesta de actualización superó el límite de tamaño de seguridad. Busca una versión nueva en la página oficial de versiones de GitHub.",
    'update_check_invalid_response':
        "GitHub devolvió información de actualización no válida. Se detuvo la comprobación. Inténtalo más tarde o abre la página oficial de versiones.",
  },
  'pt': {
    'update_check_failed':
        "Falha ao procurar atualizações. Tente novamente; se persistir, baixe a versão mais recente na página oficial de versões do GitHub.",
    'update_check_network':
        "Não foi possível conectar ao GitHub ou a solicitação expirou. Verifique a rede e tente novamente.",
    'update_check_rate_limited':
        "O GitHub limitou temporariamente as solicitações de atualização. Tente novamente mais tarde.",
    'update_check_unavailable':
        "As informações de atualização do GitHub estão indisponíveis. Tente mais tarde ou abra a página oficial de versões.",
    'update_check_response_too_large':
        "A resposta de atualização excedeu o limite de tamanho de segurança. Confira uma nova versão na página oficial de versões do GitHub.",
    'update_check_invalid_response':
        "O GitHub retornou informações de atualização inválidas. A verificação foi interrompida. Tente mais tarde ou abra a página oficial de versões.",
  },
  'fr': {
    'update_check_failed':
        "La recherche de mises à jour a échoué. Réessayez ; si le problème persiste, téléchargez la dernière version depuis la page officielle des versions GitHub.",
    'update_check_network':
        "Impossible de joindre GitHub ou délai de la requête dépassé. Vérifiez votre réseau et réessayez.",
    'update_check_rate_limited':
        "GitHub limite temporairement les demandes de mise à jour. Réessayez plus tard.",
    'update_check_unavailable':
        "Les informations de mise à jour GitHub sont indisponibles. Réessayez plus tard ou ouvrez la page officielle des versions.",
    'update_check_response_too_large':
        "La réponse de mise à jour dépasse la taille maximale de sécurité. Consultez la page officielle des versions GitHub pour une nouvelle version.",
    'update_check_invalid_response':
        "GitHub a renvoyé des informations de mise à jour non valides. La vérification a été arrêtée. Réessayez plus tard ou ouvrez la page officielle des versions.",
  },
  'nl': {
    'update_check_failed':
        "Controleren op updates is mislukt. Probeer het opnieuw; download bij aanhoudende problemen de nieuwste versie via de officiële GitHub-releasepagina.",
    'update_check_network':
        "GitHub is niet bereikbaar of het verzoek is verlopen. Controleer je netwerk en probeer het opnieuw.",
    'update_check_rate_limited':
        "GitHub beperkt updateverzoeken tijdelijk. Probeer het later opnieuw.",
    'update_check_unavailable':
        "Updategegevens van GitHub zijn niet beschikbaar. Probeer het later opnieuw of open de officiële releasepagina.",
    'update_check_response_too_large':
        "Het updateantwoord overschrijdt de veilige groottelimiet. Kijk op de officiële GitHub-releasepagina voor een nieuwe versie.",
    'update_check_invalid_response':
        "GitHub heeft ongeldige updategegevens teruggestuurd. De controle is gestopt. Probeer het later opnieuw of open de officiële releasepagina.",
  },
  'ru': {
    'update_check_failed':
        "Не удалось проверить обновления. Повторите попытку; если ошибка сохраняется, скачайте последнюю версию с официальной страницы релизов GitHub.",
    'update_check_network':
        "Не удалось подключиться к GitHub или истекло время ожидания. Проверьте сеть и повторите попытку.",
    'update_check_rate_limited':
        "GitHub временно ограничил запросы обновлений. Повторите попытку позже.",
    'update_check_unavailable':
        "Информация об обновлениях GitHub недоступна. Повторите попытку позже или откройте официальную страницу релизов.",
    'update_check_response_too_large':
        "Ответ сервера обновлений превысил безопасный предел размера. Проверьте новую версию на официальной странице релизов GitHub.",
    'update_check_invalid_response':
        "GitHub вернул некорректные данные обновления. Проверка остановлена. Повторите попытку позже или откройте официальную страницу релизов.",
  },
  'tr': {
    'update_check_failed':
        "Güncelleme denetimi başarısız oldu. Tekrar deneyin; sorun sürerse en son sürümü resmî GitHub sürümler sayfasından indirin.",
    'update_check_network':
        "GitHub’a bağlanılamadı veya istek zaman aşımına uğradı. Ağınızı kontrol edip tekrar deneyin.",
    'update_check_rate_limited':
        "GitHub güncelleme isteklerini geçici olarak sınırladı. Daha sonra tekrar deneyin.",
    'update_check_unavailable':
        "GitHub güncelleme bilgileri alınamıyor. Daha sonra tekrar deneyin veya resmî sürümler sayfasını açın.",
    'update_check_response_too_large':
        "Güncelleme yanıtı güvenli boyut sınırını aştı. Yeni sürüm için resmî GitHub sürümler sayfasına bakın.",
    'update_check_invalid_response':
        "GitHub geçersiz güncelleme bilgileri döndürdü. Denetim durduruldu. Daha sonra tekrar deneyin veya resmî sürümler sayfasını açın.",
  },
  'fa': {
    'update_check_failed':
        "بررسی به‌روزرسانی ناموفق بود. دوباره تلاش کنید؛ اگر مشکل ادامه داشت، آخرین نسخه را از صفحهٔ رسمی انتشارها در GitHub دریافت کنید.",
    'update_check_network':
        "اتصال به GitHub برقرار نشد یا مهلت درخواست پایان یافت. شبکه را بررسی کنید و دوباره تلاش کنید.",
    'update_check_rate_limited':
        "GitHub درخواست‌های به‌روزرسانی را موقتاً محدود کرده است. بعداً دوباره تلاش کنید.",
    'update_check_unavailable':
        "اطلاعات به‌روزرسانی GitHub در دسترس نیست. بعداً تلاش کنید یا صفحهٔ رسمی انتشارها را باز کنید.",
    'update_check_response_too_large':
        "پاسخ به‌روزرسانی از محدودیت اندازهٔ ایمن فراتر رفت. نسخهٔ جدید را در صفحهٔ رسمی انتشارهای GitHub بررسی کنید.",
    'update_check_invalid_response':
        "GitHub اطلاعات به‌روزرسانی نامعتبر برگرداند. بررسی متوقف شد. بعداً تلاش کنید یا صفحهٔ رسمی انتشارها را باز کنید.",
  },
  'ar': {
    'update_check_failed':
        "فشل البحث عن تحديثات. أعد المحاولة؛ وإذا استمرت المشكلة، نزّل أحدث إصدار من صفحة الإصدارات الرسمية على GitHub.",
    'update_check_network':
        "تعذر الاتصال بـ GitHub أو انتهت مهلة الطلب. تحقق من الشبكة وأعد المحاولة.",
    'update_check_rate_limited':
        "فرض GitHub حدًا مؤقتًا على طلبات التحديث. أعد المحاولة لاحقًا.",
    'update_check_unavailable':
        "معلومات تحديث GitHub غير متاحة. أعد المحاولة لاحقًا أو افتح صفحة الإصدارات الرسمية.",
    'update_check_response_too_large':
        "تجاوز رد التحديث حد الحجم الآمن. تحقق من وجود إصدار جديد في صفحة الإصدارات الرسمية على GitHub.",
    'update_check_invalid_response':
        "أعاد GitHub معلومات تحديث غير صالحة، فتوقف التحقق. أعد المحاولة لاحقًا أو افتح صفحة الإصدارات الرسمية.",
  },
  'de': {
    'update_check_failed':
        "Die Updateprüfung ist fehlgeschlagen. Versuche es erneut; lade bei anhaltenden Problemen die neueste Version von der offiziellen GitHub-Releaseseite herunter.",
    'update_check_network':
        "GitHub ist nicht erreichbar oder die Anfrage hat das Zeitlimit überschritten. Prüfe dein Netzwerk und versuche es erneut.",
    'update_check_rate_limited':
        "GitHub hat Updateanfragen vorübergehend begrenzt. Versuche es später erneut.",
    'update_check_unavailable':
        "Updateinformationen von GitHub sind nicht verfügbar. Versuche es später erneut oder öffne die offizielle Releaseseite.",
    'update_check_response_too_large':
        "Die Updateantwort überschreitet die sichere Größenbegrenzung. Suche auf der offiziellen GitHub-Releaseseite nach einer neuen Version.",
    'update_check_invalid_response':
        "GitHub hat ungültige Updateinformationen geliefert. Die Prüfung wurde gestoppt. Versuche es später erneut oder öffne die offizielle Releaseseite.",
  },
  'id': {
    'update_check_failed':
        "Pemeriksaan pembaruan gagal. Coba lagi; jika tetap gagal, unduh versi terbaru dari halaman rilis resmi GitHub.",
    'update_check_network':
        "Tidak dapat terhubung ke GitHub atau permintaan kehabisan waktu. Periksa jaringan dan coba lagi.",
    'update_check_rate_limited':
        "GitHub membatasi permintaan pembaruan untuk sementara. Coba lagi nanti.",
    'update_check_unavailable':
        "Informasi pembaruan GitHub tidak tersedia. Coba lagi nanti atau buka halaman rilis resmi.",
    'update_check_response_too_large':
        "Respons pembaruan melebihi batas ukuran aman. Periksa versi baru di halaman rilis resmi GitHub.",
    'update_check_invalid_response':
        "GitHub mengembalikan informasi pembaruan yang tidak valid. Pemeriksaan dihentikan. Coba lagi nanti atau buka halaman rilis resmi.",
  },
  'it': {
    'update_check_failed':
        "Ricerca aggiornamenti non riuscita. Riprova; se il problema persiste, scarica la versione più recente dalla pagina ufficiale delle versioni GitHub.",
    'update_check_network':
        "Impossibile contattare GitHub o richiesta scaduta. Controlla la rete e riprova.",
    'update_check_rate_limited':
        "GitHub ha limitato temporaneamente le richieste di aggiornamento. Riprova più tardi.",
    'update_check_unavailable':
        "Le informazioni sugli aggiornamenti GitHub non sono disponibili. Riprova più tardi o apri la pagina ufficiale delle versioni.",
    'update_check_response_too_large':
        "La risposta di aggiornamento supera il limite di dimensione sicuro. Cerca una nuova versione nella pagina ufficiale delle versioni GitHub.",
    'update_check_invalid_response':
        "GitHub ha restituito informazioni di aggiornamento non valide. Il controllo è stato interrotto. Riprova più tardi o apri la pagina ufficiale delle versioni.",
  },
  'pl': {
    'update_check_failed':
        "Sprawdzanie aktualizacji nie powiodło się. Spróbuj ponownie; jeśli problem się powtarza, pobierz najnowszą wersję z oficjalnej strony wydań GitHub.",
    'update_check_network':
        "Nie można połączyć się z GitHub lub upłynął limit czasu żądania. Sprawdź sieć i spróbuj ponownie.",
    'update_check_rate_limited':
        "GitHub tymczasowo ograniczył żądania aktualizacji. Spróbuj ponownie później.",
    'update_check_unavailable':
        "Informacje o aktualizacjach GitHub są niedostępne. Spróbuj później lub otwórz oficjalną stronę wydań.",
    'update_check_response_too_large':
        "Odpowiedź aktualizacji przekroczyła bezpieczny limit rozmiaru. Sprawdź nową wersję na oficjalnej stronie wydań GitHub.",
    'update_check_invalid_response':
        "GitHub zwrócił nieprawidłowe informacje o aktualizacji. Sprawdzanie zatrzymano. Spróbuj później lub otwórz oficjalną stronę wydań.",
  },
  'uk': {
    'update_check_failed':
        "Не вдалося перевірити оновлення. Спробуйте ще раз; якщо проблема зберігається, завантажте останню версію з офіційної сторінки релізів GitHub.",
    'update_check_network':
        "Не вдалося підключитися до GitHub або минув час очікування. Перевірте мережу та спробуйте ще раз.",
    'update_check_rate_limited':
        "GitHub тимчасово обмежив запити оновлень. Спробуйте пізніше.",
    'update_check_unavailable':
        "Інформація про оновлення GitHub недоступна. Спробуйте пізніше або відкрийте офіційну сторінку релізів.",
    'update_check_response_too_large':
        "Відповідь оновлення перевищила безпечний ліміт розміру. Перевірте нову версію на офіційній сторінці релізів GitHub.",
    'update_check_invalid_response':
        "GitHub повернув некоректну інформацію про оновлення. Перевірку зупинено. Спробуйте пізніше або відкрийте офіційну сторінку релізів.",
  },
  'vi': {
    'update_check_failed':
        "Không thể kiểm tra bản cập nhật. Hãy thử lại; nếu vẫn lỗi, tải phiên bản mới nhất từ trang phát hành chính thức trên GitHub.",
    'update_check_network':
        "Không thể kết nối với GitHub hoặc yêu cầu đã hết thời gian chờ. Kiểm tra mạng và thử lại.",
    'update_check_rate_limited':
        "GitHub tạm thời giới hạn yêu cầu cập nhật. Vui lòng thử lại sau.",
    'update_check_unavailable':
        "Không có thông tin cập nhật từ GitHub. Thử lại sau hoặc mở trang phát hành chính thức.",
    'update_check_response_too_large':
        "Phản hồi cập nhật vượt quá giới hạn kích thước an toàn. Kiểm tra phiên bản mới trên trang phát hành chính thức của GitHub.",
    'update_check_invalid_response':
        "GitHub trả về thông tin cập nhật không hợp lệ. Đã dừng kiểm tra. Thử lại sau hoặc mở trang phát hành chính thức.",
  },
  'th': {
    'update_check_failed':
        "ตรวจสอบการอัปเดตไม่สำเร็จ โปรดลองอีกครั้ง หากยังไม่ได้ ให้ดาวน์โหลดรุ่นล่าสุดจากหน้าเผยแพร่อย่างเป็นทางการบน GitHub",
    'update_check_network':
        "เชื่อมต่อ GitHub ไม่ได้หรือคำขอหมดเวลา โปรดตรวจสอบเครือข่ายแล้วลองอีกครั้ง",
    'update_check_rate_limited':
        "GitHub จำกัดคำขออัปเดตชั่วคราว โปรดลองอีกครั้งภายหลัง",
    'update_check_unavailable':
        "ข้อมูลอัปเดตจาก GitHub ไม่พร้อมใช้งาน โปรดลองภายหลังหรือเปิดหน้าเผยแพร่อย่างเป็นทางการ",
    'update_check_response_too_large':
        "การตอบกลับการอัปเดตเกินขีดจำกัดขนาดที่ปลอดภัย โปรดตรวจสอบรุ่นใหม่ที่หน้าเผยแพร่อย่างเป็นทางการบน GitHub",
    'update_check_invalid_response':
        "GitHub ส่งข้อมูลอัปเดตที่ไม่ถูกต้อง จึงหยุดการตรวจสอบ โปรดลองภายหลังหรือเปิดหน้าเผยแพร่อย่างเป็นทางการ",
  },
};
