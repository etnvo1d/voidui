#pragma once

#include <cstdint>
#include <memory>
#include <span>
#include <string>
#include <string_view>
#include <vector>

#include "voidui/core/geometry.h"
#include "voidui/core/style/svg.h"
#include "voidui/core/transform.h"
#include "voidui/paint/paint.h"
#include "voidui/paint/path.h"
#include "voidui/render/brush.h"

namespace voidui {

inline constexpr std::uint32_t kNoSvgIndex = 0xFFFFFFFFu;
inline constexpr std::uint16_t kNoSvgDashes = 0xFFFFu;

/// Which presentation properties the document itself pinned down for a shape.
///
/// This is the whole mechanism behind a themeable icon. A property the file
/// never mentions -- neither on the shape nor on any ancestor of it -- is left
/// open, and the widget fills it from its own cascaded VSS style. A property
/// the file did state wins, because an author who wrote `fill="#e11"` on one
/// path of a two-tone logo meant it.
///
/// It costs two bytes per shape and replaces the alternative, which is either
/// re-parsing the file per colour or shipping one file per colour.
enum SvgSpecified : std::uint16_t {
  kSvgFillSet = 1u << 0,
  kSvgFillOpacitySet = 1u << 1,
  kSvgFillRuleSet = 1u << 2,
  kSvgStrokeSet = 1u << 3,
  kSvgStrokeOpacitySet = 1u << 4,
  kSvgStrokeWidthSet = 1u << 5,
  kSvgStrokeLinecapSet = 1u << 6,
  kSvgStrokeLinejoinSet = 1u << 7,
  kSvgStrokeMiterlimitSet = 1u << 8,
  kSvgStrokeDasharraySet = 1u << 9,
  kSvgPaintOrderSet = 1u << 10,
};

/// A dash pattern as one shape uses it. Held in a side table because almost no
/// shape has one, and thirty-two bytes of zeroes per shape is thirty-two bytes
/// too many in a document with hundreds.
struct SvgDashPattern {
  SvgDashArray array;
  float offset = 0.0f;
};

/// The presentation state of one shape, after the document's own inheritance
/// has run.
struct SvgShapeStyle {
  SvgPaint fill = SvgPaint::solid(Color(0, 0, 0));
  SvgPaint stroke = SvgPaint::none();

  float fill_opacity = 1.0f;
  float stroke_opacity = 1.0f;

  /// Every `opacity` between the root and this shape, multiplied together.
  ///
  /// SVG defines a group's opacity as applying to the group's rendered result,
  /// which differs from this wherever two shapes in the group overlap. Doing it
  /// exactly needs an offscreen layer per group; folding it into the shapes
  /// costs one multiply and is right whenever the group does not overlap
  /// itself, which is the case in every icon and most illustrations.
  float opacity = 1.0f;

  float stroke_width = 1.0f;
  float miter_limit = 4.0f;

  std::uint16_t dashes = kNoSvgDashes;
  std::uint16_t specified = 0;

  FillRule fill_rule = FillRule::NonZero;
  LineCap cap = LineCap::Butt;
  LineJoin join = LineJoin::Miter;
  SvgPaintOrder order = SvgPaintOrder::FillStroke;
};

/// One drawable element of a document.
///
/// The transform its ancestors carried is already baked into the geometry, so
/// there is nothing per-shape for the painter to push: a document draws as a
/// flat run of fills and strokes under one transform. `stroke_scale` is what
/// the baking did to lengths, so a pen stated in the shape's own user units
/// still comes out the right width.
struct SvgShape {
  std::uint32_t path = 0;

  /// Index into the document's clip table, or `kNoSvgIndex`.
  std::uint32_t clip = kNoSvgIndex;

  float stroke_scale = 1.0f;

  SvgShapeStyle style;
};

/// SVG's `preserveAspectRatio`, in the terms the drawing code needs.
///
/// The ten `xMidYMax`-style keywords say two independent things -- where the
/// picture sits on each axis, and whether it is fitted inside the viewport or
/// made to cover it -- so they are stored that way rather than as an
/// enumeration that would have to be decoded again at every use.
struct SvgPreserveAspectRatio {
  float align_x = 0.5f; ///< 0 = xMin, 0.5 = xMid, 1 = xMax
  float align_y = 0.5f;

  /// False for `none`, which stretches each axis independently.
  bool uniform = true;

  /// True for `slice`: cover the viewport and overflow, rather than fit
  /// inside it.
  bool slice = false;
};

/// A parsed SVG: immutable, shared, and ready to draw at any size.
///
/// The expensive half of an SVG -- reading the XML, the `d` grammar and the
/// transform lists, flattening the element tree, resolving inheritance -- is
/// done once, here. What survives is a flat array of shapes over an arena of
/// paths, in the document's own user units, with every ancestor transform
/// already folded into the points.
///
/// Nothing in it depends on the size it will be drawn at, which is the point:
/// a document is scaled by the painter's transform, so the same object serves
/// a 16-pixel toolbar icon and a 512-pixel hero image, and resizing a window
/// rebuilds no geometry at all. Combined with the renderer keying its coverage
/// masks on the outline rather than on the position, a list of forty rows
/// showing one icon costs one parse, one set of paths, and one rasterisation.
///
/// Not supported, and skipped rather than approximated: `<text>`, `<image>`,
/// `<use>`, masks, filters, patterns, and CSS `<style>` blocks. `clip-path` is
/// honoured when it resolves to a single axis-aligned rectangle -- the shape a
/// design tool emits when it clips artwork to its frame -- and ignored
/// otherwise. Gradients parse into brushes; how faithfully one is painted is
/// the renderer's business, and today a path filled with one takes a
/// representative colour from it.
class SvgDocument {
public:
  /// The `viewBox`, or the intrinsic size when the file states no view box, or
  /// a unit square when it states neither.
  Rect<float> view_box() const { return view_box_; }

  /// The `width`/`height` the file asks for, in logical pixels. Zero on an axis
  /// the file left to the context -- which is the common case for an icon, and
  /// why the view box is what usually decides the aspect ratio.
  Size<float> intrinsic_size() const { return intrinsic_; }

  const SvgPreserveAspectRatio &preserve_aspect_ratio() const {
    return preserve_;
  }

  std::span<const SvgShape> shapes() const { return shapes_; }

  const std::shared_ptr<const Path> &path(std::uint32_t index) const {
    return paths_[index];
  }

  /// In the document's own user units, like everything else here.
  Rect<float> clip(std::uint32_t index) const { return clips_[index]; }

  const SvgDashPattern &dashes(std::uint16_t index) const {
    return dash_patterns_[index];
  }

  /// A `url(#id)` paint, already resolved against the shape that referenced it
  /// -- so a gradient in user space and one in bounding-box space arrive the
  /// same way.
  const Brush &brush(std::uint16_t index) const { return brushes_[index]; }

  /// What this document is costing, near enough for a cache budget.
  std::size_t byte_size() const;

  struct Result {
    std::shared_ptr<const SvgDocument> document;

    /// Empty when the source parsed. A document that parsed may still have
    /// skipped elements it does not support; only a failure to find any
    /// `<svg>` element at all is reported here.
    std::string error;

    explicit operator bool() const { return document != nullptr; }
  };

  /// Reads SVG source. Thread-safe and self-contained: it touches no global
  /// state and no resources, so it runs on a worker as happily as on the UI
  /// thread.
  static Result parse(std::string_view source);

private:
  friend class SvgBuilder;

  Rect<float> view_box_{0.0f, 0.0f, 1.0f, 1.0f};
  Size<float> intrinsic_{0.0f, 0.0f};
  SvgPreserveAspectRatio preserve_;

  std::vector<SvgShape> shapes_;
  std::vector<std::shared_ptr<const Path>> paths_;
  std::vector<Rect<float>> clips_;
  std::vector<SvgDashPattern> dash_patterns_;
  std::vector<Brush> brushes_;
};

} // namespace voidui
