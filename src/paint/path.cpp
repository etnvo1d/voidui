#include "voidui/paint/path.h"

#include <algorithm>
#include <array>
#include <cmath>

namespace voidui {

namespace {

/// Distance from a circle's axis endpoints to the Bezier control points that
/// approximate a quarter arc. The classic 4/3*(sqrt(2)-1).
constexpr float kArcMagic = 0.5522847498307933f;

} // namespace

void Path::invalidate_analytic_() { analytic_ = AnalyticShape::None; }

void Path::mix_(const void *data, std::size_t size) {
  // FNV-1a over the bytes as they are appended. Order-sensitive, which is what
  // is wanted: two paths visiting the same points in a different order are two
  // different outlines.
  const auto *bytes = static_cast<const unsigned char *>(data);
  for (std::size_t i = 0; i < size; ++i) {
    hash_ ^= static_cast<std::uint64_t>(bytes[i]);
    hash_ *= 1099511628211ull;
  }
}

void Path::mix_verb_point_(PathVerb verb, const Point<float> *points,
                           std::size_t count) {
  mix_(&verb, sizeof(verb));
  mix_(points, count * sizeof(Point<float>));
}

void Path::rehash_() {
  // Replays the build order rather than hashing the two arenas end to end, so
  // a rebuilt path and a rehashed one agree. They must: the two would otherwise
  // be separate entries in the renderer's mask cache for the same outline.
  hash_ = kHashSeed;
  std::size_t at = 0;
  for (const PathVerb verb : verbs_) {
    std::size_t count = 0;
    switch (verb) {
    case PathVerb::Move:
    case PathVerb::Line:
      count = 1;
      break;
    case PathVerb::Quad:
      count = 2;
      break;
    case PathVerb::Cubic:
      count = 3;
      break;
    case PathVerb::Close:
      count = 0;
      break;
    }
    mix_verb_point_(verb, points_.data() + at, count);
    at += count;
  }
}

void Path::reserve(std::size_t verbs, std::size_t points) {
  verbs_.reserve(verbs);
  points_.reserve(points);
}

Path &Path::apply_transform(const Transform &t) {
  for (Point<float> &p : points_)
    p = t.apply(p);

  // The verbs did not move, but the incremental hash cannot be unwound, so the
  // whole thing is folded again. Baking happens once per path, not per frame.
  rehash_();
  invalidate_analytic_();
  return *this;
}

void Path::begin_shape_(AnalyticShape shape, Rect<float> bounds, Radius radius) {
  analytic_ = shape;
  analytic_bounds_ = bounds;
  analytic_radius_ = radius;
}

Path &Path::move_to(Point<float> p) {
  invalidate_analytic_();
  verbs_.push_back(PathVerb::Move);
  points_.push_back(p);
  mix_verb_point_(PathVerb::Move, &p, 1);
  return *this;
}

Path &Path::line_to(Point<float> p) {
  invalidate_analytic_();
  verbs_.push_back(PathVerb::Line);
  points_.push_back(p);
  mix_verb_point_(PathVerb::Line, &p, 1);
  return *this;
}

Path &Path::quad_to(Point<float> control, Point<float> end) {
  invalidate_analytic_();
  verbs_.push_back(PathVerb::Quad);
  points_.push_back(control);
  points_.push_back(end);
  const Point<float> added[2]{control, end};
  mix_verb_point_(PathVerb::Quad, added, 2);
  return *this;
}

Path &Path::cubic_to(Point<float> c1, Point<float> c2, Point<float> end) {
  invalidate_analytic_();
  verbs_.push_back(PathVerb::Cubic);
  points_.push_back(c1);
  points_.push_back(c2);
  points_.push_back(end);
  const Point<float> added[3]{c1, c2, end};
  mix_verb_point_(PathVerb::Cubic, added, 3);
  return *this;
}

Path &Path::close() {
  // Closing does not disturb the analytic tag: the shape factories rely on it.
  verbs_.push_back(PathVerb::Close);
  mix_verb_point_(PathVerb::Close, nullptr, 0);
  return *this;
}

void Path::clear() {
  verbs_.clear();
  points_.clear();
  hash_ = kHashSeed;
  invalidate_analytic_();
}

Path Path::rect(Rect<float> bounds) {
  Path path;
  const float x0 = bounds.origin.x;
  const float y0 = bounds.origin.y;
  const float x1 = x0 + bounds.size.width;
  const float y1 = y0 + bounds.size.height;

  path.move_to(Point<float>(x0, y0));
  path.line_to(Point<float>(x1, y0));
  path.line_to(Point<float>(x1, y1));
  path.line_to(Point<float>(x0, y1));
  path.close();

  path.begin_shape_(AnalyticShape::Rect, bounds, Radius(0.0f));
  return path;
}

Path Path::rounded_rect(Rect<float> bounds, Radius radius) {
  const float w = bounds.size.width;
  const float h = bounds.size.height;

  // A radius may not exceed half the shorter side, matching what the shader
  // clamps to; doing it here keeps the geometry and the fast path in agreement.
  const float limit = std::max(std::min(w, h) * 0.5f, 0.0f);
  const auto clamp_radius = [limit](float value) {
    return std::clamp(value, 0.0f, limit);
  };

  const Radius r(clamp_radius(radius.left_top), clamp_radius(radius.right_top),
                 clamp_radius(radius.right_bottom), clamp_radius(radius.left_bottom));

  if (r.left_top == 0.0f && r.right_top == 0.0f && r.right_bottom == 0.0f &&
      r.left_bottom == 0.0f) {
    return rect(bounds);
  }

  Path path;
  const float x0 = bounds.origin.x;
  const float y0 = bounds.origin.y;
  const float x1 = x0 + w;
  const float y1 = y0 + h;
  const float k = kArcMagic;

  path.move_to(Point<float>(x0 + r.left_top, y0));
  path.line_to(Point<float>(x1 - r.right_top, y0));
  path.cubic_to(Point<float>(x1 - r.right_top + r.right_top * k, y0),
                Point<float>(x1, y0 + r.right_top - r.right_top * k),
                Point<float>(x1, y0 + r.right_top));

  path.line_to(Point<float>(x1, y1 - r.right_bottom));
  path.cubic_to(Point<float>(x1, y1 - r.right_bottom + r.right_bottom * k),
                Point<float>(x1 - r.right_bottom + r.right_bottom * k, y1),
                Point<float>(x1 - r.right_bottom, y1));

  path.line_to(Point<float>(x0 + r.left_bottom, y1));
  path.cubic_to(Point<float>(x0 + r.left_bottom - r.left_bottom * k, y1),
                Point<float>(x0, y1 - r.left_bottom + r.left_bottom * k),
                Point<float>(x0, y1 - r.left_bottom));

  path.line_to(Point<float>(x0, y0 + r.left_top));
  path.cubic_to(Point<float>(x0, y0 + r.left_top - r.left_top * k),
                Point<float>(x0 + r.left_top - r.left_top * k, y0),
                Point<float>(x0 + r.left_top, y0));
  path.close();

  path.begin_shape_(AnalyticShape::RoundedRect, bounds, r);
  return path;
}

Path Path::oval(Rect<float> bounds) {
  const float w = std::max(bounds.size.width, 0.0f);
  const float h = std::max(bounds.size.height, 0.0f);

  // A circle is exactly a rounded rect whose corner radii are half its side, so
  // it keeps the analytic tag. A genuine ellipse cannot be: Radius carries one
  // scalar per corner, not a pair of semi-axes.
  if (std::abs(w - h) < 1e-4f)
    return rounded_rect(bounds, Radius(w * 0.5f));

  const float rx = w * 0.5f;
  const float ry = h * 0.5f;
  const float cx = bounds.origin.x + rx;
  const float cy = bounds.origin.y + ry;
  const float kx = rx * kArcMagic;
  const float ky = ry * kArcMagic;

  Path path;
  path.move_to(Point<float>(cx, cy - ry));
  path.cubic_to(Point<float>(cx + kx, cy - ry), Point<float>(cx + rx, cy - ky),
                Point<float>(cx + rx, cy));
  path.cubic_to(Point<float>(cx + rx, cy + ky), Point<float>(cx + kx, cy + ry),
                Point<float>(cx, cy + ry));
  path.cubic_to(Point<float>(cx - kx, cy + ry), Point<float>(cx - rx, cy + ky),
                Point<float>(cx - rx, cy));
  path.cubic_to(Point<float>(cx - rx, cy - ky), Point<float>(cx - kx, cy - ry),
                Point<float>(cx, cy - ry));
  path.close();
  return path;
}

Path Path::circle(Point<float> center, float radius) {
  const float r = std::max(radius, 0.0f);
  return rounded_rect(Rect<float>(center.x - r, center.y - r, r * 2.0f, r * 2.0f),
                      Radius(r));
}

namespace {

/// Where a walk along a contour currently sits in the dash pattern.
///
/// The pattern is already normalised to an even length, so `on` alternates with
/// the index and no entry has to say which it is. A zero-length entry is legal
/// -- `stroke-dasharray: 0 4` is how a dotted line is written -- so every step
/// has to tolerate consuming nothing and moving on; progress is guaranteed by
/// the caller having checked that the pattern sums to something positive.
class DashWalker {
public:
  DashWalker(std::span<const float> pattern, float offset, float total)
      : pattern_(pattern) {
    float ahead = std::fmod(offset, total);
    if (ahead < 0.0f)
      ahead += total;

    remaining_ = pattern_[0];
    while (ahead > 0.0f) {
      if (ahead < remaining_) {
        remaining_ -= ahead;
        break;
      }
      ahead -= remaining_;
      step();
    }
  }

  bool on() const { return on_; }
  float remaining() const { return remaining_; }

  void consume(float length) { remaining_ -= length; }

  void step() {
    index_ = (index_ + 1) % pattern_.size();
    on_ = !on_;
    remaining_ = pattern_[index_];
  }

private:
  std::span<const float> pattern_;
  std::size_t index_ = 0;
  float remaining_ = 0.0f;
  bool on_ = true;
};

Point<float> lerp_point(Point<float> a, Point<float> b, float t) {
  return Point<float>(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
}

} // namespace

Path dash_path(const Path &path, std::span<const float> pattern, float offset,
               float tolerance) {
  // An even-length copy of the pattern: SVG repeats an odd list once so that
  // "on" and "off" alternate, and doing it here means the walk never has to ask
  // how long the list was.
  std::array<float, 16> storage{};
  std::size_t count = 0;
  float total = 0.0f;
  const std::size_t repeats = (pattern.size() % 2 == 0) ? 1 : 2;
  for (std::size_t r = 0; r < repeats; ++r) {
    for (const float entry : pattern) {
      if (!(entry >= 0.0f) || count >= storage.size())
        return path;
      storage[count++] = entry;
      total += entry;
    }
  }

  if (count == 0 || !(total > 0.0f))
    return path;

  const std::span<const float> dashes(storage.data(), count);

  Path out;
  for (const std::vector<Point<float>> &contour : flatten_path(path, tolerance)) {
    if (contour.size() < 2)
      continue;

    DashWalker walker(dashes, offset, total);
    if (walker.on())
      out.move_to(contour[0]);

    for (std::size_t i = 1; i < contour.size(); ++i) {
      const Point<float> from = contour[i - 1];
      const Point<float> to = contour[i];
      const float dx = to.x - from.x;
      const float dy = to.y - from.y;
      const float length = std::sqrt(dx * dx + dy * dy);
      if (!(length > 0.0f))
        continue;

      float travelled = 0.0f;
      while (travelled < length) {
        const float step = std::min(walker.remaining(), length - travelled);
        travelled += step;
        walker.consume(step);

        const Point<float> at = lerp_point(from, to, travelled / length);
        if (walker.on())
          out.line_to(at);

        if (walker.remaining() > 0.0f)
          continue;

        // A dash ended exactly here. The run that just finished already has
        // its endpoint from the line_to above, so turning "off" emits nothing
        // and turning "on" only opens the next contour. A zero-length "on" run
        // still leaves two coincident points behind, which is what makes a
        // round cap render the dot that `stroke-dasharray: 0 4` asks for.
        walker.step();
        if (walker.on())
          out.move_to(at);
      }
    }
  }

  return out;
}

Rect<float> Path::bounds() const {
  if (points_.empty())
    return Rect<float>(0.0f, 0.0f, 0.0f, 0.0f);

  float min_x = points_[0].x, max_x = points_[0].x;
  float min_y = points_[0].y, max_y = points_[0].y;

  for (const Point<float> &p : points_) {
    min_x = std::min(min_x, p.x);
    max_x = std::max(max_x, p.x);
    min_y = std::min(min_y, p.y);
    max_y = std::max(max_y, p.y);
  }

  return Rect<float>(min_x, min_y, max_x - min_x, max_y - min_y);
}

} // namespace voidui
