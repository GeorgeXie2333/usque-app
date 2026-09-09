//! Uninstall confirmation copy keyed by Windows UI locale.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UninstallCopy {
    pub title: &'static str,
    pub body: &'static str,
    pub delete_data: &'static str,
    pub warning: &'static str,
    pub uninstall: &'static str,
    pub cancel: &'static str,
}

pub const EN: UninstallCopy = UninstallCopy {
    title: "Uninstall Usque",
    body: "This will remove the Usque application and the Usque Agent service.",
    delete_data: "Delete profiles, settings, logs, caches, and WARP identities for this Windows user.",
    warning: "This cannot be undone. Leave this option unchecked to keep local data. Other Windows users are not affected.",
    uninstall: "Uninstall",
    cancel: "Cancel",
};

/// Resolve copy for a Windows locale name such as `zh-CN` or `ja-JP`.
pub fn copy_for_locale(name: &str) -> UninstallCopy {
    let normalized = name.replace('_', "-");
    let lower = normalized.to_ascii_lowercase();
    let (language, region) = match lower.split_once('-') {
        Some((language, rest)) => (language, rest.split('-').next().unwrap_or("")),
        None => (lower.as_str(), ""),
    };
    match (language, region) {
        ("zh", "hk" | "mo") => ZH_HK,
        ("zh", "tw") | ("zh", "hant") => ZH_TW,
        ("zh", _) => ZH_CN,
        ("ja", _) => JA,
        ("ko", _) => KO,
        ("de", _) => DE,
        ("es", _) => ES,
        ("fr", _) => FR,
        ("pt", _) => PT,
        ("nl", _) => NL,
        ("it", _) => IT,
        ("pl", _) => PL,
        ("ru", _) => RU,
        ("uk", _) => UK,
        ("tr", _) => TR,
        ("ar", _) => AR,
        ("fa", _) => FA,
        ("id" | "in", _) => ID,
        ("vi", _) => VI,
        ("th", _) => TH,
        _ => EN,
    }
}

const ZH_CN: UninstallCopy = UninstallCopy {
    title: "卸载 Usque",
    body: "这将移除 Usque 应用和 Usque Agent 服务。",
    delete_data: "删除此 Windows 用户的配置、设置、日志、缓存和 WARP 身份。",
    warning: "此操作无法撤销。取消勾选可保留本地数据。其他 Windows 用户不受影响。",
    uninstall: "卸载",
    cancel: "取消",
};

const ZH_HK: UninstallCopy = UninstallCopy {
    title: "解除安裝 Usque",
    body: "這會移除 Usque 應用程式及 Usque Agent 服務。",
    delete_data: "刪除此 Windows 使用者的設定檔、設定、記錄、快取及 WARP 身分。",
    warning: "此操作無法還原。取消勾選可保留本機資料。其他 Windows 使用者不受影響。",
    uninstall: "解除安裝",
    cancel: "取消",
};

const ZH_TW: UninstallCopy = UninstallCopy {
    title: "解除安裝 Usque",
    body: "這會移除 Usque 應用程式與 Usque Agent 服務。",
    delete_data: "刪除此 Windows 使用者的設定檔、設定、記錄、快取與 WARP 身分。",
    warning: "此操作無法復原。取消勾選可保留本機資料。其他 Windows 使用者不受影響。",
    uninstall: "解除安裝",
    cancel: "取消",
};

const JA: UninstallCopy = UninstallCopy {
    title: "Usque をアンインストール",
    body: "Usque アプリと Usque Agent サービスを削除します。",
    delete_data: "この Windows ユーザーのプロファイル、設定、ログ、キャッシュ、WARP 識別情報を削除します。",
    warning: "この操作は元に戻せません。オフのままにするとローカルデータは残ります。他の Windows ユーザーには影響しません。",
    uninstall: "アンインストール",
    cancel: "キャンセル",
};

const KO: UninstallCopy = UninstallCopy {
    title: "Usque 제거",
    body: "Usque 앱과 Usque Agent 서비스가 제거됩니다.",
    delete_data: "이 Windows 사용자의 프로필, 설정, 로그, 캐시, WARP 신원을 삭제합니다.",
    warning: "이 작업은 되돌릴 수 없습니다. 선택을 해제하면 로컬 데이터가 유지됩니다. 다른 Windows 사용자에게는 영향이 없습니다.",
    uninstall: "제거",
    cancel: "취소",
};

const DE: UninstallCopy = UninstallCopy {
    title: "Usque deinstallieren",
    body: "Dadurch werden die Usque-Anwendung und der Usque-Agent-Dienst entfernt.",
    delete_data: "Profile, Einstellungen, Protokolle, Caches und WARP-Identitäten dieses Windows-Benutzers löschen.",
    warning: "Dies kann nicht rückgängig gemacht werden. Deaktiviert lassen, um lokale Daten zu behalten. Andere Windows-Benutzer sind nicht betroffen.",
    uninstall: "Deinstallieren",
    cancel: "Abbrechen",
};

const ES: UninstallCopy = UninstallCopy {
    title: "Desinstalar Usque",
    body: "Esto quitará la aplicación Usque y el servicio Usque Agent.",
    delete_data: "Eliminar perfiles, ajustes, registros, cachés e identidades WARP de este usuario de Windows.",
    warning: "Esto no se puede deshacer. Deje la opción desactivada para conservar los datos locales. No afecta a otros usuarios de Windows.",
    uninstall: "Desinstalar",
    cancel: "Cancelar",
};

const FR: UninstallCopy = UninstallCopy {
    title: "Désinstaller Usque",
    body: "Cela supprimera l’application Usque et le service Usque Agent.",
    delete_data: "Supprimer les profils, paramètres, journaux, caches et identités WARP de cet utilisateur Windows.",
    warning: "Cette action est irréversible. Laissez l’option décochée pour conserver les données locales. Les autres utilisateurs Windows ne sont pas concernés.",
    uninstall: "Désinstaller",
    cancel: "Annuler",
};

const PT: UninstallCopy = UninstallCopy {
    title: "Desinstalar o Usque",
    body: "Isso removerá o aplicativo Usque e o serviço Usque Agent.",
    delete_data: "Excluir perfis, configurações, logs, caches e identidades WARP deste usuário do Windows.",
    warning: "Isso não pode ser desfeito. Deixe desmarcado para manter os dados locais. Outros usuários do Windows não são afetados.",
    uninstall: "Desinstalar",
    cancel: "Cancelar",
};

const NL: UninstallCopy = UninstallCopy {
    title: "Usque verwijderen",
    body: "Hiermee worden de Usque-toepassing en de Usque Agent-service verwijderd.",
    delete_data: "Profielen, instellingen, logboeken, caches en WARP-identiteiten van deze Windows-gebruiker verwijderen.",
    warning: "Dit kan niet ongedaan worden gemaakt. Laat de optie uit om lokale gegevens te behouden. Andere Windows-gebruikers worden niet beïnvloed.",
    uninstall: "Verwijderen",
    cancel: "Annuleren",
};

const IT: UninstallCopy = UninstallCopy {
    title: "Disinstalla Usque",
    body: "Questo rimuoverà l’applicazione Usque e il servizio Usque Agent.",
    delete_data: "Elimina profili, impostazioni, registri, cache e identità WARP di questo utente Windows.",
    warning: "L’operazione non può essere annullata. Lascia deselezionata l’opzione per conservare i dati locali. Gli altri utenti Windows non sono interessati.",
    uninstall: "Disinstalla",
    cancel: "Annulla",
};

const PL: UninstallCopy = UninstallCopy {
    title: "Odinstaluj Usque",
    body: "Spowoduje to usunięcie aplikacji Usque i usługi Usque Agent.",
    delete_data: "Usuń profile, ustawienia, dzienniki, pamięć podręczną i tożsamości WARP tego użytkownika Windows.",
    warning: "Tego nie można cofnąć. Pozostaw opcję wyłączoną, aby zachować dane lokalne. Inni użytkownicy Windows nie są objęci.",
    uninstall: "Odinstaluj",
    cancel: "Anuluj",
};

const RU: UninstallCopy = UninstallCopy {
    title: "Удалить Usque",
    body: "Будут удалены приложение Usque и служба Usque Agent.",
    delete_data: "Удалить профили, параметры, журналы, кэш и идентичности WARP этого пользователя Windows.",
    warning: "Это нельзя отменить. Оставьте параметр снятым, чтобы сохранить локальные данные. Другие пользователи Windows не затрагиваются.",
    uninstall: "Удалить",
    cancel: "Отмена",
};

const UK: UninstallCopy = UninstallCopy {
    title: "Вилучити Usque",
    body: "Буде вилучено програму Usque і службу Usque Agent.",
    delete_data: "Видалити профілі, параметри, журнали, кеш і ідентичності WARP цього користувача Windows.",
    warning: "Це неможливо скасувати. Залиште параметр знятим, щоб зберегти локальні дані. Інших користувачів Windows це не стосується.",
    uninstall: "Вилучити",
    cancel: "Скасувати",
};

const TR: UninstallCopy = UninstallCopy {
    title: "Usque’yu kaldır",
    body: "Bu, Usque uygulamasını ve Usque Agent hizmetini kaldırır.",
    delete_data: "Bu Windows kullanıcısının profillerini, ayarlarını, günlüklerini, önbelleklerini ve WARP kimliklerini sil.",
    warning: "Bu işlem geri alınamaz. Yerel verileri tutmak için seçeneği işaretlemeyin. Diğer Windows kullanıcıları etkilenmez.",
    uninstall: "Kaldır",
    cancel: "İptal",
};

const AR: UninstallCopy = UninstallCopy {
    title: "إزالة Usque",
    body: "سيؤدي هذا إلى إزالة تطبيق Usque وخدمة Usque Agent.",
    delete_data: "حذف الملفات الشخصية والإعدادات والسجلات وذاكرة التخزين المؤقت وهويات WARP لمستخدم Windows هذا.",
    warning: "لا يمكن التراجع عن هذا. اترك الخيار دون تحديد للاحتفاظ بالبيانات المحلية. لا يتأثر مستخدمو Windows الآخرون.",
    uninstall: "إزالة",
    cancel: "إلغاء",
};

const FA: UninstallCopy = UninstallCopy {
    title: "حذف Usque",
    body: "با این کار برنامهٔ Usque و خدمت Usque Agent حذف می‌شوند.",
    delete_data: "نمایه‌ها، تنظیمات، گزارش‌ها، حافظهٔ نهان و هویت‌های WARP این کاربر ویندوز را حذف کن.",
    warning: "این کار برگشت‌پذیر نیست. برای نگه داشتن داده‌های محلی گزینه را خالی بگذارید. سایر کاربران ویندوز تحت تأثیر نیستند.",
    uninstall: "حذف",
    cancel: "لغو",
};

const ID: UninstallCopy = UninstallCopy {
    title: "Copot Usque",
    body: "Ini akan menghapus aplikasi Usque dan layanan Usque Agent.",
    delete_data: "Hapus profil, pengaturan, log, cache, dan identitas WARP pengguna Windows ini.",
    warning: "Ini tidak dapat dibatalkan. Biarkan opsi tidak dicentang untuk menyimpan data lokal. Pengguna Windows lain tidak terpengaruh.",
    uninstall: "Copot",
    cancel: "Batal",
};

const VI: UninstallCopy = UninstallCopy {
    title: "Gỡ Usque",
    body: "Thao tác này sẽ gỡ ứng dụng Usque và dịch vụ Usque Agent.",
    delete_data: "Xóa hồ sơ, cài đặt, nhật ký, bộ nhớ đệm và danh tính WARP của người dùng Windows này.",
    warning: "Không hoàn tác được. Để trống tùy chọn này nếu muốn giữ dữ liệu cục bộ. Người dùng Windows khác không bị ảnh hưởng.",
    uninstall: "Gỡ cài đặt",
    cancel: "Hủy",
};

const TH: UninstallCopy = UninstallCopy {
    title: "ถอนการติดตั้ง Usque",
    body: "การดำเนินการนี้จะเอาแอป Usque และบริการ Usque Agent ออก",
    delete_data: "ลบโปรไฟล์ การตั้งค่า บันทึก แคช และข้อมูลประจำตัว WARP ของผู้ใช้ Windows นี้",
    warning: "ย้อนกลับไม่ได้ ปล่อยตัวเลือกนี้ว่างไว้เพื่อเก็บข้อมูลในเครื่อง ผู้ใช้ Windows คนอื่นไม่ได้รับผลกระทบ",
    uninstall: "ถอนการติดตั้ง",
    cancel: "ยกเลิก",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chinese_regions_map_to_distinct_copy() {
        assert_eq!(copy_for_locale("zh-CN").uninstall, "卸载");
        assert_eq!(copy_for_locale("zh-HK").uninstall, "解除安裝");
        assert!(copy_for_locale("zh-TW").body.contains("應用程式"));
        assert_ne!(copy_for_locale("ja-JP").title, EN.title);
        assert_eq!(copy_for_locale("en-US"), EN);
        assert_eq!(copy_for_locale("in-ID").cancel, "Batal");
    }
}
