// Exercises image drawing: small images packed into a shared page, one large
// enough to get a texture of its own, alpha compositing, tinting, and images
// under a rounded clip.
//
// The pixels are generated here so the example needs no decoder.

#include <cmath>
#include <vector>

#include "voidui/core/window.h"
#include "voidui/paint/image.h"
#include "voidui/widgets/canvas.h"

using namespace voidui;

namespace {

/// Transparent-cornered checkerboard, to show alpha survives the round trip.
std::shared_ptr<Image> checkerboard(int size, int cell) {
  std::vector<std::uint8_t> pixels(static_cast<std::size_t>(size) * size * 4);
  const float centre = static_cast<float>(size) * 0.5f;

  for (int y = 0; y < size; ++y) {
    for (int x = 0; x < size; ++x) {
      const bool dark = ((x / cell) + (y / cell)) % 2 == 0;
      const float dx = static_cast<float>(x) - centre;
      const float dy = static_cast<float>(y) - centre;
      const float edge = std::sqrt(dx * dx + dy * dy) / centre;
      const float alpha = std::clamp(1.6f - edge * 1.6f, 0.0f, 1.0f);

      std::uint8_t *p = &pixels[(static_cast<std::size_t>(y) * size + x) * 4];
      p[0] = dark ? 40 : 235;
      p[1] = dark ? 90 : 240;
      p[2] = dark ? 160 : 250;
      p[3] = static_cast<std::uint8_t>(alpha * 255.0f);
    }
  }

  return Image::from_rgba8(std::move(pixels), size, size);
}

/// A smooth field, sized past the shared-page threshold so it exercises the
/// standalone-texture path.
std::shared_ptr<Image> field(int width, int height) {
  std::vector<std::uint8_t> pixels(static_cast<std::size_t>(width) * height * 4);

  for (int y = 0; y < height; ++y) {
    for (int x = 0; x < width; ++x) {
      const float u = static_cast<float>(x) / static_cast<float>(width);
      const float v = static_cast<float>(y) / static_cast<float>(height);
      const float ripple = 0.5f + 0.5f * std::sin((u * 9.0f) + std::cos(v * 7.0f) * 2.0f);

      std::uint8_t *p = &pixels[(static_cast<std::size_t>(y) * width + x) * 4];
      p[0] = static_cast<std::uint8_t>((0.25f + 0.6f * u) * 255.0f);
      p[1] = static_cast<std::uint8_t>((0.30f + 0.5f * ripple) * 255.0f);
      p[2] = static_cast<std::uint8_t>((0.85f - 0.5f * v) * 255.0f);
      p[3] = 255;
    }
  }

  return Image::from_rgba8(std::move(pixels), width, height);
}

void draw_scene(Rect<float> bounds, Painter &p) {
  static const std::shared_ptr<Image> small = checkerboard(64, 8);
  static const std::shared_ptr<Image> large = field(700, 400);

  const float w = bounds.size.width;
  const float h = bounds.size.height;
  const float pad = w * 0.06f;

  p.fill_rect(Rect<float>(0.0f, 0.0f, w, h), Paint(Color(245, 246, 249)));

  // --- one large image, on its own texture, scaled down ---
  const Rect<float> hero(pad, h * 0.06f, w - pad * 2.0f, h * 0.30f);
  p.draw_shadow(hero, Radius(14.0f),
                Shadow(Color(0, 0, 0, 90), Point<float>(0.0f, 4.0f), 8.0f));
  p.save();
  p.clip_rrect(hero, Radius(14.0f));
  p.draw_image(large, hero);
  p.restore();

  // --- the same small image at three scales, all from the shared page ---
  float x = pad;
  for (float size : {32.0f, 56.0f, 84.0f}) {
    p.draw_image(small, Rect<float>(x, h * 0.42f, size, size));
    x += size + w * 0.04f;
  }

  // --- tint and opacity ---
  x = pad;
  const Color tints[3] = {Color(255, 255, 255), Color(255, 255, 255, 128),
                          Color(255, 255, 255, 40)};
  for (int i = 0; i < 3; ++i) {
    Paint paint(tints[i]);
    p.draw_image(small, Rect<float>(x, h * 0.62f, 64.0f, 64.0f), paint);
    x += 64.0f + w * 0.04f;
  }

  // --- an image clipped to a circle, next to the shape pipeline ---
  const float d = h * 0.16f;
  const Rect<float> avatar(w - pad - d, h * 0.62f, d, d);
  p.save();
  p.clip_rrect(avatar, Radius(d * 0.5f));
  p.draw_image(large, avatar);
  p.restore();
  p.stroke_rrect(avatar, Radius(d * 0.5f), Paint(Color(40, 44, 56)),
                 Pen(2.0f, StrokeAlign::Inside));

  // --- images interleaved with shapes, to show batches stay in order ---
  for (int i = 0; i < 6; ++i) {
    const float bx = pad + static_cast<float>(i) * (w - pad * 2.0f) / 6.0f;
    const Rect<float> cell(bx, h * 0.84f, (w - pad * 2.0f) / 6.0f - 6.0f, h * 0.10f);
    p.fill_rrect(cell, Radius(6.0f), Paint(Color(220, 224, 232)));
    p.draw_image(small, Rect<float>(cell.origin.x + 4.0f, cell.origin.y + 4.0f,
                                    cell.size.height - 8.0f, cell.size.height - 8.0f));
  }
}

} // namespace

int main() {
  Window window("VoidUI Images", 440, 440);
  window.run(std::make_unique<Canvas>(draw_scene));
  return 0;
}
