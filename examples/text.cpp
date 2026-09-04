// Exercises text: the platform's own UI font and fallback chain, HarfBuzz
// shaping, FreeType rasterisation into the shared atlas, subpixel-positioned
// runs at several sizes, and text under a clip.
//
// Nothing here names a font file. FontStack::system_ui asks the operating
// system which family it uses for interface text, and shaping walks the
// platform fallback chain, so mixed-script text needs no configuration.

#include <string>
#include <vector>

#include "voidui/core/window.h"
#include "voidui/paint/font.h"
#include "voidui/widgets/canvas.h"

using namespace voidui;

namespace {

/// The sources are UTF-8; this just documents that intent at the call sites.
std::string u8_str(const char *text) { return text; }

std::shared_ptr<FontStack> ui_font(float size) {
  if (auto stack = FontStack::system_ui(size))
    return stack;

  // No platform provider (or no usable system font): fall back to whatever is
  // lying around, which is what the rest of this renderer used to require.
  auto font = Font::from_first_available(
      {"C:/Windows/Fonts/msyh.ttc", "C:/Windows/Fonts/segoeui.ttf",
       "/System/Library/Fonts/SFNS.ttf",
       "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"},
      size);
  return FontStack::from_font(font);
}

void draw_scene(Rect<float> bounds, Painter &p) {
  const float w = bounds.size.width;
  const float h = bounds.size.height;
  const Color ink(26, 26, 34);
  const Color dim(120, 126, 140);

  p.fill_rect(Rect<float>(0.0f, 0.0f, w, h), Paint(Color(252, 251, 248)));

  auto body = ui_font(15.0f);
  if (!body) {
    p.fill_rrect(Rect<float>(w * 0.07f, h * 0.45f, w * 0.86f, 40.0f), Radius(8.0f),
                 Paint(Color(220, 80, 80)));
    return;
  }

  float y = h * 0.08f;

  // What the platform actually chose, so the demo is self-describing.
  {
    auto small = ui_font(12.0f);
    const std::string label = "system UI family: " + body->family() +
                              "   locale: " + body->locale();
    p.draw_text(Point<float>(w * 0.07f, y), small, label, Paint(dim));
    y += 26.0f;
  }

  // --- a size ramp ---
  for (float size : {12.0f, 16.0f, 22.0f}) {
    auto font = ui_font(size);
    p.draw_text(Point<float>(w * 0.07f, y), font, "Hamburgefonstiv 0123", Paint(ink));
    y += font->line_height() * 1.2f;
  }

  y += 10.0f;

  // --- mixed scripts in one string: the base face covers none of the CJK, so
  //     every one of these lines crosses at least one fallback boundary ---
  const std::vector<std::string> lines = {
      u8_str("English 中文 日本語 한국어"),
      u8_str("你好，世界。Hello, world."),
      u8_str("温度 24°C · 湿度 60%"),
      u8_str("😀 🎨 🚀 emoji"),
  };

  for (const std::string &line : lines) {
    p.draw_text(Point<float>(w * 0.07f, y), body, line, Paint(ink));
    y += body->line_height() * 1.15f;
  }

  y += 12.0f;

  // --- text under a rounded clip, composed with the shape pipeline ---
  const Rect<float> panel(w * 0.07f, y, w * 0.86f, h * 0.16f);
  p.fill_rrect(panel, Radius(12.0f), Paint(Color(238, 240, 246)));
  p.stroke_rrect(panel, Radius(12.0f), Paint(Color(200, 206, 218)),
                 Pen(1.0f, StrokeAlign::Inside));

  p.save();
  p.clip_rrect(panel, Radius(12.0f));
  p.draw_text(Point<float>(panel.origin.x + 14.0f, panel.origin.y + 26.0f), body,
              "\xe8\xa2\xab\xe8\xa3\x81\xe5\x88\x87\xe7\x9a\x84\xe6\x96\x87\xe6\x9c\xac"
              " -- deliberately too long to fit inside this panel",
              Paint(ink));
  p.restore();
}

} // namespace

int main() {
  Window window("VoidUI Text", 460, 420);
  window.run(std::make_unique<Canvas>(draw_scene));
  return 0;
}
