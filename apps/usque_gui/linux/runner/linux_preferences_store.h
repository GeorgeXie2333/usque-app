#ifndef USQUE_LINUX_PREFERENCES_STORE_H_
#define USQUE_LINUX_PREFERENCES_STORE_H_
#include <functional>
#include <memory>
#include <string>

class UsqueLinuxPreferencesStore {
 public:
  using ReadCompletion = std::function<void(bool, const std::string&)>;
  explicit UsqueLinuxPreferencesStore(std::string data_directory,
                                      std::string application_id,
                                      std::string executable_name,
                                      std::function<void()> before_read = {});
  ~UsqueLinuxPreferencesStore();
  void Read(ReadCompletion completion);
  void Write(std::string contents, std::function<void(bool)> completion);
  void Flush(std::function<void(bool)> completion);
 private:
  struct State;
  std::shared_ptr<State> state_;
};
#endif
