#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <vector>

#include "voidui/paint/font.h"

namespace voidui {

/// A shaped and wrapped paragraph.
///
/// Building one is the most expensive thing this renderer does: shaping crosses
/// into HarfBuzz, and resolving the fallback chain crosses into the platform's
/// font machinery. So a layout is built once, kept behind a shared_ptr, and
/// handed to the painter by reference every frame -- drawing it copies no
/// glyphs and allocates nothing.
class TextLayout {
public:
  struct Line {
    std::uint32_t first_run = 0;
    std::uint32_t run_count = 0;
    float baseline = 0.0f; ///< distance from the layout's top to this baseline
    float width = 0.0f;
    std::uint32_t begin = 0; ///< byte range of the source this line covers
    std::uint32_t end = 0;
    std::uint32_t first_caret = 0;
    std::uint32_t caret_count = 0;
  };

  struct Caret {
    std::uint32_t offset = 0; ///< UTF-8 byte offset / HarfBuzz cluster edge
    float x = 0.0f;
  };

  /// `max_width` of zero or less means no wrapping; only explicit newlines
  /// break the text. `max_lines` of zero means unlimited.
  ///
  /// `device_scale` rounds the line box -- and only the line box -- onto the
  /// device pixel grid, so that stacked lines sit a whole number of pixels
  /// apart. Shaping stays scale-independent: glyph advances are untouched.
  static std::shared_ptr<const TextLayout>
  build(std::shared_ptr<FontStack> fonts, std::string text, float max_width,
        TextAlign align = TextAlign::Left, int max_lines = 0,
        float device_scale = 1.0f, float line_height = 0.0f);

  const std::vector<GlyphRun> &runs() const { return runs_; }
  const std::vector<Line> &lines() const { return lines_; }
  const std::string &text() const { return text_; }

  /// Maps a point in layout-local coordinates to the nearest legal cluster
  /// edge. The caret table is compiled with shaping, so pointer motion does a
  /// binary search and never measures or reshapes text.
  std::uint32_t hit_test(Point<float> point) const;

  /// Returns one layout-local highlight rectangle for a line, if the selected
  /// byte range intersects it. A caller can iterate lines without allocating a
  /// temporary rectangle vector on every paint.
  bool selection_rect(std::size_t line, std::uint32_t begin, std::uint32_t end,
                      Rect<float> &out) const;

  /// Returns the zero-width caret for a UTF-8 byte offset in layout space.
  bool caret_rect(std::uint32_t offset, Rect<float> &out) const;

  std::pair<std::uint32_t, std::uint32_t> word_at(std::uint32_t offset) const;

  /// Width of the widest line, and the total height across all of them.
  Size<float> size() const { return size_; }

  /// The width this layout was wrapped to, so a caller can tell whether a new
  /// constraint requires rebuilding.
  float wrap_width() const { return wrap_width_; }

  /// The scale the line box was rounded to, so a caller can tell whether a
  /// monitor change requires rebuilding.
  float device_scale() const { return device_scale_; }
  /// Resolved line box in logical pixels. Zero is only possible for an empty
  /// or missing font.
  float line_height() const { return line_height_; }
  TextAlign align() const { return align_; }
  int max_lines() const { return max_lines_; }
  const std::shared_ptr<FontStack> &fonts() const { return fonts_; }

  /// How many layouts have been built since the process started.
  ///
  /// A diagnostic, and the only reliable way to tell whether a caller's caching
  /// actually works: this should climb when text or geometry changes and sit
  /// still otherwise. If it tracks the frame rate, something is reshaping every
  /// frame.
  static std::uint64_t builds_performed();

private:
  std::shared_ptr<FontStack> fonts_;
  std::string text_;
  std::vector<GlyphRun> runs_;
  std::vector<Line> lines_;
  std::vector<Caret> carets_;
  Size<float> size_{0.0f, 0.0f};
  float wrap_width_ = 0.0f;
  float device_scale_ = 1.0f;
  float line_height_ = 0.0f;
  TextAlign align_ = TextAlign::Left;
  int max_lines_ = 0;
};

} // namespace voidui
