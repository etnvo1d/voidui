#pragma once

#include <cstdint>
#include <memory>
#include <utility>

#include "voidui/core/async/executor.h"

namespace voidui {

/// Work required to make the current tree visible. Values are ordered so the
/// more expensive request can absorb the cheaper one with a simple comparison.
enum class Invalidation : std::uint8_t {
  None = 0,
  Paint = 1,
  Layout = 2,
};

constexpr Invalidation max_invalidation(Invalidation a, Invalidation b) {
  return a < b ? b : a;
}

class WidgetTree;

namespace detail {

/// A tree's liveness, published separately from the tree itself so a handle can
/// outlive one. Cleared by ~WidgetTree on the UI thread, which is also the only
/// thread that ever reads it.
struct TreeToken {
  WidgetTree *tree = nullptr;
};

} // namespace detail

/// A thread-safe, copyable way to ask a tree for another frame.
///
/// Background work finishes on a worker thread, and whatever started it may be
/// gone by then -- a widget rebuilt, a row scrolled out of the tree, a window
/// closed. So this holds neither a widget nor a tree. It holds the UI queue's
/// endpoint and the tree's liveness token, and asking a tree that has gone away
/// quietly does nothing.
///
/// The request is always posted, never applied in place, so it arrives at the
/// event loop's hand-off point like every other cross-thread result: on the UI
/// thread, between events and layout, never inside a layout or paint pass.
class Invalidator {
public:
  Invalidator() = default;

  Invalidator(std::shared_ptr<detail::TreeToken> token,
              async::UiDispatcher dispatcher)
      : token_(std::move(token)), dispatcher_(std::move(dispatcher)) {}

  /// False when there is nothing left to invalidate -- the tree is gone, its
  /// window's queue has closed, or this handle was default-constructed.
  bool request_layout() const;
  bool request_paint() const;

  explicit operator bool() const noexcept {
    return token_ != nullptr && static_cast<bool>(dispatcher_);
  }

  /// Identifies the tree, not the caller. Every widget in one tree shares it,
  /// which is what lets a list of a thousand rows waiting on one load queue one
  /// wake-up between them rather than a thousand.
  const void *tree_key() const noexcept { return token_.get(); }

private:
  bool post_(Invalidation what) const;

  std::shared_ptr<detail::TreeToken> token_;
  async::UiDispatcher dispatcher_;
};

} // namespace voidui
