#ifndef RUNNER_SHELL_INTEGRATION_H_
#define RUNNER_SHELL_INTEGRATION_H_

#include <optional>
#include <string>
#include <vector>

namespace usque::shell {

enum class CommandMode { kNone, kInvalid, kQuery, kApply, kRemove };
enum class DesktopChoice { kKeep, kCreate };
enum class StartupChoice { kKeep, kEnable, kDisable };

struct Command {
  CommandMode mode = CommandMode::kNone;
  DesktopChoice desktop = DesktopChoice::kKeep;
  StartupChoice startup = StartupChoice::kKeep;
};

enum class EntryState { kAbsent, kOwned, kSameTarget, kForeign, kError };
enum class ItemStatus {
  kNotRequested, kCreated, kPresent, kAbsent, kEnabled, kDisabled,
  kUnchanged, kKept, kRemoved, kConflict, kError
};

struct ItemResult {
  ItemStatus status = ItemStatus::kNotRequested;
  std::optional<bool> enabled;
};

struct CommandResult {
  int exit_code = 0;
  std::string status = "ok";
  ItemResult desktop;
  ItemResult startup;
};

// The platform boundary permits deterministic tests without a real Desktop,
// Run registry entry, user token, Flutter window, or engine process.
class Platform {
 public:
  virtual ~Platform() = default;
  virtual bool IsOriginalUserContext() = 0;
  virtual std::wstring ExecutablePath() = 0;
  virtual EntryState DesktopState(const std::wstring& executable) = 0;
  virtual EntryState StartupState(const std::wstring& executable) = 0;
  virtual ItemResult CreateDesktopLink(const std::wstring& executable) = 0;
  virtual ItemResult RemoveDesktopLink(const std::wstring& executable) = 0;
  virtual ItemResult SetStartup(const std::wstring& executable,
                                bool enabled) = 0;
};

Command ParseCommand(const std::vector<std::string>& arguments);
CommandResult ExecuteCommand(const Command& command, Platform& platform);
std::string ResultJson(const CommandResult& result);

// In-process installer entry point. Only query/apply are accepted. The caller
// supplies its trusted MSI installation target; this function normalizes the
// absolute path, requires the usque.exe filename, and checks the original-user
// context before touching current-user settings. It never launches that file,
// removes shell entries, initializes Flutter, or starts an engine. COM is
// initialized and balanced on the calling thread, including on failure.
CommandResult ExecuteInstallerOptions(const Command& command,
                                     const std::wstring& installed_usque_path);

// Deterministic platform seam for the same installer policy. ExecutablePath()
// supplies the proposed installed target; this overload does not initialize
// COM or call Win32 unless the supplied platform does so.
CommandResult ExecuteInstallerOptions(const Command& command, Platform& platform);

// Pure path and ownership checks never resolve a link, search for a moved
// target, or depend on the process working directory.
std::optional<std::wstring> NormalizeAbsolutePath(const std::wstring& path);
bool SameExecutable(const std::wstring& left, const std::wstring& right);
bool StartupCommandOwned(const std::wstring& command,
                         const std::wstring& executable);
bool ShortcutTargetsExecutable(const std::wstring& target,
                                const std::wstring& arguments,
                                const std::wstring& executable);
bool ShortcutOwned(const std::wstring& target, const std::wstring& arguments,
                   const std::wstring& app_id, const std::wstring& executable);

// These operate on an explicitly supplied .lnk path. Production only passes
// the current user's known Desktop; tests only pass paths in a temporary dir.
// The caller must initialize COM before calling these three functions.
EntryState InspectShortcut(const std::wstring& path,
                           const std::wstring& executable);
ItemResult CreateShortcut(const std::wstring& path,
                          const std::wstring& executable);
ItemResult RemoveShortcut(const std::wstring& path,
                          const std::wstring& executable);

bool IsStartOnLoginEnabled();
bool SetStartOnLogin(bool enabled);

// Returns nullopt for normal app arguments. Internal commands are handled
// before any Flutter construction, single-instance forwarding, or IPC.
std::optional<int> HandleCommandLine(const std::vector<std::string>& arguments);

}  // namespace usque::shell

#endif  // RUNNER_SHELL_INTEGRATION_H_
