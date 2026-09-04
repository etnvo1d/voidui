#include "render/rasterizer.h"

#include <algorithm>
#include <cmath>

namespace voidui {

namespace {

constexpr float kPi = 3.14159265358979323846f;

Point<float> lerp(Point<float> a, Point<float> b, float t) {
  return Point<float>(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
}

float length(Point<float> v) { return std::sqrt(v.x * v.x + v.y * v.y); }

Point<float> normalize(Point<float> v) {
  const float len = length(v);
  return len > 1e-9f ? Point<float>(v.x / len, v.y / len) : Point<float>(0.0f, 0.0f);
}

/// Segment count that keeps a curve within `tolerance` of its true shape. The
/// deviation of a Bezier from its chord scales with the second difference of
/// the control points, and subdividing into n pieces cuts it by n^2.
int segments_for(float deviation, float tolerance) {
  if (!(deviation > 0.0f) || tolerance <= 0.0f)
    return 1;
  return std::clamp(static_cast<int>(std::ceil(std::sqrt(deviation / tolerance))), 1, 256);
}

void flatten_quad(Point<float> p0, Point<float> p1, Point<float> p2, float tolerance,
                  std::vector<Point<float>> &out) {
  const Point<float> second(p0.x - 2.0f * p1.x + p2.x, p0.y - 2.0f * p1.y + p2.y);
  const int n = segments_for(length(second) * 0.25f, tolerance);

  for (int i = 1; i <= n; ++i) {
    const float t = static_cast<float>(i) / static_cast<float>(n);
    out.push_back(lerp(lerp(p0, p1, t), lerp(p1, p2, t), t));
  }
}

void flatten_cubic(Point<float> p0, Point<float> p1, Point<float> p2, Point<float> p3,
                   float tolerance, std::vector<Point<float>> &out) {
  const Point<float> d1(p0.x - 2.0f * p1.x + p2.x, p0.y - 2.0f * p1.y + p2.y);
  const Point<float> d2(p1.x - 2.0f * p2.x + p3.x, p1.y - 2.0f * p2.y + p3.y);
  const int n = segments_for(std::max(length(d1), length(d2)) * 0.75f, tolerance);

  for (int i = 1; i <= n; ++i) {
    const float t = static_cast<float>(i) / static_cast<float>(n);
    const Point<float> a = lerp(p0, p1, t);
    const Point<float> b = lerp(p1, p2, t);
    const Point<float> c = lerp(p2, p3, t);
    out.push_back(lerp(lerp(a, b, t), lerp(b, c, t), t));
  }
}

/// Signed-area accumulator.
///
/// Every edge deposits, into each pixel it crosses, the signed area it sweeps
/// there. A prefix sum along the row turns those deltas into the fractional
/// winding number, from which both fill rules follow exactly. This is the same
/// machinery a font rasteriser uses, and it is why paths take the CPU route
/// here: it yields 256 levels of coverage where 4x MSAA would yield five.
class Accumulator {
public:
  Accumulator(int width, int height) : width_(width), height_(height) {
    // A spare column absorbs what an edge deposits past the right-hand side.
    area_.assign(static_cast<std::size_t>(width + 1) * height, 0.0f);
  }

  void add_line(Point<float> p0, Point<float> p1);
  void resolve(FillRule rule, std::vector<std::uint8_t> &out) const;

private:
  /// Splits one pixel's worth of swept area between the pixel the edge sits in
  /// and its right-hand neighbour, in proportion to where the edge crosses.
  void deposit(float *row, int x, float amount, float x_mid) {
    if (x >= width_)
      return;

    if (x < 0) {
      // Wholly left of the mask: the whole row past this point is inside.
      row[0] += amount;
      return;
    }

    row[x] += amount * (1.0f - x_mid);
    row[x + 1] += amount * x_mid;
  }

  int width_;
  int height_;
  std::vector<float> area_;
};

void Accumulator::add_line(Point<float> p0, Point<float> p1) {
  if (p0.y == p1.y)
    return;

  float direction = 1.0f;
  if (p1.y < p0.y) {
    std::swap(p0, p1);
    direction = -1.0f;
  }

  const float dxdy = (p1.x - p0.x) / (p1.y - p0.y);

  const int y_begin = std::max(static_cast<int>(std::floor(p0.y)), 0);
  const int y_end = std::min(static_cast<int>(std::ceil(p1.y)), height_);

  for (int y = y_begin; y < y_end; ++y) {
    const float top = std::max(p0.y, static_cast<float>(y));
    const float bottom = std::min(p1.y, static_cast<float>(y) + 1.0f);
    const float dy = bottom - top;
    if (dy <= 0.0f)
      continue;

    float xa = p0.x + (top - p0.y) * dxdy;
    float xb = p0.x + (bottom - p0.y) * dxdy;
    if (xa > xb)
      std::swap(xa, xb);

    float *row = &area_[static_cast<std::size_t>(y) * (width_ + 1)];

    const int xa_index = static_cast<int>(std::floor(xa));
    const int xb_index = static_cast<int>(std::floor(xb));

    if (xa_index == xb_index) {
      const float mid = 0.5f * (xa + xb) - static_cast<float>(xa_index);
      deposit(row, xa_index, direction * dy, std::clamp(mid, 0.0f, 1.0f));
      continue;
    }

    // The edge slants across several columns; hand each the share of dy that
    // corresponds to its slice of the horizontal span.
    const float inv_dx = 1.0f / (xb - xa);

    const float first_edge = static_cast<float>(xa_index + 1);
    const float first_dy = dy * (first_edge - xa) * inv_dx;
    deposit(row, xa_index, direction * first_dy,
            std::clamp(0.5f * (xa + first_edge) - static_cast<float>(xa_index), 0.0f, 1.0f));

    const float column_dy = dy * inv_dx;
    for (int x = xa_index + 1; x < xb_index; ++x)
      deposit(row, x, direction * column_dy, 0.5f);

    const float last_edge = static_cast<float>(xb_index);
    const float last_dy = dy * (xb - last_edge) * inv_dx;
    deposit(row, xb_index, direction * last_dy,
            std::clamp(0.5f * (last_edge + xb) - static_cast<float>(xb_index), 0.0f, 1.0f));
  }
}

void Accumulator::resolve(FillRule rule, std::vector<std::uint8_t> &out) const {
  out.assign(static_cast<std::size_t>(width_) * height_, 0);

  for (int y = 0; y < height_; ++y) {
    const float *row = &area_[static_cast<std::size_t>(y) * (width_ + 1)];
    std::uint8_t *dst = &out[static_cast<std::size_t>(y) * width_];

    float winding = 0.0f;
    for (int x = 0; x < width_; ++x) {
      winding += row[x];

      float coverage;
      if (rule == FillRule::NonZero) {
        coverage = std::min(std::abs(winding), 1.0f);
      } else {
        // Triangle wave of period two -- the even-odd rule extended to the
        // fractional winding numbers that antialiasing produces.
        coverage = std::abs(winding - 2.0f * std::round(winding * 0.5f));
      }

      dst[x] =
          static_cast<std::uint8_t>(std::clamp(coverage, 0.0f, 1.0f) * 255.0f + 0.5f);
    }
  }
}

bool is_closed(const std::vector<Point<float>> &contour) {
  return contour.size() > 2 && std::abs(contour.front().x - contour.back().x) < 1e-5f &&
         std::abs(contour.front().y - contour.back().y) < 1e-5f;
}

float signed_area(const std::vector<Point<float>> &contour) {
  float total = 0.0f;
  for (std::size_t i = 0, n = contour.size(); i < n; ++i) {
    const Point<float> &a = contour[i];
    const Point<float> &b = contour[(i + 1) % n];
    total += a.x * b.y - b.x * a.y;
  }
  return total * 0.5f;
}

/// Emits one convex piece of the stroke as its own closed contour, wound
/// consistently so overlapping pieces union under the nonzero rule instead of
/// cancelling. Stamping pieces this way avoids offset-curve arithmetic
/// entirely, and self-overlap on tight corners resolves for free.
void emit(Path &out, const std::vector<Point<float>> &polygon) {
  if (polygon.size() < 3)
    return;

  float area = 0.0f;
  for (std::size_t i = 0, n = polygon.size(); i < n; ++i) {
    const Point<float> &a = polygon[i];
    const Point<float> &b = polygon[(i + 1) % n];
    area += a.x * b.y - b.x * a.y;
  }

  if (area >= 0.0f) {
    out.move_to(polygon.front());
    for (std::size_t i = 1; i < polygon.size(); ++i)
      out.line_to(polygon[i]);
  } else {
    out.move_to(polygon.back());
    for (std::size_t i = polygon.size(); i-- > 1;)
      out.line_to(polygon[i - 1]);
  }
  out.close();
}

void emit_disc(Path &out, Point<float> centre, float radius, float tolerance) {
  const int steps = std::clamp(
      static_cast<int>(std::ceil(kPi / std::acos(std::max(1.0f - tolerance / radius, -1.0f)))),
      6, 128);

  std::vector<Point<float>> circle;
  circle.reserve(steps);
  for (int i = 0; i < steps; ++i) {
    const float a = 2.0f * kPi * static_cast<float>(i) / static_cast<float>(steps);
    circle.push_back(Point<float>(centre.x + std::cos(a) * radius,
                                  centre.y + std::sin(a) * radius));
  }
  emit(out, circle);
}

} // namespace

std::vector<std::vector<Point<float>>> flatten_path(const Path &path, float tolerance) {
  std::vector<std::vector<Point<float>>> contours;
  std::vector<Point<float>> current;
  std::size_t index = 0;
  Point<float> cursor(0.0f, 0.0f);
  Point<float> start(0.0f, 0.0f);

  const auto flush = [&]() {
    if (current.size() >= 2)
      contours.push_back(current);
    current.clear();
  };

  for (PathVerb verb : path.verbs()) {
    switch (verb) {
    case PathVerb::Move:
      flush();
      cursor = start = path.points()[index++];
      current.push_back(cursor);
      break;
    case PathVerb::Line:
      cursor = path.points()[index++];
      current.push_back(cursor);
      break;
    case PathVerb::Quad: {
      const Point<float> c = path.points()[index++];
      const Point<float> e = path.points()[index++];
      flatten_quad(cursor, c, e, tolerance, current);
      cursor = e;
      break;
    }
    case PathVerb::Cubic: {
      const Point<float> c1 = path.points()[index++];
      const Point<float> c2 = path.points()[index++];
      const Point<float> e = path.points()[index++];
      flatten_cubic(cursor, c1, c2, e, tolerance, current);
      cursor = e;
      break;
    }
    case PathVerb::Close:
      if (!current.empty())
        current.push_back(start);
      cursor = start;
      break;
    }
  }

  flush();
  return contours;
}

Path stroke_to_fill(const Path &path, const Pen &pen, float tolerance) {
  Path out;
  const float half = std::max(pen.width, 0.0f) * 0.5f;
  if (half <= 0.0f)
    return out;

  for (auto contour : flatten_path(path, tolerance)) {
    const bool closed = is_closed(contour);
    if (closed)
      contour.pop_back();

    // Stroke alignment shifts the centreline; which way is "in" depends on how
    // the contour winds, so consult its signed area.
    if (pen.align != StrokeAlign::Center && closed && contour.size() >= 3) {
      const float shift = (pen.align == StrokeAlign::Inside) ? -half : half;
      const float orientation = signed_area(contour) >= 0.0f ? 1.0f : -1.0f;

      std::vector<Point<float>> shifted;
      shifted.reserve(contour.size());
      for (std::size_t i = 0, n = contour.size(); i < n; ++i) {
        const Point<float> prev = contour[(i + n - 1) % n];
        const Point<float> next = contour[(i + 1) % n];
        const Point<float> tangent = normalize(Point<float>(next.x - prev.x, next.y - prev.y));
        const Point<float> normal(-tangent.y * orientation, tangent.x * orientation);
        shifted.push_back(
            Point<float>(contour[i].x + normal.x * shift, contour[i].y + normal.y * shift));
      }
      contour = std::move(shifted);
    }

    if (contour.size() < 2) {
      if (contour.size() == 1 && pen.cap == LineCap::Round)
        emit_disc(out, contour.front(), half, tolerance);
      continue;
    }

    const std::size_t segments = closed ? contour.size() : contour.size() - 1;

    for (std::size_t i = 0; i < segments; ++i) {
      const Point<float> a = contour[i];
      const Point<float> b = contour[(i + 1) % contour.size()];
      const Point<float> dir = normalize(Point<float>(b.x - a.x, b.y - a.y));
      if (dir.x == 0.0f && dir.y == 0.0f)
        continue;

      const Point<float> n(-dir.y * half, dir.x * half);

      Point<float> a0 = a, b0 = b;
      if (!closed && pen.cap == LineCap::Square) {
        if (i == 0) {
          a0 = Point<float>(a.x - dir.x * half, a.y - dir.y * half);
        }
        if (i + 1 == segments) {
          b0 = Point<float>(b.x + dir.x * half, b.y + dir.y * half);
        }
      }

      emit(out, {Point<float>(a0.x + n.x, a0.y + n.y),
                 Point<float>(b0.x + n.x, b0.y + n.y),
                 Point<float>(b0.x - n.x, b0.y - n.y),
                 Point<float>(a0.x - n.x, a0.y - n.y)});
    }

    // Joins. Each is stamped as its own convex piece; the nonzero rule unions
    // them with the segment quads on either side, so nothing has to be trimmed.
    const std::size_t joins = closed ? contour.size() : contour.size() - 1;
    for (std::size_t i = closed ? 0 : 1; i < joins; ++i) {
      const Point<float> prev = contour[(i + contour.size() - 1) % contour.size()];
      const Point<float> here = contour[i];
      const Point<float> next = contour[(i + 1) % contour.size()];

      const Point<float> d0 = normalize(Point<float>(here.x - prev.x, here.y - prev.y));
      const Point<float> d1 = normalize(Point<float>(next.x - here.x, next.y - here.y));
      if ((d0.x == 0.0f && d0.y == 0.0f) || (d1.x == 0.0f && d1.y == 0.0f))
        continue;

      if (pen.join == LineJoin::Round) {
        emit_disc(out, here, half, tolerance);
        continue;
      }

      // Outer side of the turn: the one the two segment quads leave a gap on.
      const float cross = d0.x * d1.y - d0.y * d1.x;
      if (std::abs(cross) < 1e-6f)
        continue;
      const float side = cross < 0.0f ? 1.0f : -1.0f;

      const Point<float> o0(here.x - d0.y * half * side, here.y + d0.x * half * side);
      const Point<float> o1(here.x - d1.y * half * side, here.y + d1.x * half * side);

      if (pen.join == LineJoin::Miter) {
        const float cos_half =
            std::sqrt(std::max(0.5f * (1.0f + (d0.x * d1.x + d0.y * d1.y)), 0.0f));

        if (cos_half > 1e-4f && 1.0f / cos_half <= pen.miter_limit) {
          const Point<float> bisector = normalize(
              Point<float>((o0.x - here.x) + (o1.x - here.x),
                           (o0.y - here.y) + (o1.y - here.y)));
          const float extent = half / cos_half;
          emit(out, {here, o0,
                     Point<float>(here.x + bisector.x * extent,
                                  here.y + bisector.y * extent),
                     o1});
          continue;
        }
        // Past the miter limit a join degenerates to a bevel, as it should.
      }

      emit(out, {here, o0, o1});
    }

    if (!closed && pen.cap == LineCap::Round) {
      emit_disc(out, contour.front(), half, tolerance);
      emit_disc(out, contour.back(), half, tolerance);
    }
  }

  return out;
}

Mask rasterize_path(const Path &path, const Transform &to_device, FillRule rule) {
  const float scale = std::max(to_device.approximate_scale(), 1e-4f);
  auto contours = flatten_path(path, 0.1f / scale);

  if (contours.empty())
    return Mask{};

  float min_x = 1e30f, min_y = 1e30f, max_x = -1e30f, max_y = -1e30f;
  for (auto &contour : contours) {
    for (Point<float> &p : contour) {
      p = to_device.apply(p);
      min_x = std::min(min_x, p.x);
      min_y = std::min(min_y, p.y);
      max_x = std::max(max_x, p.x);
      max_y = std::max(max_y, p.y);
    }
  }

  // A one-pixel skirt keeps the antialiased edge inside the mask.
  const int origin_x = static_cast<int>(std::floor(min_x)) - 1;
  const int origin_y = static_cast<int>(std::floor(min_y)) - 1;
  const int width = static_cast<int>(std::ceil(max_x)) - origin_x + 2;
  const int height = static_cast<int>(std::ceil(max_y)) - origin_y + 2;

  if (width <= 0 || height <= 0 || width > 8192 || height > 8192)
    return Mask{};

  Accumulator accumulator(width, height);

  for (auto &contour : contours) {
    if (contour.size() < 2)
      continue;

    for (Point<float> &p : contour) {
      p.x -= static_cast<float>(origin_x);
      p.y -= static_cast<float>(origin_y);
    }

    for (std::size_t i = 1; i < contour.size(); ++i)
      accumulator.add_line(contour[i - 1], contour[i]);

    // An open contour still bounds a region; close it so the windings balance.
    if (contour.front().x != contour.back().x || contour.front().y != contour.back().y)
      accumulator.add_line(contour.back(), contour.front());
  }

  Mask mask;
  mask.width = width;
  mask.height = height;
  mask.origin_x = origin_x;
  mask.origin_y = origin_y;
  accumulator.resolve(rule, mask.pixels);
  return mask;
}

} // namespace voidui
