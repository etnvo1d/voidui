// End-to-end exercise of the style system: parsing, descendant and child
// combinators, the shadow boundary, ::part() piercing, theming, hover
// re-resolution and computed-style sharing.
#include "voidui/core/style.h"

#include <array>
#include <cmath>
#include <cstdio>
#include <memory>
#include <vector>

using namespace voidui;

namespace demo {

class Window {
public:
  VOIDUI_STYLE_SCOPE(Window, "window")
};

class Panel {
public:
  VOIDUI_STYLE_SCOPE(Panel, "panel")
};

class Text {
public:
  VOIDUI_STYLE_SCOPE(Text, "text")
};

class Button {
public:
  VOIDUI_STYLE_SCOPE(Button, "button")
};

class Input {
public:
  VOIDUI_STYLE_SCOPE(Input, "input")
  VOIDUI_STYLE_PROPERTY(Input, CaretColor, Color, "caret-color", NotInherited,
                        Paint, Color(0, 0, 0));
  VOIDUI_STYLE_PROPERTY(Input, PlaceholderColor, Color, "placeholder-color",
                        Inherited, Paint, Color(128, 128, 128));
  VOIDUI_STYLE_PROPERTY(Input, CaretWidth, float, "caret-width", NotInherited,
                        Paint, 1.0f);
};

} // namespace demo

namespace {

int failures = 0;

struct MoveTracked {
  inline static int live = 0;
  inline static int moves = 0;

  explicit MoveTracked(int value) : value(value) { ++live; }
  MoveTracked(const MoveTracked &other) : value(other.value) { ++live; }
  MoveTracked(MoveTracked &&other) noexcept : value(other.value) {
    other.value = -1;
    ++live;
    ++moves;
  }
  ~MoveTracked() { --live; }

  bool operator==(const MoveTracked &other) const {
    return value == other.value;
  }

  int value;
};

static_assert(sizeof(MoveTracked) <= PropertyValue::kInlineSize);
static_assert(sizeof(LinearGradient) <= PropertyValue::kInlineSize);

// A Brush is the widest built-in style value there is, and it has to stay
// inline: `background` lands in one on every painted node, and spilling it to
// the heap would put an allocation in the middle of resolving a tree.
static_assert(sizeof(Brush) <= PropertyValue::kInlineSize);
static_assert(sizeof(Color) <= PropertyValue::kInlineSize);

std::uint64_t style_value_hash(const MoveTracked &value) {
  return static_cast<std::uint64_t>(value.value);
}

void check(bool condition, const char *what) {
  if (!condition) {
    ++failures;
    std::printf("  FAIL  %s\n", what);
  } else {
    std::printf("  ok    %s\n", what);
  }
}

bool same(Color a, Color b) {
  return a.r == b.r && a.g == b.g && a.b == b.b && a.a == b.a;
}

bool near(float a, float b, float epsilon = 0.001f) {
  return std::abs(a - b) <= epsilon;
}

Color color_of(const ComputedStyle &style, PropertyIndex property) {
  const PropertyValue *value = style.find(property);
  if (!value)
    return Color(0, 0, 0, 0);
  if (const Color *direct = value->as<Color>())
    return *direct;
  if (const Brush *brush = value->as<Brush>())
    if (const Color *from_brush = std::get_if<Color>(brush))
      return *from_brush;
  return Color(0, 0, 0, 0);
}

/// Owns the nodes so the test can build trees without a widget tree.
struct Tree {
  std::vector<std::unique_ptr<StyleNode>> nodes;

  StyleNode *add(StyleNode *parent, std::type_index type,
                 std::initializer_list<const char *> classes = {},
                 bool internal = false, const char *part = nullptr) {
    auto node = std::make_unique<StyleNode>();
    node->type = type;
    for (const char *name : classes)
      node->classes.push_back(AtomTable::instance().intern(name));
    std::sort(node->classes.begin(), node->classes.end());
    node->is_internal = internal;
    if (part)
      node->part = AtomTable::instance().intern(part);
    node->parent = parent;
    StyleNode *raw = node.get();
    nodes.push_back(std::move(node));
    if (parent)
      parent->children.push_back(raw);
    return raw;
  }
};

const char *kStyleSheet = R"vss(
/* Rules never mention a literal colour: the theme owns those. */
window            { background: $surface.base; color: $text.primary;
                    line-height: 24px; font-weight: 600; }
.sidebar          { background: $surface.raised; padding: 8 12; }
window > .sidebar { border-radius: 4; }

/* Descendant: any text under the sidebar, at any depth. */
.sidebar text     { font-size: 12; }

/* Child: only a button that is a direct child of the sidebar. */
.sidebar > button { padding: 4 8; }

button:hover      { background: $surface.raised.hover; }

/* A component property, written unprefixed inside its own rule. */
input             { caret-color: $accent; caret-width: 2; font-size: 13; }

/* Piercing the shadow boundary on purpose. */
input::part(placeholder) { color: $text.muted; }
)vss";

const char *kTheme = R"vtheme(
@name "Dark";
$surface.base:         #16181C;
$surface.raised:       #2A2D31;
$surface.raised.hover: #35393E;
$text.primary:         #E6E6E6;
$text.muted:           #8A8F98;
$accent:               #4C9AFF;
)vtheme";

} // namespace

int main() {
  // -- Linear gradients ----------------------------------------------------

  {
    Brush css_gradient = Color::TRANSPARENT;
    check(parse_style_value("linear-gradient(to right, blue, magenta)",
                            css_gradient),
          "CSS linear-gradient direction parses");
    const LinearGradient *right = std::get_if<LinearGradient>(&css_gradient);
    const auto right_axis = right ? right->axis_for({200.0f, 100.0f})
                                  : std::pair<Point<float>, Point<float>>{};
    check(right && near(right_axis.first.x, 0.0f) &&
              near(right_axis.first.y, 0.5f) &&
              near(right_axis.second.x, 1.0f) &&
              near(right_axis.second.y, 0.5f),
          "to right resolves in box-local 0..1 coordinates");

    Brush default_gradient = Color::TRANSPARENT;
    check(parse_style_value("linear-gradient(red, blue)", default_gradient),
          "CSS linear-gradient accepts an omitted direction");
    const LinearGradient *down = std::get_if<LinearGradient>(&default_gradient);
    const auto down_axis = down ? down->axis_for({200.0f, 100.0f})
                                : std::pair<Point<float>, Point<float>>{};
    check(down && near(down_axis.first.x, 0.5f) &&
              near(down_axis.first.y, 0.0f) && near(down_axis.second.x, 0.5f) &&
              near(down_axis.second.y, 1.0f),
          "an omitted direction defaults to bottom");

    Brush stopped_gradient = Color::TRANSPARENT;
    check(parse_style_value("linear-gradient(135deg, yellow, blue 20%, #0f0)",
                            stopped_gradient),
          "CSS angles, percentages and multiple color stops parse");
    const LinearGradient *stopped =
        std::get_if<LinearGradient>(&stopped_gradient);
    std::array<Color, kMaxGradientStops> colors{
        Color::TRANSPARENT, Color::TRANSPARENT, Color::TRANSPARENT,
        Color::TRANSPARENT, Color::TRANSPARENT, Color::TRANSPARENT,
        Color::TRANSPARENT, Color::TRANSPARENT};
    std::array<float, kMaxGradientStops> positions{};
    const std::size_t stop_count =
        stopped ? stopped->resolve_stops({200.0f, 100.0f}, colors, positions)
                : 0;
    check(stop_count == 3 && near(positions[0], 0.0f) &&
              near(positions[1], 0.2f) && near(positions[2], 1.0f),
          "implicit CSS color-stop positions are fixed up");
    if (stopped) {
      const auto axis = stopped->axis_for({200.0f, 100.0f});
      const float dx = (axis.second.x - axis.first.x) * 200.0f;
      const float dy = (axis.second.y - axis.first.y) * 100.0f;
      check(near(std::abs(dx), std::abs(dy)),
            "CSS angles are measured in physical box space");
    }

    Brush corner_gradient = Color::TRANSPARENT;
    check(parse_style_value(
              "linear-gradient(to top right, red 10px 20%, white, blue)",
              corner_gradient),
          "CSS corner directions and double-position stops parse");
    const LinearGradient *corner =
        std::get_if<LinearGradient>(&corner_gradient);
    check(corner && corner->stops().size() == 4,
          "a double-position stop expands into a hard color band");

    LinearGradient local(Color(90, 130, 255), Color(220, 90, 200), {0.0f, 0.5f},
                         {1.0f, 0.5f});
    const auto local_axis = local.axis_for({420.0f, 60.0f});
    check(near(local_axis.first.x, 0.0f) && near(local_axis.second.x, 1.0f),
          "the C++ LinearGradient API uses local 0..1 points");

    Brush same_css_gradient = Color::TRANSPARENT;
    parse_style_value("linear-gradient(to right, blue, magenta)",
                      same_css_gradient);
    const auto *same_right = std::get_if<LinearGradient>(&same_css_gradient);
    check(right && same_right && style_value_equals(*right, *same_right) &&
              style_value_hash(*right) == style_value_hash(*same_right),
          "gradient equality and hashing use contents, not shared addresses");
  }

  // -- Property value lifetime ---------------------------------------------

  {
    PropertyValue source(MoveTracked(42));
    MoveTracked::moves = 0;

    PropertyValue moved(std::move(source));
    check(!source.has_value(), "moving clears the source PropertyValue");
    check(MoveTracked::moves == 1,
          "an inline non-trivial value uses its move constructor");
    check(moved.as<MoveTracked>() && moved.as<MoveTracked>()->value == 42,
          "an inline non-trivial value survives move construction");

    PropertyValue assigned;
    assigned = std::move(moved);
    check(!moved.has_value(), "move assignment clears its source");
    check(MoveTracked::moves == 2,
          "move assignment uses the inline value's move constructor");
    check(assigned.as<MoveTracked>() && assigned.as<MoveTracked>()->value == 42,
          "an inline non-trivial value survives move assignment");
  }
  check(MoveTracked::live == 0,
        "inline non-trivial values are destroyed exactly once");

  // -- Parse ----------------------------------------------------------------

  StyleParser::Result parsed = StyleParser::parse(kStyleSheet, "test.vss");
  for (const StyleDiagnostic &diagnostic : parsed.diagnostics)
    std::printf("  diagnostic: %s\n", diagnostic.to_string().c_str());
  check(parsed.diagnostics.empty(), "stylesheet parses without diagnostics");
  check(parsed.sheet->size() == 8, "eight rules parsed");

  StyleParser::ThemeResult theme =
      StyleParser::parse_theme(kTheme, "dark.vtheme");
  for (const StyleDiagnostic &diagnostic : theme.diagnostics)
    std::printf("  diagnostic: %s\n", diagnostic.to_string().c_str());
  check(theme.diagnostics.empty(), "theme parses without diagnostics");
  check(theme.theme->name() == "Dark", "theme name read from @name");

  // -- Build a tree ---------------------------------------------------------
  //
  //   window
  //     .sidebar (panel)
  //       button            <- direct child
  //         text            <- descendant of sidebar
  //       panel
  //         button          <- NOT a direct child of sidebar
  //       input
  //         text  [internal, part=placeholder]
  //     text                <- outside the sidebar

  Tree tree;
  StyleNode *window = tree.add(nullptr, typeid(demo::Window));
  StyleNode *sidebar = tree.add(window, typeid(demo::Panel), {"sidebar"});
  StyleNode *near_button = tree.add(sidebar, typeid(demo::Button));
  StyleNode *button_label = tree.add(near_button, typeid(demo::Text));
  StyleNode *inner_panel = tree.add(sidebar, typeid(demo::Panel));
  StyleNode *far_button = tree.add(inner_panel, typeid(demo::Button));
  StyleNode *input = tree.add(sidebar, typeid(demo::Input));
  StyleNode *placeholder =
      tree.add(input, typeid(demo::Text), {}, /*internal=*/true, "placeholder");
  StyleNode *outside_text = tree.add(window, typeid(demo::Text));

  style_rebuild_blooms(*window);

  StyleResolver resolver;
  resolver.set_stylesheet(parsed.sheet);
  resolver.set_theme(theme.theme);
  check(resolver.resolve_tree(*window) == Invalidation::Layout,
        "the initial style pass requests layout");

  const PropertyIndex background = styles::Background::index();
  const PropertyIndex foreground = styles::Foreground::index();
  const PropertyIndex font_size = styles::FontSize::index();
  const PropertyIndex padding = styles::Padding::index();

  // -- Combinators ----------------------------------------------------------

  check(same(color_of(*window->computed, background),
             Color::from_rgb_u32(0x16181C)),
        "window background comes from the theme token");
  check(same(color_of(*sidebar->computed, background),
             Color::from_rgb_u32(0x2A2D31)),
        "sidebar background comes from the theme token");

  check(button_label->computed->get<styles::FontSize>() == 12.0f,
        "descendant selector reaches a text nested two levels down");
  check(outside_text->computed->get<styles::FontSize>() == 14.0f,
        "descendant selector does not reach text outside the sidebar");
  check(button_label->computed->get<styles::LineHeight>() == 24.0f,
        "line-height parses in logical pixels and inherits");
  check(font_weight_value(button_label->computed->get<styles::FontWeight>()) ==
            600,
        "numeric font-weight parses and inherits");

  FontWeight keyword_weight = FontWeight::Normal;
  check(parse_style_value("bold", keyword_weight) &&
            keyword_weight == FontWeight::Bold,
        "font-weight accepts CSS keywords");
  check(!parse_style_value("1001", keyword_weight),
        "font-weight rejects values outside 1..1000");

  const auto retired_foreground =
      StyleParser::parse("text { foreground: red; }");
  check(!retired_foreground.diagnostics.empty(),
        "the retired VSS foreground name is rejected");

  const Spacing<float> near_padding =
      near_button->computed->get<styles::Padding>();
  check(near_padding.top == 4.0f && near_padding.left == 8.0f,
        "child selector applies to a direct child");
  const Spacing<float> far_padding =
      far_button->computed->get<styles::Padding>();
  check(far_padding.top == 0.0f && far_padding.left == 0.0f,
        "child selector does not apply through an intermediate node");

  const Spacing<float> sidebar_padding =
      sidebar->computed->get<styles::Padding>();
  check(sidebar_padding.top == 8.0f && sidebar_padding.left == 12.0f,
        "two-value padding reads as CSS does");
  check(sidebar->computed->get<styles::BorderRadius>().left_top == 4.0f,
        "window > .sidebar matched");

  // -- Inheritance ----------------------------------------------------------

  check(same(color_of(*button_label->computed, foreground),
             Color::from_rgb_u32(0xE6E6E6)),
        "color inherits from the window down to a nested text");
  check(!button_label->computed->has<styles::Background>(),
        "background does not inherit");

  // -- Component properties -------------------------------------------------

  check(same(input->computed->get<demo::Input::CaretColor>(),
             Color::from_rgb_u32(0x4C9AFF)),
        "a component property set by an unprefixed name in its own rule");
  check(input->computed->get<demo::Input::CaretWidth>() == 2.0f,
        "a second component property on the same rule");
  check(near_button->computed->get<demo::Input::CaretWidth>() == 1.0f,
        "an unrelated widget falls back to the declared default");

  // -- Shadow boundary and ::part() -----------------------------------------

  check(placeholder->computed->get<styles::FontSize>() == 13.0f,
        "an internal child inherits from its host through the boundary");
  check(!placeholder->computed->has<styles::Padding>(),
        "but picks up nothing else from outside rules");
  check(same(color_of(*placeholder->computed, foreground),
             Color::from_rgb_u32(0x8A8F98)),
        "::part() reaches an exposed internal child");

  // -- Component-owned stylesheets -----------------------------------------

  {
    // Parse this deliberately with the parser's normal User default.
    // Registering it as component VSS must still force every rule down to
    // WidgetDefault.
    StyleParser::Result component_defaults =
        StyleParser::parse("button { background: #C01020; }\n"
                           "button text { color: #F0E0D0; }\n",
                           "button.default.vss");

    StyleResolver default_resolver;
    default_resolver.add_default_stylesheet(component_defaults.sheet);
    default_resolver.resolve_tree(*window);
    check(same(color_of(*near_button->computed, background),
               Color::from_rgb_u32(0xC01020)),
          "a component stylesheet styles its host");
    check(same(color_of(*button_label->computed, foreground),
               Color::from_rgb_u32(0xF0E0D0)),
          "a component stylesheet reaches a descendant");

    StyleParser::Result application =
        StyleParser::parse("button { background: #1020C0; }\n"
                           "text { color: #0D0E0F; }\n",
                           "application.vss");
    default_resolver.set_stylesheet(application.sheet);
    default_resolver.resolve_tree(*window);
    check(same(color_of(*near_button->computed, background),
               Color::from_rgb_u32(0x1020C0)),
          "application VSS overrides a component host default");
    check(
        same(color_of(*button_label->computed, foreground),
             Color::from_rgb_u32(0x0D0E0F)),
        "application VSS overrides a more-specific component descendant rule");
  }

  resolver.resolve_tree(*window);

  {
    // `.sidebar text` must not reach inside the input, or every component
    // built out of Text would be repainted by an unrelated application rule.
    StyleParser::Result probe = StyleParser::parse(
        ".sidebar text { border-width: 3; }", "probe.vss");
    auto probe_sheet = std::make_shared<StyleSheet>(*parsed.sheet);
    probe_sheet->append(*probe.sheet);
    StyleResolver probe_resolver;
    probe_resolver.set_stylesheet(probe_sheet);
    probe_resolver.set_theme(theme.theme);
    probe_resolver.resolve_tree(*window);
    check(button_label->computed->get<styles::BorderWidth>() == 3.0f,
          "a light-tree text is reached by .sidebar text");
    check(placeholder->computed->get<styles::BorderWidth>() == 0.0f,
          "the same rule does not pierce into the input's internals");
  }

  resolver.resolve_tree(*window);

  // -- Hover ----------------------------------------------------------------

  const auto before = near_button->computed;
  near_button->status |= StatusBits::kHovered;
  check(resolver.resolve_subtree(*near_button, /*force_subtree=*/true) ==
            Invalidation::Paint,
        "a background-only hover requests paint");
  check(same(color_of(*near_button->computed, background),
             Color::from_rgb_u32(0x35393E)),
        ":hover applies on a status change");
  near_button->status &= static_cast<std::uint8_t>(~StatusBits::kHovered);
  check(resolver.resolve_subtree(*near_button, /*force_subtree=*/true) ==
            Invalidation::Paint,
        "leaving a background-only hover requests paint");
  check(near_button->computed == before,
        "leaving hover returns to the identical shared style object");

  {
    StyleParser::Result layout_change =
        StyleParser::parse("button { padding: 20; }", "layout-change.vss");
    resolver.set_stylesheet(layout_change.sheet);
    check(resolver.resolve_tree(*window) == Invalidation::Layout,
          "a padding change requests layout");
    resolver.set_stylesheet(parsed.sheet);
    resolver.resolve_tree(*window);
  }

  // -- Theme switch ---------------------------------------------------------

  auto light = std::make_shared<Theme>("Light");
  light->set("surface.base", "#FFFFFF");
  light->set("surface.raised", "#F2F3F5");
  light->set("surface.raised.hover", "#E4E6EA");
  light->set("text.primary", "#1A1A1A");
  light->set("text.muted", "#6B7078");
  light->set("accent", "#0B66FF");

  resolver.set_theme(light);
  resolver.resolve_tree(*window);
  check(same(color_of(*window->computed, background), Color(255, 255, 255)),
        "switching theme rebinds tokens without touching the rules");
  check(same(input->computed->get<demo::Input::CaretColor>(),
             Color::from_rgb_u32(0x0B66FF)),
        "component properties follow the theme too");

  // -- Sharing --------------------------------------------------------------

  Tree list;
  StyleNode *root = list.add(nullptr, typeid(demo::Panel), {"sidebar"});
  for (int i = 0; i < 500; ++i) {
    StyleNode *row = list.add(root, typeid(demo::Button));
    list.add(row, typeid(demo::Text));
  }
  style_rebuild_blooms(*root);
  StyleResolver list_resolver;
  list_resolver.set_stylesheet(parsed.sheet);
  list_resolver.set_theme(light);
  list_resolver.resolve_tree(*root);

  check(root->children[0]->computed == root->children[400]->computed,
        "identical siblings share one ComputedStyle allocation");
  std::printf("  1001 nodes -> %zu distinct computed styles\n",
              list_resolver.cache().size());
  check(list_resolver.cache().size() <= 4,
        "a thousand-node list collapses to a handful of styles");

  // -- Cursor ---------------------------------------------------------------

  {
    StyleParser::Result cursor_sheet =
        StyleParser::parse("button { cursor: pointer; }\n"
                           "button:hover { cursor: not-allowed; }",
                           "cursor.vss");
    check(cursor_sheet.diagnostics.empty(), "CSS cursor values parse");

    Tree cursor_tree;
    StyleNode *cursor_button = cursor_tree.add(nullptr, typeid(demo::Button));
    StyleNode *cursor_label =
        cursor_tree.add(cursor_button, typeid(demo::Text));
    style_rebuild_blooms(*cursor_button);

    StyleResolver cursor_resolver;
    cursor_resolver.set_stylesheet(cursor_sheet.sheet);
    cursor_resolver.resolve_tree(*cursor_button);
    check(PropertyRegistry::instance()
              .describe(styles::Cursor::index())
              .inherited,
          "cursor is registered as an inherited property");
    check(cursor_button->computed->get<styles::Cursor>() ==
              CursorShape::Pointer,
          "cursor applies to the matched widget");
    check(cursor_label->computed->get<styles::Cursor>() == CursorShape::Pointer,
          "cursor inherits into the deepest hovered descendant");

    cursor_button->status |= StatusBits::kHovered;
    check(cursor_resolver.resolve_subtree(*cursor_button, true) ==
              Invalidation::None,
          "a cursor-only state change does not dirty layout or paint");
    check(cursor_label->computed->get<styles::Cursor>() ==
              CursorShape::NotAllowed,
          ":hover cursor overrides the inherited normal value");
  }

  // -- Robustness -----------------------------------------------------------

  {
    StyleParser::Result broken =
        StyleParser::parse("button { background: #112233; }\n"
                           "nosuchwidget { background: #445566; }\n"
                           "button { padding: not-a-number; }\n"
                           "button { font-size: 20; }\n",
                           "broken.vss");
    check(broken.diagnostics.size() == 2,
          "an unknown widget and a bad value each report once");
    check(broken.sheet->size() == 3, "the readable rules survive");
  }

  {
    StyleResolver missing;
    missing.set_stylesheet(parsed.sheet);
    missing.set_theme(std::make_shared<Theme>("Empty"));
    missing.resolve_tree(*window);
    check(!missing.diagnostics().empty(),
          "undefined tokens are reported, not fatal");
    check(same(color_of(*window->computed, background), Color(0, 0, 0, 0)),
          "an undefined token leaves the property at its default");
  }

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
