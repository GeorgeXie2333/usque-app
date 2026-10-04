#include "state.h"
#include "options_result.h"
#include "process_output.h"

#include <cstdio>

int main(int argc, char** argv) {
  using namespace usque::setup;
  constexpr char expected_output[] = R"({"schema":1,"status":"partial","desktopShortcut":{"status":"created","enabled":true},"startOnLogin":{"status":"conflict","enabled":null}})";
  if (argc == 2 && std::string_view(argv[1]) == "--emit-result") {
    Sleep(10);  // Exercise the write-during-parent-wait boundary.
    DWORD count = 0;
    WriteFile(GetStdHandle(STD_OUTPUT_HANDLE), expected_output, sizeof(expected_output) - 1, &count, nullptr);
    return 1;
  }
  int failures = 0;
  const auto check = [&failures](bool condition, const char* label) {
    if (!condition) { std::fprintf(stderr, "FAILED: %s\n", label); ++failures; }
  };
  check(SelectMode({}) == Mode::install, "fresh install");
  check(SelectMode({true, false, true, true, false}) == Mode::maintenance,
        "exact ProductCode maintenance");
  check(SelectMode({false, true, false, true, false}) == Mode::upgrade,
        "same-version different ProductCode replacement");
  check(SelectMode({false, false, true, false, false}) == Mode::residual,
        "residual bundle cleanup");
  check(SelectMode({true, true, true, true, true}) == Mode::blocked, "downgrade blocked");
  check(!CanInstall(Mode::install, false, true), "license is required");
  check(!CanInstall(Mode::maintenance, true, true), "maintenance never repairs");
  check(CanInstall(Mode::upgrade, true, true), "upgrade accepted");
  check(TerminalExitCode(Page::failed, 1603, true) == 1603, "failure survives reboot requirement");
  check(TerminalExitCode(Page::failed, 1223, false) == 1223, "UAC decline result is preserved");
  check(TerminalExitCode(Page::complete, 0, true) == 3010, "restart later returns reboot required");
  check(TerminalExitCode(Page::options_failed, 5, false) == 0, "optional failure does not undo successful install");
  check(!CanRetryFailure(false, true), "pending reboot blocks retry after failure");
  check(ChooseCloseAction(Page::ready, CancelStage::before_start, false, false) == CloseAction::close,
        "before-start window close is allowed");
  check(ChooseCloseAction(Page::working, CancelStage::running, false, false) == CloseAction::request_cancel,
        "supported running phase can request cancellation");
  for (auto stage : {CancelStage::rollback, CancelStage::recovery, CancelStage::deleting_data, CancelStage::registration_cleanup}) {
    check(ChooseCloseAction(Page::working, stage, false, false) == CloseAction::wait,
          "critical phase blocks button and window-close cancellation");
    check(ChooseCloseAction(Page::files_in_use, stage, false, false) == CloseAction::wait,
          "critical-phase file prompt cannot cancel cleanup");
  }
  check(ActionCancelStage(L"InstallFiles", ActionCancelStage(L"PurgeUserData", CancelStage::running)) == CancelStage::deleting_data,
        "irreversible boundary remains closed until transaction ends");
  check(ActionCancelStage(L"Rollback", CancelStage::running) == CancelStage::rollback, "MSI rollback disables cancellation");
  check(ChooseCloseAction(Page::working, CancelStage::running, true, false) == CloseAction::wait, "cancel request is not repeated");
  check(!CommonDataCancelPermission(true, L"2", L"0"), "native MSI disable record closes cancellation gate");
  check(CommonDataCancelPermission(false, L"2", L"1"), "native MSI enable record opens ordinary phase");
  check(!CommonDataCancelPermission(true, L"2", L""), "malformed native permission fails closed");
  check(ChooseCloseAction(Page::working, CancelStage::running, false, false, false) == CloseAction::wait,
        "window close obeys native MSI permission");
  check(!CanReturnCancellation(CancelStage::running, false, true), "queued cancellation suppressed after native withdrawal");
  check(!CanReturnCancellation(CancelStage::rollback, true, true), "queued cancellation suppressed during rollback");
  check(!CanReturnCancellation(CancelStage::registration_cleanup, true, true), "queued cancellation suppressed during cleanup");
  check(CanReturnCancellation(CancelStage::running, true, true), "queued request returned only while both gates permit");
  const auto sanitized = SanitizedDetails(L"C:\\Users\\Alice\\secret", L"token\r\nvalue", 0x80070643U);
  check(sanitized.find("Alice") == std::string::npos && sanitized.find("token") == std::string::npos &&
        sanitized.find("result=0x80070643") != std::string::npos, "saved details reject paths and untrusted text");
  check(ValidInstallFolder(L"C:\\Program Files\\Usque"), "valid folder");
  for (auto path : {L"C:\\", L"relative", L"\\\\server\\share", L"C:\\Usque:stream",
                    L"C:\\Usque\\..\\Windows", L"C:\\Usque. ", L"C:\\Usque\""}) {
    check(!ValidInstallFolder(path), "invalid folder");
  }
  FinishState finish;
  check(!finish.desktop && finish.launch && !finish.startup, "finish defaults");
  finish.desktop = true;
  finish.startup = true;
  finish.desktop_done = true;
  check(!finish.NeedDesktop() && finish.NeedStartup(), "retry only failed option");
  finish.startup_done = true;
  check(!finish.OptionsPending(), "successful options not repeated");
  finish.reboot = true;
  check(!finish.NeedLaunch(), "reboot blocks app launch");
  finish.desktop_done = false;
  check(finish.OptionsPending(), "reboot preserves optional desktop/startup work");
  check(ResolveRebootFinish(false, false) == RebootFinishAction::exit_later, "restart later never restarts Windows");
  check(ResolveRebootFinish(true, false) == RebootFinishAction::restart, "explicit final action authorizes restart");
  check(ResolveRebootFinish(true, true) == RebootFinishAction::confirm_again, "retry after optional failure requires fresh restart confirmation");
  check(LaunchAfterSkip(true, false, false), "explicit skip honors selected launch");
  check(!LaunchAfterSkip(false, false, false), "explicit skip honors unchecked launch");
  check(!LaunchAfterSkip(true, true, false), "explicit skip never retries a failed app launch");
  check(!LaunchAfterSkip(true, false, true), "explicit skip never launches before required reboot");
  const auto partial = ResultParser(R"({"schema":1,"status":"partial","desktopShortcut":{"status":"created","enabled":true},"startOnLogin":{"status":"conflict","enabled":null}})").Parse();
  check(partial.valid && partial.desktop.Success() && !partial.startup.Success(), "partial independent result");
  const auto reordered = ResultParser(R"({ "startOnLogin": {"enabled":true,"status":"enabled"},"status":"ok","desktopShortcut":{"status":"absent","enabled":false},"schema":1 })").Parse();
  check(reordered.valid && reordered.startup.enabled == true, "query whitespace and object order");
  check(!ResultParser(R"({"schema":1,"schema":1})").Parse().valid, "duplicate JSON keys rejected");
  check(!ResultParser(R"({"schema":2,"status":"ok","desktopShortcut":{"status":"created","enabled":true},"startOnLogin":{"status":"enabled","enabled":true}})").Parse().valid, "unknown schema rejected");
  check(!ResultParser(R"({"schema":1,"status":"ok","desktopShortcut":{"status":"created","enabled":true},"startOnLogin":{"status":"enabled","enabled":true}}garbage)").Parse().valid, "trailing output rejected");
  for (int attempt = 0; attempt < 12; ++attempt) {
    SECURITY_ATTRIBUTES attributes{sizeof(attributes), nullptr, TRUE};
    HANDLE read_pipe = nullptr, write_pipe = nullptr;
    check(CreatePipe(&read_pipe, &write_pipe, &attributes, 0) != FALSE, "create inert output pipe");
    SetHandleInformation(read_pipe, HANDLE_FLAG_INHERIT, 0);
    wchar_t executable[32768]{};
    GetModuleFileNameW(nullptr, executable, 32768);
    std::wstring command = L"\"" + std::wstring(executable) + L"\" --emit-result";
    STARTUPINFOW startup{sizeof(startup)};
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdOutput = write_pipe; startup.hStdError = write_pipe;
    PROCESS_INFORMATION process{};
    const bool created = CreateProcessW(executable, command.data(), nullptr, nullptr, TRUE,
      CREATE_NO_WINDOW, nullptr, nullptr, &startup, &process) != FALSE;
    check(created, "start inert child");
    CloseHandle(write_pipe);
    if (created) {
      const auto actual = ReadChildOutput(process.hProcess, read_pipe);
      check(actual.code == 1 && actual.output == expected_output, "drain final JSON after child exit");
      CloseHandle(process.hThread); CloseHandle(process.hProcess);
    }
    CloseHandle(read_pipe);
  }
  std::printf("BOOTSTRAPPER_STATE_TESTS=%s\n", failures == 0 ? "PASS" : "FAIL");
  return failures == 0 ? 0 : 1;
}
