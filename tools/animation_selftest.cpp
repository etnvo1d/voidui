#include "voidui/core/style.h"

#include <cmath>
#include <cstdio>
#include <memory>

using namespace voidui;

namespace test_widgets {

class Button {
public:
  VOIDUI_STYLE_SCOPE(Button, "button")
};

} // namespace test_widgets

namespace {

int failures = 0;

void check(bool condition, const char *message) {
  std::printf("  %s  %s\n", condition ? "ok  " : "FAIL", message);
  failures += condition ? 0 : 1;
}

bool near(float a, float b, float epsilon = 0.001f) {
  return std::abs(a - b) <= epsilon;
}

StyleNode make_button() {
  StyleNode node;
  node.type = typeid(test_widgets::Button);
  node.rebuild_bloom();
  return node;
}

} // namespace

int main() {
  std::puts("animation self-test");

  {
    const char *source = R"vss(
      button {
        border-color: conic-gradient(from 0deg,
          #ff3b3b, #ffcc00, #30e88c, #38bdf8, #a855f7, #ff3b3b);
        animation: rainbow-flow 4s linear infinite;
      }

      @keyframes rainbow-flow {
        to {
          border-color: conic-gradient(from 360deg,
            #ff3b3b, #ffcc00, #30e88c, #38bdf8, #a855f7, #ff3b3b);
        }
      }
    )vss";
    StyleParser::Result parsed = StyleParser::parse(source, "rainbow.vss");
    check(parsed.diagnostics.empty(),
          "CSS-like keyframes and conic gradients parse");

    StyleNode node = make_button();
    StyleResolver resolver;
    resolver.set_stylesheet(parsed.sheet);
    const double start = resolver.animation_time();
    check(resolver.resolve_tree(node) == Invalidation::Layout,
          "the first resolve requests layout");
    check(resolver.has_active_animations(),
          "an infinite keyframe animation enters the active set");

    const Brush &initial_brush = node.computed->get<styles::BorderColor>();
    const ConicGradient *initial = std::get_if<ConicGradient>(&initial_brush);
    check(initial && near(initial->angle(), 0.0f),
          "a missing from keyframe uses the cascaded value");
    const GradientStop *shared_stops =
        initial ? initial->stops().data() : nullptr;
    const std::uint64_t styles_before = resolver.cache().statistics().interned;

    check(resolver.advance_animations(start + 1.0) == Invalidation::Paint,
          "a border gradient frame is paint-dirty, not layout-dirty");
    const Brush &quarter_brush = node.computed->get<styles::BorderColor>();
    const ConicGradient *quarter = std::get_if<ConicGradient>(&quarter_brush);
    constexpr float kHalfPi = 1.57079632679489661923f;
    check(quarter && near(quarter->angle(), kHalfPi),
          "linear keyframes sample the conic angle at the requested time");
    check(quarter && quarter->stops().data() == shared_stops,
          "angle-only animation shares immutable gradient stops");

    for (int frame = 2; frame <= 240; ++frame)
      resolver.advance_animations(start + frame / 60.0);
    check(resolver.cache().statistics().interned == styles_before,
          "animation frames do not allocate computed styles in the cache");
    check(resolver.has_active_animations(),
          "an infinite animation remains scheduled after one cycle");
    resolver.forget_animations(node);
    check(!resolver.has_active_animations(),
          "discarding a node removes its animation in constant time");
  }

  {
    const char *source = R"vss(
      button {
        transform: none;
        transition: transform 200ms linear, box-shadow 200ms ease;
      }
      button:hover {
        transform: translateY(-10px);
        box-shadow: 0 0 22px #a855f773;
      }
    )vss";
    StyleParser::Result parsed = StyleParser::parse(source, "transition.vss");
    check(parsed.diagnostics.empty(), "transform and shadow transitions parse");

    StyleNode node = make_button();
    StyleResolver resolver;
    resolver.set_stylesheet(parsed.sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    check(!resolver.has_active_animations(),
          "a transition declaration is idle until its target changes");

    node.status |= StatusBits::kHovered;
    check(resolver.resolve_subtree(node, true) == Invalidation::Paint,
          "hover starts paint-only transitions");
    check(resolver.has_active_animations(),
          "started transitions request frames");
    check(near(node.computed->get<styles::Transform>().translate_y, 0.0f),
          "a transition begins at the currently displayed value");

    resolver.advance_animations(start + 0.1);
    check(
        near(node.computed->get<styles::Transform>().translate_y, -5.0f, 0.02f),
        "a transition interpolates halfway at half its duration");

    node.status &= static_cast<std::uint8_t>(~StatusBits::kHovered);
    resolver.resolve_subtree(node, true);
    // Reversed halfway through, so css-transitions-1 gives the return trip
    // half the declared duration -- 100ms, not another full 200ms.
    resolver.advance_animations(start + 0.15);
    check(
        near(node.computed->get<styles::Transform>().translate_y, -2.5f, 0.03f),
        "an interrupted transition reverses from its displayed value");
    resolver.advance_animations(start + 0.21);
    check(near(node.computed->get<styles::Transform>().translate_y, 0.0f),
          "a reversal is shortened in proportion to how far it had come");
    // The shadow reverses under `ease`, which had already covered 80% of the
    // distance at the halfway mark, so its return trip is the longer one.
    check(resolver.has_active_animations(),
          "each property is shortened by the output of its own easing");
    resolver.advance_animations(start + 0.27);
    check(!resolver.has_active_animations(),
          "a finite transition leaves the frame scheduler when complete");
  }

  {
    const char *source = R"vss(
      button { width: 10px; animation: grow 1s linear 1; }
      @keyframes grow { to { width: 20px; } }
    )vss";
    StyleParser::Result parsed =
        StyleParser::parse(source, "layout-motion.vss");
    StyleNode node = make_button();
    StyleResolver resolver;
    resolver.set_stylesheet(parsed.sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    check(resolver.advance_animations(start + 0.5) == Invalidation::Layout,
          "a width animation correctly escalates to layout-dirty");
    const Length &width = node.computed->get<styles::Width>();
    const auto *fixed = std::get_if<Length::Fixed>(&width.value);
    check(fixed && near(fixed->value, 15.0f),
          "fixed lengths interpolate without changing their unit kind");
    resolver.advance_animations(start + 1.0);
    check(!resolver.has_active_animations(),
          "a finite keyframe animation stops scheduling frames");
  }

  // -- box-shadow ------------------------------------------------------------

  {
    const char *source = R"vss(
      button {
        box-shadow:
          0 0 0 3px oklch(0.708 0 0 / 50%),
          0 1px 2px 0 rgb(0 0 0 / 5%);
      }
      button:hover  { box-shadow: inset 0 2px 4px #00000066; }
      button:active { box-shadow: #00000066 0 2px; }
      button:focus  { box-shadow: none; }
    )vss";
    StyleParser::Result parsed = StyleParser::parse(source, "shadow.vss");
    check(parsed.diagnostics.empty(), "every box-shadow form above parses");

    StyleNode node = make_button();
    StyleResolver resolver;
    resolver.set_stylesheet(parsed.sheet);
    resolver.resolve_tree(node);

    const ShadowList &list = node.computed->get<styles::BoxShadow>();
    check(list.size() == 2, "a comma-separated box-shadow keeps both entries");
    if (list.size() == 2) {
      // The ring is listed first, and so paints on top of the drop shadow.
      check(near(list[0].spread, 3.0f) && near(list[0].blur, 0.0f) &&
                near(list[0].offset.y, 0.0f),
            "a spread ring reads its four lengths in order");
      check(near(list[0].color.a, 0.5f, 0.01f),
            "a slash-separated alpha survives into the shadow's colour");
      // CSS states the blur as the width the edge fades over, twice the
      // Gaussian deviation the shader actually wants.
      check(near(list[1].offset.y, 1.0f) && near(list[1].blur, 1.0f) &&
                near(list[1].spread, 0.0f),
            "a blur radius is halved into a standard deviation");
      check(!list[0].inset && !list[1].inset,
            "a shadow is outer unless it says otherwise");
    }

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    const ShadowList &inner = node.computed->get<styles::BoxShadow>();
    check(inner.size() == 1 && inner[0].inset,
          "`inset` is recognised ahead of the lengths");

    node.status = StatusBits::kActive;
    resolver.resolve_subtree(node, true);
    const ShadowList &two = node.computed->get<styles::BoxShadow>();
    check(two.size() == 1 && near(two[0].offset.y, 2.0f) &&
              near(two[0].blur, 0.0f),
          "a leading colour and two lengths are a complete shadow");

    node.status = StatusBits::kFocused;
    resolver.resolve_subtree(node, true);
    check(node.computed->get<styles::BoxShadow>().empty(),
          "`none` resolves to an empty list");
  }

  {
    const char *source = R"vss(
      button        { box-shadow: 0 1px 2px -1px red, 0 0 0 1px blue; }
      button:hover  { box-shadow: 0 4px 8px 0 red; }
      button:active { box-shadow: inset 0 4px 8px 0 red; }
    )vss";
    StyleParser::Result parsed = StyleParser::parse(source, "shadow-lerp.vss");
    StyleNode node = make_button();
    StyleResolver resolver;
    resolver.set_stylesheet(parsed.sheet);
    resolver.resolve_tree(node);

    const PropertyDescriptor &descriptor =
        PropertyRegistry::instance().describe(
            styles::BoxShadow::index());
    check(descriptor.interpolate != nullptr,
          "box-shadow registers an interpolator");

    PropertyValue from(node.computed->get<styles::BoxShadow>());
    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    PropertyValue to(node.computed->get<styles::BoxShadow>());

    PropertyValue mixed;
    check(descriptor.interpolate(from, to, 0.5f, mixed),
          "a longer shadow list interpolates against a shorter one");
    const ShadowList *value = mixed.as<ShadowList>();
    check(value && value->size() == 2,
          "the result keeps the length of the longer list");
    if (value && value->size() == 2) {
      check(near((*value)[0].offset.y, 2.5f),
            "paired entries interpolate componentwise");
      // The second entry has no counterpart, so it fades towards a transparent
      // shadow of no size rather than snapping away at the first frame.
      check(near((*value)[1].color.a, 0.5f, 0.01f) &&
                near((*value)[1].spread, 0.5f),
            "an unpaired entry fades out instead of disappearing");
    }

    node.status = StatusBits::kActive;
    resolver.resolve_subtree(node, true);
    PropertyValue only_inner(node.computed->get<styles::BoxShadow>());
    check(!descriptor.interpolate(to, only_inner, 0.5f, mixed),
          "an outer and an inner shadow have no midpoint between them");
  }

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
