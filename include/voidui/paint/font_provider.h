#pragma once

#include <cstddef>
#include <optional>
#include <string>
#include <string_view>

#include "voidui/core/typography.h"

namespace voidui {

/// A font file on disk, plus which face inside it. TrueType collections such as
/// MSYH.TTC hold several faces, and the system fallback routinely names one
/// that is not the first.
struct FontFile {
  std::string path;
  int face_index = 0;

  bool operator==(const FontFile &other) const {
    return face_index == other.face_index && path == other.path;
  }
};

/// One step of a fallback walk: the font to use, and how far it gets.
struct FontRun {
  FontFile file;
  std::size_t length = 0; ///< bytes of the UTF-8 input this font covers
};

/// Answers "which font should I use?" by asking the operating system, so an
/// application inherits the platform's font settings and its fallback chain
/// instead of hardcoding either.
///
/// Both halves matter and they are separate problems. Discovering the UI font
/// gets the right typeface for the user's locale -- Segoe UI on an English
/// Windows, Microsoft YaHei UI on a Chinese one. Fallback is what keeps text
/// the chosen font cannot render from turning into tofu: Segoe UI genuinely has
/// no CJK glyphs, and Windows itself substitutes another face for them.
class FontProvider {
public:
  virtual ~FontProvider() = default;

  /// The family the platform uses for interface text.
  virtual std::string default_ui_family() = 0;

  /// The user's locale, in BCP-47 form ("zh-CN"). Load-bearing for fallback.
  virtual std::string default_locale() = 0;

  /// The family a CSS generic name stands for here: `sans-serif`, `serif`,
  /// `monospace`, `cursive`, `fantasy`, `system-ui`, `ui-sans-serif`,
  /// `ui-serif`, `ui-monospace`, `emoji`, `math`.
  ///
  /// No platform font API answers this -- DirectWrite has no notion of
  /// "monospace" -- so it is a table per platform, the same way every browser
  /// engine does it. Empty means this platform offers nothing for that generic,
  /// and the caller moves on to the next family in the list.
  virtual std::string generic_family(std::string_view generic) {
    (void)generic;
    return {};
  }

  virtual std::optional<FontFile>
  resolve(std::string_view family, FontWeight weight = FontWeight::Normal) = 0;

  /// The font covering the start of `utf8`, and how much of it that font
  /// covers.
  ///
  /// `locale` is not optional in practice: Han unification means U+4F60 U+597D
  /// is valid Chinese *and* valid Japanese, and the locale is the only thing
  /// that decides whether the reader gets 你好 in Chinese or Japanese shapes.
  virtual std::optional<FontRun>
  fallback(std::string_view utf8, std::string_view base_family,
           std::string_view locale, FontWeight weight = FontWeight::Normal) = 0;

  /// True when this provider actually talks to a platform. The fallback stub
  /// used on platforms without an implementation reports false.
  virtual bool available() const = 0;

  /// The platform provider for this build.
  static FontProvider &system();
};

} // namespace voidui
