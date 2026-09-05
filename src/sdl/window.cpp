#include <cstdio>

#include "voidui/core/async/executor.h"
#include "voidui/core/async/thread_pool.h"
#include "voidui/core/window.h"

#include "render/gpu/renderer.h"
#include "rhi/device.h"

#include <SDL3/SDL.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <cstdint>
#include <limits>
#include <memory>
#include <optional>

namespace voidui {

namespace {

constexpr size_t cursor_shape_count = static_cast<size_t>(CursorShape::Count);

SDL_SystemCursor convert_cursor_shape(CursorShape shape) {
  switch (shape) {
  case CursorShape::Auto:
  case CursorShape::Default:
  case CursorShape::None:
  case CursorShape::Count:
    return SDL_SYSTEM_CURSOR_DEFAULT;
  case CursorShape::Pointer:
    return SDL_SYSTEM_CURSOR_POINTER;
  case CursorShape::Text:
    return SDL_SYSTEM_CURSOR_TEXT;
  case CursorShape::Wait:
    return SDL_SYSTEM_CURSOR_WAIT;
  case CursorShape::Progress:
    return SDL_SYSTEM_CURSOR_PROGRESS;
  case CursorShape::Crosshair:
    return SDL_SYSTEM_CURSOR_CROSSHAIR;
  case CursorShape::Move:
    return SDL_SYSTEM_CURSOR_MOVE;
  case CursorShape::NotAllowed:
    return SDL_SYSTEM_CURSOR_NOT_ALLOWED;
  case CursorShape::HorizontalResize:
    return SDL_SYSTEM_CURSOR_EW_RESIZE;
  case CursorShape::VerticalResize:
    return SDL_SYSTEM_CURSOR_NS_RESIZE;
  case CursorShape::NorthwestSoutheastResize:
    return SDL_SYSTEM_CURSOR_NWSE_RESIZE;
  case CursorShape::NortheastSouthwestResize:
    return SDL_SYSTEM_CURSOR_NESW_RESIZE;
  case CursorShape::NorthwestResize:
    return SDL_SYSTEM_CURSOR_NW_RESIZE;
  case CursorShape::NorthResize:
    return SDL_SYSTEM_CURSOR_N_RESIZE;
  case CursorShape::NortheastResize:
    return SDL_SYSTEM_CURSOR_NE_RESIZE;
  case CursorShape::EastResize:
    return SDL_SYSTEM_CURSOR_E_RESIZE;
  case CursorShape::SoutheastResize:
    return SDL_SYSTEM_CURSOR_SE_RESIZE;
  case CursorShape::SouthResize:
    return SDL_SYSTEM_CURSOR_S_RESIZE;
  case CursorShape::SouthwestResize:
    return SDL_SYSTEM_CURSOR_SW_RESIZE;
  case CursorShape::WestResize:
    return SDL_SYSTEM_CURSOR_W_RESIZE;
  }

  return SDL_SYSTEM_CURSOR_DEFAULT;
}

class SystemCursorCache {
public:
  ~SystemCursorCache() {
    if (current_shape_ && *current_shape_ == CursorShape::None)
      SDL_ShowCursor();
    if (SDL_Cursor *default_cursor = SDL_GetDefaultCursor())
      SDL_SetCursor(default_cursor);

    for (SDL_Cursor *cursor : cursors_) {
      if (cursor)
        SDL_DestroyCursor(cursor);
    }
  }

  void set(CursorShape shape) {
    if (current_shape_ && *current_shape_ == shape)
      return;

    if (shape == CursorShape::None) {
      if (!SDL_HideCursor())
        SDL_Log("Failed to hide system cursor: %s", SDL_GetError());
      else
        current_shape_ = shape;
      return;
    }

    if (current_shape_ && *current_shape_ == CursorShape::None &&
        !SDL_ShowCursor()) {
      SDL_Log("Failed to show system cursor: %s", SDL_GetError());
      return;
    }

    SDL_Cursor *cursor = get_(shape);
    if (!cursor) {
      SDL_Log("Failed to create system cursor: %s", SDL_GetError());
      return;
    }

    if (!SDL_SetCursor(cursor)) {
      SDL_Log("Failed to set system cursor: %s", SDL_GetError());
      return;
    }

    current_shape_ = shape;
  }

private:
  SDL_Cursor *get_(CursorShape shape) {
    if (shape == CursorShape::Auto || shape == CursorShape::Default)
      return SDL_GetDefaultCursor();

    const size_t index = static_cast<size_t>(shape);
    SDL_Cursor *&cursor = cursors_[index];

    if (!cursor)
      cursor = SDL_CreateSystemCursor(convert_cursor_shape(shape));

    return cursor;
  }

private:
  std::array<SDL_Cursor *, cursor_shape_count> cursors_{};
  std::optional<CursorShape> current_shape_;
};

std::optional<MouseButton> convert_mouse_button(Uint8 button) {
  switch (button) {
  case SDL_BUTTON_LEFT:
    return MouseButton::Left;
  case SDL_BUTTON_MIDDLE:
    return MouseButton::Middle;
  case SDL_BUTTON_RIGHT:
    return MouseButton::Right;
  case SDL_BUTTON_X1:
    return MouseButton::Side1;
  case SDL_BUTTON_X2:
    return MouseButton::Side2;
  default:
    return std::nullopt;
  }
}

KeyModifiers convert_key_modifiers(SDL_Keymod modifiers) {
  KeyModifiers result;
  if ((modifiers & SDL_KMOD_SHIFT) != 0)
    result.bits |= KeyModifiers::Shift;
  if ((modifiers & SDL_KMOD_CTRL) != 0)
    result.bits |= KeyModifiers::Control;
  if ((modifiers & SDL_KMOD_ALT) != 0)
    result.bits |= KeyModifiers::Alt;
  if ((modifiers & SDL_KMOD_GUI) != 0)
    result.bits |= KeyModifiers::Gui;
  return result;
}

Point<float> window_pos_to_logical(const WindowMetrics &metrics, float x,
                                   float y) {
  return {x * metrics.window_to_logical_x, y * metrics.window_to_logical_y};
}

std::unique_ptr<Event> convert_sdl_event(const SDL_Event &event,
                                         const WindowMetrics &metrics) {
  switch (event.type) {
  case SDL_EVENT_QUIT:
  case SDL_EVENT_WINDOW_CLOSE_REQUESTED:
    return std::make_unique<WindowClosedEvent>();

  case SDL_EVENT_WINDOW_RESIZED:
    return std::make_unique<WindowResizedEvent>(event.window.data1,
                                                event.window.data2);

  case SDL_EVENT_KEY_DOWN:
    return std::make_unique<KeyPressedEvent>(
        static_cast<Keycode>(event.key.key),
        convert_key_modifiers(event.key.mod));

  case SDL_EVENT_KEY_UP:
    return std::make_unique<KeyReleasedEvent>(
        static_cast<Keycode>(event.key.key),
        convert_key_modifiers(event.key.mod));

  case SDL_EVENT_TEXT_INPUT:
    return std::make_unique<TextInputEvent>(event.text.text);

  case SDL_EVENT_TEXT_EDITING:
    return std::make_unique<TextEditingEvent>(event.edit.text, event.edit.start,
                                              event.edit.length);

  case SDL_EVENT_MOUSE_MOTION:
    return std::make_unique<MouseMovedEvent>(
        window_pos_to_logical(metrics, event.motion.x, event.motion.y));

  case SDL_EVENT_WINDOW_MOUSE_LEAVE:
    return std::make_unique<MouseLeftEvent>();

  case SDL_EVENT_WINDOW_FOCUS_LOST:
    return std::make_unique<WindowFocusLostEvent>();

  case SDL_EVENT_MOUSE_BUTTON_DOWN: {
    const auto button = convert_mouse_button(event.button.button);
    if (!button)
      return nullptr;

    return std::make_unique<MousePressedEvent>(
        *button, window_pos_to_logical(metrics, event.button.x, event.button.y),
        event.button.clicks);
  }

  case SDL_EVENT_MOUSE_BUTTON_UP: {
    const auto button = convert_mouse_button(event.button.button);
    if (!button)
      return nullptr;

    return std::make_unique<MouseReleasedEvent>(
        *button, window_pos_to_logical(metrics, event.button.x, event.button.y),
        event.button.clicks);
  }

  case SDL_EVENT_MOUSE_WHEEL: {
    const float direction =
        event.wheel.direction == SDL_MOUSEWHEEL_FLIPPED ? -1.0f : 1.0f;

    return std::make_unique<MouseScrolledEvent>(
        event.wheel.x * direction, event.wheel.y * direction,
        window_pos_to_logical(metrics, event.wheel.mouse_x,
                              event.wheel.mouse_y));
  }

  default:
    return nullptr;
  }
}

void update_metrics(SDL_Window *window, WindowMetrics &m) {
  SDL_GetWindowSize(window, &m.window_width, &m.window_height);
  SDL_GetWindowSizeInPixels(window, &m.pixel_width, &m.pixel_height);

  m.display_scale = SDL_GetWindowDisplayScale(window);
  if (m.display_scale <= 0.0f)
    m.display_scale = 1.0f;

  m.logical_width = static_cast<float>(m.pixel_width) / m.display_scale;
  m.logical_height = static_cast<float>(m.pixel_height) / m.display_scale;

  m.window_to_logical_x =
      m.window_width == 0
          ? 1.0f
          : m.logical_width / static_cast<float>(m.window_width);
  m.window_to_logical_y =
      m.window_height == 0
          ? 1.0f
          : m.logical_height / static_cast<float>(m.window_height);
}

bool sync_surface(SDL_Window *window, WidgetTree &tree, GpuRenderer &renderer,
                  WindowMetrics &metrics) {
  WindowMetrics next{};
  update_metrics(window, next);
  const bool changed = next.window_width != metrics.window_width ||
                       next.window_height != metrics.window_height ||
                       next.pixel_width != metrics.pixel_width ||
                       next.pixel_height != metrics.pixel_height ||
                       next.display_scale != metrics.display_scale;
  metrics = next;
  if (!changed)
    return false;

  renderer.resize(metrics.pixel_width, metrics.pixel_height,
                  metrics.logical_width, metrics.logical_height,
                  metrics.display_scale);
  tree.set_device_scale(metrics.display_scale);
  tree.request_layout();
  return true;
}

/// Seconds on the monotonic clock. The animator, the caret, an executor timer
/// and the window's idle wait all read this one, so a deadline armed while
/// painting is directly comparable to the moment the scheduler decides how long
/// to sleep. It forwards to the executor's clock rather than sampling its own:
/// two definitions of the same epoch would compare fine right up until one of
/// them changed.
double monotonic_seconds() { return async::UiExecutor::now(); }

void render_frame(WidgetTree &tree, GpuRenderer &renderer,
                  const WindowMetrics &metrics, DisplayList &display_list,
                  Color clear_color, bool force_present = false) {
  tree.advance_animations(monotonic_seconds());
  if (tree.needs_layout())
    tree.layout({metrics.logical_width, metrics.logical_height});

  bool display_list_changed = false;
  if (tree.needs_paint()) {
    display_list.clear();
    Painter painter(display_list,
                    Size<float>(metrics.logical_width, metrics.logical_height));
    tree.render(painter);
    display_list_changed = true;
  }

  if (force_present || display_list_changed)
    renderer.render(display_list, clear_color);
}

struct NativeTextInputArea {
  SDL_Rect rect{};
  int cursor = 0;

  bool operator==(const NativeTextInputArea &other) const {
    return rect.x == other.rect.x && rect.y == other.rect.y &&
           rect.w == other.rect.w && rect.h == other.rect.h &&
           cursor == other.cursor;
  }
};

std::optional<NativeTextInputArea>
native_text_input_area(const WindowMetrics &metrics,
                       const TextInputArea &logical) {
  if (!(metrics.window_to_logical_x > 0.0f) ||
      !(metrics.window_to_logical_y > 0.0f) ||
      logical.rect.size.width <= 0.0f || logical.rect.size.height <= 0.0f)
    return std::nullopt;

  // SDL_SetTextInputArea takes window coordinates, while VoidUI layout uses
  // renderer logical coordinates. They differ on high-DPI surfaces and can
  // also differ slightly per axis after the native window is resized.
  const float left = logical.rect.origin.x / metrics.window_to_logical_x;
  const float top = logical.rect.origin.y / metrics.window_to_logical_y;
  const float right = (logical.rect.origin.x + logical.rect.size.width) /
                      metrics.window_to_logical_x;
  const float bottom = (logical.rect.origin.y + logical.rect.size.height) /
                       metrics.window_to_logical_y;
  const int native_left = static_cast<int>(std::floor(left));
  const int native_top = static_cast<int>(std::floor(top));
  const int native_right = static_cast<int>(std::ceil(right));
  const int native_bottom = static_cast<int>(std::ceil(bottom));
  const float caret_x =
      (logical.rect.origin.x + logical.cursor) / metrics.window_to_logical_x;

  NativeTextInputArea result{
      {native_left, native_top, std::max(native_right - native_left, 1),
       std::max(native_bottom - native_top, 1)},
      static_cast<int>(std::lround(caret_x)) - native_left};
  result.cursor = std::clamp(result.cursor, 0, result.rect.w);
  return result;
}

class TextInputController {
public:
  explicit TextInputController(SDL_Window *window) : window_(window) {}

  bool active() const { return active_; }

  void update(const WidgetTree &tree, const WindowMetrics &metrics) {
    const bool wanted = tree.wants_text_input();
    const Node *next_client = tree.text_input_client();
    if (active_ && next_client && client_ != next_client &&
        !SDL_ClearComposition(window_))
      SDL_Log("Failed to clear IME composition: %s", SDL_GetError());

    std::optional<NativeTextInputArea> next;
    if (wanted) {
      if (const std::optional<TextInputArea> logical = tree.text_input_area())
        next = native_text_input_area(metrics, *logical);
    }

    if (next != area_) {
      const bool updated =
          next ? SDL_SetTextInputArea(window_, &next->rect, next->cursor)
               : SDL_SetTextInputArea(window_, nullptr, 0);
      if (!updated) {
        SDL_Log("Failed to update IME position: %s", SDL_GetError());
      } else {
        area_ = next;
      }
    }

    if (wanted == active_) {
      client_ = next_client;
      return;
    }
    const bool changed =
        wanted ? SDL_StartTextInput(window_) : SDL_StopTextInput(window_);
    if (!changed) {
      SDL_Log("Failed to %s text input: %s", wanted ? "start" : "stop",
              SDL_GetError());
      return;
    }
    active_ = wanted;
    client_ = next_client;
  }

  void shutdown() {
    if (active_ && !SDL_StopTextInput(window_))
      SDL_Log("Failed to stop text input: %s", SDL_GetError());
    active_ = false;
    if (area_ && !SDL_SetTextInputArea(window_, nullptr, 0))
      SDL_Log("Failed to clear IME position: %s", SDL_GetError());
    area_.reset();
    client_ = nullptr;
  }

private:
  SDL_Window *window_ = nullptr;
  std::optional<NativeTextInputArea> area_;
  const Node *client_ = nullptr;
  bool active_ = false;
};

#if defined(_WIN32)
struct LiveResizeContext {
  SDL_Window *window = nullptr;
  WidgetTree *tree = nullptr;
  GpuRenderer *renderer = nullptr;
  WindowMetrics *metrics = nullptr;
  DisplayList *display_list = nullptr;
  Color *clear_color = nullptr;
  TextInputController *text_input = nullptr;
  bool rendering = false;
};

bool SDLCALL redraw_live_resize(void *userdata, SDL_Event *event) {
  auto &context = *static_cast<LiveResizeContext *>(userdata);
  if (event->type != SDL_EVENT_WINDOW_EXPOSED || context.rendering ||
      event->window.windowID != SDL_GetWindowID(context.window))
    return true;

  context.rendering = true;
  sync_surface(context.window, *context.tree, *context.renderer,
               *context.metrics);
  render_frame(*context.tree, *context.renderer, *context.metrics,
               *context.display_list, *context.clear_color,
               /*force_present=*/true);
  context.text_input->update(*context.tree, *context.metrics);
  context.rendering = false;
  return true;
}
#endif

/// We need to create a hidden window first , then query the display scale and
/// logical size, and then resize the window to the desired logical size. This
/// is because SDL does not provide a way to create a window with a specific
/// logical size directly.
SDL_Window *create_hidden_hidpi_window(const char *title, int width,
                                       int height) {
  SDL_WindowFlags flags =
      SDL_WINDOW_RESIZABLE | SDL_WINDOW_HIGH_PIXEL_DENSITY | SDL_WINDOW_HIDDEN;
#if defined(__APPLE__)
  flags |= SDL_WINDOW_METAL;
#elif defined(__linux__)
  flags |= SDL_WINDOW_VULKAN;
#endif
  SDL_Window *window = SDL_CreateWindow(title, width, height, flags);

  WindowMetrics metrics{};
  update_metrics(window, metrics);

  const int target_window_width = static_cast<int>(
      std::lround(static_cast<float>(width) / metrics.window_to_logical_x));

  const int target_window_height = static_cast<int>(
      std::lround(static_cast<float>(height) / metrics.window_to_logical_y));

  SDL_SetWindowSize(window, target_window_width, target_window_height);
  SDL_SyncWindow(window);

  return window;
}

} // namespace

Window::Window(std::string title, int width, int height)
    : title_(std::move(title)), width_(width), height_(height) {
  watcher_.on_diagnostics([](const std::vector<StyleDiagnostic> &diagnostics) {
    for (const StyleDiagnostic &diagnostic : diagnostics)
      std::fprintf(stderr, "voidui: %s\n", diagnostic.to_string().c_str());
  });
}

void Window::watch_styles(const std::string &stylesheet_path,
                          const std::string &theme_path) {
  if (!stylesheet_path.empty())
    watcher_.watch_stylesheet(stylesheet_path);
  if (!theme_path.empty())
    watcher_.watch_theme(theme_path);
  watching_ = watcher_.enabled();

  watcher_.on_stylesheet([this](std::shared_ptr<const StyleSheet> sheet) {
    if (sheet)
      register_font_faces(*sheet);
    sheet_ = std::move(sheet);
  });
  watcher_.on_theme([this](std::shared_ptr<const Theme> theme) {
    theme_ = std::move(theme);
  });
  if (!watcher_.enabled()) {
    // Hot reload compiled out: still perform the one initial read, through the
    // same code path, so that debug and release load identically.
    StyleParser::Result parsed = StyleParser::parse_file(stylesheet_path);
    if (parsed.sheet && parsed.sheet->size() > 0)
      sheet_ = std::move(parsed.sheet);
    if (!theme_path.empty())
      theme_ = Theme::from_file(theme_path);
  } else {
    watcher_.reload_all();
  }
}

void Window::run(std::unique_ptr<Widget> root) {
  // VoidUI draws IME composition text with the focused control's FontStack.
  // Keep the candidate list native; SDL therefore suppresses only the legacy
  // platform composition window and continues positioning OS candidates from
  // SDL_SetTextInputArea below. SDL requires this hint before initialization.
  SDL_SetHint(SDL_HINT_IME_IMPLEMENTED_UI, "composition");
  SDL_Init(SDL_INIT_VIDEO);

  SDL_Window *window =
      create_hidden_hidpi_window(title_.c_str(), width_, height_);

  std::unique_ptr<rhi::Device> device = rhi::Device::create(window);
  if (!device) {
    SDL_DestroyWindow(window);
    SDL_Quit();
    return;
  }

  std::unique_ptr<GpuRenderer> renderer = GpuRenderer::create(*device);
  if (!renderer) {
    device.reset();
    SDL_DestroyWindow(window);
    SDL_Quit();
    return;
  }

  {
    async::UiExecutor executor;
    async::detail::UiExecutorScope executor_scope(executor);
    const Uint32 async_wake_event = SDL_RegisterEvents(1);
    if (async_wake_event == 0) {
      renderer.reset();
      device.reset();
      SDL_DestroyWindow(window);
      SDL_Quit();
      return;
    }
    executor.set_waker([async_wake_event] {
      SDL_Event wake{};
      wake.type = async_wake_event;
      SDL_PushEvent(&wake);
    });

    // Declared after the executor, and therefore destroyed before it: a
    // component's async slots cancel themselves as the tree comes down, while
    // the queue they may post to is still standing.
    WidgetTree widget_tree;
    if (sheet_)
      widget_tree.style_resolver().set_stylesheet(sheet_);
    if (theme_)
      widget_tree.style_resolver().set_theme(theme_);
    // Build only after the executor is bound. A use_async hook may submit its
    // first job during this render, and even an immediate completion must enter
    // through the event-loop hand-off rather than mutate the tree recursively.
    widget_tree.build(std::move(root));

    WindowMetrics metrics{};

    DisplayList display_list;

    // The window's own surface is themed too: `$app.background` in the theme
    // decides it. Resolved once here and again whenever the theme changes,
    // never per frame.
    const Color default_clear(255, 255, 255);
    Color clear_color =
        theme_ ? theme_->background(default_clear) : default_clear;
    TextInputController text_input(window);

#if defined(_WIN32)
    LiveResizeContext live_resize{window,     &widget_tree,  &*renderer,
                                  &metrics,   &display_list, &clear_color,
                                  &text_input};

    // Build and present once while hidden so the first compositor frame already
    // contains the UI instead of an uninitialized swapchain image.
    sync_surface(window, widget_tree, *renderer, metrics);
    render_frame(widget_tree, *renderer, metrics, display_list, clear_color,
                 /*force_present=*/true);
#endif

    SDL_ShowWindow(window);

#if !defined(_WIN32)
    sync_surface(window, widget_tree, *renderer, metrics);
    render_frame(widget_tree, *renderer, metrics, display_list, clear_color,
                 /*force_present=*/true);
#endif

#if defined(_WIN32)
    SDL_AddEventWatch(redraw_live_resize, &live_resize);
#endif

    {
      SystemCursorCache cursors;
      cursors.set(widget_tree.get_current_cursor_shape());

      bool running = true;
      bool present_requested = false;
      while (running) {
        if (watching_) {
          // Idle windows wake only at the watcher interval. A successful reload
          // re-resolves the tree; a broken file leaves the current style
          // intact.
          const std::shared_ptr<const StyleSheet> previous_sheet = sheet_;
          const std::shared_ptr<const Theme> previous_theme = theme_;
          if (watcher_.poll()) {
            if (sheet_ != previous_sheet)
              widget_tree.set_stylesheet(sheet_);
            if (theme_ != previous_theme) {
              widget_tree.set_theme(theme_);
              clear_color =
                  theme_ ? theme_->background(default_clear) : default_clear;
              present_requested = true;
            }
          }
        }

        SDL_Event e;
        bool has_event = false;
        if (widget_tree.needs_paint() || present_requested ||
            executor.pending()) {
          has_event = SDL_PollEvent(&e);
        } else {
          // An idle window sleeps until something wakes it: hot reload, a
          // widget paint deadline, or an executor timer. The wait is the
          // earliest of these, and with none the window blocks indefinitely at
          // zero CPU.
          std::int64_t wait = std::numeric_limits<std::int64_t>::max();
          if (watching_)
            wait = std::max<std::int64_t>(watcher_.poll_interval().count(), 1);
          const double wake =
              std::min(widget_tree.next_wake_time(), executor.next_wake_time());
          if (wake < std::numeric_limits<double>::infinity()) {
            const double remaining = (wake - monotonic_seconds()) * 1000.0;
            wait = std::min<std::int64_t>(
                wait,
                remaining > 1.0 ? static_cast<std::int64_t>(remaining) : 1);
          }
          if (wait == std::numeric_limits<std::int64_t>::max()) {
            has_event = SDL_WaitEvent(&e);
          } else {
            has_event = SDL_WaitEventTimeout(
                &e, static_cast<Sint32>(std::clamp<std::int64_t>(
                        wait, 1, std::numeric_limits<Sint32>::max())));
          }
        }

        while (has_event) {
          switch (e.type) {
          case SDL_EVENT_WINDOW_RESIZED:
          case SDL_EVENT_WINDOW_PIXEL_SIZE_CHANGED:
          case SDL_EVENT_WINDOW_DISPLAY_SCALE_CHANGED:
          case SDL_EVENT_WINDOW_DISPLAY_CHANGED:
            sync_surface(window, widget_tree, *renderer, metrics);
            break;
          case SDL_EVENT_WINDOW_EXPOSED:
            present_requested = true;
            break;
          }

          std::unique_ptr<Event> event = convert_sdl_event(e, metrics);
          if (event) {
            widget_tree.advance_animations(monotonic_seconds());
            if (event->type() == EventType::WindowClosed)
              running = false;
            widget_tree.process_event(*event);
            // Focus starts and stops the platform text service immediately.
            // A later update after rendering refreshes geometry that this event
            // may have changed through layout, scrolling or caret movement.
            if (widget_tree.wants_text_input() != text_input.active())
              text_input.update(widget_tree, metrics);
          }

          has_event = SDL_PollEvent(&e);
        }

        if (!running)
          break;

        // This is the only worker-to-UI delivery point: SDL input has
        // completed, while component reconciliation, layout, and painting have
        // not begun. The budget bounds burst latency; executor.pending() keeps
        // subsequent snapshots moving without turning an otherwise idle window
        // into a poll.
        executor.drain(0.004);

        // A deadline that has come due is a reason to draw even with nothing
        // else pending: `advance_animations` inside the frame turns it into a
        // paint request, and the widget that owns it arms the next one there.
        if (widget_tree.needs_paint() || present_requested ||
            monotonic_seconds() >= widget_tree.next_wake_time()) {
          render_frame(widget_tree, *renderer, metrics, display_list,
                       clear_color, present_requested);
          present_requested = false;
        }
        text_input.update(widget_tree, metrics);
        cursors.set(widget_tree.get_current_cursor_shape());
      }
    }

#if defined(_WIN32)
    SDL_RemoveEventWatch(redraw_live_resize, &live_resize);
#endif

    text_input.shutdown();

    // Workers may own values whose destruction depends on renderer-backed
    // resources. Joining them before renderer/device teardown makes that order
    // explicit, while cooperative tokens allow long jobs to return promptly.
    async::ThreadPool::shutdown_shared();
    executor.set_waker({});
  }

  renderer.reset();
  device.reset();

  SDL_DestroyWindow(window);
  SDL_Quit();
}

} // namespace voidui
