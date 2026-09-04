#pragma once

#include <atomic>
#include <cassert>
#include <concepts>
#include <exception>
#include <expected>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <type_traits>
#include <utility>

#include "voidui/core/async/executor.h"
#include "voidui/core/async/thread_pool.h"
#include "voidui/core/state.h"

namespace voidui::async {

/// The value carried to the UI thread when background work throws. Exceptions
/// themselves never cross the thread boundary; the worker converts them at the
/// source into this ordinary value type.
struct Error {
  std::string message;
};

template <class T> using Result = std::expected<T, Error>;

namespace detail {

template <class Work>
decltype(auto) invoke_work(Work &work, CancelToken token) {
  if constexpr (std::invocable<Work &, CancelToken>)
    return std::invoke(work, token);
  else
    return std::invoke(work);
}

template <class Work>
using run_result_t =
    decltype(invoke_work(std::declval<Work &>(), std::declval<CancelToken>()));

/// Supports both dependency-style hooks, whose closure captures everything
/// and takes no key, and resource-style hooks, which receive the key directly.
/// A cancellable overload is preferred whenever the callable provides one.
template <class Work, class Key>
decltype(auto) invoke_keyed_work(Work &work, CancelToken token,
                                 const Key &key) {
  if constexpr (std::invocable<Work &, CancelToken, const Key &>)
    return std::invoke(work, token, key);
  else if constexpr (std::invocable<Work &, const Key &, CancelToken>)
    return std::invoke(work, key, token);
  else if constexpr (std::invocable<Work &, const Key &>)
    return std::invoke(work, key);
  else if constexpr (std::invocable<Work &, CancelToken>)
    return std::invoke(work, token);
  else
    return std::invoke(work);
}

template <class Work, class Key>
using keyed_result_t = decltype(invoke_keyed_work(std::declval<Work &>(),
                                                  std::declval<CancelToken>(),
                                                  std::declval<const Key &>()));

template <class T, class Invoke> Result<T> capture_result(Invoke &&invoke) {
  try {
    if constexpr (std::is_void_v<T>) {
      std::invoke(std::forward<Invoke>(invoke));
      return {};
    } else {
      return std::invoke(std::forward<Invoke>(invoke));
    }
  } catch (const std::exception &error) {
    return std::unexpected(Error{error.what()});
  } catch (...) {
    return std::unexpected(Error{"unknown asynchronous error"});
  }
}

template <class T> struct AsyncChannel {
  // The worker writes `result` exactly once before enqueueing the publication
  // callback. UiExecutor's mutex establishes the happens-before edge. The UI
  // reads the payload only after `published` is set by that callback, so the
  // payload itself needs neither an atomic nor a second mutex.
  std::optional<Result<T>> result;
  voidui::detail::ComponentRuntime *runtime = nullptr;
  UiDispatcher dispatcher;
  std::atomic<bool> cancelled{false};
  bool published = false;
};

template <class Key, class T>
class AsyncSlot final : public voidui::detail::StateSlotBase {
public:
  ~AsyncSlot() override { cancel(); }

  /// Unmount cancellation. Runs from ~ComponentRuntime while the runtime is
  /// still alive, so a job is dropped at the moment its component leaves the
  /// tree even when a captured State handle keeps this slot's storage around.
  void detach() noexcept override { cancel(); }

  void cancel() noexcept {
    job.cancel();
    if (channel) {
      channel->cancelled.store(true, std::memory_order_release);
      channel->runtime = nullptr;
    }
  }

  std::optional<Key> key;
  std::shared_ptr<AsyncChannel<T>> channel;
  JobHandle job;
};

} // namespace detail

/// Starts one worker job and delivers its Result to `then` on the UI thread.
/// `work` may accept a CancelToken or no argument. The continuation is skipped
/// after cancellation and is never run by a worker.
template <class Work, class Then>
JobHandle run(Lane lane, Work &&work, Then &&then) {
  using StoredWork = std::decay_t<Work>;
  using StoredThen = std::decay_t<Then>;
  using Value = std::remove_cvref_t<detail::run_result_t<StoredWork>>;

  UiDispatcher dispatcher = current_ui_dispatcher();
  assert(dispatcher && "async::run requires an active VoidUI Window");

  return ThreadPool::shared().submit_cancellable(
      lane,
      [dispatcher, work = StoredWork(std::forward<Work>(work)),
       then = StoredThen(std::forward<Then>(then))](CancelToken token) mutable {
        Result<Value> result = detail::capture_result<Value>(
            [&]() -> Value { return detail::invoke_work(work, token); });
        if (token.stop_requested())
          return;

        const bool posted =
            dispatcher.post([token, then = std::move(then),
                             result = std::move(result)]() mutable {
              if (token.stop_requested())
                return;
              std::invoke(then, std::move(result));
            });
        (void)posted;
      });
}

/// Defaults to the blocking lane, matching use_async. Work reached through this
/// API is far more often a file read or a request than a decode, and putting
/// one of those on a CPU worker stalls the lane that visible work shares.
/// Pass Lane::Interactive explicitly for CPU work that a frame is waiting on.
template <class Work, class Then> JobHandle run(Work &&work, Then &&then) {
  return run(Lane::Blocking, std::forward<Work>(work),
             std::forward<Then>(then));
}

/// UI-thread view of a component-owned asynchronous operation.
template <class T> class Async {
public:
  explicit Async(std::shared_ptr<detail::AsyncChannel<T>> channel)
      : channel_(std::move(channel)) {}

  [[nodiscard]] bool loading() const noexcept { return !channel_->published; }
  [[nodiscard]] bool ready() const noexcept {
    return channel_->published && channel_->result->has_value();
  }
  [[nodiscard]] bool failed() const noexcept {
    return channel_->published && !channel_->result->has_value();
  }

  const T &get() const {
    assert(ready());
    return channel_->result->value();
  }
  const Error &error() const {
    assert(failed());
    return channel_->result->error();
  }

  const T &operator*() const { return get(); }
  const T *operator->() const { return &get(); }

private:
  std::shared_ptr<detail::AsyncChannel<T>> channel_;
};

template <> class Async<void> {
public:
  explicit Async(std::shared_ptr<detail::AsyncChannel<void>> channel)
      : channel_(std::move(channel)) {}

  [[nodiscard]] bool loading() const noexcept { return !channel_->published; }
  [[nodiscard]] bool ready() const noexcept {
    return channel_->published && channel_->result->has_value();
  }
  [[nodiscard]] bool failed() const noexcept {
    return channel_->published && !channel_->result->has_value();
  }
  const Error &error() const {
    assert(failed());
    return channel_->result->error();
  }

private:
  std::shared_ptr<detail::AsyncChannel<void>> channel_;
};

/// Component hook which restarts work whenever `key` changes.
///
/// The work callable may take the key, a CancelToken plus the key (in either
/// order), only a CancelToken, or no arguments. The no-argument form is useful
/// when the key is purely a dependency list and the closure owns the inputs.
///
/// The slot destructor and the publication callback both execute on the UI
/// thread. Consequently `cancelled == false` inside the callback proves that
/// the slot, component node, and runtime pointer are still alive. Workers only
/// produce values; they never inspect or mutate the widget tree.
template <class Key, class Work>
auto use_async(Key &&key, Work &&work, Lane lane = Lane::Blocking) {
  using StoredKey = std::decay_t<Key>;
  using StoredWork = std::decay_t<Work>;
  using Value =
      std::remove_cvref_t<detail::keyed_result_t<StoredWork, StoredKey>>;
  using Slot = detail::AsyncSlot<StoredKey, Value>;

  voidui::detail::ComponentRuntime *runtime =
      voidui::detail::current_component_runtime();
  assert(runtime && "use_async may only be called while rendering a component");
  runtime->assert_owner_thread();

  Slot *slot = runtime->template use_slot<Slot>();
  StoredKey next_key(std::forward<Key>(key));
  if (!slot->key || !(*slot->key == next_key)) {
    slot->cancel();
    slot->key.emplace(std::move(next_key));
    slot->channel = std::make_shared<detail::AsyncChannel<Value>>();
    slot->channel->runtime = runtime;
    slot->channel->dispatcher = current_ui_dispatcher();
    assert(slot->channel->dispatcher &&
           "use_async requires an active VoidUI Window");

    const std::shared_ptr<detail::AsyncChannel<Value>> channel = slot->channel;
    StoredKey work_key = *slot->key;
    slot->job = ThreadPool::shared().submit_cancellable(
        lane, [channel, work_key = std::move(work_key),
               work = StoredWork(std::forward<Work>(work))](
                  CancelToken token) mutable {
          channel->result.emplace(detail::capture_result<Value>([&]() -> Value {
            return detail::invoke_keyed_work(work, token, work_key);
          }));

          if (token.stop_requested() ||
              channel->cancelled.load(std::memory_order_acquire))
            return;

          const bool posted = channel->dispatcher.post([channel] {
            // AsyncSlot destruction and this callback are serialized by the
            // UI executor. There is therefore no check-then-use race on the
            // intentionally non-atomic runtime pointer.
            if (channel->cancelled.load(std::memory_order_acquire))
              return;
            channel->published = true;
            channel->runtime->invalidate();
          });
          (void)posted;
        });
  }

  return Async<Value>(slot->channel);
}

/// A thread-safe one-element queue for streams where only the newest value is
/// useful (video frames, progress snapshots, telemetry). `push` overwrites an
/// older unconsumed value instead of allowing producer latency to accumulate.
template <class T> class Mailbox {
public:
  Mailbox() : state_(std::make_shared<State>()) {}

  void push(T value) const {
    std::lock_guard lock(state_->mutex);
    state_->latest.emplace(std::move(value));
  }

  /// Replaces the current value and coalesces UI notification until the queued
  /// callback runs. The notification should only invalidate or consume state;
  /// it executes at the same pre-layout hand-off point as every async result.
  ///
  /// `take_latest` may legitimately find nothing there. The pending flag clears
  /// before the notification runs, so a value pushed in that window is consumed
  /// by the notification already in flight and leaves a second, empty one
  /// behind it. Coalescing without that ordering would be the alternative, and
  /// it would drop the newest frame instead.
  [[nodiscard]] bool push(T value, UiDispatcher dispatcher, Task notify) const {
    push(std::move(value));
    if (state_->notification_pending.exchange(true, std::memory_order_acq_rel))
      return true;

    const std::shared_ptr<State> state = state_;
    const bool posted =
        dispatcher.post([state, notify = std::move(notify)]() mutable {
          state->notification_pending.store(false, std::memory_order_release);
          std::invoke(std::move(notify));
        });
    if (!posted)
      state_->notification_pending.store(false, std::memory_order_release);
    return posted;
  }

  [[nodiscard]] std::optional<T> take_latest() const {
    std::lock_guard lock(state_->mutex);
    std::optional<T> value = std::move(state_->latest);
    state_->latest.reset();
    return value;
  }

private:
  struct State {
    std::mutex mutex;
    std::optional<T> latest;
    std::atomic<bool> notification_pending{false};
  };

  std::shared_ptr<State> state_;
};

} // namespace voidui::async

namespace voidui {
using async::use_async;
}
