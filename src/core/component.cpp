#include "voidui/core/component.h"

#include "voidui/core/context.h"
#include "voidui/core/widget_tree.h"

namespace voidui {
namespace detail {
namespace {

thread_local ComponentRuntime *active_runtime = nullptr;

} // namespace

ComponentRuntime *current_component_runtime() { return active_runtime; }

void set_current_component_runtime(ComponentRuntime *runtime) {
  active_runtime = runtime;
}

#ifndef NDEBUG
void ComponentRuntime::assert_owner_thread_impl() const noexcept {
  tree_->assert_owner_thread_();
}
#endif

ComponentRuntime::~ComponentRuntime() {
  // Unmount in two steps, and in this order. Clearing the back-pointer first
  // makes every State handle that outlives this node inert before anything
  // else runs. Detaching the slots afterwards happens while `this` is still a
  // valid target, which is what lets an asynchronous slot cancel against the
  // runtime it was started from -- its own destructor would be too late, since
  // a captured handle can keep the storage alive well past this point.
  storage_->runtime = nullptr;
  for (const std::unique_ptr<StateSlotBase> &slot : storage_->hooks)
    slot->detach();
}

void ComponentRuntime::invalidate() {
  assert_owner_thread();
  if (queued_)
    return;
  queued_ = true;
  tree_->queue_component_(node_);
}

} // namespace detail

Size<float> ComponentBase::layout(Constraints constraints, LayoutContext &ctx) {
  const Size<float> size = ctx.constrain_child(0, constraints);
  ctx.place_child(0, Point<float>{});
  return size;
}

} // namespace voidui
