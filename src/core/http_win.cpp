// WinHTTP-backed network provider, with an on-disk cache of encoded bytes.
//
// WinHTTP rather than WinINet: it is the supported stack for a service or a
// worker thread, it does not consult Internet Explorer's per-user settings, and
// it does not have WinINet's habit of putting up UI. Everything here runs on
// the pool's blocking lane, several requests at a time.

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

#include <winhttp.h>

#include "voidui/core/http.h"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <condition_variable>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <mutex>
#include <string>
#include <vector>

namespace voidui {

namespace {

std::wstring widen(std::string_view utf8) {
  if (utf8.empty())
    return {};
  const int n = MultiByteToWideChar(CP_UTF8, 0, utf8.data(),
                                    static_cast<int>(utf8.size()), nullptr, 0);
  std::wstring wide(n > 0 ? static_cast<std::size_t>(n) : 0, L'\0');
  if (n > 0)
    MultiByteToWideChar(CP_UTF8, 0, utf8.data(), static_cast<int>(utf8.size()),
                        wide.data(), n);
  return wide;
}

std::string narrow(const std::wstring &wide) {
  if (wide.empty())
    return {};
  const int n = WideCharToMultiByte(CP_UTF8, 0, wide.data(),
                                    static_cast<int>(wide.size()), nullptr, 0,
                                    nullptr, nullptr);
  std::string out(n > 0 ? static_cast<std::size_t>(n) : 0, '\0');
  if (n > 0)
    WideCharToMultiByte(CP_UTF8, 0, wide.data(), static_cast<int>(wide.size()),
                        out.data(), n, nullptr, nullptr);
  return out;
}

/// FNV-1a, used to name a cache file after the URL it holds.
std::uint64_t hash_url(std::string_view text) {
  std::uint64_t hash = 1469598103934665603ull;
  for (const char c : text) {
    hash ^= static_cast<unsigned char>(c);
    hash *= 1099511628211ull;
  }
  return hash;
}

std::string hex64(std::uint64_t value) {
  char buffer[17];
  std::snprintf(buffer, sizeof(buffer), "%016llx",
                static_cast<unsigned long long>(value));
  return buffer;
}

std::int64_t now_seconds() {
  return std::chrono::duration_cast<std::chrono::seconds>(
             std::chrono::system_clock::now().time_since_epoch())
      .count();
}

/// A parsed URL: what WinHTTP needs to open a connection and send a request.
struct Target {
  std::wstring host;
  std::wstring resource; ///< path plus query, with its leading slash
  INTERNET_PORT port = INTERNET_DEFAULT_HTTP_PORT;
  bool secure = false;
};

bool parse_url(std::string_view url, Target &target) {
  const std::wstring wide = widen(url);

  URL_COMPONENTS parts{};
  parts.dwStructSize = sizeof(parts);
  parts.dwHostNameLength = static_cast<DWORD>(-1);
  parts.dwUrlPathLength = static_cast<DWORD>(-1);
  parts.dwExtraInfoLength = static_cast<DWORD>(-1);
  parts.dwSchemeLength = static_cast<DWORD>(-1);

  if (!WinHttpCrackUrl(wide.c_str(), static_cast<DWORD>(wide.size()), 0, &parts))
    return false;
  if (parts.nScheme != INTERNET_SCHEME_HTTP && parts.nScheme != INTERNET_SCHEME_HTTPS)
    return false;
  if (!parts.lpszHostName || parts.dwHostNameLength == 0)
    return false;

  target.host.assign(parts.lpszHostName, parts.dwHostNameLength);
  target.port = parts.nPort;
  target.secure = parts.nScheme == INTERNET_SCHEME_HTTPS;

  target.resource.clear();
  if (parts.lpszUrlPath && parts.dwUrlPathLength > 0)
    target.resource.assign(parts.lpszUrlPath, parts.dwUrlPathLength);
  if (parts.lpszExtraInfo && parts.dwExtraInfoLength > 0)
    target.resource.append(parts.lpszExtraInfo, parts.dwExtraInfoLength);
  if (target.resource.empty())
    target.resource = L"/";

  return true;
}

/// The metadata beside a cached body, written as a small sidecar.
///
/// A sidecar rather than a header inside the body file, so the body can be
/// memory-mapped or handed straight to a decoder later without an offset that
/// everything has to agree about.
struct CacheEntry {
  std::int64_t fetched = 0;
  std::string etag;
  std::string last_modified;
  bool valid = false;
};

CacheEntry read_sidecar(const std::filesystem::path &path) {
  CacheEntry entry;
  std::ifstream file(path);
  if (!file)
    return entry;

  std::string line;
  if (!std::getline(file, line))
    return entry;
  entry.fetched = std::strtoll(line.c_str(), nullptr, 10);
  std::getline(file, entry.etag);
  std::getline(file, entry.last_modified);
  entry.valid = true;
  return entry;
}

void write_sidecar(const std::filesystem::path &path, const CacheEntry &entry) {
  std::ofstream file(path, std::ios::trunc);
  if (!file)
    return;
  file << entry.fetched << '\n' << entry.etag << '\n' << entry.last_modified << '\n';
}

/// Caps the number of requests in flight.
///
/// Not a thread pool -- the callers are already pool workers. This only stops
/// two hundred of them from opening two hundred connections, which is slower
/// than six and not faster: the bandwidth is the same and they would all be
/// competing for it.
class Gate {
public:
  explicit Gate(int limit) : available_(std::max(1, limit)) {}

  void acquire() {
    std::unique_lock lock(mutex_);
    ready_.wait(lock, [this] { return available_ > 0; });
    --available_;
  }

  void release() {
    {
      std::lock_guard lock(mutex_);
      ++available_;
    }
    ready_.notify_one();
  }

private:
  std::mutex mutex_;
  std::condition_variable ready_;
  int available_;
};

class GateHold {
public:
  explicit GateHold(Gate &gate) : gate_(gate) { gate_.acquire(); }
  ~GateHold() { gate_.release(); }
  GateHold(const GateHold &) = delete;
  GateHold &operator=(const GateHold &) = delete;

private:
  Gate &gate_;
};

/// RAII for a WinHTTP handle, of which one request needs three.
class Handle {
public:
  Handle() = default;
  explicit Handle(HINTERNET handle) : handle_(handle) {}
  ~Handle() {
    if (handle_)
      WinHttpCloseHandle(handle_);
  }

  Handle(Handle &&other) noexcept : handle_(other.handle_) {
    other.handle_ = nullptr;
  }
  Handle &operator=(Handle &&other) noexcept {
    if (this != &other) {
      if (handle_)
        WinHttpCloseHandle(handle_);
      handle_ = other.handle_;
      other.handle_ = nullptr;
    }
    return *this;
  }

  Handle(const Handle &) = delete;
  Handle &operator=(const Handle &) = delete;

  HINTERNET get() const { return handle_; }
  explicit operator bool() const { return handle_ != nullptr; }

private:
  HINTERNET handle_ = nullptr;
};

std::string header_value(HINTERNET request, DWORD which, const wchar_t *name) {
  DWORD size = 0;
  WinHttpQueryHeaders(request, which, name, WINHTTP_NO_OUTPUT_BUFFER, &size,
                      WINHTTP_NO_HEADER_INDEX);
  if (GetLastError() != ERROR_INSUFFICIENT_BUFFER || size == 0)
    return {};

  std::wstring value(size / sizeof(wchar_t), L'\0');
  if (!WinHttpQueryHeaders(request, which, name, value.data(), &size,
                           WINHTTP_NO_HEADER_INDEX))
    return {};

  value.resize(std::wcslen(value.c_str()));
  return narrow(value);
}

class HttpProvider final : public ResourceProvider {
public:
  explicit HttpProvider(HttpOptions options)
      : options_(std::move(options)), gate_(options_.max_concurrent) {
    if (!options_.cache_directory.empty()) {
      std::error_code error;
      std::filesystem::create_directories(options_.cache_directory, error);
      if (error)
        options_.cache_directory.clear();
    }
  }

  ~HttpProvider() override {
    if (session_)
      WinHttpCloseHandle(session_);
  }

  ResourceResult<Blob> open(std::string_view url) override {
    Target target;
    if (!parse_url(url, target))
      return std::unexpected(ResourceError::BadUri);

    const std::filesystem::path body = cache_path_(url, ".bin");
    const std::filesystem::path sidecar = cache_path_(url, ".meta");

    CacheEntry cached;
    if (!options_.cache_directory.empty()) {
      cached = read_sidecar(sidecar);
      if (cached.valid &&
          now_seconds() - cached.fetched < options_.min_fresh_seconds) {
        // Fresh by the application's own floor. No request at all, which is the
        // difference between a list that redraws instantly on a second run and
        // one that flickers through its placeholders again.
        if (ResourceResult<Blob> hit = read_body_(body))
          return hit;
      }
    }

    const GateHold hold(gate_);

    Blob fetched;
    const Fetch result = fetch_(target, cached, fetched, body, sidecar);

    if (result.outcome == Fetch::Body)
      return fetched;

    // Not modified, or a request that failed while a copy is still on disk.
    // Serving the stale bytes beats showing a hole: the picture has not changed
    // just because the network did.
    if (cached.valid) {
      if (ResourceResult<Blob> hit = read_body_(body))
        return hit;
    }

    return std::unexpected(result.outcome == Fetch::NotModified
                               ? ResourceError::NotFound
                               : result.error);
  }

  /// The moment the cached copy was last confirmed. Enough for a caller
  /// watching for change; not a content hash, and not meant as one.
  std::optional<std::uint64_t> revision(std::string_view url) override {
    if (options_.cache_directory.empty())
      return std::nullopt;

    const CacheEntry entry = read_sidecar(cache_path_(url, ".meta"));
    if (!entry.valid)
      return std::nullopt;
    if (!entry.etag.empty())
      return hash_url(entry.etag);
    return static_cast<std::uint64_t>(entry.fetched);
  }

private:
  std::filesystem::path cache_path_(std::string_view url,
                                    const char *suffix) const {
    if (options_.cache_directory.empty())
      return {};
    return std::filesystem::path(options_.cache_directory) /
           (hex64(hash_url(url)) + suffix);
  }

  ResourceResult<Blob> read_body_(const std::filesystem::path &path) const {
    if (path.empty())
      return std::unexpected(ResourceError::NotFound);

    std::ifstream file(path, std::ios::binary | std::ios::ate);
    if (!file)
      return std::unexpected(ResourceError::NotFound);

    const std::streamoff size = file.tellg();
    if (size < 0)
      return std::unexpected(ResourceError::Unreadable);

    std::vector<std::uint8_t> data(static_cast<std::size_t>(size));
    file.seekg(0);
    if (size > 0 &&
        !file.read(reinterpret_cast<char *>(data.data()),
                   static_cast<std::streamsize>(size)))
      return std::unexpected(ResourceError::Unreadable);

    return Blob::own(std::move(data));
  }

  /// One session for the process. WinHTTP sessions are thread-safe and hold the
  /// connection pool, so making one per request would throw away keep-alive --
  /// and with it most of the benefit of talking to one host repeatedly.
  HINTERNET ensure_session_() {
    std::call_once(session_once_, [this] {
      session_ = WinHttpOpen(widen(options_.user_agent).c_str(),
                             WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                             WINHTTP_NO_PROXY_NAME, WINHTTP_NO_PROXY_BYPASS, 0);
      if (session_)
        WinHttpSetTimeouts(session_, options_.timeout_ms, options_.timeout_ms,
                           options_.timeout_ms, options_.timeout_ms);
    });
    return session_;
  }

  /// What one request produced. Three outcomes rather than two, because "the
  /// server says your copy is current" and "the request failed" both leave the
  /// caller reading from cache but mean opposite things when it has none.
  struct Fetch {
    enum Outcome { Body, NotModified, Failed };
    Outcome outcome = Failed;
    ResourceError error = ResourceError::NotFound;
  };

  static Fetch failed(ResourceError error) { return {Fetch::Failed, error}; }

  Fetch fetch_(const Target &target, const CacheEntry &cached, Blob &out,
               const std::filesystem::path &body,
               const std::filesystem::path &sidecar) {
    HINTERNET session = ensure_session_();
    if (!session)
      return failed(ResourceError::Unreadable);

    Handle connection(
        WinHttpConnect(session, target.host.c_str(), target.port, 0));
    if (!connection)
      return failed(ResourceError::NotFound);

    Handle request(WinHttpOpenRequest(
        connection.get(), L"GET", target.resource.c_str(), nullptr,
        WINHTTP_NO_REFERER, WINHTTP_DEFAULT_ACCEPT_TYPES,
        target.secure ? WINHTTP_FLAG_SECURE : 0));
    if (!request)
      return failed(ResourceError::NotFound);

    // Revalidation, which is the whole reason the sidecar keeps these: an
    // unchanged picture costs a 304 and no body at all.
    std::wstring headers;
    if (!cached.etag.empty())
      headers += L"If-None-Match: " + widen(cached.etag) + L"\r\n";
    if (!cached.last_modified.empty())
      headers += L"If-Modified-Since: " + widen(cached.last_modified) + L"\r\n";

    if (!WinHttpSendRequest(request.get(),
                            headers.empty() ? WINHTTP_NO_ADDITIONAL_HEADERS
                                            : headers.c_str(),
                            headers.empty() ? 0 : static_cast<DWORD>(-1),
                            WINHTTP_NO_REQUEST_DATA, 0, 0, 0))
      return failed(ResourceError::NotFound);

    if (!WinHttpReceiveResponse(request.get(), nullptr))
      return failed(ResourceError::NotFound);

    DWORD status = 0;
    DWORD status_size = sizeof(status);
    if (!WinHttpQueryHeaders(
            request.get(), WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            WINHTTP_HEADER_NAME_BY_INDEX, &status, &status_size,
            WINHTTP_NO_HEADER_INDEX))
      return failed(ResourceError::NotFound);

    if (status == 304)
      return {Fetch::NotModified, ResourceError::NotFound};
    if (status == 401 || status == 403)
      return failed(ResourceError::Denied);
    if (status >= 400)
      return failed(status == 404 ? ResourceError::NotFound
                                  : ResourceError::Unreadable);

    std::vector<std::uint8_t> data;
    DWORD available = 0;
    while (WinHttpQueryDataAvailable(request.get(), &available) && available > 0) {
      const std::size_t offset = data.size();

      // Checked as it arrives rather than after. A server declaring one length
      // and sending another is ordinary, and a caller must never be able to
      // decide how much memory this process spends.
      if (offset + available > options_.max_bytes)
        return failed(ResourceError::Unreadable);

      data.resize(offset + available);
      DWORD read = 0;
      if (!WinHttpReadData(request.get(), data.data() + offset, available, &read))
        return failed(ResourceError::Unreadable);
      data.resize(offset + read);
      if (read == 0)
        break;
    }

    const std::string cache_control =
        header_value(request.get(), WINHTTP_QUERY_CUSTOM, L"Cache-Control");
    const bool storable =
        cache_control.find("no-store") == std::string::npos &&
        cache_control.find("private") == std::string::npos;

    if (!body.empty() && storable) {
      CacheEntry entry;
      entry.fetched = now_seconds();
      entry.etag = header_value(request.get(), WINHTTP_QUERY_CUSTOM, L"ETag");
      entry.last_modified =
          header_value(request.get(), WINHTTP_QUERY_CUSTOM, L"Last-Modified");

      std::ofstream file(body, std::ios::binary | std::ios::trunc);
      if (file) {
        file.write(reinterpret_cast<const char *>(data.data()),
                   static_cast<std::streamsize>(data.size()));
        if (file.good()) {
          file.close();
          write_sidecar(sidecar, entry);
          trim_cache_();
        }
      }
    }

    out = Blob::own(std::move(data));
    return {Fetch::Body, ResourceError::NotFound};
  }

  /// Drops the oldest files once the directory exceeds its ceiling.
  ///
  /// Cheap enough to do after a write and only after a write: a cache that is
  /// not growing is never walked, and one that is grows one entry at a time.
  void trim_cache_() {
    std::error_code error;
    std::vector<std::pair<std::filesystem::file_time_type, std::filesystem::path>>
        files;
    std::uintmax_t total = 0;

    for (const auto &entry :
         std::filesystem::directory_iterator(options_.cache_directory, error)) {
      if (error)
        return;
      if (!entry.is_regular_file(error) || error)
        continue;
      if (entry.path().extension() != ".bin")
        continue;

      const std::uintmax_t size = entry.file_size(error);
      if (error)
        continue;

      total += size;
      files.emplace_back(entry.last_write_time(error), entry.path());
    }

    if (total <= options_.cache_bytes)
      return;

    std::sort(files.begin(), files.end(),
              [](const auto &a, const auto &b) { return a.first < b.first; });

    for (const auto &[stamp, path] : files) {
      if (total <= options_.cache_bytes)
        break;
      const std::uintmax_t size = std::filesystem::file_size(path, error);
      std::filesystem::remove(path, error);
      std::filesystem::remove(std::filesystem::path(path).replace_extension(".meta"),
                              error);
      if (!error)
        total -= size;
    }
  }

  HttpOptions options_;
  Gate gate_;
  std::once_flag session_once_;
  HINTERNET session_ = nullptr;
};

} // namespace

std::shared_ptr<ResourceProvider> http_provider(HttpOptions options) {
  return std::make_shared<HttpProvider>(std::move(options));
}

std::string default_cache_directory(std::string_view application) {
  std::string root;

  wchar_t buffer[MAX_PATH];
  const DWORD length = GetEnvironmentVariableW(L"LOCALAPPDATA", buffer, MAX_PATH);
  if (length > 0 && length < MAX_PATH)
    root = narrow(std::wstring(buffer, length));

  if (root.empty())
    return {};

  return (std::filesystem::path(root) / std::string(application) / "http-cache")
      .string();
}

} // namespace voidui
