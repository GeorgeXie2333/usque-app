// Copyright 2026. Standalone test harness for the attributed Flutter excerpts.
// Standalone fixture only; this run does not integrate or validate an application.
// The excerpts retain their BSD-style license in LICENSE.flutter.
#include <atomic>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <memory>
#include <stdexcept>
#include <string>
#include <vector>

#include "candidate_timer.h"
#include "original_timer.h"

using Clock = std::chrono::high_resolution_clock;
using Time = Clock::time_point;
using namespace std::chrono_literals;

void CaseStarted(const char* name) {
  std::cout << "{\"event\":\"case_started\",\"name\":\"" << name << "\"}"
            << std::endl;
}

void Require(bool value, const char* message) {
  if (!value) throw std::runtime_error(message);
}

struct State {
  std::mutex mutex;
  std::condition_variable cv;
  std::vector<Time> times;
  bool release = false;
  bool barrier_timeout = false;
  void Callback(bool gate_first) {
    std::unique_lock<std::mutex> lock(mutex);
    if (times.size() >= 8)
      std::_Exit(91);  // Hard callback bound, not a spin test.
    times.push_back(Clock::now());
    cv.notify_all();
    if (gate_first && times.size() == 1 &&
        !cv.wait_for(lock, 3s, [this] { return release; })) {
      barrier_timeout = true;
    }
  }
  void WaitFor(size_t count) {
    std::unique_lock<std::mutex> lock(mutex);
    Require(cv.wait_for(lock, 3s, [&] { return times.size() >= count; }),
            "bounded callback wait expired");
  }
  void Release() {
    std::lock_guard<std::mutex> lock(mutex);
    release = true;
    cv.notify_all();
  }
  std::vector<Time> Snapshot() {
    std::lock_guard<std::mutex> lock(mutex);
    Require(!barrier_timeout, "controlled callback barrier timed out");
    return times;
  }
};

template <class Timer>
struct OwnedTimer {
  State& state;
  Timer timer;
  bool stopped = false;
  explicit OwnedTimer(State& s, bool gate = true)
      : state(s), timer([&s, gate] { s.Callback(gate); }) {}
  void Stop() {
    if (!stopped) {
      state.Release();
      timer.Stop();
      stopped = true;
    }
  }
  ~OwnedTimer() { Stop(); }
};

template <class Timer>
void WaitParked(Timer& timer) {
  const auto limit = Clock::now() + 2s;
  while (!timer.FixtureIsParked()) {
    Require(Clock::now() < limit, "timer did not reach parked state");
    std::this_thread::sleep_for(1ms);
  }
}

template <class Timer>
void WaitEntered(Timer& timer) {
  const auto limit = Clock::now() + 2s;
  while (timer.FixtureWaitEntries() == 0) {
    Require(Clock::now() < limit, "timer never entered actual condition wait");
    std::this_thread::sleep_for(1ms);
  }
}

template <class Timer>
void EarlierNotification(const char* implementation, bool expect_late) {
  CaseStarted("earlier_future_notification");
  State state;
  OwnedTimer<Timer> owner(state, false);
  const auto late = Clock::now() + 1200ms;
  owner.timer.ScheduleAt(late);
  owner.timer.Start();
  WaitEntered(owner.timer);
  const auto early = Clock::now() + 200ms;
  owner.timer.ScheduleAt(early);  // The original wait has already been entered.
  state.WaitFor(1);
  WaitParked(owner.timer);
  auto times = state.Snapshot();
  owner.Stop();
  const bool delayed_to_old_deadline = times[0] >= late;
  Require(times.size() == 1 && times[0] >= early &&
              delayed_to_old_deadline == expect_late,
          "unexpected earlier-future notification result");
  std::cout
      << "{\"case\":\"earlier_future_notification_after_wait_entry\","
         "\"implementation\":\""
      << implementation << "\",\"relative_to_new_deadline_ms\":"
      << std::chrono::duration<double, std::milli>(times[0] - early).count()
      << ",\"delayed_to_old_deadline\":"
      << (delayed_to_old_deadline ? "true" : "false")
      << ",\"contract_pass\":" << (expect_late ? "false" : "true") << "}"
      << std::endl;
}

template <class Timer>
void FutureWhileParked(const char* implementation, bool expect_missing) {
  CaseStarted("future_notification_while_parked");
  State state;
  OwnedTimer<Timer> owner(state, false);
  owner.timer.Start();
  WaitEntered(owner.timer);
  const auto future = Clock::now() + 200ms;
  owner.timer.ScheduleAt(future);
  std::this_thread::sleep_until(future + 350ms);
  const auto times = state.Snapshot();
  owner.Stop();
  Require(times.empty() == expect_missing,
          "unexpected parked-future delivery result");
  Require(times.empty() || (times.size() == 1 && times[0] >= future),
          "parked-future fired early");
  std::cout
      << "{\"case\":\"future_notification_while_parked\",\"implementation\":\""
      << implementation
      << "\",\"observed_after_deadline_ms\":350,\"callbacks\":" << times.size()
      << ",\"contract_pass\":" << (expect_missing ? "false" : "true") << "}"
      << std::endl;
}

template <class Timer>
bool ControlledRace(const char* implementation, int round, bool expect_early) {
  CaseStarted("controlled_callback_rearm");
  State state;
  OwnedTimer<Timer> owner(state);
  owner.timer.Start();
  owner.timer.ScheduleAt(Clock::now());
  state.WaitFor(
      1);  // The first callback holds this gate with Timer mutex free.
  const Time deadline = Clock::now() + 1s;
  owner.timer.ScheduleAt(deadline);  // Returns before callback1 is released.
  const Time scheduling_finished = Clock::now();
  state.Release();
  state.WaitFor(2);
  auto times = state.Snapshot();
  const bool early = times[1] < deadline;
  if (expect_early) {
    // Observe the original callback is parked after its premature second fire.
    WaitParked(owner.timer);
    std::this_thread::sleep_until(deadline + 100ms);
  } else {
    WaitParked(owner.timer);
    std::this_thread::sleep_for(25ms);
  }
  times = state.Snapshot();
  owner.Stop();
  Require(times.size() == 2, "unexpected additional callbacks");
  const double offset_ms =
      std::chrono::duration<double, std::milli>(times[1] - deadline).count();
  const double schedule_to_fire_ms =
      std::chrono::duration<double, std::milli>(times[1] - scheduling_finished)
          .count();
  std::cout << "{\"case\":\"controlled_callback_rearm\",\"implementation\":\""
            << implementation << "\",\"round\":" << round
            << ",\"requested_delay_ms\":1000,\"callback_count\":"
            << times.size()
            << ",\"second_callback_relative_to_deadline_ms\":" << offset_ms
            << ",\"schedule_return_to_second_callback_ms\":"
            << schedule_to_fire_ms
            << ",\"early_callback\":" << (early ? "true" : "false")
            << ",\"contract_pass\":" << (early ? "false" : "true")
            << ",\"callback_observed_at_or_after_future_deadline\":"
            << (early ? "false" : "true") << "}" << std::endl;
  Require(early == expect_early, "unexpected controlled race result");
  return early;
}

void EarliestWinsDuringCallback() {
  CaseStarted("candidate_earliest_wins");
  State state;
  OwnedTimer<candidate::TimerThread> owner(state);
  owner.timer.Start();
  owner.timer.ScheduleAt(Clock::now());
  state.WaitFor(1);
  const auto now = Clock::now();
  const auto early = now + 250ms;
  const auto late = now + 1500ms;
  owner.timer.ScheduleAt(late);
  owner.timer.ScheduleAt(early);
  owner.timer.ScheduleAt(now + 2s);
  state.Release();
  state.WaitFor(2);
  WaitParked(owner.timer);
  auto times = state.Snapshot();
  owner.Stop();
  Require(times.size() == 2 && times[1] >= early && times[1] < late,
          "earliest callback deadline was early or lost/overwritten");
  std::cout
      << "{\"case\":\"candidate_earliest_wins_during_callback\",\"pass\":true,"
         "\"relative_to_early_deadline_ms\":"
      << std::chrono::duration<double, std::milli>(times[1] - early).count()
      << "}" << std::endl;
}

void SelfRearm() {
  CaseStarted("candidate_self_rearm");
  State state;
  candidate::TimerThread* pointer = nullptr;
  Time requested{};
  candidate::TimerThread timer([&] {
    state.Callback(false);
    if (state.Snapshot().size() == 1) {
      requested = Clock::now() + 100ms;
      pointer->ScheduleAt(requested);
    }
  });
  pointer = &timer;
  timer.Start();
  timer.ScheduleAt(Clock::now());
  state.WaitFor(2);
  WaitParked(timer);
  timer.Stop();  // joins before reading requested written by the timer thread
  const auto times = state.Snapshot();
  Require(times.size() == 2 && times[1] >= requested,
          "self-rearmed deadline fired early");
  std::cout << "{\"case\":\"candidate_callback_self_rearm\",\"pass\":true}"
            << std::endl;
}

void StopWake(bool scheduled) {
  CaseStarted("candidate_stop_wait");
  State state;
  OwnedTimer<candidate::TimerThread> owner(state, false);
  if (scheduled) owner.timer.ScheduleAt(Clock::now() + 1h);
  owner.timer.Start();
  WaitEntered(owner.timer);
  const auto started = Clock::now();
  owner.Stop();
  const auto elapsed =
      std::chrono::duration<double, std::milli>(Clock::now() - started).count();
  Require(state.Snapshot().empty() && elapsed < 1000,
          "Stop did not wake without dispatching");
  std::cout << "{\"case\":\"candidate_stop_"
            << (scheduled ? "future_wait" : "parked")
            << "\",\"pass\":true,\"stop_ms\":" << elapsed << "}" << std::endl;
}

void StopDuringCallback() {
  CaseStarted("candidate_stop_during_callback");
  State gate;
  auto lifetime = std::make_shared<int>(42);
  std::weak_ptr<int> witness = lifetime;
  candidate::TimerThread timer([keep = lifetime, &gate] {
    Require(*keep == 42, "callback capture lifetime lost");
    gate.Callback(true);
    Require(*keep == 42, "callback capture died before invocation completed");
  });
  lifetime.reset();
  timer.Start();
  timer.ScheduleAt(Clock::now());
  gate.WaitFor(1);
  std::atomic<bool> stopped{false};
  std::thread stopper([&] {
    timer.Stop();
    stopped.store(true, std::memory_order_release);
  });
  const auto limit = Clock::now() + 2s;
  while (timer.FixtureHasCallback() && Clock::now() < limit)
    std::this_thread::sleep_for(1ms);
  const bool member_cleared = !timer.FixtureHasCallback();
  const bool local_copy_kept_alive = !witness.expired();
  const bool stop_waited = !stopped.load(std::memory_order_acquire);
  gate.Release();
  stopper.join();
  Require(member_cleared && local_copy_kept_alive && stop_waited &&
              witness.expired(),
          "Stop/callback ownership or join contract failed");
  Require(gate.Snapshot().size() == 1, "Stop dispatched another callback");
  std::cout << "{\"case\":\"candidate_stop_during_callback\",\"pass\":true,"
               "\"member_cleared_before_gate_release\":true,\"weak_witness_"
               "alive_during_callback\":true,\"stop_waited_for_gate_release\":"
               "true,\"weak_witness_expired_after_stop\":true,\"callbacks\":1}"
            << std::endl;
}

int main() {
  std::cout << std::fixed << std::setprecision(6);
  std::mutex watchdog_mutex;
  std::condition_variable watchdog_cv;
  bool done = false;
  std::thread watchdog([&] {
    std::unique_lock<std::mutex> lock(watchdog_mutex);
    if (!watchdog_cv.wait_for(lock, 20s, [&] { return done; })) std::_Exit(92);
  });
  int code = 0;
  try {
    Require(Clock::is_steady,
            "fixture requires the actual MSVC steady high-resolution clock");
    for (int round = 1; round <= 3; ++round)
      ControlledRace<original::TimerThread>("frozen_original", round, true);
    for (int round = 1; round <= 3; ++round)
      ControlledRace<candidate::TimerThread>("patched_candidate", round, false);
    EarlierNotification<original::TimerThread>("frozen_original", true);
    EarlierNotification<candidate::TimerThread>("patched_candidate", false);
    FutureWhileParked<original::TimerThread>("frozen_original", true);
    FutureWhileParked<candidate::TimerThread>("patched_candidate", false);
    EarliestWinsDuringCallback();
    SelfRearm();
    StopWake(false);
    StopWake(true);
    StopDuringCallback();
    std::cout << "{\"summary\":\"original_expiry_race_and_known_notification_"
                 "bug_reproduced_candidate_checks_passed\",\"original_expiry_"
                 "reproductions\":3,\"original_notification_reproductions\":2,"
                 "\"candidate_cases_passed\":10,\"candidate_sdk_integrated\":"
                 "false,\"ui_network_or_service_operations\":0,\"vm_high_cpu_"
                 "attribution_proven\":false}"
              << std::endl;
  } catch (const std::exception& error) {
    std::cerr << "fixture failed: " << error.what() << std::endl;
    code = 1;
  }
  {
    std::lock_guard<std::mutex> lock(watchdog_mutex);
    done = true;
  }
  watchdog_cv.notify_all();
  watchdog.join();
  return code;
}
