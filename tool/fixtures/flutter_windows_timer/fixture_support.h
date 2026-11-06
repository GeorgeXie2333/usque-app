// Copyright 2026. Usque standalone test support; no product or platform APIs.
// Extracted Flutter code retains its copyright and LICENSE.flutter.
#ifndef USQUE_FLUTTER_TIMER_FIXTURE_SUPPORT_H_
#define USQUE_FLUTTER_TIMER_FIXTURE_SUPPORT_H_

#include <atomic>
#include <cassert>
#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <mutex>
#include <optional>
#include <thread>
#include <utility>

#define FML_DCHECK(condition) assert(condition)

// Witness wait entry while the Timer mutex is still held. Both overloads
// delegate unchanged to std::condition_variable. ScheduleAt then acquiring
// that same mutex proves the real wait has released it, without a sleep guess.
class FixtureConditionVariable {
 public:
  template <class C, class D, class P>
  bool wait_until(std::unique_lock<std::mutex>& lock,
                  const std::chrono::time_point<C, D>& at, P predicate) {
    entries_.fetch_add(1, std::memory_order_release);
    return value_.wait_until(lock, at, std::move(predicate));
  }
  template <class C, class D>
  std::cv_status wait_until(std::unique_lock<std::mutex>& lock,
                            const std::chrono::time_point<C, D>& at) {
    entries_.fetch_add(1, std::memory_order_release);
    return value_.wait_until(lock, at);
  }
  void notify_all() { value_.notify_all(); }
  unsigned wait_entries() const {
    return entries_.load(std::memory_order_acquire);
  }

 private:
  std::condition_variable value_;
  std::atomic<unsigned> entries_{0};
};

#endif
