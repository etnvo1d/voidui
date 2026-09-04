#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "voidui/core/geometry.h"
#include "voidui/core/resource.h"
#include "voidui/paint/font_provider.h"

namespace voidui {

struct FontImpl;
struct FontData;

/// One positioned glyph within a run. Offsets are in logical pixels, relative
/// to the run's origin on the baseline.
struct PositionedGlyph {
  std::uint32_t id = 0;

  /// Byte offset in the source string this glyph came from. Several glyphs can
  /// share one cluster (a decomposed accent) and one glyph can span several
  /// (a ligature), so this is where a caret may sit -- and where a line may be
  /// broken.
  std::uint32_t cluster = 0;

  Point<float> offset{0.0f, 0.0f};
};

class Font;

/// A shaped span of text drawn with a single face.
///
/// Shaping is independent of display scale, so a run laid out once stays valid
/// when the window moves to a monitor with a different DPI -- only the
/// rasterised glyphs change.
struct GlyphRun {
  std::shared_ptr<const Font> font;
  std::vector<PositionedGlyph> glyphs;
  float advance = 0.0f;
};

/// A typeface at a particular size.
///
/// HarfBuzz reads the font tables directly rather than going through an
/// FT_Face, which keeps shaping independent of whatever pixel size the
/// rasteriser is currently set to.
class Font : public std::enable_shared_from_this<Font> {
public:
  /// `face_index` selects a face inside a TrueType collection; MSYH.TTC and the
  /// other CJK system fonts routinely need a value other than zero.
  static std::shared_ptr<Font> from_file(const std::string &path, float size,
                                         int face_index = 0);
  static std::shared_ptr<Font> from_memory(std::vector<std::uint8_t> data,
                                           float size, int face_index = 0);

  /// Reads a face already in memory -- a packed resource, most often.
  ///
  /// The blob is held for the life of the face, so bytes baked into the binary
  /// are used where they lie. Two calls with the same blob share one face, the
  /// way two calls naming one file do.
  static std::shared_ptr<Font> from_blob(Blob bytes, float size,
                                         int face_index = 0);

  /// Tries each path in turn; useful for naming a few likely system fonts.
  static std::shared_ptr<Font>
  from_first_available(const std::vector<std::string> &paths, float size);

  ~Font();

  Font(const Font &) = delete;
  Font &operator=(const Font &) = delete;

  float size() const { return size_; }
  float ascent() const;
  float descent() const;
  float line_height() const;

  GlyphRun shape(std::string_view utf8) const;
  float measure(std::string_view utf8) const;

  /// Whether this face carries colour glyphs (an emoji font, typically).
  bool has_color_glyphs() const;

  /// Distinguishes one face from another in cache keys.
  std::uint64_t id() const { return id_; }

  FontImpl *impl() const { return impl_.get(); }

private:
  Font(std::unique_ptr<FontImpl> impl, float size, std::uint64_t id);

  /// Wraps a shared face at one size. The face itself is reused across sizes;
  /// only the shaper state is per-size.
  static std::shared_ptr<Font> from_data_(std::shared_ptr<FontData> data,
                                          float size);

  std::unique_ptr<FontImpl> impl_;
  float size_ = 0.0f;
  std::uint64_t id_ = 0;
};

/// A base font together with whatever the system substitutes for text that font
/// cannot render.
///
/// This is what an application normally holds. Asking for `system_ui` gets the
/// platform's own interface font -- Segoe UI on an English Windows, Microsoft
/// YaHei UI on a Chinese one -- and shaping walks the platform's fallback chain
/// so mixed-script text needs no per-script configuration.
class FontStack {
public:
  /// The platform's interface font. `family` overrides which family to start
  /// from; `locale` overrides the user's, which decides between the Chinese,
  /// Japanese and Traditional Chinese forms of shared codepoints.
  static std::shared_ptr<FontStack>
  system_ui(float size, std::string locale = {},
            FontWeight weight = FontWeight::Normal);
  static std::shared_ptr<FontStack>
  create(std::string family, float size, std::string locale = {},
         FontWeight weight = FontWeight::Normal);

  /// Resolves a `font-family` list: each family in turn until one is found,
  /// falling back to the platform's UI font when none is.
  ///
  /// This is family fallback, which is a different thing from the per-glyph
  /// fallback `shape` does. This one picks the base face; that one covers
  /// whatever the base face turns out to have no glyphs for.
  static std::shared_ptr<FontStack>
  create(const FontFamilyList &families, float size, std::string locale = {},
         FontWeight weight = FontWeight::Normal);

  /// Reuses one stack for equal family, locale, size, and weight tuples.
  static std::shared_ptr<FontStack>
  cached(std::string family, float size, std::string locale = {},
         FontWeight weight = FontWeight::Normal);

  static std::shared_ptr<FontStack>
  cached(const FontFamilyList &families, float size, std::string locale = {},
         FontWeight weight = FontWeight::Normal);

  /// Wraps a face that was loaded directly. Fallback still applies, using the
  /// platform chain, with this face preferred.
  static std::shared_ptr<FontStack>
  from_font(std::shared_ptr<Font> font, std::string family = {},
            std::string locale = {}, FontWeight weight = FontWeight::Normal);

  /// Shapes `utf8`, splitting into runs wherever the face has to change. Every
  /// run's glyph offsets are relative to the same origin, so a caller draws
  /// them all at one point.
  std::vector<GlyphRun> shape(std::string_view utf8) const;
  float measure(std::string_view utf8) const;

  const std::shared_ptr<Font> &base() const { return base_; }
  const std::string &family() const { return family_; }
  const std::string &locale() const { return locale_; }
  FontWeight weight() const { return weight_; }

  float size() const { return size_; }
  float ascent() const;
  float descent() const;
  float line_height() const;

private:
  std::shared_ptr<Font> face_for_(const FontFile &file) const;

  std::string family_;
  std::string locale_;
  float size_ = 0.0f;
  FontWeight weight_ = FontWeight::Normal;
  std::shared_ptr<Font> base_;

  // Faces the fallback chain has produced so far. A UI touches a handful of
  // scripts at most, so this stays tiny.
  mutable std::unordered_map<std::string, std::shared_ptr<Font>> faces_;
};

} // namespace voidui
