// Exercises the path pipeline: both fill rules on a self-intersecting outline,
// curve flattening, and stroking with each join and cap style.
//
// Paths are rasterised on the CPU into exact-area coverage masks and drawn
// through the same textured pipeline as glyphs and images.

#include <cmath>

#include "voidui/core/window.h"
#include "voidui/widgets/canvas.h"

using namespace voidui;

namespace {

constexpr float kPi = 3.14159265358979323846f;

/// A five-pointed star as one self-intersecting contour. Under the nonzero
/// rule it fills solid; under even-odd the middle pentagon drops out. Nothing
/// else distinguishes the two rules as plainly.
Path star(Point<float> centre, float radius) {
  Path path;
  for (int i = 0; i < 5; ++i) {
    const float a = -kPi * 0.5f + static_cast<float>(i) * 4.0f * kPi / 5.0f;
    const Point<float> p(centre.x + std::cos(a) * radius, centre.y + std::sin(a) * radius);
    if (i == 0)
      path.move_to(p);
    else
      path.line_to(p);
  }
  path.close();
  return path;
}

void draw_scene(Rect<float> bounds, Painter &p) {
  const float w = bounds.size.width;
  const float h = bounds.size.height;
  const Color ink(28, 28, 36);

  const float r = w * 0.11f;

  // --- the two fill rules, same geometry ---
  p.fill_path(star(Point<float>(w * 0.20f, h * 0.16f), r), Paint(Color(240, 90, 90)),
              FillRule::NonZero);
  p.fill_path(star(Point<float>(w * 0.50f, h * 0.16f), r), Paint(Color(90, 150, 240)),
              FillRule::EvenOdd);

  // --- a filled star also stroked, as two independent commands ---
  const Path outlined = star(Point<float>(w * 0.80f, h * 0.16f), r);
  p.fill_path(outlined, Paint(Color(250, 200, 80)), FillRule::NonZero);
  p.stroke_path(outlined, Paint(ink), Pen(2.5f));

  // --- curves: a flattened cubic, stroked with round caps and joins ---
  Path wave;
  wave.move_to(Point<float>(w * 0.08f, h * 0.42f));
  wave.cubic_to(Point<float>(w * 0.30f, h * 0.28f), Point<float>(w * 0.45f, h * 0.58f),
                Point<float>(w * 0.62f, h * 0.42f));
  wave.quad_to(Point<float>(w * 0.78f, h * 0.30f), Point<float>(w * 0.92f, h * 0.45f));

  Pen round_pen(7.0f);
  round_pen.cap = LineCap::Round;
  round_pen.join = LineJoin::Round;
  p.stroke_path(wave, Paint(Color(80, 190, 160)), round_pen);

  // --- joins and caps: the same zig-zag under each style ---
  const char *labels[3] = {"miter", "round", "bevel"};
  (void)labels;

  for (int i = 0; i < 3; ++i) {
    const float y = h * (0.60f + static_cast<float>(i) * 0.135f);
    Path zig;
    zig.move_to(Point<float>(w * 0.10f, y));
    zig.line_to(Point<float>(w * 0.26f, y + h * 0.085f));
    zig.line_to(Point<float>(w * 0.42f, y));
    zig.line_to(Point<float>(w * 0.58f, y + h * 0.085f));

    Pen pen(9.0f);
    pen.join = static_cast<LineJoin>(i);
    pen.cap = static_cast<LineCap>(i);
    p.stroke_path(zig, Paint(Color(120, 110, 220)), pen);
  }

  // --- a path clipped by a rounded rect, to show the two systems compose ---
  p.save();
  p.clip_rrect(Rect<float>(w * 0.66f, h * 0.58f, w * 0.28f, h * 0.34f), Radius(w * 0.06f));
  for (int i = 0; i < 7; ++i) {
    p.fill_path(star(Point<float>(w * (0.68f + static_cast<float>(i % 3) * 0.11f),
                                  h * (0.62f + static_cast<float>(i / 3) * 0.13f)),
                     w * 0.05f),
                Paint(Color(255, 140, 60)), FillRule::NonZero);
  }
  p.restore();
}

} // namespace

int main() {
  Window window("VoidUI Paths", 420, 420);
  window.run(std::make_unique<Canvas>(draw_scene));
  return 0;
}
