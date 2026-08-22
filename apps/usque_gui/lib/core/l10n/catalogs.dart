import 'en.dart';
import 'es.dart';
import 'fa.dart';
import 'fr.dart';
import 'ja.dart';
import 'ko.dart';
import 'nl.dart';
import 'pt.dart';
import 'ru.dart';
import 'tr.dart';
import 'zh_cn.dart';
import 'zh_hk.dart';
import 'zh_tw.dart';

export 'en.dart';
export 'es.dart';
export 'fa.dart';
export 'fr.dart';
export 'ja.dart';
export 'ko.dart';
export 'nl.dart';
export 'pt.dart';
export 'ru.dart';
export 'tr.dart';
export 'zh_cn.dart';
export 'zh_hk.dart';
export 'zh_tw.dart';

/// Catalog id → string table. Ids match [AppStrings.resolveCatalogId].
const Map<String, Map<String, String>> kCatalogs =
    <String, Map<String, String>>{
      'en': kEnCatalog,
      'zh_CN': kZhCnCatalog,
      'zh_HK': kZhHkCatalog,
      'zh_TW': kZhTwCatalog,
      'ja': kJaCatalog,
      'ko': kKoCatalog,
      'es': kEsCatalog,
      'pt': kPtCatalog,
      'fr': kFrCatalog,
      'nl': kNlCatalog,
      'tr': kTrCatalog,
      'ru': kRuCatalog,
      'fa': kFaCatalog,
    };

const List<String> kPlaceholderTokens = <String>[
  '{count}',
  '{current}',
  '{total}',
];
