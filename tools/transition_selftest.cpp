// A standalone exercise of VSS transitions against the CSS they are meant to
// mirror: the easing-function grammar, the five longhands and how a shorthand
// expands into them, `all` and `none`, negative delays, discrete properties,
// and the reversing rule that keeps a flicked-at button honest.
//
// Every check names the behaviour it asserts, so a regression shows up as a
// named failing line rather than as a wrong pixel somewhere downstream.

#include "voidui/core/style.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <string>

using namespace voidui;

namespace test_widgets {

class Card {
public:
  VOIDUI_STYLE_SCOPE(Card, "card")
  VOIDUI_STYLE_PROPERTY(Card, Glow, float, "glow", NotInherited, Paint, 0.0f);
};

class Label {
public:
  VOIDUI_STYLE_SCOPE(Label, "label")
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

Easing easing_of(const char *text) {
  Easing easing = Easing::linear();
  if (!parse_style_value(text, easing))
    easing = Easing::steps(1, StepPosition::JumpNone); // an unmistakable marker
  return easing;
}

bool parses(const char *text) {
  Easing easing;
  return parse_style_value(text, easing);
}

void add_class(StyleNode &node, const char *name) {
  node.classes.push_back(AtomTable::instance().intern(name));
  std::sort(node.classes.begin(), node.classes.end());
  node.rebuild_bloom();
}

StyleNode make_card() {
  StyleNode node;
  node.type = typeid(test_widgets::Card);
  node.rebuild_bloom();
  return node;
}

/// Parses a stylesheet, reporting its diagnostics as a named check.
std::shared_ptr<const StyleSheet> sheet_of(const char *source,
                                           const char *message) {
  StyleParser::Result parsed = StyleParser::parse(source, "transition.vss");
  if (!parsed.diagnostics.empty())
    std::printf("        %s\n", parsed.diagnostics.front().to_string().c_str());
  check(parsed.diagnostics.empty(), message);
  return parsed.sheet;
}

float opacity_of(const StyleNode &node) {
  return node.computed->get<styles::Opacity>();
}

Color foreground_of(const StyleNode &node) {
  const Brush &brush = node.computed->get<styles::Foreground>();
  const Color *color = std::get_if<Color>(&brush);
  return color ? *color : Color::TRANSPARENT;
}

} // namespace

int main() {
  std::puts("transition self-test");

  // -- The easing-function grammar -------------------------------------------
  {
    const Easing bezier = easing_of("cubic-bezier(0.4, 0, 0.2, 1)");
    check(bezier.kind() == Easing::Kind::CubicBezier && near(bezier.x1(), 0.4f),
          "cubic-bezier() reads its four control coordinates");
    check(near(bezier(0.0f), 0.0f) && near(bezier(1.0f), 1.0f),
          "a cubic-bezier easing is pinned to both endpoints");
    check(bezier(0.5f) > 0.5f,
          "the standard material curve is ahead of linear at its midpoint");

    const Easing identity = easing_of("cubic-bezier(0, 0, 1, 1)");
    check(near(identity(0.3f), 0.3f) && near(identity(0.87f), 0.87f),
          "a bezier whose controls sit on the diagonal is the identity");

    // A curve with a vertical tangent at the origin is the case Newton alone
    // cannot solve, and the one that catches a missing bisection fallback.
    const Easing steep = easing_of("cubic-bezier(0, 0.9, 0.1, 1)");
    check(steep(0.001f) >= 0.0f && steep(0.001f) <= 1.0f && steep(0.5f) > 0.9f,
          "a curve with a vanishing derivative still solves");

    const Easing ease = easing_of("ease");
    check(ease == Easing::ease() && Easing{} == Easing::ease(),
          "`ease` is the keyword and the initial value of the property");

    const Easing steps = easing_of("steps(4)");
    check(steps.kind() == Easing::Kind::Steps && steps.step_count() == 4 &&
              steps.step_position() == StepPosition::JumpEnd,
          "steps() defaults to jump-end");
    check(near(steps(0.0f), 0.0f) && near(steps(0.3f), 0.25f) &&
              near(steps(1.0f), 1.0f),
          "steps(4) holds zero until the first quarter is complete");

    const Easing start = easing_of("steps(4, jump-start)");
    check(near(start(0.0f), 0.25f) && near(start(1.0f), 1.0f),
          "jump-start takes its first step immediately");
    const Easing none = easing_of("steps(3, jump-none)");
    check(near(none(0.0f), 0.0f) && near(none(0.5f), 0.5f) &&
              near(none(1.0f), 1.0f),
          "jump-none holds both endpoints and splits the rest evenly");
    const Easing both = easing_of("steps(3, jump-both)");
    check(near(both(0.0f), 0.25f) && near(both(1.0f), 1.0f),
          "jump-both spends a step at each end");
    check(easing_of("step-start") == Easing::steps(1, StepPosition::JumpStart) &&
              easing_of("step-end") == Easing::steps(1, StepPosition::JumpEnd),
          "step-start and step-end are the one-step shorthands");
    check(easing_of("steps(4, start)") == easing_of("steps(4, jump-start)") &&
              easing_of("steps(4, end)") == easing_of("steps(4, jump-end)"),
          "the legacy start and end keywords still read");

    const Easing points = easing_of("linear(0, 0.25 75%, 1)");
    check(points.kind() == Easing::Kind::Points && near(points(0.75f), 0.25f),
          "linear() honours an explicit control point");
    check(near(points(0.375f), 0.125f) && near(points(0.875f), 0.625f),
          "linear() interpolates inside each of its segments");
    check(easing_of("linear(0, 0.5 20% 80%, 1)")(0.5f) == 0.5f,
          "a two-input linear() entry describes a flat segment");
    check(near(easing_of("linear(0, 1)")(0.4f), 0.4f),
          "linear() with implicit inputs spreads them evenly");
    check(easing_of("linear(0, 0.5 75%, 1)") ==
              easing_of("linear(0, 0.5 75%, 1)"),
          "two identical linear() curves intern to one entry and compare equal");

    check(!parses("cubic-bezier(1.4, 0, 0.2, 1)"),
          "a cubic-bezier x coordinate outside [0, 1] is rejected");
    check(!parses("steps(0)") && !parses("steps(1, jump-none)") &&
              !parses("steps(4, sideways)"),
          "an unsatisfiable steps() is rejected");
    check(!parses("wobble") && !parses("cubic-bezier(0, 0, 1)"),
          "an unknown easing function is rejected rather than guessed at");
  }

  // -- Shorthand, longhands, and how they cascade ----------------------------
  {
    auto sheet = sheet_of(R"vss(
      card {
        opacity: 1;
        transition: opacity 200ms cubic-bezier(0.4, 0, 0.2, 1) 50ms;
      }
      card.quick { transition-duration: 100ms; }
      card:hover { opacity: 0; }
    )vss",
                          "a shorthand with an easing and a delay parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    resolver.resolve_tree(node);

    const TransitionPropertyList &properties =
        node.computed->get<styles::TransitionProperty>();
    check(properties.size() == 1 &&
              properties[0] == styles::Opacity::index(),
          "the shorthand expands into a resolved transition-property list");
    check(node.computed->get<styles::TransitionDuration>().size() == 1 &&
              near(node.computed->get<styles::TransitionDuration>()[0], 0.2f) &&
              near(node.computed->get<styles::TransitionDelay>()[0], 0.05f),
          "the two positional times land in duration and delay");
    check(node.computed->get<styles::TransitionTimingFunction>()[0] ==
              Easing::cubic_bezier(0.4f, 0.0f, 0.2f, 1.0f),
          "the easing survives the expansion");

    add_class(node, "quick");
    resolver.resolve_subtree(node, true);
    check(near(node.computed->get<styles::TransitionDuration>()[0], 0.1f) &&
              near(node.computed->get<styles::TransitionDelay>()[0], 0.05f) &&
              node.computed->get<styles::TransitionTimingFunction>()[0] ==
                  Easing::cubic_bezier(0.4f, 0.0f, 0.2f, 1.0f),
          "a longhand overrides one part of a shorthand and leaves the rest");
  }

  // -- The delay, including a negative one ------------------------------------
  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; transition: opacity 200ms linear 100ms; }
      card:hover { opacity: 0; }
    )vss",
                          "a positive delay parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.05);
    check(near(opacity_of(node), 1.0f),
          "a delayed transition holds its start value while it waits");
    check(resolver.has_active_animations(),
          "a delayed transition still asks for frames");
    resolver.advance_animations(start + 0.2);
    check(near(opacity_of(node), 0.5f), "the delay offsets the whole run");
    resolver.advance_animations(start + 0.31);
    check(!resolver.has_active_animations(),
          "the run ends one duration after the delay elapses");
  }

  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; transition: opacity 200ms linear -100ms; }
      card:hover { opacity: 0; }
    )vss",
                          "a negative delay parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    check(near(opacity_of(node), 0.5f),
          "a negative delay starts the transition already partway through");
    resolver.advance_animations(start + 0.11);
    check(!resolver.has_active_animations(),
          "and it finishes that much sooner");
  }

  // -- transition-property: all, and none -------------------------------------
  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; border-radius: 0; transition: all 200ms linear; }
      card:hover { opacity: 0; border-radius: 12; }
    )vss",
                          "`transition: all` parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    check(!node.computed->get<styles::TransitionProperty>().empty() &&
              node.computed->get<styles::TransitionProperty>()[0] ==
                  kAllTransitionProperties,
          "`all` is stored as the wildcard rather than as a name");

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.1);
    check(near(opacity_of(node), 0.5f) &&
              near(node.computed->get<styles::BorderRadius>().left_top, 6.0f),
          "`all` transitions every property whose value moved");
    check(node.computed->get<styles::TransitionDuration>().size() == 1,
          "and does not transition the transition properties themselves");
  }

  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; transition: opacity 200ms linear; }
      card.still { transition-property: none; }
      card:hover { opacity: 0; }
    )vss",
                          "`transition-property: none` parses");

    StyleNode node = make_card();
    add_class(node, "still");
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    resolver.resolve_tree(node);

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    check(near(opacity_of(node), 0.0f) && !resolver.has_active_animations(),
          "`none` overrides the duration a shorthand left behind");
  }

  // -- List cycling across the longhands --------------------------------------
  {
    auto sheet = sheet_of(R"vss(
      card {
        opacity: 1;
        border-radius: 0;
        transition-property: opacity, border-radius;
        transition-duration: 100ms;
        transition-timing-function: linear;
      }
      card:hover { opacity: 0; border-radius: 20; }
    )vss",
                          "separate longhands of different lengths parse");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.05);
    check(near(opacity_of(node), 0.5f) &&
              near(node.computed->get<styles::BorderRadius>().left_top, 10.0f),
          "a one-entry duration list repeats across both properties");
    resolver.advance_animations(start + 0.11);
    check(!resolver.has_active_animations(),
          "both entries end together when they share a duration");
  }

  // -- Discrete properties and transition-behavior ----------------------------
  {
    auto sheet = sheet_of(R"vss(
      card {
        user-select: auto;
        transition: user-select 200ms linear allow-discrete;
      }
      card:hover { user-select: none; }
    )vss",
                          "`allow-discrete` parses in the shorthand");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.05);
    check(node.computed->get<styles::UserSelect>() == UserSelect::Auto,
          "a discrete property holds its old value through the first half");
    resolver.advance_animations(start + 0.15);
    check(node.computed->get<styles::UserSelect>() == UserSelect::None,
          "and flips to the new one at the halfway point");
    resolver.advance_animations(start + 0.25);
    check(!resolver.has_active_animations(),
          "a discrete transition ends like any other");
  }

  {
    auto sheet = sheet_of(R"vss(
      card { user-select: auto; transition: user-select 200ms linear; }
      card:hover { user-select: none; }
    )vss",
                          "the same rule without allow-discrete parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    resolver.resolve_tree(node);
    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    check(node.computed->get<styles::UserSelect>() == UserSelect::None &&
              !resolver.has_active_animations(),
          "without the opt-in a discrete property changes at once");
  }

  // -- Reversing an interrupted transition ------------------------------------
  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; transition: opacity 1s linear; }
      card:hover { opacity: 0; }
    )vss",
                          "a one-second opacity transition parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.25);
    check(near(opacity_of(node), 0.75f), "a quarter of the way down");

    node.status &= static_cast<std::uint8_t>(~StatusBits::kHovered);
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.375);
    check(near(opacity_of(node), 0.875f),
          "the reversal covers the remaining distance at the declared speed");
    resolver.advance_animations(start + 0.51);
    check(near(opacity_of(node), 1.0f) && !resolver.has_active_animations(),
          "a quarter-done transition reverses in a quarter of the duration");
  }

  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; transition: opacity 1s linear; }
      card.dim { opacity: 0.5; }
      card:hover { opacity: 0; }
    )vss",
                          "a rule with three opacity states parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.5);
    check(near(opacity_of(node), 0.5f), "halfway to transparent");

    // Not a reversal: the new destination is somewhere else entirely, so the
    // run restarts at full length from where it had got to.
    node.status &= static_cast<std::uint8_t>(~StatusBits::kHovered);
    add_class(node, "dim");
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 1.4);
    check(near(opacity_of(node), 0.5f) && !resolver.has_active_animations(),
          "an interruption toward a third value gets the full duration");
  }

  // -- Retargeting mid-flight without restarting ------------------------------
  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; transition: opacity 1s linear; }
      card.tagged { border-radius: 4; }
      card:hover { opacity: 0; }
    )vss",
                          "a rule whose classes touch an untransitioned "
                          "property parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.5);

    add_class(node, "tagged");
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.75);
    check(near(opacity_of(node), 0.25f),
          "an unrelated style change does not restart a run already in flight");
  }

  // -- Animated inherited values propagate every frame -----------------------
  {
    auto initial = sheet_of(R"vss(
      card { color: #000000; transition: color 150ms linear; }
    )vss",
                            "an inherited color transition parses");
    auto reloaded = sheet_of(R"vss(
      card { color: oklch(0.922 0 0); transition: color 150ms linear; }
    )vss",
                             "a hot-reloaded Oklch color parses");

    StyleNode parent = make_card();
    StyleNode child;
    StyleNode sibling;
    for (StyleNode *node : {&child, &sibling}) {
      node->type = typeid(test_widgets::Label);
      node->parent = &parent;
      parent.children.push_back(node);
    }
    style_rebuild_blooms(parent);

    StyleResolver resolver;
    resolver.set_stylesheet(initial);
    const double start = resolver.animation_time();
    resolver.resolve_tree(parent);
    check(child.computed == sibling.computed,
          "two identical children share one computed style at rest");

    // Replacing the stylesheet is the resolver side of a VSS hot reload.
    resolver.set_stylesheet(reloaded);
    resolver.resolve_tree(parent);

    const std::uint64_t interned = resolver.cache().statistics().interned;
    for (int frame = 1; frame <= 8; ++frame)
      resolver.advance_animations(start + frame * 0.015);
    check(foreground_of(child) == foreground_of(parent),
          "a child inherits every frame of its parent's color transition");
    check(resolver.cache().statistics().interned == interned,
          "inherited animation samples do not accumulate cache entries");

    resolver.advance_animations(start + 0.16);
    check(foreground_of(child) == foreground_of(parent),
          "the inherited child reaches the reloaded color without a hover");
    check(!resolver.has_active_animations(), "and the transition is over");
    // The private styles the moving frames handed out are transient. Once the
    // run settles the subtree goes back to sharing, or a list that faded once
    // would carry one unshared style per row for the rest of its life.
    check(child.computed == sibling.computed,
          "a settled subtree returns to sharing one interned style");
  }

  // -- Component-scoped property names ----------------------------------------
  {
    auto sheet = sheet_of(R"vss(
      card { glow: 0; transition: glow 100ms linear; }
      card:hover { glow: 10; }
    )vss",
                          "an unprefixed component property transitions "
                          "inside its own rule");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    resolver.advance_animations(start + 0.05);
    check(near(node.computed->get<test_widgets::Card::Glow>(), 5.0f),
          "`transition: glow` resolves to `card.glow` in a card rule");
  }

  // -- Cost --------------------------------------------------------------------
  {
    auto sheet = sheet_of(R"vss(
      card { opacity: 1; transition: opacity 1s linear; }
      card:hover { opacity: 0; }
    )vss",
                          "the cost fixture parses");

    StyleNode node = make_card();
    StyleResolver resolver;
    resolver.set_stylesheet(sheet);
    const double start = resolver.animation_time();
    resolver.resolve_tree(node);
    check(!resolver.has_active_animations(),
          "declaring a transition allocates no runtime state on its own");

    node.status |= StatusBits::kHovered;
    resolver.resolve_subtree(node, true);
    const std::uint64_t interned = resolver.cache().statistics().interned;
    for (int frame = 1; frame <= 60; ++frame)
      resolver.advance_animations(start + frame / 120.0);
    check(resolver.cache().statistics().interned == interned,
          "transition frames do not intern a computed style per frame");
    check(resolver.advance_animations(start + 0.6) == Invalidation::Paint,
          "an opacity transition reports paint, not layout");
  }

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
