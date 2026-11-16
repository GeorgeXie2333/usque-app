import Foundation

/// Remember the source independently of the displayed translation. AppKit can
/// also change a command (for example Enter/Exit Full Screen) between updates.
final class NativeMenuTitleRecord: NSObject {
  private var source: String
  private var displayed: String

  init(_ title: String) { source = title; displayed = title }

  func title(current: String, localization: NativeLocalization) -> String {
    if current != displayed { source = current }
    displayed = localization.menuTitle(source)
    return displayed
  }
}

/// App-owned native copy follows the same resolved catalog as Flutter. Reading
/// the preferred system languages is only a startup fallback; it writes no
/// language preference and never changes the language of system permissions.
struct NativeLocalization {
  let catalogID: String
  private let stringsBundle: Bundle?

  init(catalogID: String? = nil, preferredLanguages: [String] = Locale.preferredLanguages) {
    let id = Self.resolve(catalogID ?? preferredLanguages.first ?? "en")
    self.catalogID = id
    // Both Traditional Chinese regions use the same native menu terminology.
    let resource = id == "zh_CN" ? "zh-Hans" : (id.hasPrefix("zh_") ? "zh-Hant" : id)
    stringsBundle = Bundle.main.url(forResource: resource, withExtension: "lproj").flatMap(Bundle.init(url:))
  }

  private static let languages: Set<String> = [
    "en", "ja", "ko", "es", "pt", "fr", "nl", "tr", "ru", "fa", "ar", "de", "id", "it", "pl", "th", "uk", "vi",
  ]

  private static func resolve(_ identifier: String) -> String {
    guard identifier.utf8.count <= 128 else { return "en" }
    let parts = identifier.replacingOccurrences(of: "_", with: "-").lowercased().split(separator: "-")
    guard let code = parts.first else { return "en" }
    if code != "zh" {
      let language = code == "in" ? "id" : String(code)
      return languages.contains(language) ? language : "en"
    }
    if parts.contains("hk") || parts.contains("mo") { return "zh_HK" }
    if parts.contains("tw") || parts.contains("hant") { return "zh_TW" }
    return "zh_CN"
  }

  func text(_ source: String) -> String {
    stringsBundle?.localizedString(forKey: source, value: source, table: "UsqueNative") ?? source
  }

  func menuTitle(_ original: String) -> String {
    text(original.replacingOccurrences(of: "APP_NAME", with: "Usque"))
  }
}
