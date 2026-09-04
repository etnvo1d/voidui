#pragma once

#include <chrono>
#include <cstdint>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "voidui/core/style/parser.h"
#include "voidui/core/style/stylesheet.h"
#include "voidui/core/style/theme.h"

#ifndef VOIDUI_HOT_RELOAD
#ifdef NDEBUG
#define VOIDUI_HOT_RELOAD 0
#else
#define VOIDUI_HOT_RELOAD 1
#endif
#endif

namespace voidui {

/// Watches stylesheets and themes and rebuilds them when they change.
///
/// Polling on the UI thread rather than a watcher thread: a revision check on a
/// handful of documents is far cheaper than the synchronisation a background
/// thread would need, and it lands the reload at a point in the frame where
/// re-resolving the tree is safe.
///
/// What "changed" means is the resource layer's answer, not the filesystem's. A
/// document served from a directory reports its modification time; one baked
/// into the binary reports that it cannot change at all and is then never
/// polled again -- so mounting a source directory over a packed build is the
/// only thing standing between a shipped binary and a live-editing one.
///
/// The whole class compiles away when VOIDUI_HOT_RELOAD is 0 -- the parser, the
/// filesystem access and the source paths all leave the binary, which is why a
/// release build can ship pre-compiled stylesheets with no reader at all.
class StyleWatcher {
public:
  /// Called with the freshly built sheet whenever any watched file changes.
  /// A parse that produced no usable rules never reaches here: the previously
  /// applied sheet stays in place and the diagnostics are reported instead.
  using SheetCallback = std::function<void(std::shared_ptr<const StyleSheet>)>;
  using ThemeCallback = std::function<void(std::shared_ptr<const Theme>)>;
  using DiagnosticCallback =
      std::function<void(const std::vector<StyleDiagnostic> &)>;

  void watch_stylesheet(ResourceUri document,
                        StyleOrigin origin = StyleOrigin::User);
  void watch_stylesheet(std::string_view document,
                        StyleOrigin origin = StyleOrigin::User);
  void watch_theme(ResourceUri document);
  void watch_theme(std::string_view document);

  void on_stylesheet(SheetCallback callback) {
    sheet_callback_ = std::move(callback);
  }
  void on_theme(ThemeCallback callback) {
    theme_callback_ = std::move(callback);
  }
  void on_diagnostics(DiagnosticCallback callback) {
    diagnostic_callback_ = std::move(callback);
  }

  /// Minimum interval between revision checks.
  void set_poll_interval(std::chrono::milliseconds interval) {
    interval_ = interval;
  }
  std::chrono::milliseconds poll_interval() const { return interval_; }

  /// Call whenever the scheduler reaches the polling deadline. Returns true
  /// when something was reloaded.
  bool poll();

  /// Loads everything watched, ignoring revisions. Called once at startup so
  /// the same code path builds the initial sheet and every reload.
  bool reload_all();

  bool enabled() const { return VOIDUI_HOT_RELOAD != 0; }

private:
  struct Watched {
    ResourceUri document;
    StyleOrigin origin = StyleOrigin::User;
    bool is_theme = false;

    /// The revision last seen. Empty means either "not looked at yet" or "this
    /// document cannot change"; both are answered the same way -- leave it
    /// alone -- so the two never need telling apart.
    std::optional<std::uint64_t> revision;
  };

  bool rebuild_();

  std::vector<Watched> watched_;
  SheetCallback sheet_callback_;
  ThemeCallback theme_callback_;
  DiagnosticCallback diagnostic_callback_;

  std::chrono::milliseconds interval_{200};
  std::chrono::steady_clock::time_point last_poll_{};
};

} // namespace voidui
