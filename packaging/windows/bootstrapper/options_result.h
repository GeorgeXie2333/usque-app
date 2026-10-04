#pragma once

#include <map>
#include <optional>
#include <string>
#include <string_view>

namespace usque::setup {
struct OptionItem {
  std::string status;
  std::optional<bool> enabled;
  bool Success() const {
    return status != "conflict" && status != "error" && status != "not_requested";
  }
};
struct OptionsReport {
  bool valid = false;
  std::string status;
  OptionItem desktop, startup;
};

// Parse the runner's deliberately restricted, non-sensitive schema. The
// parser rejects duplicate keys, unknown schema/fields, trailing output and
// malformed JSON instead of guessing from a substring or overall exit code.
class ResultParser {
 public:
  explicit ResultParser(std::string_view input) : input_(input) {}
  OptionsReport Parse() {
    OptionsReport result;
    if (!Object("", 0)) return result;
    Space();
    if (position_ != input_.size() || values_.size() != 6 ||
        values_["schema"] != "1" || !StringValue("status", result.status) ||
        !Item("desktopShortcut", result.desktop) || !Item("startOnLogin", result.startup)) return result;
    if (result.status != "ok" && result.status != "partial" &&
        result.status != "invalid_arguments" && result.status != "user_context_required") return result;
    result.valid = true;
    return result;
  }
 private:
  void Space() { while (position_ < input_.size() &&
    (input_[position_] == ' ' || input_[position_] == '\n' || input_[position_] == '\r' || input_[position_] == '\t')) ++position_; }
  bool Take(char value) { Space(); if (position_ >= input_.size() || input_[position_] != value) return false; ++position_; return true; }
  bool String(std::string& out) {
    if (!Take('"')) return false;
    const size_t start = position_;
    while (position_ < input_.size() && input_[position_] != '"') {
      // No schema key/status contains an escaped or non-ASCII character.
      if (input_[position_] < 32 || input_[position_] > 126 || input_[position_] == '\\') return false;
      ++position_;
    }
    if (position_ == input_.size()) return false;
    out = input_.substr(start, position_ - start); ++position_; return true;
  }
  bool Object(const std::string& prefix, int depth) {
    if (depth > 1 || !Take('{')) return false;
    std::map<std::string, bool> names;
    do {
      std::string name;
      if (!String(name) || !names.emplace(name, true).second || !Take(':')) return false;
      const std::string key = prefix.empty() ? name : prefix + "." + name;
      Space();
      if (position_ >= input_.size()) return false;
      if (input_[position_] == '{') { if (!Object(key, depth + 1)) return false; }
      else {
        std::string value;
        if (input_[position_] == '"') { if (!String(value)) return false; value = '"' + value + '"'; }
        else {
          const auto start = position_;
          while (position_ < input_.size() && input_[position_] != ',' && input_[position_] != '}' &&
                 input_[position_] != ' ' && input_[position_] != '\r' && input_[position_] != '\n' && input_[position_] != '\t') ++position_;
          value = input_.substr(start, position_ - start);
          if (value != "1" && value != "true" && value != "false" && value != "null") return false;
        }
        if (!values_.emplace(key, value).second) return false;
      }
      Space();
      if (position_ < input_.size() && input_[position_] == '}') { ++position_; return true; }
    } while (Take(','));
    return false;
  }
  bool StringValue(const std::string& key, std::string& out) {
    const auto found = values_.find(key);
    if (found == values_.end() || found->second.size() < 2 || found->second.front() != '"') return false;
    out = found->second.substr(1, found->second.size() - 2); return true;
  }
  bool Item(const std::string& key, OptionItem& item) {
    if (!StringValue(key + ".status", item.status)) return false;
    const auto found = values_.find(key + ".enabled");
    if (found == values_.end()) return false;
    if (found->second == "true") item.enabled = true;
    else if (found->second == "false") item.enabled = false;
    else if (found->second != "null") return false;
    for (std::string_view allowed : {"created", "present", "absent", "enabled", "disabled", "unchanged", "kept", "removed", "conflict", "error", "not_requested"})
      if (item.status == allowed) return true;
    return false;
  }
  std::string_view input_;
  size_t position_ = 0;
  std::map<std::string, std::string> values_;
};
}  // namespace usque::setup
