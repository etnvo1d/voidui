// Exercises the Painter API: shadows, gradients, strokes with each alignment,
// nested rounded clips, and independent fill/stroke calls over shared geometry.
//
// Everything is laid out relative to the widget's own rectangle so the scene
// stays whole at any window size or display scale.

#include "voidui/core/window.h"
#include "voidui/widgets/canvas.h"

using namespace voidui;

namespace {

void draw_scene(Rect<float> bounds, Painter &p) {
  static const LinearGradient band_gradient(
      Color(90, 130, 255), Color(220, 90, 200), Point<float>(0.0f, 0.5f),
      Point<float>(1.0f, 0.5f));
  const Color ink(24, 24, 32);
  const Color panel(250, 250, 252);

  const float w = bounds.size.width;
  const float h = bounds.size.height;
  const float pad = w * 0.06f;
  const float inner = w - pad * 2.0f;

  // --- shadows: analytic, no blur passes, one quad each ---
  const float card_w = inner / 3.4f;
  const float card_h = h * 0.16f;
  for (int i = 0; i < 3; ++i) {
    const Rect<float> card(pad +
                               static_cast<float>(i) * (inner - card_w) / 2.0f,
                           h * 0.06f, card_w, card_h);
    p.draw_shadow(card, Radius(card_h * 0.22f),
                  Shadow(Color(0, 0, 0, 120), Point<float>(0.0f, h * 0.008f),
                         h * (0.004f + static_cast<float>(i) * 0.014f)));
    p.fill_rrect(card, Radius(card_h * 0.22f), Paint(panel));
  }

  // --- gradient fill plus an independent stroke over the same geometry ---
  const Rect<float> band(pad, h * 0.28f, inner, h * 0.14f);
  p.fill_rrect(band, Radius(band.size.height * 0.5f), Paint(band_gradient));
  p.stroke_rrect(band, Radius(band.size.height * 0.5f), Paint(ink),
                 Pen(2.0f, StrokeAlign::Inside));

  // --- the three stroke alignments: same rect, same width, different sides ---
  const float box_w = inner / 3.6f;
  const float box_h = h * 0.14f;
  for (int i = 0; i < 3; ++i) {
    const Rect<float> box(pad + static_cast<float>(i) * (inner - box_w) / 2.0f,
                          h * 0.48f, box_w, box_h);
    p.fill_rrect(box, Radius(box_h * 0.3f), Paint(Color(255, 214, 102)));
    p.stroke_rrect(box, Radius(box_h * 0.3f), Paint(ink),
                   Pen(box_h * 0.11f, static_cast<StrokeAlign>(i)));
  }

  // --- two rounded clips intersecting, applied to a run of plain rects ---
  const Rect<float> clip_a(pad, h * 0.68f, inner, h * 0.26f);
  const Rect<float> clip_b(pad + inner * 0.18f, h * 0.63f, inner * 0.64f,
                           h * 0.36f);

  p.save();
  p.clip_rrect(clip_a, Radius(h * 0.05f));
  p.clip_rrect(clip_b, Radius(h * 0.13f));

  const int bars = 24;
  const float bar_pitch = inner / static_cast<float>(bars);
  for (int i = 0; i < bars; ++i) {
    p.fill_rect(Rect<float>(pad + static_cast<float>(i) * bar_pitch, h * 0.60f,
                            bar_pitch * 0.6f, h * 0.40f),
                Paint(Color(i % 2 ? 70 : 140, 190, 160)));
  }
  p.restore();
}

} // namespace

int main() {
  Window window("VoidUI Shapes", 420, 420);
  window.run(std::make_unique<Canvas>(draw_scene));
  return 0;
}
