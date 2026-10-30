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

  init(catalogID: String? = nil, preferredLanguages: [String] = Locale.preferredLanguages) {
    self.catalogID = Self.resolve(catalogID ?? preferredLanguages.first ?? "en")
  }

  private static func resolve(_ identifier: String) -> String {
    guard identifier.utf8.count <= 128 else { return "en" }
    let parts = identifier.replacingOccurrences(of: "_", with: "-").lowercased().split(separator: "-")
    guard parts.first == "zh" else { return "en" }
    // Preserve the shared Flutter catalog's region-first Chinese policy.
    if parts.contains("hk") || parts.contains("mo") { return "zh_HK" }
    if parts.contains("tw") { return "zh_TW" }
    if parts.contains("hant") { return "zh_TW" }
    return "zh_CN"
  }

  func text(_ source: String) -> String {
    let values: [String: String]
    switch catalogID {
    case "zh_HK", "zh_TW": values = Self.traditional
    case "zh_CN": values = Self.simplified
    default: return source
    }
    return values[source] ?? source
  }

  func menuTitle(_ original: String) -> String {
    text(original.replacingOccurrences(of: "APP_NAME", with: "Usque"))
  }

  private static let traditional = [
    "Disconnected": "未連線",
    "Open Usque": "開啟 Usque",
    "Connect": "連線",
    "Disconnect": "中斷連線",
    "Disconnect and Quit": "中斷連線並結束",
    "Settings…": "設定…",
    "Preferences…": "設定…",
    "Usque could not confirm disconnection. Retry Disconnect before quitting.": "尚未確認中斷連線，請重試「中斷」後再結束。",
    "OK": "好",
    "Import configurations": "匯入設定檔",
    "Select OpenVPN or WireGuard configuration files.": "請選取 OpenVPN 或 WireGuard 設定檔。",
    "Import": "匯入",
    "Export diagnostics": "匯出診斷",
    "Export WARP credentials": "匯出 WARP 認證資料",
    "Save": "儲存",
    "Drag Usque to Applications and open that copy before enabling login startup.": "請將 Usque 拖入「應用程式」，開啟該副本後再啟用登入時啟動。",
    "Approve Usque in Login Items, then retry.": "請在系統設定的「登入項目」中允許 Usque，然後重試。",
    "macOS could not change the login item. Retry in System Settings.": "macOS 未能修改登入項目，請在系統設定中檢查後重試。",
    "Cloudflare login did not complete. Retry the login.": "Cloudflare 登入尚未完成，請重試登入。",
    "The desktop shell request is malformed.": "桌面操作請求的格式不正確。",
    "Enter one Cloudflare Zero Trust team name.": "請輸入一個 Cloudflare Zero Trust 團隊名稱。",
    "macOS could not start the authentication browser. Retry the login.": "macOS 未能開啟驗證瀏覽器，請重試登入。",
    "A file picker is already open.": "已有檔案選擇視窗開啟。",
    "Select at most 128 configuration files.": "一次最多可選取 128 個設定檔。",
    "The verification challenge expired.": "驗證要求已過期。",
    "The official verification window could not complete the challenge.": "官方驗證視窗未能完成驗證。",
    "The bundled official verification component is unavailable or could not be verified.": "內附的官方驗證元件無法使用，或未能確認其完整性。",
    "About Usque": "關於 Usque",
    "Services": "服務",
    "Hide Usque": "隱藏 Usque",
    "Hide Others": "隱藏其他",
    "Show All": "全部顯示",
    "Quit Usque": "結束 Usque",
    "Edit": "編輯",
    "Undo": "還原",
    "Redo": "重做",
    "Cut": "剪下",
    "Copy": "拷貝",
    "Paste": "貼上",
    "Paste and Match Style": "貼上並符合樣式",
    "Delete": "刪除",
    "Select All": "全選",
    "Find": "尋找",
    "Find…": "尋找…",
    "Find and Replace…": "尋找與取代…",
    "Find Next": "尋找下一個",
    "Find Previous": "尋找上一個",
    "Use Selection for Find": "使用所選範圍尋找",
    "Jump to Selection": "跳至所選範圍",
    "Spelling and Grammar": "拼字與文法",
    "Spelling": "拼字",
    "Show Spelling and Grammar": "顯示拼字與文法",
    "Check Document Now": "立即檢查文件",
    "Check Spelling While Typing": "輸入時檢查拼字",
    "Check Grammar With Spelling": "檢查拼字時也檢查文法",
    "Correct Spelling Automatically": "自動修正拼字",
    "Substitutions": "替代項目",
    "Show Substitutions": "顯示替代項目",
    "Smart Copy/Paste": "智慧型拷貝／貼上",
    "Smart Quotes": "智慧型引號",
    "Smart Dashes": "智慧型破折號",
    "Smart Links": "智慧型連結",
    "Data Detectors": "資料偵測器",
    "Text Replacement": "文字替換",
    "Transformations": "轉換",
    "Make Upper Case": "轉為大寫",
    "Make Lower Case": "轉為小寫",
    "Capitalize": "字首大寫",
    "Speech": "語音",
    "Start Speaking": "開始朗讀",
    "Stop Speaking": "停止朗讀",
    "View": "檢視",
    "Enter Full Screen": "進入全螢幕",
    "Exit Full Screen": "離開全螢幕",
    "Window": "視窗",
    "Minimize": "縮到最小",
    "Zoom": "縮放",
    "Bring All to Front": "全部移到最前方",
    "Help": "輔助說明",
  ]

  private static let simplified = [
    "Disconnected": "未连接",
    "Open Usque": "打开 Usque",
    "Connect": "连接",
    "Disconnect": "断开连接",
    "Disconnect and Quit": "断开连接并退出",
    "Settings…": "设置…",
    "Preferences…": "设置…",
    "Usque could not confirm disconnection. Retry Disconnect before quitting.": "尚未确认断开连接，请重试“断开”后再退出。",
    "OK": "好",
    "Import configurations": "导入配置文件",
    "Select OpenVPN or WireGuard configuration files.": "请选择 OpenVPN 或 WireGuard 配置文件。",
    "Import": "导入",
    "Export diagnostics": "导出诊断",
    "Export WARP credentials": "导出 WARP 凭据",
    "Save": "保存",
    "Drag Usque to Applications and open that copy before enabling login startup.": "请将 Usque 拖入“应用程序”，打开该副本后再启用登录时启动。",
    "Approve Usque in Login Items, then retry.": "请在系统设置的“登录项”中允许 Usque，然后重试。",
    "macOS could not change the login item. Retry in System Settings.": "macOS 未能修改登录项，请在系统设置中检查后重试。",
    "Cloudflare login did not complete. Retry the login.": "Cloudflare 登录尚未完成，请重试登录。",
    "The desktop shell request is malformed.": "桌面操作请求的格式不正确。",
    "Enter one Cloudflare Zero Trust team name.": "请输入一个 Cloudflare Zero Trust 团队名称。",
    "macOS could not start the authentication browser. Retry the login.": "macOS 未能打开验证浏览器，请重试登录。",
    "A file picker is already open.": "已有文件选择窗口打开。",
    "Select at most 128 configuration files.": "一次最多可选择 128 个配置文件。",
    "The verification challenge expired.": "验证请求已过期。",
    "The official verification window could not complete the challenge.": "官方验证窗口未能完成验证。",
    "The bundled official verification component is unavailable or could not be verified.": "内附的官方验证组件无法使用，或未能确认其完整性。",
    "About Usque": "关于 Usque",
    "Services": "服务",
    "Hide Usque": "隐藏 Usque",
    "Hide Others": "隐藏其他",
    "Show All": "全部显示",
    "Quit Usque": "退出 Usque",
    "Edit": "编辑",
    "Undo": "撤销",
    "Redo": "重做",
    "Cut": "剪切",
    "Copy": "拷贝",
    "Paste": "粘贴",
    "Paste and Match Style": "粘贴并匹配样式",
    "Delete": "删除",
    "Select All": "全选",
    "Find": "查找",
    "Find…": "查找…",
    "Find and Replace…": "查找并替换…",
    "Find Next": "查找下一个",
    "Find Previous": "查找上一个",
    "Use Selection for Find": "使用所选内容查找",
    "Jump to Selection": "跳到所选内容",
    "Spelling and Grammar": "拼写与语法",
    "Spelling": "拼写",
    "Show Spelling and Grammar": "显示拼写与语法",
    "Check Document Now": "立即检查文稿",
    "Check Spelling While Typing": "键入时检查拼写",
    "Check Grammar With Spelling": "检查拼写时检查语法",
    "Correct Spelling Automatically": "自动纠正拼写",
    "Substitutions": "替换",
    "Show Substitutions": "显示替换",
    "Smart Copy/Paste": "智能拷贝／粘贴",
    "Smart Quotes": "智能引号",
    "Smart Dashes": "智能破折号",
    "Smart Links": "智能链接",
    "Data Detectors": "数据检测器",
    "Text Replacement": "文本替换",
    "Transformations": "转换",
    "Make Upper Case": "转换为大写",
    "Make Lower Case": "转换为小写",
    "Capitalize": "首字母大写",
    "Speech": "语音",
    "Start Speaking": "开始朗读",
    "Stop Speaking": "停止朗读",
    "View": "显示",
    "Enter Full Screen": "进入全屏幕",
    "Exit Full Screen": "退出全屏幕",
    "Window": "窗口",
    "Minimize": "最小化",
    "Zoom": "缩放",
    "Bring All to Front": "前置全部窗口",
    "Help": "帮助",
  ]
}
