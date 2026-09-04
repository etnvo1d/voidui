#include <atomic>
#include <chrono>
#include <cstdio>
#include <future>
#include <memory>
#include <optional>
#include <thread>

#include "voidui/core/async.h"
#include "voidui/core/component.h"
#include "voidui/core/context.h"
#include "voidui/core/widget_tree.h"

namespace {

using namespace std::chrono_literals;

class Probe : public voidui::Widget {
public:
  explicit Probe(int value) : value_(value) {}

  void register_children(voidui::Registrar &) override {}
  voidui::Size<float> layout(voidui::Constraints,
                             voidui::LayoutContext &) override {
    return {10.0f, 10.0f};
  }
  void draw(const voidui::DrawContext &, voidui::Painter &) override {}
  voidui::EventResult on_event(voidui::Event &) override {
    return voidui::EventResult::Unhandled;
  }
  std::unique_ptr<voidui::Widget> clone() const override {
    return std::make_unique<Probe>(value_);
  }

  int value() const { return value_; }

private:
  int value_;
};

bool expect(bool condition, const char *message) {
  if (!condition)
    std::fprintf(stderr, "FAIL: %s\n", message);
  return condition;
}

bool wait_until_pending(voidui::async::UiExecutor &executor) {
  for (int i = 0; i < 500 && !executor.pending(); ++i)
    std::this_thread::sleep_for(1ms);
  return executor.pending();
}

} // namespace

int main() {
  voidui::async::UiExecutor executor;
  voidui::async::detail::UiExecutorScope scope(executor);
  std::atomic<int> wakes = 0;
  executor.set_waker([&] { ++wakes; });

  bool ok = true;
  std::atomic<int> executed = 0;
  std::atomic<bool> accepted = true;
  std::thread producer([&] {
    for (int i = 0; i < 100; ++i) {
      if (!voidui::async::post_to_ui([&] { ++executed; }))
        accepted.store(false);
    }
  });
  producer.join();
  ok &= expect(accepted, "post_to_ui is available to arbitrary producers");
  ok &= expect(wakes == 1, "a producer burst is represented by one wakeup");
  ok &= expect(executor.drain(0.0) == 100 && executed == 100,
               "the complete producer snapshot drains on the UI thread");

  int nested = 0;
  executor.post([&] {
    ++nested;
    executor.post([&] { ++nested; });
  });
  executor.drain(0.0);
  ok &= expect(nested == 1 && executor.pending(),
               "a callback cannot recursively extend its drain snapshot");
  executor.drain(0.0);
  ok &= expect(nested == 2, "nested work runs on the following turn");

  int timed = 0;
  executor.post_at(voidui::async::UiExecutor::now(), [&] { ++timed; });
  ok &= expect(executor.pending(), "a due UI timer makes the executor ready");
  executor.drain(0.0);
  ok &= expect(timed == 1, "a due UI timer joins the next drain snapshot");

  int budgeted = 0;
  executor.post([&] {
    std::this_thread::sleep_for(3ms);
    ++budgeted;
  });
  executor.post([&] { ++budgeted; });
  ok &= expect(executor.drain(0.001) == 1 && executor.pending(),
               "the drain budget carries excess work into the next turn");
  executor.drain(0.0);
  ok &= expect(budgeted == 2, "budgeted work resumes without being dropped");

  int run_value = 0;
  voidui::async::run(
      [] { return 42; },
      [&](voidui::async::Result<int> result) { run_value = result.value(); });
  ok &= expect(wait_until_pending(executor),
               "a completed worker job wakes the UI executor");
  executor.drain(0.0);
  ok &= expect(run_value == 42, "async::run publishes its value on the UI");

  int cancelled_continuations = 0;
  auto cancelled_job = voidui::async::run(
      [] { return 7; },
      [&](voidui::async::Result<int>) { ++cancelled_continuations; });
  ok &= expect(wait_until_pending(executor),
               "a cancellable continuation reaches the UI queue");
  cancelled_job.cancel();
  executor.drain(0.0);
  ok &= expect(cancelled_continuations == 0,
               "cancellation also suppresses an already queued continuation");

  std::optional<voidui::State<int>> key;
  int rendered = -1;
  auto view = voidui::component([&] {
    key = voidui::use_state(1);
    const int input = key->get();
    auto value = voidui::use_async(input, [input] { return input * 10; });
    rendered = value.ready() ? value.get() : 0;
    return Probe(rendered);
  });
  voidui::WidgetTree tree(voidui::transfer_widget(std::move(view)));
  tree.layout({100.0f, 100.0f});
  ok &= expect(wait_until_pending(executor),
               "use_async completion is posted to the component executor");
  executor.drain(0.0);
  tree.layout({100.0f, 100.0f});
  ok &= expect(rendered == 10,
               "use_async invalidates and republishes component output");

  key->set(2);
  tree.layout({100.0f, 100.0f});
  ok &= expect(rendered == 0, "a changed key starts a fresh loading state");
  ok &= expect(wait_until_pending(executor),
               "the replacement operation completes independently");
  executor.drain(0.0);
  tree.layout({100.0f, 100.0f});
  ok &= expect(rendered == 20, "the replacement key owns the visible result");

  std::promise<void> started_promise;
  std::promise<void> release_promise;
  std::future<void> started = started_promise.get_future();
  std::shared_future<void> release = release_promise.get_future().share();
  auto pending_view = voidui::component([&] {
    auto value = voidui::use_async(1, [&](const int &) {
      started_promise.set_value();
      release.wait();
      return 99;
    });
    return Probe(value.ready() ? value.get() : 0);
  });
  tree.build(voidui::transfer_widget(std::move(pending_view)));
  tree.layout({100.0f, 100.0f});
  started.wait();
  tree.build(std::make_unique<Probe>(5));
  tree.layout({100.0f, 100.0f});
  release_promise.set_value();
  std::this_thread::sleep_for(10ms);
  executor.drain(0.0);
  ok &= expect(!tree.needs_layout(),
               "an unmounted component discards its late worker result");

  // The shape every "click a button, load a thing" flow takes: a continuation
  // captures a State handle, and the component is gone before the worker
  // finishes. The handle must absorb the write rather than dangle.
  std::optional<voidui::State<int>> stale;
  auto stale_view = voidui::component([&] {
    auto value = voidui::use_state(0);
    stale = value;
    return Probe(*value);
  });
  tree.build(voidui::transfer_widget(std::move(stale_view)));
  tree.layout({100.0f, 100.0f});
  voidui::async::run([] { return 7; },
                     [state = *stale](voidui::async::Result<int> result) {
                       state.set(result.value());
                     });
  tree.build(std::make_unique<Probe>(5));
  tree.layout({100.0f, 100.0f});
  ok &= expect(!stale->mounted(), "an unmounted State handle reports itself");
  ok &= expect(wait_until_pending(executor),
               "a continuation outliving its component still reaches the UI");
  executor.drain(0.0);
  ok &= expect(stale->get() == 7 && !tree.needs_layout(),
               "a detached State handle takes the write without touching the "
               "tree it has left");

  voidui::async::Mailbox<int> mailbox;
  mailbox.push(1);
  mailbox.push(2);
  ok &= expect(mailbox.take_latest() == 2,
               "Mailbox drops stale values and retains the newest one");

  int mailbox_notifications = 0;
  (void)mailbox.push(3, executor.dispatcher(),
                     [&] { ++mailbox_notifications; });
  (void)mailbox.push(4, executor.dispatcher(),
                     [&] { ++mailbox_notifications; });
  executor.drain(0.0);
  ok &= expect(mailbox_notifications == 1 && mailbox.take_latest() == 4,
               "Mailbox coalesces wakeups while retaining the newest value");

  voidui::async::ThreadPool::shared().shutdown();
  return ok ? 0 : 1;
}
