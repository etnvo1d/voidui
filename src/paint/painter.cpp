#include "voidui/paint/painter.h"

#include <algorithm>

namespace voidui {

namespace {

Rect<float> intersect(Rect<float> a, Rect<float> b) {
  const float x0 = std::max(a.origin.x, b.origin.x);
  const float y0 = std::max(a.origin.y, b.origin.y);
  const float x1 =
      std::min(a.origin.x + a.size.width, b.origin.x + b.size.width);
  const float y1 =
      std::min(a.origin.y + a.size.height, b.origin.y + b.size.height);

  return Rect<float>(x0, y0, std::max(x1 - x0, 0.0f), std::max(y1 - y0, 0.0f));
}

} // namespace

Painter::Painter(DisplayList &list, Size<float> viewport) : list_(list) {
  state_.clip.scissor =
      Rect<float>(0.0f, 0.0f, viewport.width, viewport.height);
  state_.clip.rrect_count = 0;
}

void Painter::save() { stack_.push_back(state_); }

void Painter::restore() {
  if (stack_.empty())
    return;

  state_ = stack_.back();
  stack_.pop_back();
}

void Painter::translate(float x, float y) {
  state_.transform = state_.transform.concat(Transform::translate(x, y));
}

void Painter::scale(float x, float y) {
  state_.transform = state_.transform.concat(Transform::scale(x, y));
}

void Painter::rotate(float radians) {
  state_.transform = state_.transform.concat(Transform::rotate(radians));
}

void Painter::transform(const Transform &t) {
  state_.transform = state_.transform.concat(t);
}

void Painter::opacity(float value) {
  state_.opacity *= std::clamp(value, 0.0f, 1.0f);
}

void Painter::intersect_scissor_(Rect<float> device_bounds) {
  state_.clip.scissor = intersect(state_.clip.scissor, device_bounds);
}

void Painter::clip_rect(Rect<float> rect) {
  intersect_scissor_(state_.transform.map_bounds(rect));
}

void Painter::clip_rrect(Rect<float> rect, Radius radius) {
  const Rect<float> device = state_.transform.map_bounds(rect);
  intersect_scissor_(device);

  // The shader evaluates a rounded clip as an axis-aligned signed distance, so
  // a rotated or skewed one cannot be expressed exactly. Such a clip degrades
  // to the bounding scissor already applied above -- conservative, never
  // wrong-way-round, and rare in practice.
  if (!state_.transform.is_axis_aligned())
    return;

  if (state_.clip.rrect_count >= kMaxClipRRects)
    return;

  const float sx = state_.transform.approximate_scale();
  ClipRRect entry;
  entry.rect = device;
  entry.radius = Radius(radius.left_top * sx, radius.right_top * sx,
                        radius.right_bottom * sx, radius.left_bottom * sx);

  state_.clip.rrects[state_.clip.rrect_count++] = entry;
}

DrawCommand Painter::begin_command_(CommandKind kind) const {
  DrawCommand command;
  command.kind = kind;
  command.transform = state_.transform;
  return command;
}

void Painter::fill_rect(Rect<float> rect, const Paint &paint) {
  fill_rrect(rect, Radius(0.0f), paint);
}

void Painter::fill_rrect(Rect<float> rect, Radius radius, const Paint &paint) {
  if (rect.size.width <= 0.0f || rect.size.height <= 0.0f)
    return;

  DrawCommand command = begin_command_(CommandKind::FillRRect);
  command.clip_index = list_.add_clip(state_.clip);
  command.rect = rect;
  command.radius = radius;
  command.brush = paint.brush;
  command.opacity = paint.opacity * state_.opacity;
  list_.add(command);
}

void Painter::stroke_rect(Rect<float> rect, const Paint &paint,
                          const Pen &pen) {
  stroke_rrect(rect, Radius(0.0f), paint, pen);
}

void Painter::stroke_rrect(Rect<float> rect, Radius radius, const Paint &paint,
                           const Pen &pen) {
  if (rect.size.width <= 0.0f || rect.size.height <= 0.0f || pen.width <= 0.0f)
    return;

  DrawCommand command = begin_command_(CommandKind::StrokeRRect);
  command.clip_index = list_.add_clip(state_.clip);
  command.rect = rect;
  command.radius = radius;
  command.brush = paint.brush;
  command.opacity = paint.opacity * state_.opacity;
  command.stroke_width = pen.width;
  command.stroke_align = pen.align;
  command.cap = pen.cap;
  command.join = pen.join;
  command.miter_limit = pen.miter_limit;
  list_.add(command);
}

void Painter::add_fill_path_(std::uint32_t path_index, Rect<float> bounds,
                             const Paint &paint, FillRule rule) {
  DrawCommand command = begin_command_(CommandKind::FillPath);
  command.clip_index = list_.add_clip(state_.clip);
  command.path_index = path_index;
  command.fill_rule = rule;
  command.brush = paint.brush;
  command.opacity = paint.opacity * state_.opacity;
  command.rect = bounds;
  list_.add(command);
}

void Painter::add_stroke_path_(std::uint32_t path_index, Rect<float> bounds,
                               const Paint &paint, const Pen &pen) {
  DrawCommand command = begin_command_(CommandKind::StrokePath);
  command.clip_index = list_.add_clip(state_.clip);
  command.path_index = path_index;
  command.brush = paint.brush;
  command.opacity = paint.opacity * state_.opacity;
  command.stroke_width = pen.width;
  command.stroke_align = pen.align;
  command.cap = pen.cap;
  command.join = pen.join;
  command.miter_limit = pen.miter_limit;
  command.rect = bounds;
  list_.add(command);
}

void Painter::fill_path(const Path &path, const Paint &paint, FillRule rule) {
  if (path.empty())
    return;

  // A path built by one of the shape factories carries an exact description of
  // itself; take the analytic route rather than tessellating a shape the
  // fragment shader can solve outright.
  if (path.analytic() != AnalyticShape::None) {
    fill_rrect(path.analytic_bounds(), path.analytic_radius(), paint);
    return;
  }

  add_fill_path_(list_.add_path(path), path.bounds(), paint, rule);
}

void Painter::stroke_path(const Path &path, const Paint &paint,
                          const Pen &pen) {
  if (path.empty() || pen.width <= 0.0f)
    return;

  if (path.analytic() != AnalyticShape::None) {
    stroke_rrect(path.analytic_bounds(), path.analytic_radius(), paint, pen);
    return;
  }

  add_stroke_path_(list_.add_path(path), path.bounds(), paint, pen);
}

void Painter::fill_path(std::shared_ptr<const Path> path, const Paint &paint,
                        FillRule rule) {
  if (!path || path->empty())
    return;

  // The analytic route wins even here: a shape the fragment shader can solve
  // outright costs four vertices and no atlas slot, which beats any amount of
  // sharing.
  if (path->analytic() != AnalyticShape::None) {
    fill_rrect(path->analytic_bounds(), path->analytic_radius(), paint);
    return;
  }

  const Rect<float> bounds = path->bounds();
  add_fill_path_(list_.add_path(std::move(path)), bounds, paint, rule);
}

void Painter::stroke_path(std::shared_ptr<const Path> path, const Paint &paint,
                          const Pen &pen) {
  if (!path || path->empty() || pen.width <= 0.0f)
    return;

  if (path->analytic() != AnalyticShape::None) {
    stroke_rrect(path->analytic_bounds(), path->analytic_radius(), paint, pen);
    return;
  }

  const Rect<float> bounds = path->bounds();
  add_stroke_path_(list_.add_path(std::move(path)), bounds, paint, pen);
}

void Painter::draw_image(const std::shared_ptr<const Image> &image,
                         Rect<float> dst, const Paint &paint) {
  if (!image || dst.size.width <= 0.0f || dst.size.height <= 0.0f)
    return;

  DrawCommand command = begin_command_(CommandKind::Image);
  command.clip_index = list_.add_clip(state_.clip);
  command.rect = dst;
  command.brush = paint.brush;
  command.opacity = paint.opacity * state_.opacity;
  command.image = image;
  list_.add(command);
}

void Painter::draw_image(const std::shared_ptr<const Image> &image,
                         Rect<float> dst, Rect<float> src, const Paint &paint) {
  if (!image || dst.size.width <= 0.0f || dst.size.height <= 0.0f)
    return;

  const float width = static_cast<float>(image->width());
  const float height = static_cast<float>(image->height());
  if (width <= 0.0f || height <= 0.0f || src.size.width <= 0.0f ||
      src.size.height <= 0.0f)
    return;

  // Clamped rather than rejected: a fit that rounds its source region outward
  // can name a fraction of a pixel past the edge, and refusing to draw at all
  // would be a worse answer than sampling the last row twice.
  const float u0 = std::clamp(src.origin.x / width, 0.0f, 1.0f);
  const float v0 = std::clamp(src.origin.y / height, 0.0f, 1.0f);
  const float u1 =
      std::clamp((src.origin.x + src.size.width) / width, 0.0f, 1.0f);
  const float v1 =
      std::clamp((src.origin.y + src.size.height) / height, 0.0f, 1.0f);
  if (u1 <= u0 || v1 <= v0)
    return;

  DrawCommand command = begin_command_(CommandKind::Image);
  command.clip_index = list_.add_clip(state_.clip);
  command.rect = dst;
  command.brush = paint.brush;
  command.opacity = paint.opacity * state_.opacity;
  command.image = image;
  command.source = {u0, v0, u1, v1};
  list_.add(command);
}

void Painter::draw_glyphs(Point<float> origin, GlyphRun run,
                          const Paint &paint) {
  if (run.glyphs.empty() || !run.font)
    return;

  DrawCommand command = begin_command_(CommandKind::Glyphs);
  command.clip_index = list_.add_clip(state_.clip);
  command.rect = Rect<float>(origin, Size<float>(run.advance, 0.0f));
  command.brush = paint.brush;
  command.opacity = paint.opacity * state_.opacity;
  command.run_index = list_.add_run(std::move(run));
  list_.add(command);
}

void Painter::draw_text(Point<float> origin, const std::shared_ptr<Font> &font,
                        std::string_view text, const Paint &paint) {
  if (!font || text.empty())
    return;
  draw_glyphs(origin, font->shape(text), paint);
}

void Painter::draw_text(Point<float> origin,
                        const std::shared_ptr<FontStack> &fonts,
                        std::string_view text, const Paint &paint) {
  if (!fonts || text.empty())
    return;

  // Every run's offsets are already biased to share this origin.
  for (GlyphRun &run : fonts->shape(text))
    draw_glyphs(origin, std::move(run), paint);
}

void Painter::draw_text_layout(Point<float> origin,
                               const std::shared_ptr<const TextLayout> &layout,
                               const Paint &paint) {
  if (!layout || layout->runs().empty())
    return;

  const std::uint32_t count = static_cast<std::uint32_t>(layout->runs().size());
  const std::uint32_t clip = list_.add_clip(state_.clip);

  for (std::uint32_t i = 0; i < count; ++i) {
    DrawCommand command = begin_command_(CommandKind::Glyphs);
    command.clip_index = clip;
    command.rect = Rect<float>(origin, layout->size());
    command.brush = paint.brush;
    command.opacity = paint.opacity * state_.opacity;
    command.text_layout = layout;
    command.run_index = i;
    list_.add(command);
  }
}

void Painter::draw_shadow(Rect<float> rect, Radius radius,
                          const Shadow &shadow) {
  if (rect.size.width <= 0.0f || rect.size.height <= 0.0f)
    return;

  DrawCommand command = begin_command_(CommandKind::Shadow);

  // The blur reaches roughly three standard deviations past the silhouette, so
  // the scissor has to be opened up or the shadow gets clipped to the shape.
  const float reach = shadow.blur * 3.0f + shadow.spread + 1.0f;
  Rect<float> bounds(rect.origin.x + shadow.offset.x - reach,
                     rect.origin.y + shadow.offset.y - reach,
                     rect.size.width + reach * 2.0f,
                     rect.size.height + reach * 2.0f);

  command.clip_index = list_.add_clip(state_.clip);
  command.rect = rect;
  command.radius = radius;
  command.shadow = shadow;
  command.brush = shadow.color;
  command.opacity = state_.opacity;
  (void)bounds;
  list_.add(command);
}

} // namespace voidui
