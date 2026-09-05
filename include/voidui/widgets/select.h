#pragma once

#include "voidui/core/context.h"
#include "voidui/core/state.h"

namespace voidui {

struct SelectOption {
  std::string value;
  std::string label;
  bool disabled = false;
  bool operator==(const SelectOption &) const = default;
};

namespace detail {
struct SelectModel;
}

/// A single-value select. Values are stable identifiers; labels are display
/// text.
class Select : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Select, "select")
  using Appearance = styles::Appearance;

  explicit Select(std::vector<SelectOption> options = {});
  VOIDUI_WIDGET_SIZE_STYLE
#define VOIDUI_SELECT_STYLE(name, property)                                    \
  template <typename Self>                                                     \
  Self &&name(this Self &&self, styles::property::Value value) {               \
    self.template set_style<styles::property>(std::move(value));               \
    return std::forward<Self>(self);                                           \
  }
  VOIDUI_SELECT_STYLE(background, Background)
  VOIDUI_SELECT_STYLE(color, Foreground)
  VOIDUI_SELECT_STYLE(padding, Padding)
  VOIDUI_SELECT_STYLE(font_size, FontSize)
  VOIDUI_SELECT_STYLE(font_family, FontFamily)
  VOIDUI_SELECT_STYLE(font_weight, FontWeight)
  VOIDUI_SELECT_STYLE(line_height, LineHeight)
  VOIDUI_SELECT_STYLE(cursor, Cursor)
  VOIDUI_SELECT_STYLE(border_radius, BorderRadius)
  VOIDUI_SELECT_STYLE(border_width, BorderWidth)
  VOIDUI_SELECT_STYLE(border_color, BorderColor)
  VOIDUI_SELECT_STYLE(box_shadow, BoxShadow)
#undef VOIDUI_SELECT_STYLE
  VOIDUI_FLUENT_METHOD(border, (Border value),
                       set_style<styles::BorderRadius>(value.get_radius());
                       set_style<styles::BorderWidth>(value.get_width());
                       set_style<styles::BorderColor>(value.get_brush());)
  VOIDUI_FLUENT_METHOD(appearance, (SelectAppearance value),
                       set_style<Appearance>(value);)
  VOIDUI_FLUENT_METHOD(value, (std::string value),
                       set_value_(std::move(value));)
  VOIDUI_FLUENT_METHOD(
      value, (State<std::string> state), set_value_(state.get());
      on_change_ = [state](const std::string &next) { state.set(next); };)
  VOIDUI_FLUENT_METHOD(placeholder, (std::string value),
                       set_placeholder_(std::move(value));)
  VOIDUI_FLUENT_METHOD(disabled, (bool value), set_disabled_(value);)
  VOIDUI_FLUENT_METHOD(picker_max_height, (float value),
                       set_picker_height_(value);)
  VOIDUI_FLUENT_SETTER(on_change, on_change_,
                       std::function<void(const std::string &)>)

  static SelectorBuilder option_selector();
  const std::string &value() const;
  bool is_open() const;
  bool focusable() const override;
  std::uint8_t style_status() const override;
  void focus_lost(Event &event) override;
  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  bool clips_children() const override { return true; }
  EventResult on_event(Event &event) override;
  void inherit_runtime(const Widget &previous) override;
  std::unique_ptr<Widget> clone() const override;
  std::shared_ptr<const StyleSheet> default_stylesheet() const override;

private:
  void set_value_(std::string value);
  void set_placeholder_(std::string value);
  void set_disabled_(bool value);
  void set_picker_height_(float value);
  std::shared_ptr<detail::SelectModel> model_;
  std::function<void(const std::string &)> on_change_;
  bool value_set_ = false;
};

[[nodiscard]] inline Select select(std::vector<SelectOption> options = {}) {
  return Select(std::move(options));
}
[[nodiscard]] inline Select select(std::vector<SelectOption> options,
                                   State<std::string> value) {
  return Select(std::move(options)).value(std::move(value));
}
} // namespace voidui
