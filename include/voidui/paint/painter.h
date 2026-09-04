#pragma once

#include <vector>

#include "voidui/paint/display_list.h"

namespace voidui {

/// The recording API widgets draw through.
///
/// Nothing here touches the GPU: every call appends to a DisplayList, which the
/// scene builder later turns into batches. Fill and stroke are separate calls
/// and stay separate commands -- fusing an adjacent pair that happens to share
/// geometry is the renderer's business, not the caller's.
class Painter {
public:
  Painter(DisplayList &list, Size<float> viewport);

  void save();
  void restore();

  void translate(float x, float y);
  void scale(float x, float y);
  void rotate(float radians);
  void transform(const Transform &t);
  void opacity(float value);

  /// Intersects the current clip. An axis-aligned rectangle becomes a scissor;
  /// a rounded one additionally rides along in the shader.
  void clip_rect(Rect<float> rect);
  void clip_rrect(Rect<float> rect, Radius radius);

  void fill_rect(Rect<float> rect, const Paint &paint);
  void fill_rrect(Rect<float> rect, Radius radius, const Paint &paint);
  void stroke_rect(Rect<float> rect, const Paint &paint, const Pen &pen);
  void stroke_rrect(Rect<float> rect, Radius radius, const Paint &paint, const Pen &pen);

  void fill_path(const Path &path, const Paint &paint,
                 FillRule rule = FillRule::NonZero);
  void stroke_path(const Path &path, const Paint &paint, const Pen &pen);

  /// The same, for an outline that outlives the frame.
  ///
  /// Anything drawn again next frame exactly as it was drawn in this one -- an
  /// icon, a chart series, a map layer -- should arrive this way: the display
  /// list references it instead of copying it, so redrawing costs a refcount
  /// bump rather than a copy of every point. The pixels are identical either
  /// way.
  void fill_path(std::shared_ptr<const Path> path, const Paint &paint,
                 FillRule rule = FillRule::NonZero);
  void stroke_path(std::shared_ptr<const Path> path, const Paint &paint,
                   const Pen &pen);

  void draw_shadow(Rect<float> rect, Radius radius, const Shadow &shadow);

  /// `origin` is the left end of the baseline, the way a text API is normally
  /// anchored. Shaping is independent of display scale -- glyph advances are
  /// the same at any DPI. Two things around it are not: the rasterised glyphs,
  /// and the line box a multi-line layout stacks with, which `TextLayout`
  /// rounds to whole device pixels so stacked lines cannot round unevenly
  /// against one another.
  /// Draws `image` scaled into `dst`. The paint's brush is ignored; its
  /// opacity and the brush colour's alpha modulate the image.
  void draw_image(const std::shared_ptr<const Image> &image, Rect<float> dst,
                  const Paint &paint = Paint(Color(255, 255, 255)));

  /// The same, drawing only `src` -- a region in source pixels, which may lie
  /// partly outside the image and is clamped to it.
  ///
  /// This is how a cropping fit should be drawn. Clipping the widget's box and
  /// drawing an oversized destination gets the same pixels on screen, but at
  /// the price of a scissor change that splits the batch; a narrower source
  /// costs nothing beyond the texture coordinates it was going to write anyway.
  void draw_image(const std::shared_ptr<const Image> &image, Rect<float> dst,
                  Rect<float> src,
                  const Paint &paint = Paint(Color(255, 255, 255)));

  void draw_glyphs(Point<float> origin, GlyphRun run, const Paint &paint);
  void draw_text(Point<float> origin, const std::shared_ptr<Font> &font,
                 std::string_view text, const Paint &paint);

  /// Draws through a font stack, so text the base face cannot render picks up
  /// the platform's substitute rather than turning into tofu.
  ///
  /// This shapes on every call. Anything drawn repeatedly should be shaped once
  /// into a TextLayout and drawn with the overload below.
  void draw_text(Point<float> origin, const std::shared_ptr<FontStack> &fonts,
                 std::string_view text, const Paint &paint);

  /// Draws a prepared paragraph. `origin` is its top-left corner, not a
  /// baseline -- the layout already knows where its baselines sit. Copies no
  /// glyphs.
  void draw_text_layout(Point<float> origin,
                        const std::shared_ptr<const TextLayout> &layout,
                        const Paint &paint);

  const Transform &current_transform() const { return state_.transform; }

private:
  struct State {
    Transform transform{};
    ClipState clip{};
    float opacity = 1.0f;
  };

  DrawCommand begin_command_(CommandKind kind) const;
  void intersect_scissor_(Rect<float> device_bounds);

  /// The half of a path command that does not depend on where the geometry
  /// lives: brush, pen, clip and culling bounds. `path_index` has already been
  /// taken from whichever arena the caller used.
  void add_fill_path_(std::uint32_t path_index, Rect<float> bounds,
                      const Paint &paint, FillRule rule);
  void add_stroke_path_(std::uint32_t path_index, Rect<float> bounds,
                        const Paint &paint, const Pen &pen);

  DisplayList &list_;
  State state_{};
  std::vector<State> stack_;
};

} // namespace voidui
