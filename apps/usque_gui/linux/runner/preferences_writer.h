#ifndef USQUE_LINUX_PREFERENCES_WRITER_H_
#define USQUE_LINUX_PREFERENCES_WRITER_H_

#include <functional>
#include <memory>
#include <string>

// All entry points and completions run on the owning GLib main context. Only
// immutable serialized preferences cross to the single background writer.
class UsquePreferencesWriter {
 public:
  using Completion = std::function<void(bool)>;
  using Write = std::function<bool(const std::string&)>;
  explicit UsquePreferencesWriter(std::string path, Write write = {},
                                  unsigned debounce_ms = 150);
  ~UsquePreferencesWriter();
  void Save(std::string contents, Completion completion = {});
  // Drain the latest snapshot without blocking the main loop, including when
  // a prior write is still running. Call before terminating the application.
  void Flush(Completion completion);

 private:
  struct State;
  std::shared_ptr<State> state_;
};

#endif
