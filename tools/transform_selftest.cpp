// A translated subtree has to move as one piece.
//
// Every item that reaches the renderer snaps its own final position onto the
// device pixel grid, and that rounding carries a shift through unchanged only
// when the shift is a whole number of device pixels. A fractional one --
// `translateY(1px)` is 1.25 device pixels at 125% -- puts each item on its own
// side of its own rounding boundary, so a button's background moves one pixel
// while its label moves two, and under a transition they cross on different
// frames. The tree therefore quantises a translation where it enters the tree
// rather than where each item leaves it; this asserts that it still does.
#include "voidui/core/pixel_snap.h"
#include "voidui/core/widget_tree.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/text.h"

#include <cmath>
#include <cstdio>
#include <vector>

using namespace voidui;

namespace {

int failures = 0;

void check(bool condition, const char *message) {
  std::printf("  %s  %s\n", condition ? "ok  " : "FAIL", message);
  failures += condition ? 0 : 1;
}

constexpr float kScales[] = {1.0f, 1.25f, 1.5f, 2.0f};

/// The translation every command in `list` shares, or false when they disagree.
bool shared_translation(const DisplayList &list, Point<float> &out) {
  if (list.commands().empty())
    return false;

  const Transform &first = list.commands().front().transform;
  for (const DrawCommand &command : list.commands()) {
    const Transform &t = command.transform;
    if (t.a != first.a || t.b != first.b || t.c != first.c || t.d != first.d ||
        t.e != first.e || t.f != first.f)
      return false;
  }
  out = Point<float>(first.e, first.f);
  return true;
}

bool whole_device_pixels(float logical, float scale) {
  const float device = logical * scale;
  return std::abs(device - round_half_up(device)) < 1e-3f;
}

/// The device row a background quad lands on, as the renderer places one.
float quad_row(const DrawCommand &command, float scale) {
  Rect<float> rect = snap_rect_to_pixel(command.rect, scale);
  rect.origin.y += round_half_up(command.transform.f * scale) / scale;
  return round_half_up(rect.origin.y * scale);
}

/// The device row a glyph run lands on, as the renderer places one.
float run_row(const DrawCommand &command, float scale) {
  return snap_with_shift(command.rect.origin.y, command.transform.f, scale);
}

/// Every background and every glyph run in `list`, by device row.
void rows(const DisplayList &list, float scale, std::vector<float> &quads,
          std::vector<float> &runs) {
  quads.clear();
  runs.clear();
  for (const DrawCommand &command : list.commands()) {
    if (command.kind == CommandKind::FillRRect)
      quads.push_back(quad_row(command, scale));
    else if (command.kind == CommandKind::Glyphs)
      runs.push_back(run_row(command, scale));
  }
}

DisplayList render(WidgetTree &tree) {
  DisplayList list;
  Painter painter(list, Size<float>(400.0f, 200.0f));
  tree.render(painter);
  return list;
}

} // namespace

int main() {
  std::puts("transform self-test");

  StyleParser::Result parsed = StyleParser::parse(R"vss(
    button:active { transform: translateY(1px); }
  )vss",
                                                  "transform.vss");
  check(parsed.diagnostics.empty(), "the pressed-state transform parses");

  for (const float scale : kScales) {
    std::printf(" device scale %.2f\n", scale);

    WidgetTree tree(transfer_widget(button("Press")));
    tree.set_device_scale(scale);
    tree.style_resolver().set_stylesheet(parsed.sheet);
    tree.restyle();
    tree.layout(Constraints(400.0f, 200.0f));

    Node *root = tree.root();
    Point<float> idle{0.0f, 0.0f};
    check(shared_translation(render(tree), idle) && idle.x == 0.0f &&
              idle.y == 0.0f,
          "an untransformed button draws its background and label unshifted");

    const Point<float> centre{root->global_pos.x + root->size.width * 0.5f,
                              root->global_pos.y + root->size.height * 0.5f};
    MousePressedEvent press(MouseButton::Left, centre);
    tree.process_event(press);

    Point<float> pressed{0.0f, 0.0f};
    check(shared_translation(render(tree), pressed),
          "the background and the label are shifted by one common translation");
    check(whole_device_pixels(pressed.y, scale),
          "the shift is a whole number of device pixels");
    check(pressed.y > 0.0f, "a one-pixel press still moves the button");

    // The press only counts as landing on the button if hit testing walks the
    // same matrix rendering used -- the point is mapped back through it.
    Point<float> bottom{centre.x,
                        root->global_pos.y + root->size.height - 0.25f};
    MousePressedEvent edge(MouseButton::Left, bottom);
    tree.process_event(edge);
    check(root->status.is_active(),
          "hit testing follows the snapped transform, not the raw one");

    MouseReleasedEvent release(MouseButton::Left, centre);
    tree.process_event(release);
    Point<float> released{1.0f, 1.0f};
    check(shared_translation(render(tree), released) && released.y == 0.0f,
          "releasing puts the whole subtree back where it started");
  }

  // Mid-transition is where the two used to come apart: the label crossed its
  // rounding boundary several frames before the background crossed its own.
  {
    std::puts(" through a transition");
    StyleParser::Result animated = StyleParser::parse(R"vss(
      button { transition: transform 200ms linear; }
      button:active { transform: translateY(1px); }
    )vss",
                                                      "transition.vss");
    check(animated.diagnostics.empty(), "the transition parses");

    const float scale = 1.25f;
    WidgetTree tree(transfer_widget(button("Press")));
    tree.set_device_scale(scale);
    tree.style_resolver().set_stylesheet(animated.sheet);
    tree.restyle();
    const double start = tree.style_resolver().animation_time();
    tree.layout(Constraints(400.0f, 200.0f));

    Node *root = tree.root();
    MousePressedEvent press(MouseButton::Left,
                            {root->global_pos.x + root->size.width * 0.5f,
                             root->global_pos.y + root->size.height * 0.5f});
    tree.process_event(press);

    bool rigid = true;
    bool integral = true;
    float travelled = 0.0f;
    for (int step = 0; step <= 40; ++step) {
      tree.advance_animations(start + step * 0.008);
      Point<float> shift{0.0f, 0.0f};
      rigid = rigid && shared_translation(render(tree), shift);
      integral = integral && whole_device_pixels(shift.y, scale);
      travelled = std::max(travelled, shift.y * scale);
    }
    check(rigid, "every frame of the transition shifts the subtree as one");
    check(integral, "every frame lands on a whole device pixel");
    check(std::abs(travelled - 1.0f) < 1e-3f,
          "a one-logical-pixel press travels exactly one device pixel at 125%");
  }

  // The shift has to survive a rounding tie, and ties are dense: at 1.25x every
  // even logical coordinate is one. Folding the shift in before the rounding
  // passes every check above and still loses the label here.
  {
    std::puts(" across rounding ties");
    bool equivariant = true;
    for (const float scale : kScales) {
      for (int shift = -4; shift <= 4; ++shift) {
        const float t = round_half_up(static_cast<float>(shift)) / scale;
        for (int step = 0; step < 2000; ++step) {
          const float y = static_cast<float>(step) * 0.1f;
          const float moved = snap_with_shift(y, t, scale);
          const float still = snap_with_shift(y, 0.0f, scale);
          equivariant = equivariant &&
                        std::abs((moved - still) - static_cast<float>(shift)) <
                            1e-3f;
        }
      }
    }
    check(equivariant,
          "a whole-pixel shift moves every coordinate by exactly that many "
          "pixels");
  }

  // examples/counter.cpp: a column of buttons, whose labels sit at coordinates
  // that land on ties at 125%. This is the case that shipped broken -- the
  // background dropped a pixel and the label stayed exactly where it was.
  {
    std::puts(" a column of pressable buttons");
    const float scale = 1.25f;

    auto view = row(column(button("Increment"), text("0"), button("Decrement"))
                        .gap(8.0f))
                    .gap(24.0f);
    WidgetTree tree(transfer_widget(view));
    tree.set_device_scale(scale);
    tree.style_resolver().set_stylesheet(parsed.sheet);
    tree.restyle();
    tree.layout(Constraints(480.0f, 240.0f));

    std::vector<float> idle_quads, idle_runs, held_quads, held_runs;
    rows(render(tree), scale, idle_quads, idle_runs);
    check(idle_quads.size() >= 2 && idle_runs.size() >= 3,
          "the column draws both buttons and all three labels");

    Node *stack = tree.root()->children.front().get();
    for (const std::size_t index : {std::size_t{0}, std::size_t{2}}) {
      Node *target = stack->children[index].get();
      MousePressedEvent press(
          MouseButton::Left, {target->global_pos.x + target->size.width * 0.5f,
                              target->global_pos.y + target->size.height * 0.5f});
      tree.process_event(press);
      rows(render(tree), scale, held_quads, held_runs);

      bool shape_held = held_quads.size() == idle_quads.size() &&
                        held_runs.size() == idle_runs.size();
      float moved_quad = 0.0f;
      float moved_run = 0.0f;
      bool only_whole_steps = true;
      if (shape_held) {
        for (std::size_t i = 0; i < held_quads.size(); ++i) {
          const float delta = held_quads[i] - idle_quads[i];
          only_whole_steps = only_whole_steps && (delta == 0.0f || delta == 1.0f);
          moved_quad = std::max(moved_quad, delta);
        }
        for (std::size_t i = 0; i < held_runs.size(); ++i) {
          const float delta = held_runs[i] - idle_runs[i];
          only_whole_steps = only_whole_steps && (delta == 0.0f || delta == 1.0f);
          moved_run = std::max(moved_run, delta);
        }
      }

      const char *which = index == 0 ? "the first button" : "the last button";
      std::printf("  %s: background +%.0f, label +%.0f device px\n", which,
                  moved_quad, moved_run);
      check(shape_held && only_whole_steps,
            "pressing moves rows by whole device pixels or not at all");
      check(moved_quad == 1.0f,
            "the pressed background drops exactly one device pixel");
      check(moved_run == moved_quad,
            "its label drops with it, by the same pixel");

      MouseReleasedEvent release(MouseButton::Left, press.get_pos());
      tree.process_event(release);
    }
  }

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
