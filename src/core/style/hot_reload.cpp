#include "voidui/core/style/hot_reload.h"

namespace voidui {

void StyleWatcher::watch_stylesheet(ResourceUri document, StyleOrigin origin) {
#if VOIDUI_HOT_RELOAD
  Watched entry;
  entry.document = std::move(document);
  entry.origin = origin;
  entry.is_theme = false;
  watched_.push_back(std::move(entry));
#else
  (void)document;
  (void)origin;
#endif
}

void StyleWatcher::watch_stylesheet(std::string_view document,
                                    StyleOrigin origin) {
  const ResourceResult<ResourceUri> uri = ResourceUri::parse(document);
  watch_stylesheet(uri ? *uri : ResourceUri{}, origin);
}

void StyleWatcher::watch_theme(ResourceUri document) {
#if VOIDUI_HOT_RELOAD
  Watched entry;
  entry.document = std::move(document);
  entry.is_theme = true;
  watched_.push_back(std::move(entry));
#else
  (void)document;
#endif
}

void StyleWatcher::watch_theme(std::string_view document) {
  const ResourceResult<ResourceUri> uri = ResourceUri::parse(document);
  watch_theme(uri ? *uri : ResourceUri{});
}

bool StyleWatcher::poll() {
#if VOIDUI_HOT_RELOAD
  const auto now = std::chrono::steady_clock::now();
  if (now - last_poll_ < interval_)
    return false;
  last_poll_ = now;

  const Resources &resources = Resources::global();

  bool dirty = false;
  for (Watched &entry : watched_) {
    const std::optional<std::uint64_t> revision =
        resources.revision(entry.document);
    // No revision means either that the document cannot change or that it is
    // momentarily unreadable -- a file being rewritten vanishes for an instant.
    // Neither is a reason to rebuild; a real edit shows up on the next poll.
    if (!revision)
      continue;
    if (entry.revision != revision) {
      entry.revision = revision;
      dirty = true;
    }
  }
  if (!dirty)
    return false;
  return rebuild_();
#else
  return false;
#endif
}

bool StyleWatcher::reload_all() {
#if VOIDUI_HOT_RELOAD
  const Resources &resources = Resources::global();
  for (Watched &entry : watched_)
    entry.revision = resources.revision(entry.document);
  return rebuild_();
#else
  return false;
#endif
}

bool StyleWatcher::rebuild_() {
#if VOIDUI_HOT_RELOAD
  auto sheet = std::make_shared<StyleSheet>();
  std::shared_ptr<Theme> theme;
  std::vector<StyleDiagnostic> diagnostics;
  bool any_sheet = false;

  for (const Watched &entry : watched_) {
    if (entry.is_theme) {
      StyleParser::ThemeResult result =
          StyleParser::parse_theme_document(entry.document);
      diagnostics.insert(diagnostics.end(), result.diagnostics.begin(),
                         result.diagnostics.end());
      if (result.theme) {
        // Later theme files layer over earlier ones rather than replacing
        // them, so a palette file and an overrides file can be watched
        // together.
        if (theme)
          result.theme->set_base(theme);
        theme = std::move(result.theme);
      }
      continue;
    }

    StyleParser::Result result =
        StyleParser::parse_document(entry.document, entry.origin);
    diagnostics.insert(diagnostics.end(), result.diagnostics.begin(),
                       result.diagnostics.end());
    if (result.sheet && result.sheet->size() > 0) {
      sheet->append(*result.sheet);
      any_sheet = true;
    }
  }

  if (!diagnostics.empty() && diagnostic_callback_)
    diagnostic_callback_(diagnostics);

  bool applied = false;
  // A file that produced nothing usable leaves the running style alone. That
  // is the difference between a typo showing up as a diagnostic and a typo
  // blanking the window.
  if (any_sheet && sheet_callback_) {
    sheet_callback_(std::move(sheet));
    applied = true;
  }
  if (theme && theme_callback_) {
    theme_callback_(std::move(theme));
    applied = true;
  }
  return applied;
#else
  return false;
#endif
}

} // namespace voidui
