// Five things you can watch happen, each proving a different part of the
// asynchronous path actually works. Run it from a terminal: every panel also
// logs to stderr with the time it happened, so what the window shows and what
// the worker threads did can be checked against each other.
//
//   1. Idle wake-up. Start the app and touch nothing. Two seconds later the
//      first line changes by itself. Nothing polls: the window is blocked in
//      SDL_WaitEvent until the worker's result wakes it. Leave the animation
//      off while checking this one -- a running animation keeps the window
//      awake and would prove nothing.
//
//   2. The UI thread never blocks. Turn the animation on, then start the three
//      second job. The square keeps turning and every button stays clickable
//      while a worker is stuck in a sleep loop.
//
//   3. Restart on a key change. Each click cancels the job in flight and
//      starts a fresh one. The cancelled worker exits early rather than
//      running to completion -- watch the stderr log say how far it got.
//
//   4. Cancellation on unmount. Hide the child component while its ten second
//      job is running: the job stops within one poll interval, without the
//      component having to do anything about it.
//
//   5. A thread the application owns. The ticker is not a VoidUI concept at
//      all -- it is the decoder, socket pump or device callback an application
//      already has. It touches exactly two things: a Mailbox to drop its
//      newest value into, and post_to_ui to say a new one is there.

#include <atomic>
#include <chrono>
#include <cstdio>
#include <functional>
#include <string>
#include <thread>

#include "voidui/core/async.h"
#include "voidui/core/component.h"
#include "voidui/core/window.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/text.h"

using namespace voidui;
using namespace std::chrono_literals;

namespace {

const char *vss = R"(
.app {
  width: fill;
  height: fill;
  background: #fbfbfd;
  padding: 18px;
}

.panel {
  background: #ffffff;
  border-color: #e7e7ef;
  border-width: 1px;
  border-radius: 10px;
  padding: 10px 12px;
}

.title  { font-size: 12px; color: #6b7280; }
.value  { font-size: 13px; color: #111827; line-height: 32px; }
.hint   { font-size: 12px; color: #9ca3af; }

button { font-size: 13px; padding: 6px 12px; border-radius: 8px; }
button:hover { background: #f3f4f6; }

/* The proof that the UI thread is free: this keeps turning at 60fps while a
   worker is asleep, and stops dead the moment anything blocks the loop. */
.spinner {
  width: 18px;
  height: 18px;
  background: #4f46e5;
  border-radius: 5px;
  animation: spin 1.1s linear infinite;
}

.spinner-idle {
  width: 18px;
  height: 18px;
  background: #d1d5db;
  border-radius: 5px;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}
)";

double elapsed() {
  static const double start = async::UiExecutor::now();
  return async::UiExecutor::now() - start;
}

std::string stamp() {
  char buffer[32];
  std::snprintf(buffer, sizeof buffer, "+%.2fs", elapsed());
  return buffer;
}

void log(const std::string &message) {
  std::fprintf(stderr, "[%7s] %s\n", stamp().c_str(), message.c_str());
  std::fflush(stderr);
}

std::atomic<int> cancelled_jobs{0};

/// Work that genuinely occupies a thread for `millis`, checking its token
/// often enough that cancellation is visible rather than theoretical.
///
/// Nothing in here touches the widget tree, and that is the whole rule: a
/// worker produces a value, and the value crosses to the UI thread through the
/// executor. The debug build asserts if this rule is broken.
std::string slow_work(async::CancelToken token, const char *label, int millis) {
  log(std::string(label) + " worker started, " + std::to_string(millis) + "ms");
  for (int done = 0; done < millis; done += 20) {
    std::this_thread::sleep_for(20ms);
    if (token.stop_requested()) {
      ++cancelled_jobs;
      log(std::string(label) + " worker cancelled after " +
          std::to_string(done) + "ms");
      return {};
    }
  }
  log(std::string(label) + " worker finished");
  return "done at " + stamp();
}

Column panel(std::string title, Row body) {
  return column(text(std::move(title)).add_class("title"),
                std::move(body).gap(12))
      .add_class("panel")
      .gap(2);
}

// -- 1. an idle window wakes itself ------------------------------------------

auto idle_panel() {
  return component([] {
    // No key, no restart: one job for the life of the component.
    auto result = use_async(0, [](async::CancelToken token) {
      return slow_work(token, "(1)", 2000);
    });

    return panel("1. idle wake-up",
                 row(text(result.ready()
                              ? result.get() + " -- with no input at all"
                              : "waiting, touch nothing...")
                         .add_class("value")));
  });
}

// -- 2. a blocking job leaves the UI thread alone ----------------------------

auto blocking_panel() {
  return component([] {
    auto status = use_state(std::string("idle"));

    return panel(
        "2. three seconds on a worker, UI stays live",
        row(button("Run").on_click([status] {
              status.set("running... keep clicking things");
              // Fire and forget. The continuation runs on the UI thread, at the
              // one hand-off point between events and layout.
              async::run(
                  [](async::CancelToken token) {
                    return slow_work(token, "(2)", 3000);
                  },
                  [status](async::Result<std::string> result) {
                    log("(2) continuation on the UI thread");
                    status.set(result.has_value() ? result.value()
                                                  : result.error().message);
                  });
            }),
            text(status).add_class("value")));
  });
}

// -- 3. a changed key cancels and restarts -----------------------------------

auto restart_panel() {
  return component([] {
    auto epoch = use_state(1);
    auto result = use_async(*epoch, [](async::CancelToken token,
                                       const int &key) {
      return slow_work(token, "(3)", 1500) + " (attempt " +
             std::to_string(key) + ")";
    });

    return panel("3. a changed key cancels the job in flight",
                 row(button("Reload").on_click([epoch] { epoch++; }),
                     text(result.ready() ? result.get() : "loading...")
                         .add_class("value")));
  });
}

// -- 4. unmounting cancels ---------------------------------------------------

auto long_job_panel() {
  return component([] {
    auto result = use_async(0, [](async::CancelToken token) {
      return slow_work(token, "(4)", 10000);
    });

    return row(text(result.ready() ? result.get()
                                   : "child component, ten seconds of work...")
                   .add_class("value"));
  });
}

auto unmount_panel() {
  return component([] {
    auto shown = use_state(true);

    // The count lags one interaction behind, because the worker notices its
    // token up to a poll interval after the click. The stderr log is the
    // precise record; this is the at-a-glance one.
    Column content = panel(
        "4. unmounting a component cancels its work",
        row(button(*shown ? "Unmount the child" : "Mount the child")
                .on_click([shown] { shown.set(!shown.get()); }),
            text(std::to_string(cancelled_jobs.load()) + " jobs cancelled")
                .add_class("value")));

    if (*shown)
      content.add(long_job_panel());

    return content;
  });
}

// -- 5. a thread the application owns ----------------------------------------

/// Stands in for a decoder, a socket pump, or an audio callback: a producer
/// that already exists and knows nothing about widgets.
class Ticker {
public:
  ~Ticker() { stop(); }

  /// Called on the UI thread. `on_frame` runs there too, once per batch of
  /// frames rather than once per frame.
  void start(std::function<void()> on_frame) {
    if (running_.exchange(true))
      return;
    on_frame_ = std::move(on_frame);
    dispatcher_ = async::current_ui_dispatcher();

    thread_ = std::thread([this] {
      int frame = 0;
      while (running_.load(std::memory_order_relaxed)) {
        std::this_thread::sleep_for(16ms);
        // Latest value wins. A producer faster than the display must never be
        // able to queue up a thousand stale frames ahead of the current one,
        // and the notification is coalesced the same way.
        if (!frames.push(++frame, dispatcher_, [this] { on_frame_(); })) {
          log("(5) window is gone, ticker stopping");
          return;
        }
      }
    });
    log("(5) producer thread started");
  }

  void stop() {
    if (running_.exchange(false) && thread_.joinable()) {
      thread_.join();
      log("(5) producer thread joined");
    }
  }

  async::Mailbox<int> frames;

private:
  std::function<void()> on_frame_;
  async::UiDispatcher dispatcher_;
  std::thread thread_;
  std::atomic<bool> running_{false};
};

auto ticker_panel(Ticker *ticker) {
  return component([ticker] {
    auto frame = use_state(0);
    auto running = use_state(false);

    return panel(
        "5. a producer thread the application owns",
        row(button(*running ? "Stop" : "Start").on_click([ticker, frame,
                                                          running] {
              if (*running) {
                ticker->stop();
                running.set(false);
                return;
              }
              ticker->start([ticker, frame] {
                // On the UI thread, so writing state here is the ordinary,
                // legal thing to do.
                if (std::optional<int> latest = ticker->frames.take_latest())
                  frame.set(*latest);
              });
              running.set(true);
            }),
            text("frame " + std::to_string(*frame)).add_class("value")));
  });
}

// -- the window ---------------------------------------------------------------

auto app(Ticker *ticker) {
  return component([ticker] {
    auto animating = use_state(false);

    return column(
               row(column().add_class(*animating ? "spinner" : "spinner-idle"),
                   button(*animating ? "Stop animation" : "Start animation")
                       .on_click([animating] {
                         animating.set(!animating.get());
                       }),
                   text(*animating
                            ? "the loop is awake every frame now"
                            : "leave this off while checking panel 1")
                       .add_class("hint"))
                   .gap(12),
               idle_panel(), blocking_panel(), restart_panel(),
               unmount_panel(), ticker_panel(ticker))
        .add_class("app")
        .gap(12);
  });
}

} // namespace

int main() {
  log("window starting -- panel 1 lands in two seconds, with no input");

  Ticker ticker;
  Window window("VoidUI Async", 560, 700);
  window.set_stylesheet(StyleParser::parse(vss, "async.vss").sheet);
  window.run(app(&ticker));

  // The window's executor is gone by now, so anything the ticker posts is
  // simply refused. Joining here is tidiness, not a correctness requirement.
  ticker.stop();
  log("window closed");
  return 0;
}
