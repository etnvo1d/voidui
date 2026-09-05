#pragma once

#include <functional>
#include <memory>
#include <string>
#include <string_view>
#include <utility>

#include "voidui/core/context.h"
#include "voidui/core/state.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"
#include "voidui/paint/text_layout.h"

namespace voidui {

namespace detail {

class TextControl : public Widget {
public:
  using ChangeHandler = std::function<void(const std::string &)>;
  using SubmitHandler = std::function<void(const std::string &)>;

  const std::string &value() const { return value_; }

  void register_children(Registrar &registrar) override;
  Size<float> layout(Constraints constraints, LayoutContext &ctx) override;
  void draw(const DrawContext &ctx, Painter &painter) override;
  EventResult on_event(Event &event) override;
  void inherit_runtime(const Widget &previous) override;

  bool focusable() const override { return true; }
  bool clips_children() const override { return true; }
  Rect<float> children_clip(Rect<float> bounds) const override;
  bool supports_text_selection() const override { return true; }
  bool accepts_text_input() const override { return true; }
  std::optional<TextInputArea>
  text_input_area(Rect<float> bounds) const override;
  void text_input_stopped() override { clear_composition_(); }
  std::uint32_t selection_hit_test(Point<float> point,
                                   Rect<float> bounds) const override;
  std::pair<std::uint32_t, std::uint32_t>
  selection_word_at(std::uint32_t offset) const override;
  std::string_view selection_text() const override { return value_; }
  void selection_changed(std::uint32_t anchor, std::uint32_t focus) override;
  std::optional<Rect<float>> selection_scroll_viewport(Rect<float> bounds) const override;
  Invalidation selection_scroll_by(Point<float> delta) override;
  std::pair<std::uint32_t, std::uint32_t> text_selection() const override {
    return {anchor_, focus_};
  }

protected:
  explicit TextControl(bool multiline) : multiline_(multiline) {}
  TextControl(bool multiline, std::string value)
      : value_(value), declared_value_(std::move(value)), multiline_(multiline),
        value_explicit_(true) {}

  void set_value_(std::string value);
  void set_placeholder_(std::string value);
  void set_inline_start_(std::unique_ptr<Widget> value);
  void set_inline_end_(std::unique_ptr<Widget> value);
  void set_block_start_(std::unique_ptr<Widget> value);
  void set_block_end_(std::unique_ptr<Widget> value);
  void copy_to_(TextControl &target) const;

  virtual float inline_gap_(const ComputedStyle &style) const = 0;
  virtual float block_gap_(const ComputedStyle &style) const = 0;
  virtual const Brush &caret_color_(const ComputedStyle &style) const = 0;
  virtual float caret_blink_(const ComputedStyle &style) const = 0;
  virtual const Brush &placeholder_color_(const ComputedStyle &style) const = 0;

  ChangeHandler on_change_;
  SubmitHandler on_submit_;

private:
  void adopt_font_(const ComputedStyle &style);
  void ensure_layout_(float width);
  void update_composition_(const TextEditingEvent &editing, Event &event);
  void clear_composition_();
  void replace_selection_(std::string_view text, Event &event);
  void erase_selection_(Event &event);
  void move_horizontal_(bool right, bool extend, Event &event);
  void move_line_(bool down, bool extend, Event &event);
  void set_caret_(std::uint32_t offset, bool extend);
  void notify_change_();
  static std::size_t previous_codepoint_(std::string_view text,
                                         std::size_t offset);
  static std::size_t next_codepoint_(std::string_view text, std::size_t offset);
  static std::size_t byte_offset_for_codepoint_(std::string_view text,
                                                std::int32_t index);

  bool has_composition_() const { return !composition_text_.empty(); }
  std::uint32_t composition_display_begin_() const {
    return composition_begin_;
  }
  std::uint32_t composition_display_end_() const {
    return composition_begin_ +
           static_cast<std::uint32_t>(composition_text_.size());
  }
  std::uint32_t composition_caret_() const {
    return composition_display_begin_() + composition_cursor_;
  }

  /// One line box, rounded the way `TextLayout` rounds its own. An empty field
  /// has no layout to ask and its caret must still be exactly as tall as the
  /// caret in a field with text in it.
  float line_box_() const;

  /// The caret's width in logical units, chosen as a whole number of device
  /// pixels: the renderer rounds a rect's two edges independently, so a width
  /// that is not whole comes out a pixel wider at some positions than others
  /// and the caret changes thickness as it moves through the text.
  float caret_width_() const;

  /// The editor box translated from its stable control-local layout geometry
  /// to the control's current global position. A parent is allowed to place a
  /// child after measuring it, so no global position may be cached by layout.
  Rect<float> global_editor_bounds_(Point<float> control_origin) const;

  /// Where the text layout's top-left sits in global coordinates, scroll
  /// included. Drawing, hit testing and the caret all read this one expression
  /// instead of repeating it and drifting apart.
  Point<float> text_origin_(Point<float> control_origin) const;

  /// The caret in layout-local coordinates. Never asked of the layout while
  /// that layout holds the placeholder, which is text the caret is not in.
  Rect<float> caret_local_() const;

  /// The caret in global coordinates, on the device pixel grid, held inside
  /// the editor box so neither end of the text can clip it away.
  Rect<float> caret_rect_(Point<float> control_origin) const;

  /// Scrolls by the least amount that brings the caret fully into view.
  void reveal_caret_();

  /// Advances the blink and says whether the caret shows on this frame, arming
  /// the deadline for the next toggle as it goes.
  bool draw_caret_(const DrawContext &ctx);

  /// Records that the caret moved: the blink restarts solid, and the column
  /// that vertical motion aims at is forgotten.
  void caret_moved_();

  std::string value_;
  std::string declared_value_;
  std::string placeholder_;
  std::string composition_text_;
  std::unique_ptr<Widget> inline_start_;
  std::unique_ptr<Widget> inline_end_;
  std::unique_ptr<Widget> block_start_;
  std::unique_ptr<Widget> block_end_;

  FontFamilyList families_;
  std::shared_ptr<FontStack> fonts_;
  std::shared_ptr<const TextLayout> layout_;
  std::string layout_text_;
  /// Editor geometry relative to this control. Containers measure children
  /// before placing them and may translate the whole subtree afterwards.
  Rect<float> editor_bounds_;
  float layout_width_ = -1.0f;
  float device_scale_ = 1.0f;
  float line_height_ = 0.0f;
  float horizontal_scroll_ = 0.0f;
  float vertical_scroll_ = 0.0f;
  std::uint32_t anchor_ = 0;
  std::uint32_t focus_ = 0;
  std::uint32_t composition_begin_ = 0;
  std::uint32_t composition_end_ = 0;
  std::uint32_t composition_cursor_ = 0;
  std::uint32_t composition_selection_end_ = 0;
  std::size_t inline_start_index_ = static_cast<std::size_t>(-1);
  std::size_t inline_end_index_ = static_cast<std::size_t>(-1);
  std::size_t block_start_index_ = static_cast<std::size_t>(-1);
  std::size_t block_end_index_ = static_cast<std::size_t>(-1);

  /// The x that a vertical move aims for, in layout coordinates, held across a
  /// run of Up/Down presses. Without it, passing through one short line clips
  /// the column for every line after it. Negative when there is none.
  float goal_x_ = -1.0f;

  /// The blink is phase-locked to the last caret movement rather than to the
  /// clock, so the caret is solid the instant a key is pressed -- which is what
  /// makes fast typing legible -- and only starts counting from there.
  double blink_epoch_ = 0.0;
  std::uint64_t caret_revision_ = 0;
  std::uint64_t painted_caret_revision_ = 0;

  bool caret_focused_ = false;
  bool multiline_ = false;
  bool value_explicit_ = false;
};

} // namespace detail

/// A single-line editable text control with four optional widget slots.
class Input : public detail::TextControl {
public:
  VOIDUI_STYLE_SCOPE(Input, "input")

  VOIDUI_STYLE_PROPERTY(Input, InlineGap, float, "inline-gap", NotInherited,
                        Layout, 8.0f);
  VOIDUI_STYLE_PROPERTY(Input, BlockGap, float, "block-gap", NotInherited,
                        Layout, 6.0f);
  VOIDUI_STYLE_PROPERTY(Input, CaretColor, Brush, "caret-color", NotInherited,
                        Paint, Brush(Color(20, 20, 20)));
  VOIDUI_STYLE_PROPERTY(Input, CaretBlink, float, "caret-blink", NotInherited,
                        Paint, 0.53f);
  VOIDUI_STYLE_PROPERTY(Input, PlaceholderColor, Brush, "placeholder-color",
                        NotInherited, Paint, Brush(Color(115, 115, 115)));

  Input() : TextControl(false) {}
  explicit Input(std::string value) : TextControl(false, std::move(value)) {}
  using TextControl::value;

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(foreground, (Brush value),
                       set_style<styles::Foreground>(std::move(value));)
  VOIDUI_FLUENT_METHOD(color, (Brush value),
                       set_style<styles::Foreground>(std::move(value));)
  VOIDUI_FLUENT_METHOD(line_height, (float value),
                       set_style<styles::LineHeight>(value);)
  VOIDUI_FLUENT_METHOD(font_weight, (FontWeight value),
                       set_style<styles::FontWeight>(value);)
  VOIDUI_FLUENT_METHOD(padding, (Padding value),
                       set_style<styles::Padding>(value);)
  VOIDUI_FLUENT_METHOD(
      border, (Border value),
      set_style<styles::BorderRadius>(value.get_radius());
      set_style<styles::BorderWidth>(value.get_width());
      set_style<styles::BorderColor>(value.get_brush());)
  VOIDUI_FLUENT_METHOD(value, (std::string value),
                       set_value_(std::move(value));)
  VOIDUI_FLUENT_METHOD(placeholder, (std::string value),
                       set_placeholder_(std::move(value));)
  VOIDUI_FLUENT_METHOD(inline_gap, (float value), set_style<InlineGap>(value);)
  VOIDUI_FLUENT_METHOD(block_gap, (float value), set_style<BlockGap>(value);)
  VOIDUI_FLUENT_METHOD(caret_color, (Brush value),
                       set_style<CaretColor>(std::move(value));)
  VOIDUI_FLUENT_METHOD(caret_blink, (float seconds),
                       set_style<CaretBlink>(seconds);)
  VOIDUI_FLUENT_METHOD(placeholder_color, (Brush value),
                       set_style<PlaceholderColor>(std::move(value));)
  VOIDUI_FLUENT_SETTER(on_change, on_change_, ChangeHandler)
  VOIDUI_FLUENT_SETTER(on_submit, on_submit_, SubmitHandler)

  template <WidgetClass T> Input &inline_start(T &&widget) & {
    set_inline_start_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Input &&inline_start(T &&widget) && {
    set_inline_start_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }
  template <WidgetClass T> Input &inline_end(T &&widget) & {
    set_inline_end_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Input &&inline_end(T &&widget) && {
    set_inline_end_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }
  template <WidgetClass T> Input &block_start(T &&widget) & {
    set_block_start_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Input &&block_start(T &&widget) && {
    set_block_start_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }
  template <WidgetClass T> Input &block_end(T &&widget) & {
    set_block_end_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Input &&block_end(T &&widget) && {
    set_block_end_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }

  std::shared_ptr<const StyleSheet> default_stylesheet() const override;
  std::unique_ptr<Widget> clone() const override;

private:
  float inline_gap_(const ComputedStyle &style) const override {
    return style.get<InlineGap>();
  }
  float block_gap_(const ComputedStyle &style) const override {
    return style.get<BlockGap>();
  }
  const Brush &caret_color_(const ComputedStyle &style) const override {
    return style.get<CaretColor>();
  }
  float caret_blink_(const ComputedStyle &style) const override {
    return style.get<CaretBlink>();
  }
  const Brush &placeholder_color_(const ComputedStyle &style) const override {
    return style.get<PlaceholderColor>();
  }
};

/// A wrapping, multi-line editable text control with the same slot model.
class Textarea : public detail::TextControl {
public:
  VOIDUI_STYLE_SCOPE(Textarea, "textarea")

  VOIDUI_STYLE_PROPERTY(Textarea, InlineGap, float, "inline-gap", NotInherited,
                        Layout, 8.0f);
  VOIDUI_STYLE_PROPERTY(Textarea, BlockGap, float, "block-gap", NotInherited,
                        Layout, 6.0f);
  VOIDUI_STYLE_PROPERTY(Textarea, CaretColor, Brush, "caret-color",
                        NotInherited, Paint, Brush(Color(20, 20, 20)));
  VOIDUI_STYLE_PROPERTY(Textarea, CaretBlink, float, "caret-blink",
                        NotInherited, Paint, 0.53f);
  VOIDUI_STYLE_PROPERTY(Textarea, PlaceholderColor, Brush, "placeholder-color",
                        NotInherited, Paint, Brush(Color(115, 115, 115)));

  Textarea() : TextControl(true) {}
  explicit Textarea(std::string value) : TextControl(true, std::move(value)) {}
  using TextControl::value;

  VOIDUI_WIDGET_SIZE_STYLE
  VOIDUI_FLUENT_METHOD(background, (Brush value),
                       set_style<styles::Background>(std::move(value));)
  VOIDUI_FLUENT_METHOD(foreground, (Brush value),
                       set_style<styles::Foreground>(std::move(value));)
  VOIDUI_FLUENT_METHOD(color, (Brush value),
                       set_style<styles::Foreground>(std::move(value));)
  VOIDUI_FLUENT_METHOD(line_height, (float value),
                       set_style<styles::LineHeight>(value);)
  VOIDUI_FLUENT_METHOD(font_weight, (FontWeight value),
                       set_style<styles::FontWeight>(value);)
  VOIDUI_FLUENT_METHOD(padding, (Padding value),
                       set_style<styles::Padding>(value);)
  VOIDUI_FLUENT_METHOD(
      border, (Border value),
      set_style<styles::BorderRadius>(value.get_radius());
      set_style<styles::BorderWidth>(value.get_width());
      set_style<styles::BorderColor>(value.get_brush());)
  VOIDUI_FLUENT_METHOD(value, (std::string value),
                       set_value_(std::move(value));)
  VOIDUI_FLUENT_METHOD(placeholder, (std::string value),
                       set_placeholder_(std::move(value));)
  VOIDUI_FLUENT_METHOD(inline_gap, (float value), set_style<InlineGap>(value);)
  VOIDUI_FLUENT_METHOD(block_gap, (float value), set_style<BlockGap>(value);)
  VOIDUI_FLUENT_METHOD(caret_color, (Brush value),
                       set_style<CaretColor>(std::move(value));)
  VOIDUI_FLUENT_METHOD(caret_blink, (float seconds),
                       set_style<CaretBlink>(seconds);)
  VOIDUI_FLUENT_METHOD(placeholder_color, (Brush value),
                       set_style<PlaceholderColor>(std::move(value));)
  VOIDUI_FLUENT_SETTER(on_change, on_change_, ChangeHandler)

  template <WidgetClass T> Textarea &inline_start(T &&widget) & {
    set_inline_start_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Textarea &&inline_start(T &&widget) && {
    set_inline_start_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }
  template <WidgetClass T> Textarea &inline_end(T &&widget) & {
    set_inline_end_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Textarea &&inline_end(T &&widget) && {
    set_inline_end_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }
  template <WidgetClass T> Textarea &block_start(T &&widget) & {
    set_block_start_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Textarea &&block_start(T &&widget) && {
    set_block_start_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }
  template <WidgetClass T> Textarea &block_end(T &&widget) & {
    set_block_end_(transfer_widget(std::forward<T>(widget)));
    return *this;
  }
  template <WidgetClass T> Textarea &&block_end(T &&widget) && {
    set_block_end_(transfer_widget(std::forward<T>(widget)));
    return std::move(*this);
  }

  std::shared_ptr<const StyleSheet> default_stylesheet() const override;
  std::unique_ptr<Widget> clone() const override;

private:
  float inline_gap_(const ComputedStyle &style) const override {
    return style.get<InlineGap>();
  }
  float block_gap_(const ComputedStyle &style) const override {
    return style.get<BlockGap>();
  }
  const Brush &caret_color_(const ComputedStyle &style) const override {
    return style.get<CaretColor>();
  }
  float caret_blink_(const ComputedStyle &style) const override {
    return style.get<CaretBlink>();
  }
  const Brush &placeholder_color_(const ComputedStyle &style) const override {
    return style.get<PlaceholderColor>();
  }
};

[[nodiscard]] inline Input input() { return Input{}; }
[[nodiscard]] inline Input input(std::string value) {
  return Input(std::move(value));
}
[[nodiscard]] inline Input input(const State<std::string> &value) {
  return Input(value.get()).on_change([value](const std::string &next) {
    value.set(next);
  });
}
[[nodiscard]] inline Textarea textarea() { return Textarea{}; }
[[nodiscard]] inline Textarea textarea(std::string value) {
  return Textarea(std::move(value));
}
[[nodiscard]] inline Textarea textarea(const State<std::string> &value) {
  return Textarea(value.get()).on_change([value](const std::string &next) {
    value.set(next);
  });
}

} // namespace voidui
