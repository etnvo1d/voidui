#pragma once

#include <atomic>
#include <functional>
#include <memory>

#include "voidui/core/async/executor.h"

namespace voidui::async {

enum class Lane {
  /// Latency-sensitive CPU work such as decoding a visible resource.
  Interactive,
  /// Best-effort CPU work such as prefetching off-screen resources.
  Background,
  /// Potentially long file, network, or other blocking operations.
  Blocking,
};

namespace detail {
struct JobControl;
}

/// A cheap cooperative-cancellation view passed to long-running jobs.
/// Cancellation never interrupts a thread. Work which can stop early should
/// inspect this token at natural chunk boundaries.
class CancelToken {
public:
  CancelToken() = default;
  [[nodiscard]] bool stop_requested() const noexcept;

private:
  friend class ThreadPool;
  CancelToken(std::shared_ptr<detail::JobControl> control,
              std::shared_ptr<const std::atomic<bool>> pool_stopping)
      : control_(std::move(control)), pool_stopping_(std::move(pool_stopping)) {
  }

  std::shared_ptr<detail::JobControl> control_;
  std::shared_ptr<const std::atomic<bool>> pool_stopping_;
};

/// A retained cancellation handle. Destroying the handle does not cancel the
/// job; component-owned async slots call `cancel` explicitly on replacement or
/// unmount so fire-and-forget user jobs remain possible.
class JobHandle {
public:
  JobHandle() = default;

  void cancel() const noexcept;
  [[nodiscard]] bool cancelled() const noexcept;
  explicit operator bool() const noexcept {
    return static_cast<bool>(control_);
  }

private:
  friend class ThreadPool;
  explicit JobHandle(std::shared_ptr<detail::JobControl> control)
      : control_(std::move(control)) {}

  std::shared_ptr<detail::JobControl> control_;
};

using CancellableTask = std::move_only_function<void(CancelToken)>;

/// A lazily started process-wide worker pool with separate CPU and blocking
/// paths. Interactive work is always selected before background work, while a
/// blocked file/network job can never occupy a CPU worker.
class ThreadPool {
public:
  static ThreadPool &shared();

  /// Shuts down the shared pool only if async work initialized it. Window uses
  /// this form so an application which never submits work does not even create
  /// the pool's bookkeeping object.
  static void shutdown_shared();

  /// Low-level submissions are noexcept by contract. Prefer `async::run` when
  /// failures should be converted to Result<T> and delivered to the UI.
  JobHandle submit(Lane lane, Task task);
  JobHandle submit_cancellable(Lane lane, CancellableTask task);

  /// Cancels queued work, requests cooperative cancellation from running work,
  /// and joins every worker. A later submit lazily starts a fresh worker set.
  void shutdown();

  ThreadPool(const ThreadPool &) = delete;
  ThreadPool &operator=(const ThreadPool &) = delete;

private:
  ThreadPool();
  ~ThreadPool();

  static CancelToken
  make_cancel_token(std::shared_ptr<detail::JobControl> control,
                    std::shared_ptr<const std::atomic<bool>> pool_stopping);
  static JobHandle make_job_handle(std::shared_ptr<detail::JobControl> control);

  struct Impl;
  std::unique_ptr<Impl> impl_;
};

} // namespace voidui::async
