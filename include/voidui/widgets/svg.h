#pragma once

#include <algorithm>
#include <cmath>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <vector>

#include "voidui/core/context.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"
#include "voidui/paint/svg_cache.h"
#include "voidui/widgets/image.h"

namespace voidui {

/// One shape's presentation after both the document and the cascade have had
/// their say.
///
/// A property the file stated wins; one it left open comes from the widget's
/// VSS style. That is what makes a single icon file take its colour from the
/// button around it, follow a `:hover` rule, and animate under a `transition`
/// -- none of which the file has to know about.
struct SvgResolvedStyle {
  SvgPaint fill = SvgPaint::solid(Color(0, 0, 0));
  SvgPaint stroke = SvgPaint::none();

  float fill_opacity = 1.0f;
  float stroke_opacity = 1.0f;
  float opacity = 1.0f;
  float stroke_width = 1.0f;
  float miter_limit = 4.0f;
  float dash_offset = 0.0f;

  SvgDashArray dashes;

  FillRule fill_rule = FillRule::NonZero;
  LineCap cap = LineCap::Butt;
  LineJoin join = LineJoin::Miter;
  SvgPaintOrder order = SvgPaintOrder::FillStroke;
};

inline SvgResolvedStyle resolve_svg_style(const SvgShapeStyle &shape,
                                          const SvgDocument &document,
                                          const ComputedStyle &style) {
  const auto stated = [&](std::uint16_t bit) {
    return (shape.specified & bit) != 0;
  };

  SvgResolvedStyle out;
  out.opacity = shape.opacity;

  out.fill = stated(kSvgFillSet) ? shape.fill : style.get<styles::Fill>();
  out.stroke = stated(kSvgStrokeSet) ? shape.stroke : style.get<styles::Stroke>();

  out.fill_opacity = stated(kSvgFillOpacitySet)
                         ? shape.fill_opacity
                         : style.get<styles::FillOpacity>();
  out.stroke_opacity = stated(kSvgStrokeOpacitySet)
                           ? shape.stroke_opacity
                           : style.get<styles::StrokeOpacity>();
  out.fill_rule =
      stated(kSvgFillRuleSet) ? shape.fill_rule : style.get<styles::FillRule>();
  out.stroke_width = stated(kSvgStrokeWidthSet)
                         ? shape.stroke_width
                         : style.get<styles::StrokeWidth>();
  out.cap =
      stated(kSvgStrokeLinecapSet) ? shape.cap : style.get<styles::StrokeLinecap>();
  out.join = stated(kSvgStrokeLinejoinSet) ? shape.join
                                           : style.get<styles::StrokeLinejoin>();
  out.miter_limit = stated(kSvgStrokeMiterlimitSet)
                        ? shape.miter_limit
                        : style.get<styles::StrokeMiterlimit>();
  out.order =
      stated(kSvgPaintOrderSet) ? shape.order : style.get<styles::PaintOrder>();

  if (stated(kSvgStrokeDasharraySet) && shape.dashes != kNoSvgDashes) {
    const SvgDashPattern &pattern = document.dashes(shape.dashes);
    out.dashes = pattern.array;
    out.dash_offset = pattern.offset;
  } else if (!stated(kSvgStrokeDasharraySet)) {
    out.dashes = style.get<styles::StrokeDasharray>();
    out.dash_offset = style.get<styles::StrokeDashoffset>();
  }

  return out;
}

/// One ready-to-issue fill or stroke.
///
/// The whole point of the plan is that this struct answers every question the
/// painter will ask, so a frame of drawing is a walk over an array: no cascade
/// lookups, no paint resolution, no geometry. It is rebuilt only when the
/// document or the computed style changes, which for a static screen is never.
struct SvgDrawOp {
  std::shared_ptr<const Path> path;
  Brush brush = Color(0, 0, 0);
  float opacity = 1.0f;
  float stroke_width = 0.0f; ///< zero means this op is a fill
  float miter_limit = 4.0f;
  std::uint32_t clip = kNoSvgIndex;
  FillRule fill_rule = FillRule::NonZero;
  LineCap cap = LineCap::Butt;
  LineJoin join = LineJoin::Miter;

  /// `vector-effect: non-scaling-stroke`: the width is in logical pixels and
  /// has to be divided by the viewport scale rather than multiplied by it.
  bool non_scaling_stroke = false;
};

/// A vector picture.
///
/// The expensive parts happen once and are shared. `SvgCache` parses a document
/// once per process, and the result is size-independent -- a document is drawn
/// through the painter's transform, so one object serves a 16-pixel toolbar
/// icon and a 512-pixel illustration and a window resize rebuilds no geometry.
/// The renderer keys its coverage masks on the outline rather than on where it
/// sits, so a list of forty rows showing one icon also rasterises it once.
///
/// What is left per widget is the draw plan: the document's shapes merged with
/// this widget's cascaded style, resolved down to a flat array of fills and
/// strokes. It is rebuilt when the style changes -- a hover, a theme switch, a
/// frame of a `transition: fill` -- and walked, unchanged, on every other
/// frame.
class SvgView : public Widget {
public:
  VOIDUI_STYLE_SCOPE(SvgView, "svg")

  SvgView() = default;

  explicit SvgView(SvgSource source) : source_(std::move(source)) {}

  /// Names a resource or a file: `res://icons/user.svg`, `file:///c:/a.svg`, or
  /// a bare path, which is read as a file the way a command-line argument is.
  explicit SvgView(std::string_view uri) : source_(SvgSource::uri(uri)) {}

  VOIDUI_FLUENT_METHOD(
      source, (SvgSource value), if (value.key() != source_.key()) {
        source_ = std::move(value);
        handle_ = SvgHandle();
        invalidate_plan_();
      })

  /// Overrides the file's own `preserveAspectRatio`.
  ///
  /// `Contain` is what the SVG default (`xMidYMid meet`) means, and `Cover` is
  /// what `slice` means, so this is the same setting stated in the vocabulary
  /// the rest of the framework already uses for pictures.
  VOIDUI_FLUENT_METHOD(fit, (ObjectFit value), fit_ = value;)
  VOIDUI_FLUENT_METHOD(alignment, (ImageAlignment value), alignment_ = value;)

  /// The size to lay out at before the document arrives. Worth setting in a
  /// list, for the reason an image's is.
  VOIDUI_FLUENT_METHOD(natural_size, (Size<float> value), natural_ = value;)

  VOIDUI_WIDGET_SIZE_STYLE

  const SvgHandle &handle() const { return handle_; }
  std::shared_ptr<const SvgDocument> document() const {
    return handle_.document();
  }

  /// The plan this widget last drew. Exposed for tests and diagnostics: it is
  /// the one place where "what the file said" and "what the stylesheet said"
  /// have already been reconciled.
  const std::vector<SvgDrawOp> &draw_plan() const { return plan_; }

  std::shared_ptr<const StyleSheet> default_stylesheet() const override {
    static const std::shared_ptr<const StyleSheet> defaults =
        StyleParser::parse("svg { width: auto; height: auto; }",
                           "svg.default.vss", StyleOrigin::WidgetDefault)
            .sheet;
    return defaults;
  }

  std::unique_ptr<Widget> clone() const override {
    auto copy = std::make_unique<SvgView>(source_);
    copy->fit_ = fit_;
    copy->alignment_ = alignment_;
    copy->natural_ = natural_;
    return copy;
  }

  /// Carries the load and the plan across a rebuild.
  ///
  /// Without this a component that re-renders for an unrelated reason drops its
  /// handle and its plan, and pays for a cache lookup and a full re-resolve on
  /// the next frame -- for every icon on the screen, every time any of them
  /// changes.
  void inherit_runtime(const Widget &previous) override {
    const auto &other = static_cast<const SvgView &>(previous);
    if (other.source_.key() != source_.key())
      return;

    handle_ = other.handle_;
    plan_ = other.plan_;
    plan_key_ = other.plan_key_;
  }

  void register_children(Registrar &) override {}

  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    ensure_load_(ctx.invalidator());
    return constraints.resolve(ctx.style.layout_size(), intrinsic_size());
  }

  void draw(const DrawContext &ctx, Painter &painter) override {
    const std::shared_ptr<const SvgDocument> document = handle_.document();
    if (!document || document->shapes().empty())
      return;

    const Rect<float> view_box = document->view_box();
    if (!(view_box.size.width > 0.0f) || !(view_box.size.height > 0.0f))
      return;

    const Viewport viewport = viewport_for_(*document, ctx.bounds);
    if (!(viewport.scale_x > 0.0f) || !(viewport.scale_y > 0.0f))
      return;

    rebuild_plan_(*document, ctx.style);
    if (plan_.empty())
      return;

    painter.save();

    // `slice` is the only fit that leaves the box, and it is the only one that
    // needs a scissor. Everything else is already inside its bounds, and a clip
    // costs a scissor change that splits the batch.
    if (viewport.overflows)
      painter.clip_rect(ctx.bounds);

    painter.transform(
        Transform::translate(viewport.origin.x, viewport.origin.y)
            .concat(Transform::scale(viewport.scale_x, viewport.scale_y))
            .concat(Transform::translate(-view_box.origin.x,
                                         -view_box.origin.y)));

    // A pen stated in logical pixels has to be un-scaled back through the
    // viewport, since the painter is about to scale everything under it.
    const float pen_scale =
        1.0f / std::max(std::sqrt(std::abs(viewport.scale_x * viewport.scale_y)),
                        1e-6f);

    std::uint32_t clip = kNoSvgIndex;
    bool clipped = false;

    for (const SvgDrawOp &op : plan_) {
      if (op.clip != clip) {
        if (clipped) {
          painter.restore();
          clipped = false;
        }
        clip = op.clip;
        if (clip != kNoSvgIndex) {
          painter.save();
          painter.clip_rect(document->clip(clip));
          clipped = true;
        }
      }

      Paint paint(op.brush);
      paint.opacity = op.opacity;

      if (op.stroke_width <= 0.0f) {
        painter.fill_path(op.path, paint, op.fill_rule);
        continue;
      }

      Pen pen(op.non_scaling_stroke ? op.stroke_width * pen_scale
                                    : op.stroke_width);
      pen.cap = op.cap;
      pen.join = op.join;
      pen.miter_limit = op.miter_limit;
      painter.stroke_path(op.path, paint, pen);
    }

    if (clipped)
      painter.restore();
    painter.restore();
  }

  EventResult on_event(Event &) override { return EventResult::Unhandled; }

  /// The size the picture wants, in logical units.
  ///
  /// The `width`/`height` the file states, or the view box when it states none
  /// -- which is the usual case for an icon, and the reason a view box is what
  /// really fixes the aspect ratio.
  Size<float> intrinsic_size() const {
    const std::shared_ptr<const SvgDocument> document = handle_.document();
    if (!document)
      return natural_;

    const Size<float> intrinsic = document->intrinsic_size();
    if (intrinsic.width > 0.0f && intrinsic.height > 0.0f)
      return intrinsic;

    const Rect<float> view_box = document->view_box();
    if (view_box.size.width > 0.0f && view_box.size.height > 0.0f)
      return view_box.size;

    return natural_;
  }

private:
  /// Where the view box lands inside the widget's box.
  struct Viewport {
    Point<float> origin{0.0f, 0.0f};
    float scale_x = 1.0f;
    float scale_y = 1.0f;
    bool overflows = false;
  };

  void ensure_load_(const Invalidator &invalidator) {
    if (source_.empty() || handle_.state() != SvgHandle::State::Empty)
      return;
    handle_ = SvgCache::global().acquire(source_, invalidator);
  }

  void invalidate_plan_() {
    plan_.clear();
    plan_key_ = 0;
  }

  Viewport viewport_for_(const SvgDocument &document, Rect<float> box) const {
    const Rect<float> view_box = document.view_box();
    const SvgPreserveAspectRatio &preserve = document.preserve_aspect_ratio();

    float align_x = preserve.align_x;
    float align_y = preserve.align_y;
    bool uniform = preserve.uniform;
    bool slice = preserve.slice;
    float natural = 1.0f;
    bool natural_size = false;

    // An explicit fit overrides what the file asked for, and brings the
    // widget's own alignment with it.
    if (fit_) {
      align_x = alignment_.x;
      align_y = alignment_.y;
      switch (*fit_) {
      case ObjectFit::Fill:
        uniform = false;
        slice = false;
        break;
      case ObjectFit::Cover:
        uniform = true;
        slice = true;
        break;
      case ObjectFit::Contain:
        uniform = true;
        slice = false;
        break;
      case ObjectFit::ScaleDown:
        uniform = true;
        slice = false;
        natural = 1.0f;
        break;
      case ObjectFit::None:
        uniform = true;
        slice = false;
        natural_size = true;
        break;
      }
    }

    const float to_x = box.size.width / view_box.size.width;
    const float to_y = box.size.height / view_box.size.height;

    Viewport viewport;
    if (natural_size) {
      viewport.scale_x = viewport.scale_y = 1.0f;
    } else if (!uniform) {
      viewport.scale_x = to_x;
      viewport.scale_y = to_y;
    } else {
      float scale = slice ? std::max(to_x, to_y) : std::min(to_x, to_y);
      if (fit_ && *fit_ == ObjectFit::ScaleDown)
        scale = std::min(scale, natural);
      viewport.scale_x = viewport.scale_y = scale;
    }

    const float width = view_box.size.width * viewport.scale_x;
    const float height = view_box.size.height * viewport.scale_y;
    viewport.origin =
        Point<float>(box.origin.x + (box.size.width - width) * align_x,
                     box.origin.y + (box.size.height - height) * align_y);
    viewport.overflows = width > box.size.width + 0.01f ||
                         height > box.size.height + 0.01f;
    return viewport;
  }

  static std::uint64_t plan_key_for_(const SvgDocument &document,
                                     const ComputedStyle &style) {
    // The document pointer is safe to key on: this widget holds it, so it
    // cannot be freed and its address cannot be reused while the plan built
    // from it is alive.
    std::uint64_t key = style_hash_combine(
        style.hash(), reinterpret_cast<std::uintptr_t>(&document));
    return key == 0 ? 1 : key;
  }

  void rebuild_plan_(const SvgDocument &document, const ComputedStyle &style) {
    const std::uint64_t key = plan_key_for_(document, style);
    if (key == plan_key_)
      return;

    plan_.clear();
    plan_.reserve(document.shapes().size());
    plan_key_ = key;

    // A dash pattern is geometry, so it is cut here rather than every frame.
    // The tolerance is a fraction of the view box, which keeps it independent
    // of the size the picture is currently drawn at -- and therefore keeps the
    // plan from being rebuilt by a window resize.
    const Rect<float> view_box = document.view_box();
    const float tolerance =
        std::max(std::max(view_box.size.width, view_box.size.height) * 0.002f,
                 1e-3f);

    const SvgVectorEffect effect = style.get<styles::VectorEffect>();

    for (const SvgShape &shape : document.shapes()) {
      const SvgResolvedStyle resolved =
          resolve_svg_style(shape.style, document, style);

      const bool fills = resolved.fill.paints();
      const float width = resolved.stroke_width * shape.stroke_scale;
      const bool strokes = resolved.stroke.paints() && width > 0.0f;
      if (!fills && !strokes)
        continue;

      SvgDrawOp fill;
      if (fills) {
        fill.path = document.path(shape.path);
        fill.brush = brush_for_(resolved.fill, document, style);
        fill.opacity = resolved.opacity * resolved.fill_opacity;
        fill.clip = shape.clip;
        fill.fill_rule = resolved.fill_rule;
      }

      SvgDrawOp stroke;
      if (strokes) {
        std::shared_ptr<const Path> geometry = document.path(shape.path);

        if (!resolved.dashes.empty()) {
          // The geometry has the shape's ancestor transform baked into it, so
          // a pattern stated in the shape's own user units has to be carried
          // through the same scale -- otherwise an icon drawn inside a scaled
          // group would dash at the wrong rhythm.
          SvgDashArray scaled = resolved.dashes;
          for (std::uint8_t i = 0; i < scaled.count; ++i)
            scaled.lengths[i] *= shape.stroke_scale;

          Path dashed =
              dash_path(*geometry, scaled.span(),
                        resolved.dash_offset * shape.stroke_scale, tolerance);
          geometry = dashed.empty()
                         ? nullptr
                         : std::make_shared<const Path>(std::move(dashed));
        }

        if (geometry) {
          stroke.path = std::move(geometry);
          stroke.brush = brush_for_(resolved.stroke, document, style);
          stroke.opacity = resolved.opacity * resolved.stroke_opacity;
          stroke.clip = shape.clip;
          stroke.stroke_width = effect == SvgVectorEffect::NonScalingStroke
                                    ? resolved.stroke_width
                                    : width;
          stroke.non_scaling_stroke =
              effect == SvgVectorEffect::NonScalingStroke;
          stroke.miter_limit = resolved.miter_limit;
          stroke.cap = resolved.cap;
          stroke.join = resolved.join;
        }
      }

      if (resolved.order == SvgPaintOrder::StrokeFill) {
        if (stroke.path)
          plan_.push_back(std::move(stroke));
        if (fill.path)
          plan_.push_back(std::move(fill));
      } else {
        if (fill.path)
          plan_.push_back(std::move(fill));
        if (stroke.path)
          plan_.push_back(std::move(stroke));
      }
    }

    plan_.shrink_to_fit();
  }

  static Brush brush_for_(const SvgPaint &paint, const SvgDocument &document,
                          const ComputedStyle &style) {
    switch (paint.kind()) {
    case SvgPaint::Kind::Solid:
      return paint.color();
    case SvgPaint::Kind::CurrentColor:
      return style.get<styles::Foreground>();
    case SvgPaint::Kind::Server:
      return document.brush(paint.server_index());
    case SvgPaint::Kind::None:
      break;
    }
    return Color::TRANSPARENT;
  }

  SvgSource source_;
  SvgHandle handle_;

  std::optional<ObjectFit> fit_;
  ImageAlignment alignment_;
  Size<float> natural_{0.0f, 0.0f};

  std::vector<SvgDrawOp> plan_;

  /// Identifies the (document, computed style) pair the plan was built from.
  /// Zero when there is no plan.
  std::uint64_t plan_key_ = 0;
};

[[nodiscard]] inline SvgView svg(std::string_view uri) { return SvgView(uri); }

[[nodiscard]] inline SvgView svg(SvgSource source) {
  return SvgView(std::move(source));
}

/// An icon written inline, as a string literal beside the code that uses it.
/// Parsed once per distinct string, however many widgets name it.
[[nodiscard]] inline SvgView svg_markup(std::string markup) {
  return SvgView(SvgSource::markup(std::move(markup)));
}

} // namespace voidui
