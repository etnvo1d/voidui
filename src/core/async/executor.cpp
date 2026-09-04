#include "voidui/core/async/executor.h"

#include <algorithm>
#include <atomic>
#include <cassert>
#include <chrono>
#include <cstdint>
#include <iterator>
#include <limits>
#include <mutex>
#include <thread>
#include <utility>
#include <vector>

namespace voidui::async {
namespace {

double steady_seconds() {
  return std::chrono::duration<double>(
             std::chrono::steady_clock::now().time_since_epoch())
      .count();
}

} // namespace

namespace detail {

struct TimedTask {
  double when = 0.0;
  std::uint64_t sequence = 0;
  Task task;
};

struct LaterTimer {
  bool operator()(const TimedTask &left, const TimedTask &right) const {
    if (left.when != right.when)
      return left.when > right.when;
    return left.sequence > right.sequence;
  }
};

struct UiQueue {
  std::mutex mutex;
  std::vector<Task> incoming;

  // Only the UI thread touches the running snapshot and timer heap. Keeping
  // them outside the producer mutex removes all lock traffic while callbacks
  // execute and guarantees that a callback cannot recursively extend its own
  // drain turn.
  std::vector<Task> running;
  std::size_t running_offset = 0;
  bool draining = false;
  std::vector<TimedTask> timers;
  std::uint64_t next_timer_sequence = 0;

  std::atomic<bool> signalled{false};
  std::atomic<bool> alive{true};
  std::atomic<std::shared_ptr<const std::function<void()>>> waker;
  std::thread::id ui_thread = std::this_thread::get_id();
};

std::atomic<std::shared_ptr<UiQueue>> active_ui_queue;

bool enqueue(const std::shared_ptr<UiQueue> &queue, Task task) {
  if (!queue || !queue->alive.load(std::memory_order_acquire))
    return false;

  {
    std::lock_guard lock(queue->mutex);
    if (!queue->alive.load(std::memory_order_relaxed))
      return false;
    queue->incoming.push_back(std::move(task));
  }

  // One native event is enough to wake the UI for an arbitrary number of
  // producers. Clearing the bit while holding the queue mutex in `drain`
  // prevents a post racing with the swap from ever losing its wakeup.
  if (!queue->signalled.exchange(true, std::memory_order_acq_rel)) {
    const auto waker = queue->waker.load(std::memory_order_acquire);
    if (waker)
      std::invoke(*waker);
  }
  return true;
}

UiExecutorScope::UiExecutorScope(UiExecutor &executor)
    : current_(executor.queue_) {
  previous_ = active_ui_queue.exchange(current_, std::memory_order_acq_rel);
}

UiExecutorScope::~UiExecutorScope() {
  active_ui_queue.store(std::move(previous_), std::memory_order_release);
}

} // namespace detail

bool UiDispatcher::post(Task task) const {
  return detail::enqueue(queue_.lock(), std::move(task));
}

UiExecutor::UiExecutor() : queue_(std::make_shared<detail::UiQueue>()) {}

UiExecutor::~UiExecutor() {
  queue_->alive.store(false, std::memory_order_release);
  queue_->waker.store({}, std::memory_order_release);
  std::lock_guard lock(queue_->mutex);
  queue_->incoming.clear();
  queue_->running.clear();
  queue_->timers.clear();
}

UiDispatcher UiExecutor::dispatcher() const { return UiDispatcher(queue_); }

void UiExecutor::post(Task task) {
  const bool accepted = detail::enqueue(queue_, std::move(task));
  (void)accepted;
}

double UiExecutor::now() { return steady_seconds(); }

void UiExecutor::post_at(double when_seconds, Task task) {
  assert(on_ui_thread() &&
         "UiExecutor timers must be installed on its UI thread");
  queue_->timers.push_back(detail::TimedTask{
      when_seconds, queue_->next_timer_sequence++, std::move(task)});
  std::push_heap(queue_->timers.begin(), queue_->timers.end(),
                 detail::LaterTimer{});
}

std::size_t UiExecutor::drain(double budget_seconds) {
  assert(on_ui_thread() && "UiExecutor must be drained on its UI thread");
  // A callback that drains again would consume its own turn's snapshot and
  // leave the outer drain walking indices it no longer owns. One snapshot per
  // turn is the whole contract, so re-entry is a bug rather than a mode.
  assert(!queue_->draining && "UiExecutor::drain is not re-entrant");
  queue_->draining = true;
  struct DrainScope {
    detail::UiQueue &queue;
    ~DrainScope() { queue.draining = false; }
  } drain_scope{*queue_};

  const auto started = std::chrono::steady_clock::now();

  // Move due timers into the same snapshot as cross-thread arrivals. A timer
  // installed by a callback stays in the heap until the next drain, preserving
  // the one-snapshot-per-turn rule even when its deadline is already due.
  const double now = steady_seconds();
  std::vector<Task> due;
  while (!queue_->timers.empty() && queue_->timers.front().when <= now) {
    std::pop_heap(queue_->timers.begin(), queue_->timers.end(),
                  detail::LaterTimer{});
    due.push_back(std::move(queue_->timers.back().task));
    queue_->timers.pop_back();
  }

  {
    std::lock_guard lock(queue_->mutex);

    if (queue_->running_offset == queue_->running.size()) {
      queue_->running.clear();
      queue_->running_offset = 0;
    } else if (queue_->running_offset != 0) {
      // Compaction happens only after a budget overrun. The common path swaps
      // vectors and performs no per-task movement at all.
      std::move(queue_->running.begin() + queue_->running_offset,
                queue_->running.end(), queue_->running.begin());
      queue_->running.resize(queue_->running.size() - queue_->running_offset);
      queue_->running_offset = 0;
    }

    if (queue_->running.empty()) {
      queue_->running.swap(queue_->incoming);
    } else {
      queue_->running.reserve(queue_->running.size() + queue_->incoming.size());
      std::move(queue_->incoming.begin(), queue_->incoming.end(),
                std::back_inserter(queue_->running));
      queue_->incoming.clear();
    }
    std::move(due.begin(), due.end(), std::back_inserter(queue_->running));
    queue_->signalled.store(false, std::memory_order_release);
  }

  const std::size_t snapshot_end = queue_->running.size();
  std::size_t executed = 0;
  while (queue_->running_offset < snapshot_end) {
    Task task = std::move(queue_->running[queue_->running_offset++]);
    std::invoke(std::move(task));
    ++executed;

    if (budget_seconds > 0.0 && std::chrono::duration<double>(
                                    std::chrono::steady_clock::now() - started)
                                        .count() >= budget_seconds)
      break;
  }

  if (queue_->running_offset == queue_->running.size()) {
    queue_->running.clear();
    queue_->running_offset = 0;
  }
  return executed;
}

bool UiExecutor::pending() const {
  assert(on_ui_thread() && "UiExecutor readiness is a UI-thread query");
  if (queue_->running_offset != queue_->running.size() ||
      queue_->signalled.load(std::memory_order_acquire))
    return true;
  return !queue_->timers.empty() &&
         queue_->timers.front().when <= steady_seconds();
}

double UiExecutor::next_wake_time() const {
  assert(on_ui_thread() && "UiExecutor deadlines are a UI-thread query");
  return queue_->timers.empty() ? std::numeric_limits<double>::infinity()
                                : queue_->timers.front().when;
}

void UiExecutor::set_waker(std::function<void()> waker) {
  assert(on_ui_thread() && "UiExecutor wakers are configured on the UI thread");
  std::shared_ptr<const std::function<void()>> stored;
  if (waker)
    stored = std::make_shared<const std::function<void()>>(std::move(waker));
  queue_->waker.store(std::move(stored), std::memory_order_release);
}

bool UiExecutor::on_ui_thread() const noexcept {
  return queue_->ui_thread == std::this_thread::get_id();
}

UiDispatcher current_ui_dispatcher() {
  return UiDispatcher(detail::active_ui_queue.load(std::memory_order_acquire));
}

bool post_to_ui(Task task) {
  return current_ui_dispatcher().post(std::move(task));
}

} // namespace voidui::async
