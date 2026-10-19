#ifndef USQUE_WINDOWS_PROTON_VERIFICATION_H_
#define USQUE_WINDOWS_PROTON_VERIFICATION_H_
#include <flutter/encodable_value.h>
#include <flutter/method_result.h>
#include <windows.h>
#include <memory>
#include <vector>
class ProtonVerification {
 public:
  explicit ProtonVerification(HWND parent);
  ~ProtonVerification();
  void Begin(const std::vector<uint8_t>& handoff,
      std::unique_ptr<flutter::MethodResult<flutter::EncodableValue>> result);
  void Cancel();
 private:
  struct Session;
  HWND parent_;
  std::shared_ptr<Session> current_;
};
#endif
