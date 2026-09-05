#pragma once

#include <functional>
#include <string>

#include "voidui/core/geometry.h"
#include "voidui/core/invalidation.h"
#include "voidui/core/keycode.h"
#include "voidui/core/overlay.h"

namespace voidui {

enum class EventType {
  KeyPressed,
  KeyReleased,
  TextEditing,
  TextInput,
  MousePressed,
  MouseReleased,
  MouseClicked,
  MouseMoved,
  MouseLeft,
  MouseScrolled,
  WindowResized,
  WindowClosed,
  WindowFocusLost,
  FocusLost,
  OverlayDismissed,
};

#define VOIDUI_EVENT_TYPE_NAME(type_)                                          \
  static EventType static_type() { return EventType::type_; }                  \
  virtual EventType type() const override { return EventType::type_; }         \
  virtual const char *name() const override { return #type_; }

enum class EventResult {
  Handled,
  Unhandled,
};

struct KeyModifiers {
  static constexpr std::uint8_t Shift = 1u << 0;
  static constexpr std::uint8_t Control = 1u << 1;
  static constexpr std::uint8_t Alt = 1u << 2;
  static constexpr std::uint8_t Gui = 1u << 3;

  std::uint8_t bits = 0;

  bool shift() const { return (bits & Shift) != 0; }
  bool control() const { return (bits & Control) != 0; }
  bool alt() const { return (bits & Alt) != 0; }
  bool gui() const { return (bits & Gui) != 0; }
  bool primary() const { return control() || gui(); }
};

class Event;
template <typename T>
concept EventClass = std::derived_from<std::remove_cvref_t<T>, Event>;

class Event {
public:
  virtual ~Event() = default;

  virtual EventType type() const = 0;
  virtual const char *name() const = 0;
  virtual std::string to_string() const { return name(); }

  /// Widgets request only the work their state mutation requires. Requests
  /// accumulate while the event bubbles, so a layout request absorbs paint.
  void request_paint() {
    invalidation_ = max_invalidation(invalidation_, Invalidation::Paint);
  }
  void request_layout() { invalidation_ = Invalidation::Layout; }
  void request_style() { style_requested_ = true; request_paint(); }
  bool style_requested() const { return style_requested_; }
  Invalidation invalidation() const { return invalidation_; }

  template <EventClass T, typename F> EventResult dispatch(F &&dispatch_fn) {
    using U = std::remove_cvref_t<T>;

    if (auto *e = dynamic_cast<U *>(this)) {
      return std::invoke(std::forward<F>(dispatch_fn), *e);
    }

    return EventResult::Unhandled;
  }

private:
  Invalidation invalidation_ = Invalidation::None;
  bool style_requested_ = false;
};

/// Sent directly to the dismissed portal, after its runtime state is closed.
class OverlayDismissedEvent : public Event {
public:
  explicit OverlayDismissedEvent(OverlayDismissReason reason) : reason_(reason) {}
  OverlayDismissReason reason() const { return reason_; }
  VOIDUI_EVENT_TYPE_NAME(OverlayDismissed)
private:
  OverlayDismissReason reason_;
};

class FocusLostEvent : public Event {
public:
  VOIDUI_EVENT_TYPE_NAME(FocusLost)
};

class KeyEvent : public Event {
public:
  Keycode keycode() const { return keycode_; }
  KeyModifiers modifiers() const { return modifiers_; }

protected:
  KeyEvent(Keycode keycode, KeyModifiers modifiers)
      : keycode_(keycode), modifiers_(modifiers) {}

private:
  Keycode keycode_;
  KeyModifiers modifiers_;
};

class KeyPressedEvent : public KeyEvent {
public:
  explicit KeyPressedEvent(Keycode keycode, KeyModifiers modifiers = {})
      : KeyEvent(keycode, modifiers) {}

  VOIDUI_EVENT_TYPE_NAME(KeyPressed)
};

class KeyReleasedEvent : public KeyEvent {
public:
  explicit KeyReleasedEvent(Keycode keycode, KeyModifiers modifiers = {})
      : KeyEvent(keycode, modifiers) {}

  VOIDUI_EVENT_TYPE_NAME(KeyReleased)
};

class TextInputEvent : public Event {
public:
  explicit TextInputEvent(std::string text) : text_(std::move(text)) {}

  const std::string &text() const { return text_; }

  VOIDUI_EVENT_TYPE_NAME(TextInput)

private:
  std::string text_;
};

/// Transient IME composition text. `start` and `length` are measured in UTF-8
/// codepoints, matching SDL's editing-event contract rather than byte offsets.
class TextEditingEvent : public Event {
public:
  TextEditingEvent(std::string text, std::int32_t start, std::int32_t length)
      : text_(std::move(text)), start_(start), length_(length) {}

  const std::string &text() const { return text_; }
  std::int32_t start() const { return start_; }
  std::int32_t length() const { return length_; }

  VOIDUI_EVENT_TYPE_NAME(TextEditing)

private:
  std::string text_;
  std::int32_t start_ = 0;
  std::int32_t length_ = 0;
};

class MouseEvent : public Event {
public:
  Point<float> get_pos() const { return pos_; }

protected:
  MouseEvent(Point<float> pos) : pos_(pos) {}

private:
  Point<float> pos_;
};

class MousePressedEvent : public MouseEvent {
public:
  MousePressedEvent(MouseButton button, Point<float> pos,
                    std::uint8_t click_count = 1)
      : MouseEvent(pos), button_(button), click_count_(click_count) {}

  MouseButton button() const { return button_; }
  std::uint8_t click_count() const { return click_count_; }

  VOIDUI_EVENT_TYPE_NAME(MousePressed)

private:
  MouseButton button_;
  std::uint8_t click_count_ = 1;
};

class MouseReleasedEvent : public MouseEvent {
public:
  MouseReleasedEvent(MouseButton button, Point<float> pos,
                     std::uint8_t click_count = 1)
      : MouseEvent(pos), button_(button), click_count_(click_count) {}

  MouseButton button() const { return button_; }
  std::uint8_t click_count() const { return click_count_; }

  VOIDUI_EVENT_TYPE_NAME(MouseReleased)

private:
  MouseButton button_;
  std::uint8_t click_count_ = 1;
};

class MouseClickedEvent : public MouseEvent {
public:
  MouseClickedEvent(MouseButton button, Point<float> pos)
      : MouseEvent(pos), button_(button) {}

  MouseButton button() const { return button_; }

  VOIDUI_EVENT_TYPE_NAME(MouseClicked)

private:
  MouseButton button_;
};

class MouseMovedEvent : public MouseEvent {
public:
  MouseMovedEvent(Point<float> pos) : MouseEvent(pos) {}
  VOIDUI_EVENT_TYPE_NAME(MouseMoved)
};

class MouseLeftEvent : public Event {
public:
  VOIDUI_EVENT_TYPE_NAME(MouseLeft)
};

class WindowFocusLostEvent : public Event {
public:
  VOIDUI_EVENT_TYPE_NAME(WindowFocusLost)
};

class MouseScrolledEvent : public MouseEvent {
public:
  MouseScrolledEvent(float x_offset, float y_offset, Point<float> pos)
      : MouseEvent(pos), x_offset_(x_offset), y_offset_(y_offset) {}

  float x_offset() const { return x_offset_; }
  float y_offset() const { return y_offset_; }

  VOIDUI_EVENT_TYPE_NAME(MouseScrolled)

private:
  float x_offset_, y_offset_;
};

class WindowResizedEvent : public Event {
public:
  WindowResizedEvent(int width, int height) : width_(width), height_(height) {}

  int width() const { return width_; }
  int height() const { return height_; }

  VOIDUI_EVENT_TYPE_NAME(WindowResized)

private:
  int width_, height_;
};

class WindowClosedEvent : public Event {
public:
  WindowClosedEvent() = default;
  VOIDUI_EVENT_TYPE_NAME(WindowClosed)
};

#undef VOIDUI_EVENT_TYPE_NAME

} // namespace voidui
