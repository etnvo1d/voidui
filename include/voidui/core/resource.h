#pragma once

#include <atomic>
#include <cstddef>
#include <cstdint>
#include <expected>
#include <memory>
#include <mutex>
#include <optional>
#include <span>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

namespace voidui {

/// Immutable bytes, plus whatever keeps them alive.
///
/// The three places a resource can come from want three different ownership
/// stories, and the consumers -- FreeType in particular, which holds a pointer
/// into the buffer for the lifetime of the face -- want one. Bytes baked into
/// the executable are static and must not be copied; bytes read from a file are
/// owned outright; bytes inside a decompressed pack belong to the pack. A Blob
/// is a span plus an optional owner, so all three arrive as the same value and
/// copying one is a refcount bump rather than a memcpy.
class Blob {
public:
  Blob() = default;

  /// Bytes that outlive the process: generated arrays in .rdata. Nothing is
  /// owned and nothing is copied.
  static Blob borrow(std::span<const std::byte> bytes);

  static Blob own(std::vector<std::byte> data);

  /// The same, for the byte vectors the rest of the codebase already holds --
  /// decoded pixels, a font file read off disk.
  static Blob own(std::vector<std::uint8_t> data);

  /// A view into something already refcounted -- a slice of a pack file, a
  /// mapping. `owner` is kept alive as long as any copy of the Blob is.
  static Blob adopt(std::span<const std::byte> bytes,
                    std::shared_ptr<const void> owner);

  std::span<const std::byte> bytes() const { return bytes_; }
  const std::byte *data() const { return bytes_.data(); }
  std::size_t size() const { return bytes_.size(); }
  bool empty() const { return bytes_.empty(); }

  /// For text resources -- stylesheets, themes. No copy and no scan: the caller
  /// gets the raw bytes and decides what they mean.
  std::string_view text() const;

private:
  Blob(std::span<const std::byte> bytes, std::shared_ptr<const void> owner)
      : bytes_(bytes), owner_(std::move(owner)) {}

  std::span<const std::byte> bytes_;
  std::shared_ptr<const void> owner_;
};

enum class ResourceError {
  BadUri,     ///< unparseable, or a traversal that escapes its root
  NotMounted, ///< no provider claims this prefix
  NotFound,   ///< a provider claims the prefix but has no such entry
  Unreadable, ///< it exists and the read failed
  Denied,     ///< refused by policy; see Resources::set_native_access
};

std::string_view to_string(ResourceError error);

template <class T> using ResourceResult = std::expected<T, ResourceError>;

/// A scheme and a normalised path.
///
/// Three kinds of scheme exist. `res:` is the application's own namespace, the
/// one a stylesheet should name: it says nothing about where the bytes live, so
/// the same `res://fonts/Inter.ttf` reads from a directory during development
/// and from .rdata in a shipped build. `file:` is the native filesystem, which
/// exists because development tooling and the user's own configuration
/// directory are real paths, not resources. `http:` and `https:` are the
/// network, which is a scheme rather than a mount because the authority is part
/// of the name -- there is no table of hosts to consult, only a request to make.
///
/// A network URI keeps its authority as the first path segment, so
/// `https://example.com/a.png` normalises to the path `example.com/a.png` and
/// prints back as it arrived. Query strings ride along in the last segment,
/// which round-trips but is not parsed: this layer names bytes, and what a
/// server does with a query is between the caller and the server.
///
/// Both `res://fonts/x.ttf` and `res:fonts/x.ttf` parse; the first is the
/// spelling to use. Paths are normalised on construction -- separators unified,
/// `.` dropped, `..` applied -- and a `..` that would climb above the root is an
/// error rather than a clamp, so a traversal fails loudly instead of quietly
/// reading the wrong file.
class ResourceUri {
public:
  ResourceUri() = default;

  static ResourceResult<ResourceUri> parse(std::string_view text);

  /// Resolves a reference the way a stylesheet's `url()` should: an absolute
  /// reference (one carrying a scheme) is taken as it stands, and a relative one
  /// is resolved against this URI's directory, inheriting its scheme.
  ///
  /// Inheriting the scheme is what keeps an embedded stylesheet embedded. A
  /// sheet loaded from `res://theme/dark.vss` writing `url("../fonts/x.ttf")`
  /// gets `res://fonts/x.ttf` and never touches the disk, while the same sheet
  /// loaded from a loose file during development resolves beside itself.
  ResourceResult<ResourceUri> resolve(std::string_view reference) const;

  /// The directory this URI sits in -- the base for anything it references.
  ResourceUri base() const;

  std::string_view scheme() const { return scheme_; }
  std::string_view path() const { return path_; }
  bool is_resource() const { return scheme_ == "res"; }
  bool is_file() const { return scheme_ == "file"; }
  bool is_network() const { return scheme_ == "http" || scheme_ == "https"; }
  bool empty() const { return scheme_.empty(); }

  std::string to_string() const;

  /// The form to put in front of a person -- a diagnostic, a log line. A
  /// `file:` URI reads back as the plain path it names, because that is what
  /// the reader has open in an editor; anything else reads as its URI.
  std::string display() const;

  bool operator==(const ResourceUri &) const = default;

private:
  std::string scheme_;
  std::string path_;
};

/// One place bytes can come from, mounted under a prefix.
///
/// Paths reaching a provider are already normalised and already stripped of the
/// mount prefix, so a provider never parses a URI and never has to defend
/// against traversal.
class ResourceProvider {
public:
  virtual ~ResourceProvider() = default;

  virtual ResourceResult<Blob> open(std::string_view path) = 0;

  /// The real file behind `path`, when there is one.
  ///
  /// Not a convenience. DirectWrite and CoreText both want a font by file path,
  /// `FontFile` is a path plus a face index, and the hot reloader polls
  /// modification times -- all three keep working unchanged when a resource
  /// happens to be a loose file, and fall back to bytes when it is not.
  virtual std::optional<std::string> native_path(std::string_view path);

  /// Changes whenever the bytes change. `nullopt` means immutable, which is the
  /// honest answer for anything baked into the binary and the reason a release
  /// build's watcher ends up doing no work at all.
  virtual std::optional<std::uint64_t> revision(std::string_view path);
};

/// Loose files under `root`. Reports native paths and modification times, so
/// this is the provider that makes hot reload work.
std::shared_ptr<ResourceProvider> directory_provider(std::string root);

/// One entry of a generated resource table. `bytes` points into .rdata.
struct EmbeddedResource {
  std::string_view path;
  std::span<const std::byte> bytes;
};

/// Bytes compiled into the executable. `entries` must outlive the provider,
/// which is automatic for the table `voidui_add_resources` generates.
std::shared_ptr<ResourceProvider>
embedded_provider(std::span<const EmbeddedResource> entries);

/// Blobs registered at runtime -- an application that unpacks its own archive,
/// a test that wants a fixture without touching the disk.
class MemoryProvider : public ResourceProvider {
public:
  void add(std::string path, Blob blob);
  bool remove(std::string_view path);

  ResourceResult<Blob> open(std::string_view path) override;

private:
  std::vector<std::pair<std::string, Blob>> entries_;
};

/// The mount table: which provider answers for which prefix.
///
/// Mounts are matched longest prefix first, ties broken by descending priority.
/// That ordering is the whole point of the layer. A debug build mounts the
/// source `assets/` directory at a high priority over the embedded pack, so
/// editing a font or a stylesheet shows up on the next frame; a release build
/// mounts only the pack; neither the URIs nor any code naming them differs
/// between the two. The same mechanism lets an application ship a default theme
/// and let a directory of the user's shadow it.
///
/// Reads are safe from any thread -- images and fonts get decoded on the pool in
/// `voidui::async` -- and a mount taken while reads are in flight is published
/// atomically, with in-flight readers finishing against the table they started
/// with. Mounting is expected to happen at startup regardless.
class Resources {
public:
  Resources();
  ~Resources();

  Resources(const Resources &) = delete;
  Resources &operator=(const Resources &) = delete;

  /// The table a default lookup uses. An application wanting isolation -- tests,
  /// a plugin host -- can own a Resources of its own instead.
  static Resources &global();

  using MountId = std::uint64_t;

  /// `prefix` is a `res:` path prefix ("", "fonts/", "themes/"). An empty prefix
  /// claims everything that no longer prefix has claimed.
  MountId mount(std::string prefix, std::shared_ptr<ResourceProvider> provider,
                int priority = 0);
  bool unmount(MountId id);

  /// Opens a URI the caller built. This is C++ asking for something by name, so
  /// no access policy applies -- code that can call this can call fopen.
  ResourceResult<Blob> open(const ResourceUri &uri) const;

  /// Resolves `reference` against `base` and opens the result: the call a
  /// document makes for its own `url()`. Reference resolution is where a
  /// document's own origin is known, so this is the overload `NativeAccess`
  /// governs.
  ResourceResult<Blob> open(std::string_view reference,
                            const ResourceUri &base = {}) const;

  std::optional<std::string> native_path(const ResourceUri &uri) const;
  std::optional<std::uint64_t> revision(const ResourceUri &uri) const;

  /// Whether `file:` URIs resolve at all.
  ///
  /// A stylesheet is data, and in an application with themes or plugins it can
  /// be data the user supplied. Left unrestricted, a `url("file://...")` in such
  /// a sheet is an arbitrary file read. The default therefore hands the native
  /// filesystem only to documents that came from it: a loose sheet being edited
  /// during development reaches the fonts beside it, an embedded one reaches
  /// nothing.
  enum class NativeAccess {
    SameOrigin, ///< `file:` resolves only for documents loaded from `file:`
    Always,
    Never,
  };

  void set_native_access(NativeAccess access);
  NativeAccess native_access() const;

  /// Whether `http:` and `https:` URIs resolve at all.
  ///
  /// The same argument as NativeAccess, pointed outward. A stylesheet is data,
  /// and in an application with themes or plugins it can be data the user
  /// supplied; left unrestricted, a `url("https://...")` in one is an
  /// exfiltration channel and a tracking pixel. So a document reaches the
  /// network only when the application says it may, and the default is that it
  /// may not.
  ///
  /// This governs `open(reference, base)` -- what a document asks for. It does
  /// not govern `open(uri)`, which is C++ naming something directly, for the
  /// same reason NativeAccess does not: code that can call this can open a
  /// socket.
  enum class NetworkAccess {
    Never, ///< the default
    SameOrigin, ///< `http:` resolves only for documents loaded over `http:`
    Always,
  };

  void set_network_access(NetworkAccess access);
  NetworkAccess network_access() const;

  /// What answers for `http:` and `https:`. Null -- the default -- makes every
  /// network URI a NotMounted error, so an application that never sets one
  /// cannot reach the network by accident.
  ///
  /// The provider is handed the whole URI as its path, scheme and authority
  /// included, because unlike a mount there is nothing to strip: the name is
  /// the address.
  void set_network_provider(std::shared_ptr<ResourceProvider> provider);

private:
  struct Mount;
  struct Table;

  /// The table a read works against. Copy-on-write: a mount builds a new table
  /// and publishes it, so a reader never takes a lock and a reader already
  /// walking the old one stays valid. The mutex serialises writers against each
  /// other only -- mounting is a startup activity, reading is not.
  std::shared_ptr<const Table> snapshot_() const;

  std::atomic<std::shared_ptr<const Table>> table_;
  std::atomic<std::shared_ptr<ResourceProvider>> network_;
  std::atomic<NativeAccess> native_access_{NativeAccess::SameOrigin};
  std::atomic<NetworkAccess> network_access_{NetworkAccess::Never};
  std::mutex mount_mutex_;
  MountId next_mount_id_ = 1;
};

} // namespace voidui
