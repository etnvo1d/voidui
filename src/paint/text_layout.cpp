#include "voidui/paint/text_layout.h"

#include "voidui/core/pixel_snap.h"

#include <SDL3/SDL_log.h>
#include <SDL3/SDL_stdinc.h>

#include <algorithm>
#include <atomic>
#include <cmath>
#include <limits>
#include <string_view>

namespace voidui {

namespace {

struct Codepoint {
  char32_t value = 0;
  std::size_t width = 1; ///< bytes consumed
};

Codepoint decode(std::string_view text, std::size_t at) {
  if (at >= text.size())
    return {};

  const auto byte = [&](std::size_t i) {
    return static_cast<unsigned char>(text[i]);
  };

  const unsigned char lead = byte(at);
  if (lead < 0x80)
    return {lead, 1};

  const auto continuation = [&](std::size_t i) {
    return at + i < text.size() ? static_cast<char32_t>(byte(at + i) & 0x3F)
                                : 0;
  };

  if ((lead & 0xE0) == 0xC0)
    return {static_cast<char32_t>(((lead & 0x1F) << 6) | continuation(1)), 2};
  if ((lead & 0xF0) == 0xE0)
    return {static_cast<char32_t>(((lead & 0x0F) << 12) |
                                  (continuation(1) << 6) | continuation(2)),
            3};
  if ((lead & 0xF8) == 0xF0)
    return {static_cast<char32_t>(((lead & 0x07) << 18) |
                                  (continuation(1) << 12) |
                                  (continuation(2) << 6) | continuation(3)),
            4};

  return {lead, 1};
}

bool is_space(char32_t c) {
  return c == U' ' || c == U'\t' || c == 0x3000 /* ideographic space */;
}

/// Scripts written without spaces, which may therefore break between any two
/// characters.
bool is_ideograph(char32_t c) {
  return (c >= 0x1100 && c <= 0x11FF) || // Hangul Jamo
         (c >= 0x2E80 &&
          c <= 0x9FFF) || // CJK radicals through Unified Ideographs
         (c >= 0xA000 && c <= 0xA4CF) || // Yi
         (c >= 0xAC00 && c <= 0xD7AF) || // Hangul syllables
         (c >= 0xF900 && c <= 0xFAFF) || // CJK compatibility
         (c >= 0xFF00 && c <= 0xFF60) || // fullwidth forms
         (c >= 0x20000 && c <= 0x3FFFD); // CJK extensions
}

/// Characters that may not start a line -- closing brackets and the CJK
/// punctuation that would otherwise be orphaned there.
bool forbids_line_start(char32_t c) {
  switch (c) {
  case U'、':
  case U'。':
  case U'，':
  case U'．':
  case U'：':
  case U'；':
  case U'！':
  case U'？':
  case U'）':
  case U'］':
  case U'｝':
  case U'」':
  case U'』':
  case U'〉':
  case U'》':
  case U'”':
  case U'’':
  case U')':
  case U']':
  case U'}':
  case U',':
  case U'.':
  case U';':
  case U':':
  case U'!':
  case U'?':
    return true;
  default:
    return false;
  }
}

/// Characters that may not end a line.
bool forbids_line_end(char32_t c) {
  switch (c) {
  case U'（':
  case U'［':
  case U'｛':
  case U'「':
  case U'『':
  case U'〈':
  case U'《':
  case U'“':
  case U'‘':
  case U'(':
  case U'[':
  case U'{':
    return true;
  default:
    return false;
  }
}

/// Whether a line may break between `prev` and `next`.
///
/// This is not UAX #14 -- that is a large table-driven algorithm with dozens of
/// classes. It covers the two cases interface text actually meets: breaking
/// after spaces, and breaking between ideographs, which carry no spaces at all.
bool can_break_between(char32_t prev, char32_t next) {
  if (prev == 0)
    return false;
  if (is_space(next))
    return false;
  if (forbids_line_start(next) || forbids_line_end(prev))
    return false;
  if (is_space(prev))
    return true;

  return is_ideograph(prev) || is_ideograph(next);
}

enum class WordClass : std::uint8_t { Space, Word, Other };

WordClass word_class(char32_t value) {
  if (is_space(value) || value == U'\n' || value == U'\r')
    return WordClass::Space;
  if (is_ideograph(value))
    return WordClass::Other;
  if ((value >= U'a' && value <= U'z') || (value >= U'A' && value <= U'Z') ||
      (value >= U'0' && value <= U'9') || value == U'_' || value >= 0x80)
    return WordClass::Word;
  return WordClass::Other;
}

std::size_t previous_codepoint(std::string_view text, std::size_t offset) {
  if (offset == 0)
    return 0;
  std::size_t result = offset - 1;
  while (result > 0 &&
         (static_cast<unsigned char>(text[result]) & 0xC0) == 0x80)
    --result;
  return result;
}

std::atomic<std::uint64_t> g_builds{0};

/// Set VOIDUI_LOG_TEXT=1 to see every build. A window sitting still should
/// print nothing after the first frame; if this tracks the frame rate, a caller
/// is reshaping text it could have kept.
bool logging_enabled() {
  static const bool enabled = [] {
    const char *env = SDL_getenv("VOIDUI_LOG_TEXT");
    return env && SDL_strcmp(env, "0") != 0;
  }();
  return enabled;
}

} // namespace

std::uint64_t TextLayout::builds_performed() {
  return g_builds.load(std::memory_order_relaxed);
}

std::shared_ptr<const TextLayout>
TextLayout::build(std::shared_ptr<FontStack> fonts, std::string text,
                  float max_width, TextAlign align, int max_lines,
                  float device_scale, float requested_line_height) {
  // Shaping is the expensive step; a caller that lands here every frame has a
  // caching problem, and this log is how it announces itself.
  const std::uint64_t count =
      g_builds.fetch_add(1, std::memory_order_relaxed) + 1;
  if (logging_enabled()) {
    SDL_Log("voidui: TextLayout::build #%llu  (%zu bytes, wrap %.1f)",
            static_cast<unsigned long long>(count), text.size(),
            static_cast<double>(max_width));
  }

  auto layout = std::shared_ptr<TextLayout>(new TextLayout());
  layout->fonts_ = std::move(fonts);
  layout->text_ = std::move(text);
  layout->wrap_width_ = max_width;
  layout->align_ = align;
  layout->max_lines_ = max_lines;
  layout->device_scale_ = device_scale;

  if (!layout->fonts_ || layout->text_.empty())
    return layout;

  const std::string &source = layout->text_;

  // Rounded onto the device pixel grid. These come from HarfBuzz in 26.6 fixed
  // point, so at 16px a face typically reports something like 21.28125 -- and a
  // fractional pitch is what makes stacked lines land on different sides of a
  // pixel boundary from one another. The renderer rounds each baseline it is
  // handed; if the spacing between them is fractional, that rounding lands
  // unevenly and the gaps alternate between two values, with the pattern
  // shifting every time the block scrolls. Rounding the pitch once here is the
  // only place it can be fixed: by the time a baseline reaches the renderer it
  // is an opaque offset with no line index attached.
  //
  // Only the line box is rounded. Glyph advances are left alone, so shaping
  // stays independent of the display.
  const float natural_line_height = layout->fonts_->line_height();
  const float line_height =
      snap_to_pixel(requested_line_height > 0.0f ? requested_line_height
                                                 : natural_line_height,
                    device_scale);
  layout->line_height_ = line_height;
  // Split custom leading above and below the face's natural line box. This
  // keeps glyphs vertically centred when authors make a roomier line box.
  const float ascent = snap_to_pixel(
      layout->fonts_->ascent() + (line_height - natural_line_height) * 0.5f,
      device_scale);

  // The paragraph is shaped once and then sliced at the break points, rather
  // than reshaping every line. Shaping context does not cross a space or an
  // ideograph boundary in any script this handles, so the two agree -- and one
  // shaping pass is the difference between a layout that can be rebuilt on a
  // resize and one that cannot.
  const std::vector<GlyphRun> shaped = layout->fonts_->shape(source);
  if (shaped.empty())
    return layout;

  // x position of each source byte, taken from the glyph that starts its
  // cluster. Bytes inside a cluster inherit the position of its start.
  std::vector<float> offset_x(source.size() + 1,
                              std::numeric_limits<float>::quiet_NaN());
  float total_advance = 0.0f;
  for (const GlyphRun &run : shaped) {
    for (const PositionedGlyph &glyph : run.glyphs) {
      if (glyph.cluster < offset_x.size())
        offset_x[glyph.cluster] = glyph.offset.x;
    }
    total_advance = std::max(total_advance, run.advance);
  }

  // Runs are laid end to end, so the paragraph's advance is the last run's
  // right edge; the per-run advances are relative to their own origins.
  float paragraph_advance = 0.0f;
  for (const GlyphRun &run : shaped)
    paragraph_advance += run.advance;
  offset_x[source.size()] = paragraph_advance;

  float last = 0.0f;
  for (float &x : offset_x) {
    if (std::isnan(x))
      x = last;
    else
      last = x;
  }

  // --- find the break points ---
  struct Break {
    std::size_t begin = 0;
    std::size_t end = 0; ///< excludes the trailing newline, if any
  };

  std::vector<Break> breaks;
  const bool wrapping = max_width > 0.0f;

  std::size_t line_begin = 0;
  std::size_t candidate = 0; ///< most recent legal break position on this line
  char32_t previous = 0;
  std::size_t at = 0;

  while (at < source.size()) {
    const Codepoint cp = decode(source, at);

    if (cp.value == U'\n') {
      breaks.push_back({line_begin, at});
      line_begin = at + cp.width;
      candidate = 0;
      previous = 0;
      at += cp.width;
      continue;
    }

    if (can_break_between(previous, cp.value))
      candidate = at;

    if (wrapping && at > line_begin &&
        offset_x[at + cp.width] - offset_x[line_begin] > max_width) {
      // Break at the last opportunity; with none on this line (one very long
      // word) break right here so the text still advances.
      const std::size_t cut = candidate > line_begin ? candidate : at;
      breaks.push_back({line_begin, cut});
      line_begin = cut;

      // Spaces at a wrap point belong to the line that ended.
      while (line_begin < source.size()) {
        const Codepoint skip = decode(source, line_begin);
        if (!is_space(skip.value))
          break;
        line_begin += skip.width;
      }

      candidate = 0;
      previous = 0;
      at = std::max(line_begin, at);
      continue;
    }

    previous = cp.value;
    at += cp.width;
  }

  if (line_begin <= source.size())
    breaks.push_back({line_begin, source.size()});

  if (max_lines > 0 && breaks.size() > static_cast<std::size_t>(max_lines))
    breaks.resize(static_cast<std::size_t>(max_lines));

  // --- slice the shaped runs into lines ---
  float widest = 0.0f;
  for (std::size_t index = 0; index < breaks.size(); ++index) {
    const Break &br = breaks[index];
    const float baseline = ascent + static_cast<float>(index) * line_height;
    const float origin = offset_x[br.begin];
    const float width = offset_x[br.end] - origin;

    Line line;
    line.first_run = static_cast<std::uint32_t>(layout->runs_.size());
    line.baseline = baseline;
    line.width = std::max(width, 0.0f);
    line.begin = static_cast<std::uint32_t>(br.begin);
    line.end = static_cast<std::uint32_t>(br.end);

    for (const GlyphRun &run : shaped) {
      GlyphRun slice;
      slice.font = run.font;

      for (const PositionedGlyph &glyph : run.glyphs) {
        if (glyph.cluster < br.begin || glyph.cluster >= br.end)
          continue;

        PositionedGlyph moved = glyph;
        moved.offset.x -= origin;
        moved.offset.y += baseline;
        slice.glyphs.push_back(moved);
      }

      if (!slice.glyphs.empty()) {
        slice.advance = line.width;
        layout->runs_.push_back(std::move(slice));
        line.run_count++;
      }
    }

    widest = std::max(widest, line.width);
    layout->lines_.push_back(line);
  }

  // --- alignment shifts whole lines, so it costs one pass over the glyphs ---
  //
  // Lines are aligned against the widest line, not against the wrap limit: the
  // layout then describes a self-contained block whose width is its own, and
  // placing that block inside a wider box stays the caller's decision.
  if (align != TextAlign::Left) {
    const float box = widest;
    for (const Line &line : layout->lines_) {
      const float slack = box - line.width;
      const float shift = align == TextAlign::Center ? slack * 0.5f : slack;
      if (shift == 0.0f)
        continue;

      for (std::uint32_t i = 0; i < line.run_count; ++i) {
        for (PositionedGlyph &glyph : layout->runs_[line.first_run + i].glyphs)
          glyph.offset.x += shift;
      }
    }
  }

  // Retain only legal caret edges, not the byte-wide offset table used while
  // wrapping. A cluster costs eight bytes and makes every later pointer move a
  // binary search with no shaping, measuring, or temporary allocation.
  for (std::size_t index = 0; index < layout->lines_.size(); ++index) {
    Line &line = layout->lines_[index];
    const float slack = widest - line.width;
    const float shift = align == TextAlign::Center
                            ? slack * 0.5f
                            : (align == TextAlign::Right ? slack : 0.0f);
    line.first_caret = static_cast<std::uint32_t>(layout->carets_.size());

    float previous_x = std::numeric_limits<float>::quiet_NaN();
    std::size_t position = line.begin;
    while (position < line.end) {
      const float x = offset_x[position] - offset_x[line.begin] + shift;
      if (std::isnan(previous_x) || x != previous_x) {
        layout->carets_.push_back({static_cast<std::uint32_t>(position), x});
        previous_x = x;
      }
      position += decode(source, position).width;
    }

    const float end_x = line.width + shift;
    if (layout->carets_.size() == line.first_caret || end_x != previous_x) {
      layout->carets_.push_back({line.end, end_x});
    } else {
      // Preserve the right byte edge of a zero-width final cluster.
      layout->carets_.back().offset = line.end;
    }
    line.caret_count =
        static_cast<std::uint32_t>(layout->carets_.size()) - line.first_caret;
  }

  layout->size_ = Size<float>(
      widest, layout->lines_.empty()
                  ? 0.0f
                  : static_cast<float>(layout->lines_.size()) * line_height);

  if (logging_enabled()) {
    SDL_Log("  -> %zu lines, widest %.1f, limit %.1f", layout->lines_.size(),
            static_cast<double>(widest), static_cast<double>(max_width));
    for (std::size_t i = 0; i < layout->lines_.size(); ++i) {
      const Line &line = layout->lines_[i];
      SDL_Log("     line %2zu  width %7.1f  %s  bytes [%u,%u)", i,
              static_cast<double>(line.width),
              (wrapping && line.width > max_width + 0.01f) ? "OVERFLOW"
                                                           : "ok    ",
              line.begin, line.end);
    }
  }

  return layout;
}

std::uint32_t TextLayout::hit_test(Point<float> point) const {
  if (lines_.empty() || carets_.empty())
    return 0;

  const float line_height = size_.height / static_cast<float>(lines_.size());
  const int requested_line =
      line_height > 0.0f ? static_cast<int>(std::floor(point.y / line_height))
                         : 0;
  const std::size_t line_index = static_cast<std::size_t>(
      std::clamp(requested_line, 0, static_cast<int>(lines_.size()) - 1));
  const Line &line = lines_[line_index];
  const Caret *begin = carets_.data() + line.first_caret;
  const Caret *end = begin + line.caret_count;

  if (point.x <= begin->x)
    return begin->offset;
  if (point.x >= (end - 1)->x)
    return (end - 1)->offset;

  const Caret *right =
      std::lower_bound(begin, end, point.x,
                       [](const Caret &caret, float x) { return caret.x < x; });
  const Caret *left = right - 1;
  return point.x < (left->x + right->x) * 0.5f ? left->offset : right->offset;
}

bool TextLayout::selection_rect(std::size_t line_index, std::uint32_t begin,
                                std::uint32_t end, Rect<float> &out) const {
  if (line_index >= lines_.size() || begin == end)
    return false;
  if (begin > end)
    std::swap(begin, end);

  const Line &line = lines_[line_index];
  if (end <= line.begin || begin >= line.end || line.caret_count == 0)
    return false;

  const std::uint32_t first = std::max(begin, line.begin);
  const std::uint32_t last = std::min(end, line.end);
  if (first >= last)
    return false;

  const Caret *carets = carets_.data() + line.first_caret;
  const Caret *carets_end = carets + line.caret_count;
  auto x_for = [&](std::uint32_t offset, bool trailing) {
    const Caret *position =
        std::lower_bound(carets, carets_end, offset,
                         [](const Caret &caret, std::uint32_t value) {
                           return caret.offset < value;
                         });
    if (position == carets_end)
      return (carets_end - 1)->x;
    if (position->offset == offset || position == carets)
      return position->x;
    return trailing ? position->x : (position - 1)->x;
  };

  const float left = x_for(first, false);
  const float right = x_for(last, true);
  const float line_height = size_.height / static_cast<float>(lines_.size());
  out = Rect<float>(left, static_cast<float>(line_index) * line_height,
                    std::max(right - left, 0.0f), line_height);
  return out.size.width > 0.0f && out.size.height > 0.0f;
}

bool TextLayout::caret_rect(std::uint32_t offset, Rect<float> &out) const {
  if (lines_.empty() || carets_.empty())
    return false;

  const float line_height = size_.height / static_cast<float>(lines_.size());
  std::size_t line_index = lines_.size() - 1;
  for (std::size_t i = 0; i < lines_.size(); ++i) {
    if (offset <= lines_[i].end) {
      line_index = i;
      break;
    }
  }

  const Line &line = lines_[line_index];
  const Caret *begin = carets_.data() + line.first_caret;
  const Caret *end = begin + line.caret_count;
  const Caret *position = std::lower_bound(
      begin, end, offset, [](const Caret &caret, std::uint32_t value) {
        return caret.offset < value;
      });
  if (position == end)
    --position;

  out = Rect<float>(position->x, static_cast<float>(line_index) * line_height,
                    0.0f, line_height);
  return true;
}

std::pair<std::uint32_t, std::uint32_t>
TextLayout::word_at(std::uint32_t requested) const {
  if (text_.empty())
    return {0, 0};

  const std::string_view text(text_);
  std::size_t position = std::min<std::size_t>(requested, text.size());
  if (position == text.size())
    position = previous_codepoint(text, position);
  while (position > 0 &&
         (static_cast<unsigned char>(text[position]) & 0xC0) == 0x80)
    --position;

  const Codepoint selected = decode(text, position);
  const WordClass selected_class = word_class(selected.value);
  std::size_t begin = position;
  std::size_t end = std::min(position + selected.width, text.size());

  // Punctuation and ideographs select one codepoint. Words and whitespace
  // expand in both directions; this gives natural ASCII/Unicode identifiers
  // while avoiding a large Unicode word-break table in every application.
  if (selected_class != WordClass::Other) {
    while (begin > 0) {
      const std::size_t previous = previous_codepoint(text, begin);
      if (word_class(decode(text, previous).value) != selected_class)
        break;
      begin = previous;
    }
    while (end < text.size()) {
      const Codepoint next = decode(text, end);
      if (word_class(next.value) != selected_class)
        break;
      end += next.width;
    }
  }

  return {static_cast<std::uint32_t>(begin), static_cast<std::uint32_t>(end)};
}

} // namespace voidui
