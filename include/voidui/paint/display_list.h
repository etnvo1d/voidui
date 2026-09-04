#pragma once

#include <array>
#include <cstdint>
#include <memory>
#include <vector>

#include "voidui/core/transform.h"
#include "voidui/paint/font.h"
#include "voidui/paint/image.h"
#include "voidui/paint/paint.h"
#include "voidui/paint/path.h"
#include "voidui/paint/text_layout.h"

namespace voidui {

/// Rounded-rect clips are evaluated in the fragment shader, so the count is
/// bounded by what fits in the per-instance payload. Two covers the nesting a
/// real interface produces (a rounded panel inside a rounded window); anything
/// deeper falls back to the bounding scissor.
inline constexpr std::size_t kMaxClipRRects = 2;

struct ClipRRect {
  Rect<float> rect{0.0f, 0.0f, 0.0f, 0.0f};
  Radius radius{0.0f};
};

/// The clip in force for a command. The scissor is always valid and always
/// conservative; the rounded rects refine it.
struct ClipState {
  Rect<float> scissor{0.0f, 0.0f, 0.0f, 0.0f};
  std::array<ClipRRect, kMaxClipRRects> rrects{};
  std::uint32_t rrect_count = 0;
};

enum class CommandKind : std::uint8_t {
  FillRRect,
  StrokeRRect,
  FillPath,
  StrokePath,
  Shadow,
  Glyphs,
  Image,
};

/// One recorded drawing operation.
///
/// Fill and stroke are distinct kinds and stay distinct through the display
/// list. The scene builder may later notice that a fill is followed by a stroke
/// of the same geometry and emit a single shaded instance for the pair, but
/// nothing above this layer depends on that.
struct DrawCommand {
  CommandKind kind = CommandKind::FillRRect;
  std::uint32_t clip_index = 0;
  Transform transform{};

  // Analytic geometry, valid for the RRect and Shadow kinds.
  Rect<float> rect{0.0f, 0.0f, 0.0f, 0.0f};
  Radius radius{0.0f};

  // A handle onto the path, valid for the Path kinds. Opaque: it names one of
  // the display list's two path arenas, and only DisplayList::path() knows
  // which. See `add_path`.
  std::uint32_t path_index = 0;
  FillRule fill_rule = FillRule::NonZero;

  // Valid for Glyphs. The run's origin -- the left end of its baseline -- is
  // carried in `rect.origin`.
  //
  // A shared layout is referenced rather than copied: a paragraph redrawn every
  // frame then costs one refcount bump, not a copy of every glyph in it. When
  // `text_layout` is null the run lives in the display list's own arena, which
  // suits one-off labels that are shaped and thrown away.
  std::shared_ptr<const TextLayout> text_layout;
  std::uint32_t run_index = 0;

  // Valid for Image; `rect` is the destination in local coordinates.
  std::shared_ptr<const class Image> image;

  // The region of `image` that fills `rect`, normalised to the image so the
  // renderer can map it straight onto whatever atlas slot the pixels landed in.
  // Cropping here rather than clipping is what keeps `object-fit: cover` free:
  // a clip would cost a scissor change and split the batch, while a narrower
  // source is the same instance with different texture coordinates.
  std::array<float, 4> source{0.0f, 0.0f, 1.0f, 1.0f};

  Brush brush = Color(0, 0, 0);
  float opacity = 1.0f;

  float stroke_width = 0.0f;
  StrokeAlign stroke_align = StrokeAlign::Center;
  LineCap cap = LineCap::Butt;
  LineJoin join = LineJoin::Miter;
  float miter_limit = 4.0f;

  Shadow shadow{};
};

/// A frame's worth of recorded drawing, plus the arenas the commands index
/// into. Cleared and refilled each frame; the vectors keep their capacity, so a
/// steady-state frame performs no allocation.
class DisplayList {
public:
  /// Marks a path index as naming the shared arena rather than the owned one.
  /// Nothing outside `path()` and `add_path()` looks at it.
  static constexpr std::uint32_t kSharedPath = 0x80000000u;

  void clear() {
    commands_.clear();
    clips_.clear();
    paths_.clear();
    shared_paths_.clear();
    runs_.clear();
  }

  const std::vector<DrawCommand> &commands() const { return commands_; }
  const std::vector<ClipState> &clips() const { return clips_; }

  const Path &path(std::uint32_t index) const {
    return (index & kSharedPath) ? *shared_paths_[index & ~kSharedPath]
                                 : paths_[index];
  }

  const GlyphRun &run(std::uint32_t index) const { return runs_[index]; }

  void add(const DrawCommand &command) { commands_.push_back(command); }

  /// A path the caller is handing over -- a one-off outline built for this
  /// frame and thrown away with it.
  std::uint32_t add_path(Path path) {
    paths_.push_back(std::move(path));
    return static_cast<std::uint32_t>(paths_.size() - 1);
  }

  /// A path that outlives the frame: an icon's geometry, a chart's series, any
  /// outline that is drawn again next frame exactly as it was drawn in this
  /// one.
  ///
  /// Referenced rather than copied, for the reason a shared TextLayout is: a
  /// redrawn shape then costs one refcount bump instead of a copy of every
  /// point in it, and a screenful of icons stops allocating twice per contour
  /// per frame. Nothing downstream can tell the difference -- the renderer keys
  /// its mask cache on the outline itself, not on where the bytes live.
  std::uint32_t add_path(std::shared_ptr<const Path> path) {
    shared_paths_.push_back(std::move(path));
    return static_cast<std::uint32_t>(shared_paths_.size() - 1) | kSharedPath;
  }

  std::uint32_t add_run(GlyphRun run) {
    runs_.push_back(std::move(run));
    return static_cast<std::uint32_t>(runs_.size() - 1);
  }

  /// Clips are deduplicated against the most recent entry, which collapses the
  /// common case of many siblings sharing one parent's clip.
  std::uint32_t add_clip(const ClipState &clip) {
    if (!clips_.empty() && same(clips_.back(), clip))
      return static_cast<std::uint32_t>(clips_.size() - 1);

    clips_.push_back(clip);
    return static_cast<std::uint32_t>(clips_.size() - 1);
  }

private:
  static bool same(const ClipState &a, const ClipState &b) {
    if (a.rrect_count != b.rrect_count)
      return false;
    if (a.scissor.origin.x != b.scissor.origin.x ||
        a.scissor.origin.y != b.scissor.origin.y ||
        a.scissor.size.width != b.scissor.size.width ||
        a.scissor.size.height != b.scissor.size.height)
      return false;

    for (std::uint32_t i = 0; i < a.rrect_count; ++i) {
      const ClipRRect &x = a.rrects[i];
      const ClipRRect &y = b.rrects[i];
      if (x.rect.origin.x != y.rect.origin.x || x.rect.origin.y != y.rect.origin.y ||
          x.rect.size.width != y.rect.size.width ||
          x.rect.size.height != y.rect.size.height ||
          x.radius.left_top != y.radius.left_top ||
          x.radius.right_top != y.radius.right_top ||
          x.radius.right_bottom != y.radius.right_bottom ||
          x.radius.left_bottom != y.radius.left_bottom)
        return false;
    }

    return true;
  }

  std::vector<DrawCommand> commands_;
  std::vector<ClipState> clips_;
  std::vector<Path> paths_;
  std::vector<std::shared_ptr<const Path>> shared_paths_;
  std::vector<GlyphRun> runs_;
};

} // namespace voidui
