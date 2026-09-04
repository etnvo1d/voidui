// Stand-in for platforms whose provider is not written yet.
//
// macOS would use Core Text (CTFontCreateUIFontForLanguage for the UI family,
// CTFontCreateForString for the cascade); Linux would use fontconfig
// (FcMatch for the default sans, FcFontSort plus FcCharSetHasChar to walk the
// fallback list). Both expose file paths and face indices, so they fit the same
// interface as the DirectWrite backend.

#include "voidui/paint/font_provider.h"

namespace voidui {

namespace {

class NullProvider final : public FontProvider {
public:
  bool available() const override { return false; }
  std::string default_ui_family() override { return {}; }
  std::string default_locale() override { return "en-us"; }
  std::optional<FontFile> resolve(std::string_view, FontWeight) override {
    return std::nullopt;
  }
  std::optional<FontRun> fallback(std::string_view, std::string_view,
                                  std::string_view, FontWeight) override {
    return std::nullopt;
  }
};

} // namespace

FontProvider &FontProvider::system() {
  static NullProvider provider;
  return provider;
}

} // namespace voidui
