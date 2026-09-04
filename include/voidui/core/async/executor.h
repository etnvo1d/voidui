#pragma once

#include <cstddef>
#include <functional>
#include <memory>

namespace voidui::async {

using Task = std::move_only_function<void()>;

namespace detail {
struct UiQueue;
class UiExecutorScope;
} // namespace detail

/// A copyable, thread-safe endpoint for one UI executor.
///
/// Dispatchers keep no Window or WidgetTree alive. Posting simply fails after
/// the owning executor has closed, which lets a late worker result disappear
/// without dereferencing UI state that has already been destroyed.
class UiDispatcher {
public:
  UiDispatcher() = default;

  [[nodiscard]] bool post(Task task) const;
  explicit operator bool() const noexcept { return !queue_.expired(); }

private:
  friend class UiExecutor;
  friend UiDispatcher current_ui_dispatcher();

  explicit UiDispatcher(const std::shared_ptr<detail::UiQueue> &queue)
      : queue_(queue) {}

  std::weak_ptr<detail::UiQueue> queue_;
};

/// The single hand-off point between worker threads and the UI thread.
///
/// `post` is an MPSC operation. `drain` swaps out one stable snapshot, so work
/// posted by a callback is intentionally deferred to the next event-loop turn.
/// The Window drains this executor after SDL events and before layout, which
/// keeps every tree mutation on the UI thread and outside layout/paint.
class UiExecutor {
public:
  UiExecutor();
  ~UiExecutor();

  UiExecutor(const UiExecutor &) = delete;
  UiExecutor &operator=(const UiExecutor &) = delete;

  [[nodiscard]] UiDispatcher dispatcher() const;
  void post(Task task);

  /// Current time in the epoch consumed by `post_at` and `next_wake_time`.
  [[nodiscard]] static double now();

  /// Schedules UI work against the same steady-clock epoch used by Window.
  /// This method is UI-thread-only; cross-thread callers should post a task
  /// which installs the timer instead.
  void post_at(double when_seconds, Task task);

  /// Executes at most one queue snapshot, stopping after `budget_seconds`.
  /// A non-positive budget means no time limit. The returned count is the
  /// number of callbacks that ran.
  std::size_t drain(double budget_seconds);

  /// True when immediate work exists or the earliest timer is already due.
  [[nodiscard]] bool pending() const;

  /// Earliest timer deadline, or positive infinity when no timer is armed.
  [[nodiscard]] double next_wake_time() const;

  /// Installs the platform wake primitive. The callback may run on any thread
  /// and must therefore be thread-safe (SDL_PushEvent satisfies this rule).
  void set_waker(std::function<void()> waker);

  [[nodiscard]] bool on_ui_thread() const noexcept;

private:
  friend class detail::UiExecutorScope;
  std::shared_ptr<detail::UiQueue> queue_;
};

/// Returns the dispatcher bound to the currently running Window.
/// The returned endpoint is safe to retain and pass to arbitrary threads.
[[nodiscard]] UiDispatcher current_ui_dispatcher();

/// Convenience form for producers associated with the active Window.
/// Returns false when no Window is running or its executor is already closed.
[[nodiscard]] bool post_to_ui(Task task);

namespace detail {

/// Binds one executor as the process's active UI destination for the lifetime
/// of a Window::run call. VoidUI currently has one blocking event loop, so one
/// active destination also makes the no-argument `post_to_ui` API unambiguous.
class UiExecutorScope {
public:
  explicit UiExecutorScope(UiExecutor &executor);
  ~UiExecutorScope();

  UiExecutorScope(const UiExecutorScope &) = delete;
  UiExecutorScope &operator=(const UiExecutorScope &) = delete;

private:
  std::shared_ptr<UiQueue> previous_;
  std::shared_ptr<UiQueue> current_;
};

} // namespace detail
} // namespace voidui::async
