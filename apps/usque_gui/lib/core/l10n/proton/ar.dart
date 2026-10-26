const kProtonAr = <String, String>{
  'fallback_account':
      'تعذر الوصول إلى Proton. اختر حساب WARP موجودًا وأعد المحاولة.',
  'fallback_failed':
      'تعذر الاتصال عبر WARP. تحقق من حساب WARP المحدد والشبكة، ثم أعد المحاولة.',
  'fallback_starting': 'جارٍ الاتصال عبر WARP…',
  'fallback_active': 'تجري عملية الحساب هذه عبر WARP، بما فيها التحقق.',
  'connection_api':
      'فشل طلب إلى واجهة API في Proton. أعد المحاولة أو راجع حساب Proton في الوكيل المتسلسل.',
  'connection_authentication':
      'فشل تفويض Proton. افتح الوكيل المتسلسل وسجّل الدخول إلى Proton مجددًا.',
  'connection_certificate':
      'رفض Proton شهادة الاتصال وفشلت الاستعادة. أعد الاتصال أو سجّل الدخول إلى Proton مجددًا.',
  'connection_restricted':
      'خادم Proton مقيّد مؤقتًا. أعد المحاولة أو اختر خادمًا آخر.',
  'connection_sessions':
      'تم بلوغ حد اتصالات Proton المتزامنة. افصل جلسة Proton أخرى وأعد المحاولة.',
  'connection_key':
      'اكتشف Proton مفتاح الاتصال هذا في عدة جلسات. افصل الجلسة الأخرى وأعد المحاولة.',
  'connection_policy':
      'قيّد Proton هذا الاتصال وفق سياسة الحساب أو الاستخدام. تحقق من الحساب أو اختر خادمًا آخر.',
  'connection_server': 'تعذر على خادم Proton تفويض الاتصال. جرّب خادمًا آخر.',
  'connection_protocol':
      'تعذر التحقق من صحة استجابة اتصال Proton. أعد المحاولة أو اختر خادمًا آخر.',
  'connection_transport':
      'تعذر الوصول إلى Proton. تحقق من الشبكة أو اختر خادمًا آخر.',
  'connection_reconnect': 'تم تجديد تفويض Proton. جارٍ إعادة الاتصال…',
  'connection_review':
      'افتح الوكيل المتسلسل لمراجعة حساب Proton والخادم المحدد.',
  'connection_retry': 'أعد محاولة الاتصال بـ Proton أو اختر خادمًا آخر.',
  'title': 'Proton VPN',
  'guest': 'المتابعة كضيف',
  'login': 'تسجيل الدخول إلى Proton',
  'username': 'اسم المستخدم',
  'password': 'كلمة المرور',
  'totp': 'رمز تطبيق المصادقة',
  'submit': 'متابعة',
  'verify': 'إكمال التحقق',
  'verification_hint': 'أكمل اللغز في نافذة التحقق الرسمية.',
  'choose_node': 'اختيار خادم مجاني',
  'working': 'جارٍ إعداد Proton VPN…',
  'waiting_totp': 'أدخل رمز تطبيق المصادقة للمتابعة.',
  'waiting_verification': 'التحقق مطلوب',
  'waiting_node': 'اختر خادمًا لإكمال الإعداد.',
  'completed': 'تم حفظ الحساب. حدده وطبّق التغييرات لاستخدام Proton VPN.',
  'cancelled': 'تم إلغاء الإعداد',
  'failed': 'فشل إعداد Proton. أعد المحاولة عندما تكون مستعدًا.',
  'result_unconfirmed':
      'لم تُؤكّد نتيجة إعداد Proton. حدّث الحالة للمتابعة أو ألغِ الإعداد الحالي قبل البدء مجددًا.',
  'unavailable': 'Proton VPN غير متاح في هذا المحرك.',
  'bad_credentials':
      'تحقق من اسم مستخدم Proton أو كلمة المرور أو رمز المصادقة.',
  'rate_limited': 'قيّد Proton هذا الطلب. انتظر قبل إعادة المحاولة.',
  'network': 'تعذر الوصول إلى Proton. تحقق من الشبكة أو VPN، ثم أعد المحاولة.',
  'verification_failed': 'تعذر إكمال التحقق. ابدأ الإعداد مجددًا.',
  'verification_invalid':
      'رفض Proton نتيجة التحقق. ابدأ الإعداد مجددًا للحصول على تحدٍّ جديد.',
  'accounts': 'حسابات Proton',
  'add_account': 'إضافة حساب',
  'account_actions': 'إجراءات الحساب',
  'resolve_on_connect': 'يُفحص الخادم والتفويض عند الاتصال.',
  'account': 'حساب Proton',
  'signed_in': 'تم تسجيل الدخول',
  'session_requires_login': 'تتطلب الجلسة تسجيل الدخول',
  'selected_server': 'الخادم',
  'automatic_server': 'تلقائي',
  'automatic_hint': 'اختيار الخادم الأقل زمن استجابة عند الاتصال.',
  'available_servers': 'الخوادم المتاحة',
  'choose_server': 'اختيار خادم',
  'search_servers': 'البحث بالاسم أو البلد أو المدينة',
  'load_servers': 'تحميل قائمة الخوادم',
  'change_server': 'تغيير الخادم',
  'refresh_servers': 'تحديث القائمة',
  'remove_account': 'إزالة الحساب',
  'remove_account_confirm':
      'هل تريد إزالة هذا الحساب واختيار الخادم المحفوظ؟ يؤدي تسجيل الخروج أيضًا إلى إبطال الجلسات التي حفظها هذا التطبيق. الإزالة محليًا فقط لا تغيّر الجلسات البعيدة. سيُمسح اختيار الوكيل المتسلسل المحفوظ.',
  'forget_local': 'الإزالة محليًا فقط',
  'sign_out_remove': 'تسجيل الخروج والإزالة',
  'current_account': 'الحساب المتصل',
  'saved_account': 'الاختيار المحفوظ',
  'next_connection': 'تم حفظ اختيار الخادم للاتصال التالي.',
  'disconnect_to_change': 'افصل هذا الحساب قبل تغيير خادمه أو إزالته.',
  'cleanup_pending':
      'تعذر تأكيد إبطال جلسة سابقة. سجّل الخروج مجددًا أو اختر صراحةً إزالة الحساب محليًا فقط.',
  'selected_server_unavailable':
      'الخادم المحدد غير متاح. اختر خادمًا آخر أو الاختيار التلقائي.',
  'close': 'إغلاق',
  'verification_error': 'خطأ في التحقق',
  'refresh': 'تجديد التفويض أو تغيير الخادم',
  'logout': 'تسجيل الخروج والحذف',
  'forget': 'الإزالة من هذا الجهاز فقط',
  'forget_confirm':
      'هل تريد إزالة إعداد Proton هذا وجلسته المحفوظة من هذا الجهاز؟ لن يؤدي ذلك إلى إبطال جلسة Proton البعيدة.',
  'removed': 'تمت إزالة حساب Proton من هذا الجهاز.',
  'needs_login': 'تسجيل الدخول مجددًا',
  'expires': 'انتهاء التفويض',
  'unsupported_second_factor':
      'يتطلب هذا الحساب طريقة تسجيل دخول غير مدعومة هنا.',
  'reauthentication_required': 'سجّل الدخول مجددًا لتجديد جلسة Proton هذه.',
  'profile_in_use': 'افصل هذا الحساب قبل تغييره أو إزالته.',
  'session_busy':
      'لا تزال عملية Proton أخرى قيد التنفيذ. انتظر أو ألغها أولًا.',
  'api_rejected': 'رفض Proton هذا الطلب. تحدد الرموز أدناه استجابة الخادم.',
  'session_not_allowed': 'لا يسمح Proton لهذا العميل بإنشاء جلسة.',
  'credentialless_disabled': 'وصول الضيوف إلى Proton غير متاح حاليًا.',
  'no_free_nodes': 'لم يُرجع Proton أي خوادم WireGuard متاحة لهذا الحساب.',
  'invalid_directory': 'أرجع Proton قائمة خوادم غير مدعومة.',
  'invalid_client_configuration':
      'لم يُرجع Proton أي منافذ WireGuard قابلة للاستخدام.',
  'stage': 'الخطوة التي فشلت',
  'stage_anonymous_session': 'إنشاء جلسة مجهولة الهوية',
  'stage_credentialless': 'إنشاء حساب ضيف',
  'stage_session_scopes': 'فحص تفويض الضيف',
  'stage_protected_user': 'فحص الحساب',
  'stage_vpn_permissions': 'فحص صلاحية الوصول إلى VPN',
  'stage_session_refresh': 'تجديد الجلسة',
  'stage_srp_info': 'بدء تسجيل الدخول',
  'stage_srp_proof': 'التحقق من تسجيل الدخول',
  'stage_totp': 'التحقق من رمز المصادقة',
  'stage_directory': 'تحميل الخوادم',
  'stage_certificate': 'طلب تفويض VPN',
  'stage_secure_storage': 'حفظ الإعداد',
  'stage_session_revoke': 'إبطال الجلسة',
  'expired': 'انتهت صلاحية طلب التحقق أو الإدخال. ابدأ الإعداد مجددًا.',
  'underlay_changed': 'تغيّرت بيئة الشبكة. ابدأ الإعداد مجددًا.',
  'remote_unknown': 'تعذر تأكيد إبطال الجلسة البعيدة.',
  'cancelled_remote_unknown':
      'تم الإلغاء على هذا الجهاز. تعذر تأكيد إبطال الجلسة البعيدة.',
  'privacy':
      'تُستخدم كلمة المرور لتسجيل الدخول فقط. الجلسات والمفاتيح محمية على هذا الجهاز.',
  'empty': 'سجّل الدخول أو تابع كضيف لإضافة حساب Proton.',
};
