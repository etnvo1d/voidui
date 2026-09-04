#pragma once

#include <string>

#include "voidui/core/widget_tree.h"
#include "voidui/paint/font_registry.h"

namespace voidui {

struct WindowMetrics {
  int window_width, window_height;
  int pixel_width, pixel_height; // framebuffer
  float display_scale;
  float logical_width, logical_height;
  float window_to_logical_x, window_to_logical_y;
};

class Window {
public:
  Window(std::string title, int width, int height);

  template <WidgetClass T> void run(T &&root) {
    run(transfer_widget(std::forward<T>(root)));
  }
  void run(std::unique_ptr<Widget> root);

  // -- Styling ---------------------------------------------------------------

  /// Installs a sheet, and puts its `@font-face` rules into the font registry.
  /// Registration happens here rather than at parse time so that a sheet which
  /// is never installed never changes what the application draws with.
  void set_stylesheet(std::shared_ptr<const StyleSheet> sheet) {
    if (sheet)
      register_font_faces(*sheet);
    sheet_ = std::move(sheet);
  }
  void set_theme(std::shared_ptr<const Theme> theme) {
    theme_ = std::move(theme);
  }

  /// Loads the files now and, in a build with VOIDUI_HOT_RELOAD on, re-reads
  /// them whenever they change. Either path may be empty. In a release build
  /// this reduces to the initial load with no watching and no parser linked.
  void watch_styles(const std::string &stylesheet_path,
                    const std::string &theme_path = {});

  /// Where parse diagnostics go. Defaults to printing to stderr.
  void on_style_diagnostics(StyleWatcher::DiagnosticCallback callback) {
    watcher_.on_diagnostics(std::move(callback));
  }

private:
  std::string title_;
  int width_, height_;

  std::shared_ptr<const StyleSheet> sheet_;
  std::shared_ptr<const Theme> theme_;
  StyleWatcher watcher_;
  bool watching_ = false;
};

} // namespace voidui