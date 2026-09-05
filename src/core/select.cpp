#include "voidui/widgets/select.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/text.h"

#include <cctype>
#include <chrono>
#include <limits>

namespace voidui {
namespace detail {
MouseButton select_mouse_button(Event &event) {
  if (event.type() == EventType::MousePressed)
    return static_cast<MousePressedEvent &>(event).button();
  if (event.type() == EventType::MouseReleased)
    return static_cast<MouseReleasedEvent &>(event).button();
  return static_cast<MouseClickedEvent &>(event).button();
}

struct SelectModel {
  std::vector<SelectOption> options;
  std::string value;
  std::string placeholder = "Select an option";
  bool disabled = false;
  bool open = false;
  bool reveal = false;
  int active = -1;
  float picker_height = 280;
  std::vector<float> tops, heights;
  std::string search;
  std::chrono::steady_clock::time_point search_at{};
  std::function<void(const std::string &)> on_change;

  int selected() const {
    for (size_t i = 0; i < options.size(); ++i)
      if (options[i].value == value)
        return static_cast<int>(i);
    return -1;
  }
  bool enabled(int index) const {
    return index >= 0 && index < static_cast<int>(options.size()) &&
           !options[index].disabled;
  }
  int next(int from, int direction) const {
    for (int i = from + direction;
         i >= 0 && i < static_cast<int>(options.size()); i += direction)
      if (enabled(i))
        return i;
    return enabled(from) ? from : -1;
  }
  void activate(int index, Event &event) {
    if (!enabled(index))
      return;
    active = index;
    reveal = true;
    event.request_style();
    event.request_layout();
  }
  void close(Event &event) {
    if (!open)
      return;
    open = false;
    search.clear();
    event.request_style();
    event.request_layout();
  }
  void commit(int index, Event &event) {
    if (disabled || !enabled(index))
      return;
    const auto next_value = options[index].value;
    const bool changed = value != next_value;
    value = next_value;
    close(event);
    event.request_style();
    event.request_layout();
    auto callback = on_change;
    if (changed && callback)
      callback(next_value);
  }
};

// Shared label geometry for the trigger and each option. Text is clipped to its
// own allocated box so an unusually long label cannot paint over the icon.
Size<float> select_row_layout(Constraints c, LayoutContext &ctx,
                              bool icon = true) {
  const auto chrome =
      ctx.style.get<styles::Padding>() +
      Spacing<float>(std::max(ctx.style.get<styles::BorderWidth>(), 0.0f));
  const auto &specified = ctx.style.layout_size();
  float width = c.max_width;
  if (auto fixed = std::get_if<Length::Fixed>(&specified.width.value))
    width = std::clamp(fixed->value, c.min_width, c.max_width);
  const float inner_width = std::max(0.0f, width - chrome.left - chrome.right);
  auto glyph = ctx.constrain_child(
      1, {0, icon ? std::min(20.0f, inner_width) : 0, 0, c.max_height});
  const float gap = icon ? 10.0f : 0.0f;
  auto label = ctx.constrain_child(
      0, {0, std::max(0.0f, inner_width - glyph.width - gap), 0,
          std::max(0.0f, c.max_height - chrome.top - chrome.bottom)});
  auto size = c.resolve(
      specified,
      {label.width + glyph.width + gap + chrome.left + chrome.right,
       std::max(label.height, glyph.height) + chrome.top + chrome.bottom});
  const float height = std::max(0.0f, size.height - chrome.top - chrome.bottom);
  ctx.place_child(
      0, {chrome.left,
          chrome.top + std::max(0.0f, (height - label.height) * 0.5f)});
  ctx.place_child(
      1, {std::max(chrome.left, size.width - chrome.right - glyph.width),
          chrome.top + std::max(0.0f, (height - glyph.height) * 0.5f)});
  return size;
}

class SelectLabel : public Text {
public:
  SelectLabel(std::shared_ptr<SelectModel> model, int index)
      : Text(""), model_(std::move(model)), index_(index) {
    wrap(false);
  }
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    const int i = index_ == -1 ? model_->selected() : index_;
    content(i >= 0 ? model_->options[i].label : model_->placeholder);
    return Text::layout(c, ctx);
  }
  void draw(const DrawContext &ctx, Painter &painter) override {
    painter.save();
    painter.clip_rect(ctx.bounds);
    if (index_ == -1 && model_->selected() < 0)
      painter.opacity(0.6f);
    Text::draw(ctx, painter);
    painter.restore();
  }
  bool supports_text_selection() const override { return false; }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<SelectLabel>(model_, index_);
  }

private:
  std::shared_ptr<SelectModel> model_;
  int index_;
};

class SelectGlyph : public Widget {
public:
  SelectGlyph(std::shared_ptr<SelectModel> model, int index)
      : model_(std::move(model)), index_(index) {}
  void register_children(Registrar &) override {}
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    return c.resolve(ctx.style.layout_size(), {16, 16});
  }
  void draw(const DrawContext &ctx, Painter &painter) override {
    if (ctx.bounds.size.width <= 0 ||
        (index_ >= 0 && model_->selected() != index_))
      return;
    detail::draw_container_box(ctx, painter);
    const float x = ctx.bounds.origin.x + ctx.bounds.size.width * 0.5f;
    const float y = ctx.bounds.origin.y + ctx.bounds.size.height * 0.5f;
    Path path;
    if (index_ >= 0) {
      path.move_to({x - 5, y}).line_to({x - 1, y + 4}).line_to({x + 6, y - 4});
    } else {
      const float sign = model_->open ? -1.0f : 1.0f;
      path.move_to({x - 4, y - 2 * sign})
          .line_to({x, y + 2 * sign})
          .line_to({x + 4, y - 2 * sign});
    }
    painter.stroke_path(path, Paint(ctx.style.get<styles::Foreground>()),
                        Pen(1.8f));
  }
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<SelectGlyph>(model_, index_);
  }

private:
  std::shared_ptr<SelectModel> model_;
  int index_;
};

class SelectOptionWidget : public Widget {
public:
  VOIDUI_STYLE_SCOPE(SelectOptionWidget, "option")
  SelectOptionWidget(std::shared_ptr<SelectModel> model, int index)
      : model_(std::move(model)), index_(index) {}
  std::uint8_t style_status() const override {
    return (model_->selected() == index_ ? StatusBits::kChecked : 0) |
           (model_->disabled || !model_->enabled(index_)
                ? StatusBits::kDisabled
                : StatusBits::kEnabled) |
           (model_->open && model_->active == index_ ? StatusBits::kFocused
                                                     : 0);
  }
  void register_children(Registrar &r) override {
    r.take_internal_child(std::make_unique<SelectLabel>(model_, index_),
                          "label");
    r.take_internal_child(std::make_unique<SelectGlyph>(model_, index_),
                          "checkmark");
  }
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    return select_row_layout(c, ctx);
  }
  bool clips_children() const override { return true; }
  void draw(const DrawContext &ctx, Painter &p) override {
    draw_container_box(ctx, p);
  }
  EventResult on_event(Event &event) override {
    if (event.type() == EventType::MouseMoved) {
      if (model_->enabled(index_) && model_->active != index_) {
        model_->active = index_;
        event.request_style();
      }
      return EventResult::Handled;
    }
    if (event.type() == EventType::MousePressed ||
        event.type() == EventType::MouseReleased ||
        event.type() == EventType::MouseClicked) {
      if (detail::select_mouse_button(event) != MouseButton::Left)
        return EventResult::Unhandled;
      if (event.type() == EventType::MouseClicked)
        model_->commit(index_, event);
      return EventResult::Handled;
    }
    return EventResult::Unhandled;
  }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<SelectOptionWidget>(model_, index_);
  }

private:
  std::shared_ptr<SelectModel> model_;
  int index_;
};

class SelectList : public Widget {
public:
  explicit SelectList(std::shared_ptr<SelectModel> model)
      : model_(std::move(model)) {}
  void register_children(Registrar &r) override {
    for (int i = 0; i < static_cast<int>(model_->options.size()); ++i)
      r.take_child(std::make_unique<SelectOptionWidget>(model_, i));
  }
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    model_->tops.resize(ctx.child_count());
    model_->heights.resize(ctx.child_count());
    float y = 0, width = 0;
    for (size_t i = 0; i < ctx.child_count(); ++i) {
      const auto size =
          ctx.constrain_child(i, {c.min_width, c.max_width, 0,
                                  std::numeric_limits<float>::infinity()});
      model_->tops[i] = y;
      model_->heights[i] = size.height;
      ctx.place_child(i, {0, y});
      y += size.height;
      width = std::max(width, size.width);
    }
    return c.resolve(ctx.style.layout_size(), {width, y});
  }
  void draw(const DrawContext &, Painter &) override {}
  EventResult on_event(Event &) override { return EventResult::Unhandled; }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<SelectList>(model_);
  }

private:
  std::shared_ptr<SelectModel> model_;
};

class SelectPicker : public Scrollable {
public:
  explicit SelectPicker(std::shared_ptr<SelectModel> model)
      : Scrollable(std::make_unique<SelectList>(model)),
        model_(std::move(model)) {
    options_.open = false;
    options_.gap = 6;
    options_.max_width = std::numeric_limits<float>::max();
    options_.match_anchor_width = true;
    options_.constrain_to_anchor_side = true;
    options_.dismiss_on_escape = options_.dismiss_on_outside_press = true;
    options_.dismiss_on_focus_loss = true;
  }
  bool exposes_style_descendants() const override { return true; }
  const OverlayOptions *overlay_options() const override {
    options_.open = model_->open && !model_->disabled;
    return &options_;
  }
  Size<float> layout(Constraints c, LayoutContext &ctx) override {
    c.max_height = std::min(c.max_height, model_->picker_height);
    c.min_height = std::min(c.min_height, c.max_height);
    auto size = Scrollable::layout(c, ctx);
    if (model_->reveal && model_->enabled(model_->active) &&
        model_->active < static_cast<int>(model_->tops.size())) {
      const float top = model_->tops[model_->active];
      const float bottom = top + model_->heights[model_->active];
      float offset = scroll_offset().y;
      if (top < offset)
        offset = top;
      else if (bottom > offset + viewport_size().height)
        offset = bottom - viewport_size().height;
      scroll_to({0, std::max(0.0f, offset)});
      model_->reveal = false;
      size = Scrollable::layout(c, ctx);
    }
    return size;
  }
  EventResult on_event(Event &event) override {
    if (event.type() == EventType::OverlayDismissed) {
      model_->close(event);
      return EventResult::Handled;
    }
    const auto result = Scrollable::on_event(event);
    if (result == EventResult::Handled)
      return result;
    // The panel's padding and exhausted wheel must not toggle the trigger or
    // scroll the viewport containing it. Keyboard input bubbles to the select.
    if (event.type() == EventType::MouseClicked ||
        event.type() == EventType::MousePressed ||
        event.type() == EventType::MouseReleased ||
        event.type() == EventType::MouseScrolled)
      return EventResult::Handled;
    return EventResult::Unhandled;
  }
  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<SelectPicker>(model_);
  }

private:
  std::shared_ptr<SelectModel> model_;
  mutable OverlayOptions options_;
};
} // namespace detail

Select::Select(std::vector<SelectOption> options)
    : model_(std::make_shared<detail::SelectModel>()) {
  model_->options = std::move(options);
}
SelectorBuilder Select::option_selector() {
  return Selectors::of<detail::SelectOptionWidget>();
}
void Select::set_value_(std::string value) {
  model_->value = std::move(value);
  value_set_ = true;
}
void Select::set_placeholder_(std::string value) {
  model_->placeholder = std::move(value);
}
void Select::set_disabled_(bool value) {
  model_->disabled = value;
  if (value)
    model_->open = false;
}
void Select::set_picker_height_(float value) {
  model_->picker_height = std::max(0.0f, value);
}
const std::string &Select::value() const { return model_->value; }
bool Select::is_open() const { return model_->open; }
bool Select::focusable() const { return !model_->disabled; }
std::uint8_t Select::style_status() const {
  return (model_->open ? StatusBits::kOpen : 0) |
         (model_->disabled ? StatusBits::kDisabled : StatusBits::kEnabled);
}
void Select::focus_lost(Event &event) { model_->close(event); }
void Select::register_children(Registrar &r) {
  model_->on_change = on_change_;
  r.take_internal_child(std::make_unique<detail::SelectLabel>(model_, -1),
                        "label");
  r.take_internal_child(std::make_unique<detail::SelectGlyph>(model_, -1),
                        "picker-icon");
  r.take_internal_child(std::make_unique<detail::SelectPicker>(model_),
                        "picker");
}
Size<float> Select::layout(Constraints c, LayoutContext &ctx) {
  return detail::select_row_layout(
      c, ctx, ctx.style.get<Appearance>() != SelectAppearance::None);
}
void Select::draw(const DrawContext &ctx, Painter &p) {
  detail::draw_container_box(ctx, p);
}
EventResult Select::on_event(Event &event) {
  if (model_->disabled)
    return EventResult::Unhandled;
  auto open = [&] {
    if (model_->options.empty())
      return;
    model_->open = true;
    model_->search.clear();
    const int selected = model_->selected();
    model_->activate(model_->enabled(selected) ? selected : model_->next(-1, 1),
                     event);
    event.request_style();
    event.request_layout();
  };
  if (event.type() == EventType::MouseClicked) {
    if (static_cast<MouseClickedEvent &>(event).button() != MouseButton::Left)
      return EventResult::Unhandled;
    if (model_->open)
      model_->close(event);
    else
      open();
    return EventResult::Handled;
  }
  if (event.type() == EventType::MousePressed ||
      event.type() == EventType::MouseReleased)
    return detail::select_mouse_button(event) == MouseButton::Left
               ? EventResult::Handled
               : EventResult::Unhandled;
  if (event.type() != EventType::KeyPressed)
    return EventResult::Unhandled;
  const auto &key = static_cast<KeyPressedEvent &>(event);
  if (key.modifiers().control() || key.modifiers().gui())
    return EventResult::Unhandled;
  const auto code = key.keycode();
  if (code == Keycode::Escape) {
    model_->close(event);
    return EventResult::Handled;
  }
  if (code == Keycode::Return || code == Keycode::Space) {
    if (!model_->open)
      open();
    else
      model_->commit(model_->active, event);
    return EventResult::Handled;
  }
  if (code == Keycode::Up && key.modifiers().alt()) {
    model_->close(event);
    return EventResult::Handled;
  }
  if (code == Keycode::Down || code == Keycode::Up || code == Keycode::Home ||
      code == Keycode::End) {
    if (!model_->open) {
      open();
      if (code == Keycode::Down || code == Keycode::Up)
        return EventResult::Handled;
    }
    const int direction =
        (code == Keycode::Up || code == Keycode::End) ? -1 : 1;
    const int from = code == Keycode::Home ? -1
                     : code == Keycode::End
                         ? static_cast<int>(model_->options.size())
                         : model_->active;
    model_->activate(model_->next(from, direction), event);
    return EventResult::Handled;
  }
  // Keycodes provide layout-independent ASCII typeahead without enabling an IME
  // or showing a software keyboard for this non-editable control.
  const auto character = static_cast<unsigned>(code);
  if (!key.modifiers().alt() && character >= 33 && character <= 126) {
    const auto now = std::chrono::steady_clock::now();
    if (now - model_->search_at > std::chrono::milliseconds(700))
      model_->search.clear();
    model_->search_at = now;
    const char letter =
        static_cast<char>(std::tolower(static_cast<unsigned char>(character)));
    const bool cycle =
        model_->search.size() == 1 && model_->search[0] == letter;
    if (!cycle)
      model_->search += letter;
    const int start = model_->open ? model_->active : model_->selected();
    for (int step = cycle || model_->search.size() == 1 ? 1 : 0;
         step <= static_cast<int>(model_->options.size()); ++step) {
      if (model_->options.empty())
        break;
      const int index = (std::max(start, -1) + step +
                         static_cast<int>(model_->options.size())) %
                        static_cast<int>(model_->options.size());
      auto label = model_->options[index].label;
      std::transform(
          label.begin(), label.end(), label.begin(),
          [](unsigned char c) { return static_cast<char>(std::tolower(c)); });
      if (model_->enabled(index) && label.starts_with(model_->search)) {
        if (model_->open)
          model_->activate(index, event);
        else
          model_->commit(index, event);
        break;
      }
    }
    return EventResult::Handled;
  }
  return EventResult::Unhandled;
}
void Select::inherit_runtime(const Widget &previous) {
  const auto &old = static_cast<const Select &>(previous);
  if (!value_set_)
    model_->value = old.model_->value;
  if (!model_->disabled && model_->options == old.model_->options) {
    model_->open = old.model_->open;
    model_->active = old.model_->active;
    model_->reveal = model_->open;
  }
}
std::unique_ptr<Widget> Select::clone() const {
  auto copy = std::make_unique<Select>(model_->options);
  copy->model_->value = model_->value;
  copy->model_->placeholder = model_->placeholder;
  copy->model_->disabled = model_->disabled;
  copy->model_->picker_height = model_->picker_height;
  copy->on_change_ = on_change_;
  copy->value_set_ = value_set_;
  return copy;
}
std::shared_ptr<const StyleSheet> Select::default_stylesheet() const {
  static const auto sheet =
      StyleParser::parse(R"vss(
    select {
      width: 240px; padding: 10px 12px; background: white; color: #263244;
      font-size: 14px; border-width: 1px; border-color: #cbd5e1;
      border-radius: 8px; cursor: pointer; user-select: none;
    }
    select:hover { border-color: #94a3b8; background: #f8fafc; }
    select:focus, select:open { border-color: #2563eb; }
    select:focus { box-shadow: 0px 0px 0px 3px #2563eb20; }
    select:disabled { background: #f1f5f9; color: #94a3b8; border-color: #e2e8f0; cursor: not-allowed; }
    select::picker-icon { color: #64748b; }
    select:open::picker-icon { color: #2563eb; }
    select:disabled::picker-icon { color: #94a3b8; }
    select::picker(select) {
      padding: 4px; background: white; color: #263244;
      border-width: 1px; border-color: #d8e0eb; border-radius: 10px;
      box-shadow: 0px 6px 20px #00000024;
    }
    select option { width: fill; padding: 10px 12px; border-radius: 6px; cursor: pointer; }
    select option:checked { color: #1d4ed8; background: #eff6ff; }
    select option:focus, select option:hover { background: #e2e8f0; }
    select option:checked:focus, select option:checked:hover { background: #dbeafe; }
    select option:disabled { color: #94a3b8; background: transparent; cursor: not-allowed; }
  )vss",
                         "select.default.vss", StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}
} // namespace voidui
