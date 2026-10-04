#pragma once

#include <string>
#include <string_view>

namespace usque::setup {

enum class Mode { install, upgrade, maintenance, residual, blocked };
enum class Page { detecting, ready, license, working, files_in_use, complete, failed, options_failed };
enum class CancelStage { before_start, running, rollback, recovery, deleting_data, registration_cleanup };
enum class CloseAction { close, request_cancel, wait };
enum class RebootFinishAction { exit_later, restart, confirm_again };

constexpr RebootFinishAction ResolveRebootFinish(bool requested_now, bool retry_after_failure) {
  if (retry_after_failure) return RebootFinishAction::confirm_again;
  return requested_now ? RebootFinishAction::restart : RebootFinishAction::exit_later;
}

constexpr bool LaunchAfterSkip(bool selected, bool already_failed, bool reboot) {
  return selected && !already_failed && !reboot;
}

constexpr bool CanRequestCancellation(CancelStage stage) {
  return stage == CancelStage::before_start || stage == CancelStage::running;
}

constexpr bool CanReturnCancellation(CancelStage stage, bool native_permission, bool requested) {
  return requested && native_permission && CanRequestCancellation(stage);
}

inline bool CommonDataCancelPermission(bool current, std::wstring_view kind, std::wstring_view value) {
  return kind == L"2" ? value == L"1" : current;
}

constexpr CloseAction ChooseCloseAction(Page page, CancelStage stage, bool requested, bool options_busy,
                                       bool native_permission = true) {
  if (options_busy) return CloseAction::wait;
  if (page == Page::working || page == Page::detecting || page == Page::files_in_use)
    return requested || !native_permission || !CanRequestCancellation(stage) ? CloseAction::wait : CloseAction::request_cancel;
  return CloseAction::close;
}

inline CancelStage ActionCancelStage(std::wstring_view action, CancelStage current) {
  if (current == CancelStage::rollback || action == L"Rollback") return CancelStage::rollback;
  if (action == L"EmergencyRemoveKillSwitch" || action == L"RecoverAgentState" || action == L"RemoveExistingProducts")
    return CancelStage::recovery;
  if (action == L"PurgeUserData") return CancelStage::deleting_data;
  if (action == L"RemoveUserStartupRegistration" || action == L"FinalizeAgentUninstall" || action == L"UnregisterProduct")
    return CancelStage::registration_cleanup;
  return current;  // The boundary remains closed until the transaction ends.
}

inline std::string DiagnosticToken(std::wstring_view value) {
  if (value.empty() || value.size() > 64) return "unknown";
  std::string result;
  for (wchar_t ch : value) {
    if (!((ch >= L'a' && ch <= L'z') || (ch >= L'A' && ch <= L'Z') ||
          (ch >= L'0' && ch <= L'9') || ch == L'.' || ch == L'_' || ch == L'-' || ch == L'+')) return "unknown";
    result += static_cast<char>(ch);
  }
  return result;
}

inline std::string SanitizedDetails(std::wstring_view version, std::wstring_view stage, unsigned result) {
  constexpr char digits[] = "0123456789ABCDEF";
  std::string hex(8, '0');
  for (int index = 7; index >= 0; --index) { hex[static_cast<size_t>(index)] = digits[result & 15]; result >>= 4; }
  return "product=Usque\r\nversion=" + DiagnosticToken(version) + "\r\nstage=" + DiagnosticToken(stage) +
    "\r\nresult=0x" + hex + "\r\n";
}

struct Detection {
  bool exact_product = false;
  bool related_product = false;
  bool bundle_registered = false;
  bool installed_product = false;
  bool newer_product = false;
};

// Product identity, not display-version equality, determines maintenance.
constexpr Mode SelectMode(const Detection& value) {
  if (value.newer_product) return Mode::blocked;
  if (value.exact_product) return Mode::maintenance;
  if (value.related_product || value.installed_product) return Mode::upgrade;
  if (value.bundle_registered) return Mode::residual;
  return Mode::install;
}

constexpr bool CanInstall(Mode mode, bool accepted, bool valid_path) {
  return (mode == Mode::install || mode == Mode::upgrade) && accepted && valid_path;
}

constexpr bool CanRetryFailure(bool terminal, bool reboot) { return !terminal && !reboot; }

constexpr unsigned TerminalExitCode(Page page, unsigned failure_code, bool reboot) {
  // Failure remains a failure even when Windows also needs a restart.
  if (page == Page::failed) return failure_code;
  if (page == Page::complete || page == Page::options_failed) return reboot ? 3010U : 0U;
  return 1602U;  // A user leaves before the install transaction completes.
}

inline bool ValidInstallFolder(std::wstring_view path) {
  // Reject relative, UNC/device paths, drive roots, command delimiters and ADS.
  if (path.size() < 4 || path.size() > 240 || path[1] != L':' ||
      path[2] != L'\\' || !((path[0] >= L'A' && path[0] <= L'Z') ||
                            (path[0] >= L'a' && path[0] <= L'z'))) return false;
  if (path.find_first_of(L"\"<>|?*\r\n") != std::wstring_view::npos ||
      path.find(L':', 2) != std::wstring_view::npos) return false;
  size_t start = 3;
  while (start < path.size()) {
    const auto end = path.find(L'\\', start);
    const auto part = path.substr(start, end == std::wstring_view::npos ? end : end - start);
    if (part.empty() || part == L"." || part == L".." ||
        part.back() == L' ' || part.back() == L'.') return false;
    if (end == std::wstring_view::npos) break;
    start = end + 1;
  }
  return true;
}

struct FinishState {
  bool desktop = false;
  bool startup = false;
  bool initial_startup = false;
  bool launch = true;
  bool desktop_done = false;
  bool startup_done = false;
  bool launch_done = false;
  bool reboot = false;

  bool NeedDesktop() const { return desktop && !desktop_done; }
  bool NeedStartup() const { return startup != initial_startup && !startup_done; }
  bool NeedLaunch() const { return launch && !launch_done && !reboot; }
  bool OptionsPending() const { return NeedDesktop() || NeedStartup(); }
};

}  // namespace usque::setup
