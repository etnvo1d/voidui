#pragma once

#include <cstdint>
#include <span>
#include <vector>

#include "voidui/core/border.h"
#include "voidui/core/geometry.h"
#include "voidui/core/transform.h"

namespace voidui {

enum class PathVerb : std::uint8_t {
  Move,
  Line,
  Quad,
  Cubic,
  Close,
};

enum class FillRule : std::uint8_t {
  NonZero,
  EvenOdd,
};

/// Shapes the renderer can evaluate exactly in a fragment shader instead of
/// tessellating. A path keeps this tag as long as it was produced by one of the
/// shape factories and nothing has been appended since.
enum class AnalyticShape : std::uint8_t {
  None,
  Rect,
  RoundedRect,
};

/// A sequence of contours built from lines and Bezier segments.
///
/// Shape helpers such as `rounded_rect` are expressible here so callers have a
/// single vocabulary, but constructing one costs nothing extra: the analytic
/// tag survives into the display list and the renderer takes the four-vertex
/// fast path rather than flattening anything.
class Path {
public:
  Path() = default;

  static Path rect(Rect<float> bounds);
  static Path rounded_rect(Rect<float> bounds, Radius radius);
  static Path circle(Point<float> center, float radius);
  static Path oval(Rect<float> bounds);

  Path &move_to(Point<float> p);
  Path &line_to(Point<float> p);
  Path &quad_to(Point<float> control, Point<float> end);
  Path &cubic_to(Point<float> control1, Point<float> control2, Point<float> end);
  Path &close();

  void clear();

  /// Sizes both arenas up front. Worth calling when the counts are known --
  /// an SVG `d` attribute can be scanned for them before a single point is
  /// appended, which turns a hundred reallocations into one.
  void reserve(std::size_t verbs, std::size_t points);

  /// Maps every point through `t`, in place.
  ///
  /// This is how a transform is *baked*: an SVG group's matrix belongs to the
  /// geometry once and not to every frame that draws it. A baked path has no
  /// analytic tag afterwards -- the transform may have sheared the shape the
  /// tag was promising.
  Path &apply_transform(const Transform &t);

  bool empty() const { return verbs_.empty(); }
  const std::vector<PathVerb> &verbs() const { return verbs_; }
  const std::vector<Point<float>> &points() const { return points_; }

  /// A hash of the geometry, kept up to date as the path is built rather than
  /// computed on demand.
  ///
  /// The renderer keys its coverage-mask cache on the outline, so without this
  /// every frame that redraws a path re-reads every point in it just to find
  /// out that nothing changed -- for an icon of a few hundred points, on every
  /// icon, on every frame. Folding each appended point in as it arrives costs
  /// two multiplies at build time and makes the lookup O(1) forever after.
  ///
  /// It identifies geometry inside one process. A collision costs a wrong
  /// outline rather than anything worse, and the renderer mixes its own
  /// transform and pen state in on top.
  std::uint64_t content_hash() const { return hash_; }

  AnalyticShape analytic() const { return analytic_; }
  Rect<float> analytic_bounds() const { return analytic_bounds_; }
  Radius analytic_radius() const { return analytic_radius_; }

  /// Control-point bounding box. Loose for curves, which is fine for culling.
  Rect<float> bounds() const;

private:
  void begin_shape_(AnalyticShape shape, Rect<float> bounds, Radius radius);
  void invalidate_analytic_();

  void mix_(const void *data, std::size_t size);
  void mix_verb_point_(PathVerb verb, const Point<float> *points,
                       std::size_t count);
  void rehash_();

  std::vector<PathVerb> verbs_;
  std::vector<Point<float>> points_;

  static constexpr std::uint64_t kHashSeed = 14695981039346656037ull;
  std::uint64_t hash_ = kHashSeed;

  AnalyticShape analytic_ = AnalyticShape::None;
  Rect<float> analytic_bounds_{0.0f, 0.0f, 0.0f, 0.0f};
  Radius analytic_radius_{0.0f};
};

/// Approximates the path with line segments, in the path's own coordinates.
/// `tolerance` is the largest permitted deviation, in the same units.
///
/// Defined by the rasteriser, which needs it anyway; declared here because
/// dashing needs the same answer and there is no reason for two flatteners.
std::vector<std::vector<Point<float>>> flatten_path(const Path &path,
                                                    float tolerance);

/// Cuts `path` into the "on" runs of a dash pattern, as SVG's
/// `stroke-dasharray` and `stroke-dashoffset` define them.
///
/// The result is an ordinary path of open contours, meant to be stroked with
/// the pen the undashed outline would have used. Doing it this way rather than
/// teaching the pen about dashes is what keeps the dash out of the per-command
/// payload: a dashed outline is geometry, it is computed once where the shape
/// is built, and from there it caches and batches like any other path.
///
/// An empty or all-zero pattern returns the path unchanged, matching the way
/// CSS treats such a value as no dashing at all.
Path dash_path(const Path &path, std::span<const float> pattern, float offset,
               float tolerance);

} // namespace voidui
