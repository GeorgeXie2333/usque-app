import '../iso_countries.dart';

import 'countries/ar.dart';
import 'countries/de.dart';
import 'countries/es.dart';
import 'countries/fa.dart';
import 'countries/fr.dart';
import 'countries/id.dart';
import 'countries/it.dart';
import 'countries/ja.dart';
import 'countries/ko.dart';
import 'countries/nl.dart';
import 'countries/pl.dart';
import 'countries/pt.dart';
import 'countries/ru.dart';
import 'countries/th.dart';
import 'countries/tr.dart';
import 'countries/uk.dart';
import 'countries/vi.dart';
import 'countries/zh_cn.dart';
import 'countries/zh_hk.dart';
import 'countries/zh_tw.dart';

/// Display names only; routing and filtering continue to use ISO codes.
final kCountryCatalogs = <String, Map<String, String>>{
  'en': {for (final country in kIsoCountries) country.code: country.name},
  'ar': kCountriesAr,
  'de': kCountriesDe,
  'es': kCountriesEs,
  'fa': kCountriesFa,
  'fr': kCountriesFr,
  'id': kCountriesId,
  'it': kCountriesIt,
  'ja': kCountriesJa,
  'ko': kCountriesKo,
  'nl': kCountriesNl,
  'pl': kCountriesPl,
  'pt': kCountriesPt,
  'ru': kCountriesRu,
  'th': kCountriesTh,
  'tr': kCountriesTr,
  'uk': kCountriesUk,
  'vi': kCountriesVi,
  'zh_CN': kCountriesZhCn,
  'zh_HK': kCountriesZhHk,
  'zh_TW': kCountriesZhTw,
};
