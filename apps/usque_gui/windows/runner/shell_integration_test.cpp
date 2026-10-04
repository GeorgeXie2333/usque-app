#include "shell_integration_internal.h"

#include <windows.h>
#include <shlobj.h>
#include <wrl/client.h>

#include <cstdio>
#include <string>
#include <thread>
#include <vector>

namespace {

using namespace usque::shell;
int failures = 0;

void Expect(bool condition, const char* name) {
  if (condition) return;
  std::fprintf(stderr, "FAIL shellIntegration.%s\n", name);
  ++failures;
}

class FakePlatform final : public Platform {
 public:
  bool allowed = true;
  int context_reads = 0;
  int reads = 0;
  int writes = 0;
  int desktop_removals = 0;
  int startup_changes = 0;
  bool enable_requested = false;
  bool create_fails = false;
  bool remove_fails = false;
  bool startup_fails = false;
  std::wstring executable = L"C:\\Program Files\\Usque\\usque.exe";
  EntryState desktop = EntryState::kAbsent;
  EntryState startup = EntryState::kAbsent;
  std::vector<std::wstring> targets;

  bool IsOriginalUserContext() override { ++context_reads; return allowed; }
  std::wstring ExecutablePath() override { ++reads; return executable; }
  EntryState DesktopState(const std::wstring& target) override {
    ++reads; targets.push_back(target); return desktop;
  }
  EntryState StartupState(const std::wstring& target) override {
    ++reads; targets.push_back(target); return startup;
  }
  ItemResult CreateDesktopLink(const std::wstring& target) override {
    ++writes;
    targets.push_back(target);
    if (create_fails) return {ItemStatus::kError, std::nullopt};
    desktop = EntryState::kOwned;
    return {ItemStatus::kCreated, true};
  }
  ItemResult RemoveDesktopLink(const std::wstring& target) override {
    ++writes;
    ++desktop_removals;
    targets.push_back(target);
    if (remove_fails) return {ItemStatus::kError, std::nullopt};
    if (desktop == EntryState::kForeign) return {ItemStatus::kKept, std::nullopt};
    desktop = EntryState::kAbsent;
    return {ItemStatus::kRemoved, false};
  }
  ItemResult SetStartup(const std::wstring& target, bool enabled) override {
    ++writes;
    ++startup_changes;
    targets.push_back(target);
    enable_requested = enabled;
    if (startup_fails) return {ItemStatus::kError, std::nullopt};
    startup = enabled ? EntryState::kOwned : EntryState::kAbsent;
    return {enabled ? ItemStatus::kEnabled : ItemStatus::kDisabled, enabled};
  }
};

void ParsesOnlyFixedInternalCommands() {
  Expect(ParseCommand({}).mode == CommandMode::kNone, "emptyStartsNormalApp");
  Expect(ParseCommand({"--background"}).mode == CommandMode::kNone,
         "backgroundStartsNormalApp");
  Expect(ParseCommand({"--query-setup-options"}).mode == CommandMode::kQuery,
         "query");
  Expect(ParseCommand({"--remove-startup"}).mode == CommandMode::kRemove,
         "legacyCleanup");
  for (const auto* desktop : {"create", "keep"}) {
    for (const auto* startup : {"enable", "disable", "keep"}) {
      const auto command = ParseCommand({std::string("--start-on-login=") + startup,
          "--setup-options", std::string("--desktop-shortcut=") + desktop});
      Expect(command.mode == CommandMode::kApply, "allFixedChoices");
      Expect((command.desktop == DesktopChoice::kCreate) ==
             (std::string(desktop) == "create"), "desktopChoiceParsed");
      Expect((command.startup == StartupChoice::kEnable) ==
             (std::string(startup) == "enable"), "startupChoiceParsed");
    }
  }
  const std::vector<std::vector<std::string>> invalid = {
      {"--setup-options"}, {"--desktop-shortcut=create"},
      {"--start-on-login=enable"}, {"--setup-options=anything"},
      {"--query-setup-options", "--background"},
      {"--remove-startup", "--setup-options"},
      {"--setup-options", "--desktop-shortcut=create", "--desktop-shortcut=keep"},
      {"--setup-options", "--desktop-shortcut=remove", "--start-on-login=enable"},
      {"--setup-options", "--desktop-shortcut=create", "--start-on-login=yes"},
      {"--setup-options", "--desktop-shortcut=create", "--start-on-login=enable",
       "--background"},
      {"--setup-options", "--desktop-shortcut=create", "--start-on-login=enable "},
  };
  for (const auto& arguments : invalid) {
    Expect(ParseCommand(arguments).mode == CommandMode::kInvalid,
           "malformedNeverStartsApp");
  }
}

void PathsAndOwnershipAreStrict() {
  const std::wstring exe = L"C:\\Program Files\\Usque\\usque.exe";
  Expect(SameExecutable(exe, L"c:/program files/usque/USQUE.EXE"),
         "caseAndSeparatorNormalization");
  Expect(SameExecutable(exe, L"C:\\Program Files\\Usque\\temp\\..\\usque.exe"),
         "parentSegmentNormalization");
  Expect(!SameExecutable(exe, L"C:\\Program Files\\Other\\usque.exe"),
         "differentInstallNotOwned");
  Expect(!SameExecutable(exe, L"usque.exe"), "relativePathNotOwned");
  Expect(!NormalizeAbsolutePath(L"C:usque.exe"), "driveRelativeRejected");
  Expect(!NormalizeAbsolutePath(L"C:\\..\\usque.exe"), "rootEscapeRejected");
  Expect(!NormalizeAbsolutePath(L"C:\\Usque\\usque.exe:stream"), "adsRejected");
  Expect(!NormalizeAbsolutePath(L"\\\\?\\C:\\Usque\\usque.exe"), "devicePathRejected");
  Expect(!NormalizeAbsolutePath(L"C:\\Usque\\usque.exe "), "ambiguousSuffixRejected");
  Expect(!NormalizeAbsolutePath(std::wstring(L"C:\\Usque\0\\usque.exe", 19)),
         "embeddedNullRejected");
  Expect(SameExecutable(L"\\\\server\\share\\Usque\\usque.exe",
                        L"\\\\SERVER\\share\\usque\\usque.exe"), "uncPath");
  Expect(StartupCommandOwned(L"\"" + exe + L"\" --background", exe),
         "exactStartupOwned");
  Expect(!StartupCommandOwned(L"\"" + exe + L"\" --background --connect", exe),
         "extraStartupArgumentsRejected");
  Expect(!StartupCommandOwned(L"\"" + exe + L"\"", exe),
         "unrelatedCommandNotOwned");
  Expect(ShortcutTargetsExecutable(exe, L"", exe), "sameTargetRecognized");
  Expect(!ShortcutTargetsExecutable(exe, L"--background", exe),
         "desktopArgumentsNotOwned");
  Expect(ShortcutOwned(exe, L"", L"io.github.georgexie2333.usque", exe),
         "productAndTargetOwned");
  Expect(!ShortcutOwned(exe, L"", L"", exe), "unmarkedLinkNotDeleted");
  Expect(!ShortcutOwned(exe, L"", L"other.product", exe), "otherProductNotDeleted");
}

void CommandsUseCurrentUserBoundaryAndIndependentResults() {
  FakePlatform platform;
  platform.allowed = false;
  for (const auto mode : {CommandMode::kApply, CommandMode::kQuery}) {
    const auto result = ExecuteCommand({mode}, platform);
    Expect(result.exit_code == 3 && result.status == "user_context_required",
           "rejectsAdministratorProxy");
  }
  Expect(platform.reads == 0 && platform.writes == 0, "deniedContextHasNoSideEffects");
  platform.allowed = true;
  auto result = ExecuteCommand({CommandMode::kQuery}, platform);
  Expect(result.exit_code == 0 && result.startup.enabled == false &&
         platform.writes == 0, "queryNeverWrites");
  platform.desktop = platform.startup = EntryState::kOwned;
  result = ExecuteCommand({CommandMode::kApply}, platform);
  Expect(result.desktop.status == ItemStatus::kKept && result.startup.enabled == true &&
         platform.writes == 0, "keepPreservesUpgradePreferences");
  platform.create_fails = true;
  result = ExecuteCommand({CommandMode::kApply, DesktopChoice::kCreate,
                           StartupChoice::kEnable}, platform);
  Expect(result.exit_code == 1 && result.desktop.status == ItemStatus::kError &&
         result.startup.status == ItemStatus::kEnabled && platform.enable_requested,
         "startupStillRunsAfterDesktopFailure");
  platform.create_fails = false;
  platform.startup_fails = true;
  result = ExecuteCommand({CommandMode::kApply, DesktopChoice::kCreate,
                           StartupChoice::kDisable}, platform);
  Expect(result.exit_code == 1 && result.desktop.status == ItemStatus::kCreated &&
         result.startup.status == ItemStatus::kError && !platform.enable_requested,
         "desktopSuccessSurvivesStartupFailure");
  platform.startup_fails = false;
  platform.allowed = false;
  const int previous_context_reads = platform.context_reads;
  result = ExecuteCommand({CommandMode::kRemove}, platform);
  Expect(result.exit_code == 0 && !platform.enable_requested &&
         platform.context_reads == previous_context_reads, "legacyMsiCleanupCompatible");
  platform.allowed = true;
  platform.executable = L"usque.exe";
  const int previous_writes = platform.writes;
  result = ExecuteCommand({CommandMode::kApply, DesktopChoice::kCreate,
                           StartupChoice::kEnable}, platform);
  Expect(result.exit_code == 1 && platform.writes == previous_writes,
         "invalidExecutableNeverWrites");
  result = ExecuteCommand({CommandMode::kInvalid}, platform);
  Expect(result.exit_code == 2 && platform.writes == previous_writes,
         "invalidArgumentsNeverWrite");
  Expect(ResultJson(result) ==
      "{\"schema\":1,\"status\":\"invalid_arguments\",\"desktopShortcut\":"
      "{\"status\":\"not_requested\",\"enabled\":null},\"startOnLogin\":"
      "{\"status\":\"not_requested\",\"enabled\":null}}\n", "boundedJsonContract");
  result.status = "private path or token";
  Expect(ResultJson(result).find("private") == std::string::npos,
         "jsonRejectsArbitraryDetails");
}

void CleanupKeepsDesktopFailuresNonFatal() {
  for (const bool desktop_fails : {false, true}) {
    for (const bool startup_fails : {false, true}) {
      FakePlatform platform;
      platform.allowed = false;  // Legacy MSI cleanup keeps impersonation.
      platform.desktop = platform.startup = EntryState::kOwned;
      platform.remove_fails = desktop_fails;
      platform.startup_fails = startup_fails;
      const auto result = ExecuteCommand({CommandMode::kRemove}, platform);
      Expect(result.exit_code == (startup_fails ? 1 : 0),
             "cleanupExitDependsOnlyOnStartupFailure");
      Expect(result.status == (desktop_fails || startup_fails ? "partial" : "ok") &&
             result.desktop.status == (desktop_fails ? ItemStatus::kError : ItemStatus::kRemoved) &&
             result.startup.status == (startup_fails ? ItemStatus::kError : ItemStatus::kDisabled),
             "cleanupRetainsIndependentItemResults");
      Expect(platform.desktop_removals == 1 && platform.startup_changes == 1 &&
             !platform.enable_requested && platform.context_reads == 0,
             "cleanupAttemptsBothItemsInMsiUserContext");
      Expect(platform.startup == (startup_fails ? EntryState::kOwned : EntryState::kAbsent),
             "startupCleanupRunsDespiteDesktopFailure");
    }
  }
  FakePlatform foreign;
  foreign.desktop = EntryState::kForeign;
  const auto kept = ExecuteCommand({CommandMode::kRemove}, foreign);
  Expect(kept.exit_code == 0 && kept.desktop.status == ItemStatus::kKept &&
         foreign.desktop == EntryState::kForeign, "cleanupPreservesForeignDesktopItem");
}

void ComFailureStillRemovesStartup() {
  for (const bool startup_fails : {false, true}) {
    FakePlatform platform;
    platform.allowed = false;
    platform.desktop = platform.startup = EntryState::kOwned;
    platform.startup_fails = startup_fails;
    const auto result = detail::ExecuteCommandWithComState(
        {CommandMode::kRemove}, platform, false);
    Expect(result.exit_code == (startup_fails ? 1 : 0) && result.status == "partial" &&
           result.desktop.status == ItemStatus::kError &&
           result.startup.status == (startup_fails ? ItemStatus::kError : ItemStatus::kDisabled),
           "comFailurePreservesStartupResult");
    Expect(platform.desktop_removals == 0 && platform.startup_changes == 1 &&
           platform.writes == 1 && !platform.enable_requested && platform.context_reads == 0 &&
           platform.desktop == EntryState::kOwned,
           "comFailureSkipsDesktopButStillClearsStartup");
  }
  for (const auto mode : {CommandMode::kQuery, CommandMode::kApply}) {
    FakePlatform platform;
    const auto result = detail::ExecuteCommandWithComState(
        {mode, DesktopChoice::kCreate, StartupChoice::kEnable}, platform, false);
    Expect(result.exit_code == 1 && result.status == "partial" &&
           result.desktop.status == ItemStatus::kError && result.startup.status == ItemStatus::kError &&
           platform.context_reads == 0 && platform.reads == 0 && platform.writes == 0,
           "comFailureStillStopsSetupCommands");
  }
  FakePlatform available;
  const auto result = detail::ExecuteCommandWithComState(
      {CommandMode::kRemove}, available, true);
  Expect(result.exit_code == 0 && available.desktop_removals == 1 &&
         available.startup_changes == 1, "availableComRunsBothCleanupItems");
}

void InstallerOptionsRestrictModeIdentityAndTarget() {
  for (const auto mode : {CommandMode::kNone, CommandMode::kInvalid,
                          CommandMode::kRemove}) {
    FakePlatform platform;
    const auto result = ExecuteInstallerOptions({mode}, platform);
    Expect(result.exit_code == 2 && result.status == "invalid_arguments" &&
           platform.context_reads == 0 && platform.reads == 0 && platform.writes == 0,
           "installerCannotRemoveOrUseOtherModes");
  }
  for (const auto* path : {L"", L"usque.exe", L"C:usque.exe",
                          L"C:\\Usque\\usque-setup.exe", L"C:\\Usque\\usque.exe.old",
                          L"C:\\Usque\\usque.exe:stream", L"C:\\..\\usque.exe",
                          L"\\\\?\\C:\\Usque\\usque.exe"}) {
    FakePlatform platform;
    platform.executable = path;
    const auto result = ExecuteInstallerOptions(
        {CommandMode::kApply, DesktopChoice::kCreate, StartupChoice::kEnable}, platform);
    Expect(result.exit_code == 2 && platform.writes == 0 && platform.targets.empty(),
           "installerRejectsMalformedOrWrongExecutableTarget");
  }
  FakePlatform denied;
  denied.allowed = false;
  const auto rejected = ExecuteInstallerOptions(
      {CommandMode::kApply, DesktopChoice::kCreate, StartupChoice::kEnable}, denied);
  Expect(rejected.exit_code == 3 && denied.reads == 0 && denied.writes == 0,
         "installerRejectsAdministratorProxyBeforeSettingsAccess");

  FakePlatform query;
  query.executable = L"C:/Program Files/Usque/./USQUE.EXE";
  query.desktop = query.startup = EntryState::kOwned;
  const std::wstring normalized = L"C:\\Program Files\\Usque\\USQUE.EXE";
  const auto current = ExecuteInstallerOptions(
      {CommandMode::kQuery, DesktopChoice::kCreate, StartupChoice::kDisable}, query);
  Expect(current.exit_code == 0 && current.startup.enabled == true &&
         current.desktop.enabled == true && query.writes == 0 &&
         query.targets == std::vector<std::wstring>{normalized, normalized},
         "installerQueriesInstalledTargetWithoutLaunchingOrMutatingIt");
  query.targets.clear();
  const auto preserved = ExecuteInstallerOptions({CommandMode::kApply}, query);
  Expect(preserved.exit_code == 0 && query.writes == 0 &&
         preserved.startup.status == ItemStatus::kKept && preserved.startup.enabled == true,
         "installerKeepsExistingUpgradeStartupChoice");
  query.targets.clear();
  const auto applied = ExecuteInstallerOptions(
      {CommandMode::kApply, DesktopChoice::kCreate, StartupChoice::kDisable}, query);
  Expect(applied.exit_code == 0 && query.writes == 2 &&
         applied.desktop.status == ItemStatus::kCreated && applied.startup.enabled == false &&
         query.targets == std::vector<std::wstring>{normalized, normalized},
         "installerAppliesOnlyToNormalizedInstalledTarget");
}

void InstallerOptionsBalanceCallingThreadCom() {
  bool rejected_and_balanced = false;
  bool existing_apartment_kept = false;
  bool incompatible_apartment_kept = false;
  // Dedicated threads have known COM state. Every request below fails before
  // token, Desktop, or registry access; these are not real setup operations.
  std::thread fresh([&] {
    const auto result = ExecuteInstallerOptions(
        {CommandMode::kRemove}, std::wstring(L"C:\\Usque\\usque.exe"));
    const HRESULT probe = ::CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    rejected_and_balanced = result.exit_code == 2 && SUCCEEDED(probe);
    if (SUCCEEDED(probe)) ::CoUninitialize();
  });
  fresh.join();
  std::thread existing([&] {
    const HRESULT initial = ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    const auto result = ExecuteInstallerOptions(
        {CommandMode::kRemove}, std::wstring(L"C:\\Usque\\usque.exe"));
    const HRESULT probe = ::CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    existing_apartment_kept = SUCCEEDED(initial) && result.exit_code == 2 &&
                              probe == RPC_E_CHANGED_MODE;
    if (SUCCEEDED(probe)) ::CoUninitialize();
    if (SUCCEEDED(initial)) ::CoUninitialize();
  });
  existing.join();
  std::thread incompatible([&] {
    const HRESULT initial = ::CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    const auto result = ExecuteInstallerOptions(
        {CommandMode::kQuery}, std::wstring(L"C:\\Usque\\usque.exe"));
    const HRESULT probe = ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
    incompatible_apartment_kept = SUCCEEDED(initial) && result.exit_code == 1 &&
                                  probe == RPC_E_CHANGED_MODE;
    if (SUCCEEDED(probe)) ::CoUninitialize();
    if (SUCCEEDED(initial)) ::CoUninitialize();
  });
  incompatible.join();
  Expect(rejected_and_balanced, "installerBalancesComOnFailure");
  Expect(existing_apartment_kept, "installerPreservesCallersStaReference");
  Expect(incompatible_apartment_kept, "installerPreservesIncompatibleApartment");
}

bool WriteBytes(const std::wstring& path, const std::string& bytes) {
  const HANDLE file = ::CreateFileW(path.c_str(), GENERIC_WRITE, 0, nullptr,
      CREATE_NEW, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return false;
  DWORD count = 0;
  const bool saved = ::WriteFile(file, bytes.data(), static_cast<DWORD>(bytes.size()),
                                 &count, nullptr) && count == bytes.size();
  ::CloseHandle(file);
  return saved;
}

std::vector<BYTE> ReadBytes(const std::wstring& path) {
  const HANDLE file = ::CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ,
      nullptr, OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) return {};
  const DWORD size = ::GetFileSize(file, nullptr);
  std::vector<BYTE> bytes(size == INVALID_FILE_SIZE ? 0 : size);
  DWORD read = 0;
  if (bytes.empty() || !::ReadFile(file, bytes.data(), static_cast<DWORD>(bytes.size()),
                                  &read, nullptr) || read != bytes.size()) bytes.clear();
  ::CloseHandle(file);
  return bytes;
}

bool WriteUnmarkedShortcut(const std::wstring& path, const std::wstring& target) {
  Microsoft::WRL::ComPtr<IShellLinkW> link;
  Microsoft::WRL::ComPtr<IPersistFile> persistent;
  return SUCCEEDED(::CoCreateInstance(CLSID_ShellLink, nullptr, CLSCTX_INPROC_SERVER,
       IID_PPV_ARGS(&link))) && SUCCEEDED(link->SetPath(target.c_str())) &&
       SUCCEEDED(link.As(&persistent)) && SUCCEEDED(persistent->Save(path.c_str(), TRUE));
}

void NativeLinksStayInsideTemporaryDirectory() {
  wchar_t root[MAX_PATH]{};
  wchar_t generated[MAX_PATH]{};
  const DWORD count = ::GetTempPathW(MAX_PATH, root);
  if (count == 0 || count >= MAX_PATH ||
      !::GetTempFileNameW(root, L"usq", 0, generated)) {
    Expect(false, "temporaryDirectoryAvailable");
    return;
  }
  const std::wstring directory(generated);
  // The only deleted paths are this GetTempFileName-created fixture and its
  // explicitly named children. No known-folder Desktop or Run key is used.
  if (!::DeleteFileW(directory.c_str()) ||
      !::CreateDirectoryW(directory.c_str(), nullptr)) {
    Expect(false, "temporaryDirectoryCreated");
    return;
  }
  const std::wstring executable = directory + L"\\usque.exe";
  const std::wstring owned = directory + L"\\Usque.lnk";
  const std::wstring unmarked = directory + L"\\Unmarked.lnk";
  const std::wstring foreign = directory + L"\\Foreign.lnk";
  const std::wstring corrupt = directory + L"\\Corrupt.lnk";
  const std::wstring directory_link = directory + L"\\Directory.lnk";
  Expect(WriteBytes(executable, "inert fixture; never executed"), "inertExecutableFixture");
  Expect(InspectShortcut(owned, executable) == EntryState::kAbsent, "missingLink");
  Expect(CreateShortcut(owned, executable).status == ItemStatus::kCreated, "createsMarkedLink");
  Expect(InspectShortcut(owned, executable) == EntryState::kOwned, "readsMarkedLink");
  const auto original_bytes = ReadBytes(owned);
  Expect(!original_bytes.empty(), "shortcutBytesWritten");
  Expect(CreateShortcut(owned, executable).status == ItemStatus::kUnchanged &&
         ReadBytes(owned) == original_bytes, "createIsByteIdenticalWhenRepeated");
  Expect(RemoveShortcut(owned, directory + L"\\other.exe").status == ItemStatus::kKept &&
         ReadBytes(owned) == original_bytes, "differentInstallPreserved");
  const HANDLE locked = ::CreateFileW(owned.c_str(), GENERIC_READ, 0, nullptr,
      OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
  Expect(locked != INVALID_HANDLE_VALUE, "fixtureLocked");
  Expect(RemoveShortcut(owned, executable).status == ItemStatus::kError,
         "lockedLinkFailureReported");
  if (locked != INVALID_HANDLE_VALUE) ::CloseHandle(locked);
  Expect(RemoveShortcut(owned, executable).status == ItemStatus::kRemoved &&
         ::GetFileAttributesW(owned.c_str()) == INVALID_FILE_ATTRIBUTES,
         "removesOnlyMarkedMatchingLink");
  Expect(RemoveShortcut(owned, executable).status == ItemStatus::kAbsent,
         "removeIsIdempotent");
  Expect(WriteUnmarkedShortcut(unmarked, executable), "unmarkedFixture");
  const auto unmarked_bytes = ReadBytes(unmarked);
  Expect(CreateShortcut(unmarked, executable).status == ItemStatus::kUnchanged &&
         ReadBytes(unmarked) == unmarked_bytes, "sameTargetNotRewrittenOrAdopted");
  Expect(RemoveShortcut(unmarked, executable).status == ItemStatus::kKept &&
         ReadBytes(unmarked) == unmarked_bytes, "unmarkedSameTargetPreserved");
  Expect(WriteUnmarkedShortcut(foreign, L"C:\\Windows\\notepad.exe"), "foreignFixture");
  const auto foreign_bytes = ReadBytes(foreign);
  Expect(CreateShortcut(foreign, executable).status == ItemStatus::kConflict &&
         ReadBytes(foreign) == foreign_bytes, "foreignSameNameNeverOverwritten");
  Expect(RemoveShortcut(foreign, executable).status == ItemStatus::kKept &&
         ReadBytes(foreign) == foreign_bytes, "foreignSameNameNeverRemoved");
  Expect(WriteBytes(corrupt, "not a shortcut"), "corruptFixture");
  Expect(CreateShortcut(corrupt, executable).status == ItemStatus::kConflict,
         "corruptFileNeverOverwritten");
  Expect(::CreateDirectoryW(directory_link.c_str(), nullptr) != FALSE,
         "directoryFixture");
  Expect(CreateShortcut(directory_link, executable).status == ItemStatus::kError &&
         RemoveShortcut(directory_link, executable).status == ItemStatus::kError,
         "directoryNeverChanged");
  Expect(CreateShortcut(directory + L"\\missing\\Usque.lnk", executable).status ==
         ItemStatus::kError, "missingDesktopDoesNotCreateDirectories");
  for (const auto& path : {owned, unmarked, foreign, corrupt, executable}) {
    if (::GetFileAttributesW(path.c_str()) != INVALID_FILE_ATTRIBUTES) {
      Expect(::DeleteFileW(path.c_str()) != FALSE, "fixtureFileCleanup");
    }
  }
  Expect(::RemoveDirectoryW(directory_link.c_str()) != FALSE, "fixtureSubdirectoryCleanup");
  Expect(::RemoveDirectoryW(directory.c_str()) != FALSE, "fixtureDirectoryCleanup");
}

}  // namespace

int RunShellIntegrationTests() {
  failures = 0;
  ParsesOnlyFixedInternalCommands();
  PathsAndOwnershipAreStrict();
  CommandsUseCurrentUserBoundaryAndIndependentResults();
  CleanupKeepsDesktopFailuresNonFatal();
  ComFailureStillRemovesStartup();
  InstallerOptionsRestrictModeIdentityAndTarget();
  InstallerOptionsBalanceCallingThreadCom();
  const HRESULT com = ::CoInitializeEx(nullptr, COINIT_APARTMENTTHREADED);
  Expect(SUCCEEDED(com), "comAvailable");
  if (SUCCEEDED(com)) {
    NativeLinksStayInsideTemporaryDirectory();
    ::CoUninitialize();
  }
  return failures;
}

#ifdef USQUE_SHELL_INTEGRATION_TEST_MAIN
int main() {
  const int count = RunShellIntegrationTests();
  if (count != 0) return 1;
  std::printf("shell_integration_test: ok\n");
  return 0;
}
#endif
