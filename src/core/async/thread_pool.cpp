#include "voidui/core/async/thread_pool.h"

#include <algorithm>
#include <condition_variable>
#include <cstddef>
#include <deque>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

namespace voidui::async {
namespace {

std::atomic<ThreadPool *> shared_pool = nullptr;

} // namespace

namespace detail {

struct JobControl {
  std::atomic<bool> cancelled{false};
};

struct Job {
  std::shared_ptr<JobControl> control;
  CancellableTask task;
};

} // namespace detail

struct ThreadPool::Impl {
  std::mutex cpu_mutex;
  std::condition_variable cpu_ready;
  std::deque<detail::Job> interactive;
  std::deque<detail::Job> background;
  std::vector<std::thread> cpu_workers;

  std::mutex blocking_mutex;
  std::condition_variable blocking_ready;
  std::deque<detail::Job> blocking;
  std::vector<std::thread> blocking_workers;

  std::mutex lifecycle_mutex;
  std::condition_variable lifecycle_ready;
  std::atomic<bool> stopping{false};
  std::shared_ptr<std::atomic<bool>> generation_stop =
      std::make_shared<std::atomic<bool>>(false);
  bool cpu_started = false;
  bool blocking_started = false;
  bool shutdown_in_progress = false;

  static unsigned cpu_worker_count() {
    const unsigned cores = std::max(std::thread::hardware_concurrency(), 2u);
    return std::min(cores - 1, 4u);
  }

  static unsigned blocking_worker_count() {
    // Blocking work is isolated precisely because its useful concurrency is
    // not bounded by core count. Four workers cover concurrent file/network
    // waits without creating an unbounded thread-per-request policy.
    return 4;
  }

  void start_cpu_workers_locked() {
    if (cpu_started)
      return;
    cpu_started = true;
    cpu_workers.reserve(cpu_worker_count());
    for (unsigned i = 0; i < cpu_worker_count(); ++i)
      cpu_workers.emplace_back([this] { cpu_loop(); });
  }

  void start_blocking_workers_locked() {
    if (blocking_started)
      return;
    blocking_started = true;
    blocking_workers.reserve(blocking_worker_count());
    for (unsigned i = 0; i < blocking_worker_count(); ++i)
      blocking_workers.emplace_back([this] { blocking_loop(); });
  }

  void cpu_loop() {
    for (;;) {
      detail::Job job;
      {
        std::unique_lock lock(cpu_mutex);
        cpu_ready.wait(lock, [this] {
          return stopping.load(std::memory_order_acquire) ||
                 !interactive.empty() || !background.empty();
        });
        if (stopping.load(std::memory_order_relaxed))
          return;

        // Background work deliberately yields to every interactive arrival.
        // FIFO order within each lane keeps scheduling predictable.
        std::deque<detail::Job> &queue =
            interactive.empty() ? background : interactive;
        job = std::move(queue.front());
        queue.pop_front();
      }
      if (!job.control->cancelled.load(std::memory_order_acquire))
        std::invoke(std::move(job.task), ThreadPool::make_cancel_token(
                                             job.control, generation_stop));
    }
  }

  void blocking_loop() {
    for (;;) {
      detail::Job job;
      {
        std::unique_lock lock(blocking_mutex);
        blocking_ready.wait(lock, [this] {
          return stopping.load(std::memory_order_acquire) || !blocking.empty();
        });
        if (stopping.load(std::memory_order_relaxed))
          return;
        job = std::move(blocking.front());
        blocking.pop_front();
      }
      if (!job.control->cancelled.load(std::memory_order_acquire))
        std::invoke(std::move(job.task), ThreadPool::make_cancel_token(
                                             job.control, generation_stop));
    }
  }

  JobHandle submit(Lane lane, CancellableTask task) {
    auto control = std::make_shared<detail::JobControl>();
    detail::Job job{control, std::move(task)};
    std::lock_guard lifecycle(lifecycle_mutex);

    // A submission racing with shutdown is rejected as already cancelled.
    // This closes the only window in which a job could otherwise be appended
    // after queues were cleared but before their workers had exited.
    if (shutdown_in_progress || stopping.load(std::memory_order_acquire)) {
      control->cancelled.store(true, std::memory_order_release);
      return ThreadPool::make_job_handle(std::move(control));
    }

    if (lane == Lane::Blocking) {
      start_blocking_workers_locked();
      {
        std::lock_guard lock(blocking_mutex);
        blocking.push_back(std::move(job));
      }
      blocking_ready.notify_one();
    } else {
      start_cpu_workers_locked();
      {
        std::lock_guard lock(cpu_mutex);
        (lane == Lane::Interactive ? interactive : background)
            .push_back(std::move(job));
      }
      cpu_ready.notify_one();
    }
    return ThreadPool::make_job_handle(std::move(control));
  }

  void shutdown() {
    std::unique_lock lifecycle(lifecycle_mutex);
    lifecycle_ready.wait(lifecycle, [this] { return !shutdown_in_progress; });
    if (!cpu_started && !blocking_started)
      return;

    shutdown_in_progress = true;
    stopping.store(true, std::memory_order_release);
    generation_stop->store(true, std::memory_order_release);
    {
      std::lock_guard lock(cpu_mutex);
      for (detail::Job &job : interactive)
        job.control->cancelled.store(true, std::memory_order_release);
      for (detail::Job &job : background)
        job.control->cancelled.store(true, std::memory_order_release);
      interactive.clear();
      background.clear();
    }
    {
      std::lock_guard lock(blocking_mutex);
      for (detail::Job &job : blocking)
        job.control->cancelled.store(true, std::memory_order_release);
      blocking.clear();
    }
    cpu_ready.notify_all();
    blocking_ready.notify_all();

    // Do not hold a queue mutex while joining: a running cooperative job may
    // inspect its token, finish, and allow the worker to return normally.
    lifecycle.unlock();
    for (std::thread &worker : cpu_workers)
      worker.join();
    for (std::thread &worker : blocking_workers)
      worker.join();
    lifecycle.lock();

    cpu_workers.clear();
    blocking_workers.clear();
    cpu_started = false;
    blocking_started = false;
    generation_stop = std::make_shared<std::atomic<bool>>(false);
    stopping.store(false, std::memory_order_release);
    shutdown_in_progress = false;
    lifecycle.unlock();
    lifecycle_ready.notify_all();
  }
};

bool CancelToken::stop_requested() const noexcept {
  return !control_ || control_->cancelled.load(std::memory_order_acquire) ||
         (pool_stopping_ && pool_stopping_->load(std::memory_order_acquire));
}

void JobHandle::cancel() const noexcept {
  if (control_)
    control_->cancelled.store(true, std::memory_order_release);
}

bool JobHandle::cancelled() const noexcept {
  return control_ && control_->cancelled.load(std::memory_order_acquire);
}

ThreadPool &ThreadPool::shared() {
  static ThreadPool pool;
  return pool;
}

void ThreadPool::shutdown_shared() {
  if (ThreadPool *pool = shared_pool.load(std::memory_order_acquire))
    pool->shutdown();
}

ThreadPool::ThreadPool() : impl_(std::make_unique<Impl>()) {
  shared_pool.store(this, std::memory_order_release);
}
ThreadPool::~ThreadPool() {
  shutdown();
  shared_pool.store(nullptr, std::memory_order_release);
}

CancelToken ThreadPool::make_cancel_token(
    std::shared_ptr<detail::JobControl> control,
    std::shared_ptr<const std::atomic<bool>> pool_stopping) {
  return CancelToken(std::move(control), std::move(pool_stopping));
}

JobHandle
ThreadPool::make_job_handle(std::shared_ptr<detail::JobControl> control) {
  return JobHandle(std::move(control));
}

JobHandle ThreadPool::submit(Lane lane, Task task) {
  return submit_cancellable(lane,
                            [task = std::move(task)](CancelToken) mutable {
                              std::invoke(std::move(task));
                            });
}

JobHandle ThreadPool::submit_cancellable(Lane lane, CancellableTask task) {
  return impl_->submit(lane, std::move(task));
}

void ThreadPool::shutdown() { impl_->shutdown(); }

} // namespace voidui::async
