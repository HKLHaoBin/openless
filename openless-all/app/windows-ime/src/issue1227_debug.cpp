#include "issue1227_debug.h"

#include <windows.h>

#include <mutex>
#include <string>

namespace issue1227_debug {
namespace {

constexpr wchar_t kLogEnvironment[] = L"OPENLESS_ISSUE_1227_DEBUG_LOG";
constexpr wchar_t kDefaultLogName[] = L"openless-issue-1227-3e857688-session.ndjson";
constexpr char kSessionId[] = "3e857688-76d6-4d76-8747-7ff1e05ee657";

std::mutex &LogMutex() {
  static std::mutex mutex;
  return mutex;
}

std::wstring LogPath() {
  wchar_t path[32768] = {};
  const DWORD length = GetEnvironmentVariableW(kLogEnvironment, path, ARRAYSIZE(path));
  if (length != 0 && length < ARRAYSIZE(path)) {
    return std::wstring(path, length);
  }

  wchar_t temp_path[MAX_PATH] = {};
  const DWORD temp_length = GetTempPathW(ARRAYSIZE(temp_path), temp_path);
  if (temp_length == 0 || temp_length >= ARRAYSIZE(temp_path)) {
    return kDefaultLogName;
  }
  return std::wstring(temp_path, temp_length) + kDefaultLogName;
}

unsigned long long UnixMillis() {
  FILETIME file_time = {};
  GetSystemTimeAsFileTime(&file_time);
  ULARGE_INTEGER ticks = {};
  ticks.LowPart = file_time.dwLowDateTime;
  ticks.HighPart = file_time.dwHighDateTime;
  constexpr unsigned long long kUnixEpochFileTime = 116444736000000000ULL;
  if (ticks.QuadPart < kUnixEpochFileTime) {
    return 0;
  }
  return (ticks.QuadPart - kUnixEpochFileTime) / 10000ULL;
}

} // namespace

void Record(const char *probe_id, const char *hypothesis_ids, const char *event,
            const char *outcome, const char *safe_json) {
  const std::string run_id = "native-" + std::to_string(GetCurrentProcessId());
  const std::string line =
      std::string("{\"schema\":\"diagnosing-bugs.v1\",\"session_id\":\"") + kSessionId +
      "\",\"run_id\":\"" + run_id + "\",\"probe_id\":\"" + probe_id +
      "\",\"hypothesis_ids\":[" + hypothesis_ids + "]" +
      ",\"event\":\"" + event + "\",\"outcome\":\"" + outcome +
      "\",\"safe\":{" + safe_json + "},\"ts\":" + std::to_string(UnixMillis()) +
      "}\r\n";

  std::lock_guard<std::mutex> lock(LogMutex());
  const std::wstring path = LogPath();
  HANDLE file = CreateFileW(path.c_str(), FILE_APPEND_DATA, FILE_SHARE_READ | FILE_SHARE_WRITE,
                            nullptr, OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
  if (file == INVALID_HANDLE_VALUE) {
    return;
  }
  DWORD written = 0;
  (void)WriteFile(file, line.data(), static_cast<DWORD>(line.size()), &written, nullptr);
  CloseHandle(file);
}

} // namespace issue1227_debug
