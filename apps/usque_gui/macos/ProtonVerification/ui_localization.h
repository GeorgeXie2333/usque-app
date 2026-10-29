#ifndef USQUE_PROTON_VERIFICATION_UI_LOCALIZATION_H_
#define USQUE_PROTON_VERIFICATION_UI_LOCALIZATION_H_

#include <cstddef>
#include <string_view>

namespace usque::proton::macos {

struct VerificationUi {
  const char* window_title;
  const char* cef_locale;
};

inline VerificationUi UiForCatalog(const char* label) {
  constexpr VerificationUi english{"Proton verification", "en-US"};
  if (!label) return english;
  // The longest allowed label is five bytes. Read at most six bytes before
  // rejecting it; never copy or scan an arbitrary environment value.
  std::size_t length = 0;
  while (length < 6 && label[length] != '\0') ++length;
  if (length > 5) return english;
  const std::string_view catalog(label, length);
  if (catalog == "zh_CN") return {"Proton 验证", "zh-CN"};
  if (catalog == "zh_HK" || catalog == "zh_TW") return {"Proton 驗證", "zh-TW"};
  return english;
}

}  // namespace usque::proton::macos

#endif  // USQUE_PROTON_VERIFICATION_UI_LOCALIZATION_H_
