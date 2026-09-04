#include "voidui/core/resource.h"

#include <algorithm>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <unordered_map>

namespace voidui {
namespace {

bool is_alpha(char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
}

bool is_scheme_char(char c) {
  return is_alpha(c) || (c >= '0' && c <= '9') || c == '+' || c == '-' ||
         c == '.';
}

/// Splits `text` at a scheme separator, if it has one.
///
/// A scheme must be at least two characters. That is not pedantry: without it
/// every Windows path starting with a drive letter parses as the scheme "C",
/// and `C:/fonts/Inter.ttf` becomes an unknown-scheme error instead of a file.
bool split_scheme(std::string_view text, std::string_view &scheme,
                  std::string_view &rest) {
  const std::size_t colon = text.find(':');
  if (colon == std::string_view::npos || colon < 2)
    return false;

  for (std::size_t i = 0; i < colon; ++i) {
    const char c = text[i];
    if (i == 0 ? !is_alpha(c) : !is_scheme_char(c))
      return false;
  }

  scheme = text.substr(0, colon);
  rest = text.substr(colon + 1);
  return true;
}

/// A drive-letter prefix, if `text` opens with one.
bool has_drive(std::string_view text) {
  return text.size() >= 2 && is_alpha(text[0]) && text[1] == ':';
}

/// Folds separators, `.` and `..` away, and reports a traversal that would
/// leave the root rather than clamping it.
///
/// Clamping is what most path joiners do and it is the wrong answer here: a
/// stylesheet asking for `../../secrets` should fail loudly, not quietly read
/// the wrong file. The one place `..` survives is a relative `file:` path,
/// which has no root to leave -- it is resolved against the process's working
/// directory by whoever opens it.
std::optional<std::string> normalize(std::string_view input,
                                     std::string_view scheme) {
  std::string text(input);
  for (char &c : text)
    if (c == '\\')
      c = '/';

  std::string_view rest = text;
  std::string root;

  if (scheme == "file") {
    // `file:///C:/x` arrives here as `/C:/x`.
    if (rest.size() >= 3 && rest[0] == '/' && has_drive(rest.substr(1)))
      rest.remove_prefix(1);

    if (has_drive(rest)) {
      root = std::string(rest.substr(0, 2)) + '/';
      rest.remove_prefix(2);
    } else if (!rest.empty() && rest.front() == '/') {
      root = "/";
    }
  }

  while (!rest.empty() && rest.front() == '/')
    rest.remove_prefix(1);

  const bool rooted = !root.empty() || scheme != "file";

  std::vector<std::string_view> parts;
  for (std::size_t at = 0; at <= rest.size();) {
    std::size_t end = rest.find('/', at);
    if (end == std::string_view::npos)
      end = rest.size();

    const std::string_view segment = rest.substr(at, end - at);
    if (segment.find('\0') != std::string_view::npos)
      return std::nullopt;

    if (segment == "..") {
      if (!parts.empty() && parts.back() != "..")
        parts.pop_back();
      else if (rooted)
        return std::nullopt;
      else
        parts.push_back(segment);
    } else if (!segment.empty() && segment != ".") {
      parts.push_back(segment);
    }

    if (end == rest.size())
      break;
    at = end + 1;
  }

  std::string out = root;
  for (std::size_t i = 0; i < parts.size(); ++i) {
    if (i != 0)
      out += '/';
    out += parts[i];
  }
  return out;
}

ResourceResult<Blob> read_file(const std::string &path) {
  std::error_code code;
  const std::filesystem::file_status status = std::filesystem::status(path, code);
  if (code || !std::filesystem::exists(status) ||
      std::filesystem::is_directory(status))
    return std::unexpected(ResourceError::NotFound);

  std::ifstream stream(path, std::ios::binary | std::ios::ate);
  if (!stream)
    return std::unexpected(ResourceError::Unreadable);

  const std::streamoff size = stream.tellg();
  if (size < 0)
    return std::unexpected(ResourceError::Unreadable);
  stream.seekg(0, std::ios::beg);

  std::vector<std::byte> data(static_cast<std::size_t>(size));
  if (size > 0 &&
      !stream.read(reinterpret_cast<char *>(data.data()),
                   static_cast<std::streamsize>(size)))
    return std::unexpected(ResourceError::Unreadable);

  return Blob::own(std::move(data));
}

std::optional<std::uint64_t> file_revision(const std::string &path) {
  std::error_code code;
  const std::filesystem::file_time_type stamp =
      std::filesystem::last_write_time(path, code);
  if (code)
    return std::nullopt;
  return static_cast<std::uint64_t>(stamp.time_since_epoch().count());
}

/// Strips leading and trailing separators, so "fonts", "/fonts" and "fonts/"
/// all name the same mount point.
std::string clean_prefix(std::string_view prefix) {
  while (!prefix.empty() && (prefix.front() == '/' || prefix.front() == '\\'))
    prefix.remove_prefix(1);
  while (!prefix.empty() && (prefix.back() == '/' || prefix.back() == '\\'))
    prefix.remove_suffix(1);
  return std::string(prefix);
}

class DirectoryProvider : public ResourceProvider {
public:
  explicit DirectoryProvider(std::string root) : root_(std::move(root)) {
    while (!root_.empty() && (root_.back() == '/' || root_.back() == '\\'))
      root_.pop_back();
  }

  ResourceResult<Blob> open(std::string_view path) override {
    return read_file(join_(path));
  }

  std::optional<std::string> native_path(std::string_view path) override {
    const std::string full = join_(path);
    std::error_code code;
    if (!std::filesystem::is_regular_file(full, code) || code)
      return std::nullopt;
    return full;
  }

  std::optional<std::uint64_t> revision(std::string_view path) override {
    return file_revision(join_(path));
  }

private:
  /// `path` reached the mount table as a URI and left `normalize` without a
  /// single `..`, so this is a plain concatenation with nothing to defend.
  std::string join_(std::string_view path) const {
    std::string full = root_;
    if (!full.empty() && !path.empty())
      full += '/';
    full.append(path);
    return full;
  }

  std::string root_;
};

class EmbeddedProvider : public ResourceProvider {
public:
  explicit EmbeddedProvider(std::span<const EmbeddedResource> entries) {
    index_.reserve(entries.size());
    for (const EmbeddedResource &entry : entries)
      index_.emplace(entry.path, entry.bytes);
  }

  ResourceResult<Blob> open(std::string_view path) override {
    const auto found = index_.find(path);
    if (found == index_.end())
      return std::unexpected(ResourceError::NotFound);
    return Blob::borrow(found->second);
  }

private:
  std::unordered_map<std::string_view, std::span<const std::byte>> index_;
};

} // namespace

// -- Blob ---------------------------------------------------------------

Blob Blob::borrow(std::span<const std::byte> bytes) { return Blob(bytes, {}); }

Blob Blob::own(std::vector<std::byte> data) {
  auto held = std::make_shared<std::vector<std::byte>>(std::move(data));
  const std::span<const std::byte> bytes(*held);
  return Blob(bytes, std::move(held));
}

Blob Blob::own(std::vector<std::uint8_t> data) {
  auto held = std::make_shared<std::vector<std::uint8_t>>(std::move(data));
  const std::span<const std::byte> bytes(
      reinterpret_cast<const std::byte *>(held->data()), held->size());
  return Blob(bytes, std::move(held));
}

Blob Blob::adopt(std::span<const std::byte> bytes,
                 std::shared_ptr<const void> owner) {
  return Blob(bytes, std::move(owner));
}

std::string_view Blob::text() const {
  return std::string_view(reinterpret_cast<const char *>(bytes_.data()),
                          bytes_.size());
}

std::string_view to_string(ResourceError error) {
  switch (error) {
  case ResourceError::BadUri:
    return "malformed resource URI";
  case ResourceError::NotMounted:
    return "no resource mount claims this path";
  case ResourceError::NotFound:
    return "resource not found";
  case ResourceError::Unreadable:
    return "resource could not be read";
  case ResourceError::Denied:
    return "resource access denied by policy";
  }
  return "unknown resource error";
}

// -- ResourceUri --------------------------------------------------------

ResourceResult<ResourceUri> ResourceUri::parse(std::string_view text) {
  if (text.empty())
    return std::unexpected(ResourceError::BadUri);

  std::string_view scheme;
  std::string_view rest;

  if (!split_scheme(text, scheme, rest)) {
    // No scheme is the shape a path takes when it comes from a command line,
    // a config file, or existing code that predates this layer.
    scheme = "file";
    rest = text;
  } else if (rest.starts_with("//")) {
    rest.remove_prefix(2);
  }

  if (scheme != "res" && scheme != "file" && scheme != "http" &&
      scheme != "https")
    return std::unexpected(ResourceError::BadUri);

  std::optional<std::string> path = normalize(rest, scheme);
  if (!path)
    return std::unexpected(ResourceError::BadUri);

  ResourceUri uri;
  uri.scheme_ = std::string(scheme);
  uri.path_ = std::move(*path);
  return uri;
}

ResourceUri ResourceUri::base() const {
  ResourceUri out = *this;
  const std::size_t slash = out.path_.rfind('/');
  out.path_ = slash == std::string::npos ? std::string() : out.path_.substr(0, slash);
  return out;
}

ResourceResult<ResourceUri> ResourceUri::resolve(std::string_view reference) const {
  if (reference.empty())
    return std::unexpected(ResourceError::BadUri);

  std::string_view scheme;
  std::string_view rest;
  if (split_scheme(reference, scheme, rest))
    return parse(reference);

  // Spelt out rather than written as a ternary: mixing a literal with a
  // std::string there yields a temporary string, and the view outlives it.
  std::string_view inherited = scheme_;
  if (inherited.empty())
    inherited = "file";

  // A drive letter is not a URL reference. Under `file:` it names an absolute
  // path and is read as one; under any other scheme it is not something a
  // document can name at all, and saying so beats quietly assembling
  // `res://theme/C:/Users/...` and failing later with "not found".
  if (has_drive(reference)) {
    if (inherited != "file")
      return std::unexpected(ResourceError::BadUri);
    return parse(reference);
  }

  std::string joined;
  if (reference.front() == '/' || reference.front() == '\\') {
    // Root-relative. Under `file:` the root includes the drive the document
    // itself sits on, which is the only reading that keeps `/assets/x` next to
    // a document at `C:/app/theme.vss` on the same volume.
    if (inherited == "file" && has_drive(path_))
      joined = path_.substr(0, 2);
    joined.append(reference);
  } else {
    joined = base().path_;
    if (!joined.empty())
      joined += '/';
    joined.append(reference);
  }

  std::optional<std::string> path = normalize(joined, inherited);
  if (!path)
    return std::unexpected(ResourceError::BadUri);

  ResourceUri out;
  out.scheme_ = std::string(inherited);
  out.path_ = std::move(*path);
  return out;
}

std::string ResourceUri::to_string() const {
  if (scheme_.empty())
    return {};
  if (scheme_ == "file") {
    // A relative path has no authority slot to fill, so it stays bare rather
    // than growing a `//` that would reparse as a host name.
    if (path_.empty() || (path_.front() != '/' && !has_drive(path_)))
      return "file:" + path_;
    return "file:///" + (path_.front() == '/' ? path_.substr(1) : path_);
  }
  return scheme_ + "://" + path_;
}

std::string ResourceUri::display() const {
  return is_file() ? path_ : to_string();
}

// -- Providers ----------------------------------------------------------

std::optional<std::string> ResourceProvider::native_path(std::string_view) {
  return std::nullopt;
}

std::optional<std::uint64_t> ResourceProvider::revision(std::string_view) {
  return std::nullopt;
}

std::shared_ptr<ResourceProvider> directory_provider(std::string root) {
  return std::make_shared<DirectoryProvider>(std::move(root));
}

std::shared_ptr<ResourceProvider>
embedded_provider(std::span<const EmbeddedResource> entries) {
  return std::make_shared<EmbeddedProvider>(entries);
}

void MemoryProvider::add(std::string path, Blob blob) {
  const std::string cleaned = clean_prefix(path);
  for (auto &entry : entries_) {
    if (entry.first == cleaned) {
      entry.second = std::move(blob);
      return;
    }
  }
  entries_.emplace_back(cleaned, std::move(blob));
}

bool MemoryProvider::remove(std::string_view path) {
  const std::string cleaned = clean_prefix(path);
  const auto found = std::find_if(
      entries_.begin(), entries_.end(),
      [&](const auto &entry) { return entry.first == cleaned; });
  if (found == entries_.end())
    return false;
  entries_.erase(found);
  return true;
}

ResourceResult<Blob> MemoryProvider::open(std::string_view path) {
  for (const auto &entry : entries_)
    if (entry.first == path)
      return entry.second;
  return std::unexpected(ResourceError::NotFound);
}

// -- Resources ----------------------------------------------------------

struct Resources::Mount {
  std::string prefix;
  std::shared_ptr<ResourceProvider> provider;
  int priority = 0;
  MountId id = 0;

  /// Whether this mount claims `path`, and if so what is left for the provider.
  bool claims(std::string_view path, std::string_view &remainder) const {
    if (prefix.empty()) {
      remainder = path;
      return true;
    }
    if (!path.starts_with(prefix))
      return false;
    if (path.size() == prefix.size()) {
      remainder = {};
      return true;
    }
    if (path[prefix.size()] != '/')
      return false;
    remainder = path.substr(prefix.size() + 1);
    return true;
  }
};

struct Resources::Table {
  std::vector<Mount> mounts; ///< longest prefix first, then highest priority
};

Resources::Resources() = default;
Resources::~Resources() = default;

Resources &Resources::global() {
  static Resources instance;
  return instance;
}

std::shared_ptr<const Resources::Table> Resources::snapshot_() const {
  return table_.load(std::memory_order_acquire);
}

Resources::MountId Resources::mount(std::string prefix,
                                    std::shared_ptr<ResourceProvider> provider,
                                    int priority) {
  if (!provider)
    return 0;

  const std::lock_guard<std::mutex> guard(mount_mutex_);

  const std::shared_ptr<const Table> current = snapshot_();
  auto next = std::make_shared<Table>();
  if (current)
    next->mounts = current->mounts;

  Mount mount;
  mount.prefix = clean_prefix(prefix);
  mount.provider = std::move(provider);
  mount.priority = priority;
  mount.id = next_mount_id_++;
  const MountId id = mount.id;
  next->mounts.push_back(std::move(mount));

  // Stable, so two mounts on the same prefix at the same priority stay in the
  // order they were taken and the later one shadows nothing.
  std::stable_sort(next->mounts.begin(), next->mounts.end(),
                   [](const Mount &a, const Mount &b) {
                     if (a.prefix.size() != b.prefix.size())
                       return a.prefix.size() > b.prefix.size();
                     return a.priority > b.priority;
                   });

  table_.store(std::move(next), std::memory_order_release);
  return id;
}

bool Resources::unmount(MountId id) {
  const std::lock_guard<std::mutex> guard(mount_mutex_);

  const std::shared_ptr<const Table> current = snapshot_();
  if (!current)
    return false;

  auto next = std::make_shared<Table>();
  next->mounts = current->mounts;
  const auto found =
      std::find_if(next->mounts.begin(), next->mounts.end(),
                   [&](const Mount &mount) { return mount.id == id; });
  if (found == next->mounts.end())
    return false;

  next->mounts.erase(found);
  table_.store(std::move(next), std::memory_order_release);
  return true;
}

ResourceResult<Blob> Resources::open(const ResourceUri &uri) const {
  if (uri.empty())
    return std::unexpected(ResourceError::BadUri);

  if (uri.is_file())
    return read_file(std::string(uri.path()));

  // The network is one provider, not a mount table. A mount answers for a
  // prefix of the application's own namespace, which is a question about
  // deployment; a URL already carries its own authority, and there is nothing
  // left for a table to decide.
  if (uri.is_network()) {
    const std::shared_ptr<ResourceProvider> network = network_.load();
    if (!network)
      return std::unexpected(ResourceError::NotMounted);
    return network->open(uri.to_string());
  }

  const std::shared_ptr<const Table> table = snapshot_();
  if (!table)
    return std::unexpected(ResourceError::NotMounted);

  // Every claiming mount is tried, not just the first. That fall-through is
  // what makes an overlay work: a loose directory mounted over a packed build
  // holds only the files being edited, and everything else drops through to the
  // pack underneath.
  bool claimed = false;
  ResourceError failure = ResourceError::NotFound;

  for (const Mount &mount : table->mounts) {
    std::string_view remainder;
    if (!mount.claims(uri.path(), remainder))
      continue;

    claimed = true;
    ResourceResult<Blob> blob = mount.provider->open(remainder);
    if (blob)
      return blob;
    if (blob.error() != ResourceError::NotFound)
      failure = blob.error();
  }

  return std::unexpected(claimed ? failure : ResourceError::NotMounted);
}

ResourceResult<Blob> Resources::open(std::string_view reference,
                                     const ResourceUri &base) const {
  const ResourceResult<ResourceUri> uri = base.resolve(reference);
  if (!uri)
    return std::unexpected(uri.error());

  // A document may only reach the native filesystem if it came from there. The
  // check is on `base`, not on the resolved URI: an empty base means no
  // document at all -- C++ calling directly -- and code that can call this can
  // call fopen.
  if (uri->is_file()) {
    const NativeAccess access = native_access_.load(std::memory_order_relaxed);
    const bool from_native_document = base.empty() || base.is_file();
    const bool allowed =
        access == NativeAccess::Always ||
        (access == NativeAccess::SameOrigin && from_native_document);
    if (!allowed)
      return std::unexpected(ResourceError::Denied);
  }

  // The same test pointed outward, and defaulting the other way: a document
  // reaching the filesystem it came from is ordinary, while a document reaching
  // the network is a decision the application has to have made.
  if (uri->is_network()) {
    const NetworkAccess access = network_access_.load(std::memory_order_relaxed);
    const bool from_network_document = base.empty() || base.is_network();
    const bool allowed =
        access == NetworkAccess::Always ||
        (access == NetworkAccess::SameOrigin && from_network_document);
    if (!allowed)
      return std::unexpected(ResourceError::Denied);
  }

  return open(*uri);
}

void Resources::set_network_provider(std::shared_ptr<ResourceProvider> provider) {
  network_.store(std::move(provider));
}

void Resources::set_network_access(NetworkAccess access) {
  network_access_.store(access, std::memory_order_relaxed);
}

Resources::NetworkAccess Resources::network_access() const {
  return network_access_.load(std::memory_order_relaxed);
}

std::optional<std::string> Resources::native_path(const ResourceUri &uri) const {
  if (uri.empty())
    return std::nullopt;

  if (uri.is_file()) {
    std::error_code code;
    if (!std::filesystem::is_regular_file(std::string(uri.path()), code) || code)
      return std::nullopt;
    return std::string(uri.path());
  }

  // A cached copy of a remote resource is deliberately not reported as its
  // native path. Handing a font engine or the hot reloader a file the cache is
  // free to evict would make both of them wrong at a moment neither controls.
  if (uri.is_network())
    return std::nullopt;

  const std::shared_ptr<const Table> table = snapshot_();
  if (!table)
    return std::nullopt;

  for (const Mount &mount : table->mounts) {
    std::string_view remainder;
    if (!mount.claims(uri.path(), remainder))
      continue;
    if (std::optional<std::string> path = mount.provider->native_path(remainder))
      return path;
  }
  return std::nullopt;
}

std::optional<std::uint64_t> Resources::revision(const ResourceUri &uri) const {
  if (uri.empty())
    return std::nullopt;

  if (uri.is_file())
    return file_revision(std::string(uri.path()));

  // Whatever the provider makes of an entity tag or a last-modified date. A
  // provider that does not track them says so by returning nothing, which reads
  // as immutable -- the right answer for a URL nobody is watching.
  if (uri.is_network()) {
    const std::shared_ptr<ResourceProvider> network = network_.load();
    return network ? network->revision(uri.to_string()) : std::nullopt;
  }

  const std::shared_ptr<const Table> table = snapshot_();
  if (!table)
    return std::nullopt;

  // The first mount that both claims the path and holds the file answers, so a
  // revision tracks the same bytes `open` would return.
  for (const Mount &mount : table->mounts) {
    std::string_view remainder;
    if (!mount.claims(uri.path(), remainder))
      continue;
    if (std::optional<std::uint64_t> stamp = mount.provider->revision(remainder))
      return stamp;
    if (mount.provider->open(remainder))
      return std::nullopt; // it holds the file and says it never changes
  }
  return std::nullopt;
}

void Resources::set_native_access(NativeAccess access) {
  native_access_.store(access, std::memory_order_relaxed);
}

Resources::NativeAccess Resources::native_access() const {
  return native_access_.load(std::memory_order_relaxed);
}

} // namespace voidui
