#pragma once

#include <cassert>
#include <concepts>
#include <functional>
#include <memory>
#include <type_traits>
#include <utility>
#include <vector>

namespace voidui {

struct Node;
class WidgetTree;

namespace detail {

class StateSlotBase {
public:
  virtual ~StateSlotBase() = default;

  /// Called once when the owning component unmounts, while its runtime pointer
  /// is still a valid target. A slot which started asynchronous work cancels it
  /// here rather than in its destructor: a State handle captured by a callback
  /// can keep this storage alive past the node, which makes destruction too
  /// late to be the cancellation point.
  virtual void detach() noexcept {}
};

template <class T> class StateSlot final : public StateSlotBase {
public:
  template <class U>
  explicit StateSlot(U &&value) : value(std::forward<U>(value)) {}

  T value;
};

class ComponentRuntime;

/// Hook slots for one component node, plus the back-pointer that says whether
/// that node is still mounted.
///
/// Held through a shared_ptr because a State handle outlives its component far
/// too easily -- an asynchronous continuation captures one, and the node is
/// gone by the time the worker finishes. Keeping the storage alive makes such a
/// handle inert rather than dangling: reads still see the last value, and
/// `runtime` is null so writes stop invalidating a tree that no longer has this
/// component in it.
struct HookStorage {
  std::vector<std::unique_ptr<StateSlotBase>> hooks;

  /// The mounted component, or null once it has unmounted. Only ever read or
  /// written on the UI thread, so a State handle tests it without any
  /// synchronisation at all.
  ComponentRuntime *runtime = nullptr;
};

/// Persistent state owned by one mounted component node.
class ComponentRuntime {
public:
  ComponentRuntime() : storage_(std::make_shared<HookStorage>()) {
    storage_->runtime = this;
  }
  ~ComponentRuntime();

  ComponentRuntime(const ComponentRuntime &) = delete;
  ComponentRuntime &operator=(const ComponentRuntime &) = delete;

  template <class Slot, class... Args> Slot *use_slot(Args &&...args) {
    static_assert(std::derived_from<Slot, StateSlotBase>);
    if (hook_cursor_ == storage_->hooks.size())
      storage_->hooks.push_back(
          std::make_unique<Slot>(std::forward<Args>(args)...));

    StateSlotBase *slot = storage_->hooks[hook_cursor_++].get();
    // Hooks are identified by call order alone, so a conditionally called hook
    // reinterprets its neighbour's slot -- now that slots differ in type and
    // not merely in T, that would corrupt a vtable. Debug builds turn it into a
    // diagnosable failure at the point where the order first changed.
    assert(dynamic_cast<Slot *>(slot) &&
           "hook call order changed between renders");
    return static_cast<Slot *>(slot);
  }

  /// The storage a State handle holds on to. Shared rather than borrowed, so
  /// the handle stays readable after the component goes away.
  const std::shared_ptr<HookStorage> &storage() const { return storage_; }

  template <class T, class U> StateSlot<T> *use(U &&initial) {
    return use_slot<StateSlot<T>>(std::forward<U>(initial));
  }

  void assert_owner_thread() const noexcept {
#ifndef NDEBUG
    assert_owner_thread_impl();
#endif
  }

  void invalidate();

private:
  friend class ::voidui::WidgetTree;
  friend class ComponentRenderScope;

#ifndef NDEBUG
  void assert_owner_thread_impl() const noexcept;
#endif

  void begin_render() { hook_cursor_ = 0; }

  std::shared_ptr<HookStorage> storage_;
  std::size_t hook_cursor_ = 0;
  WidgetTree *tree_ = nullptr;
  Node *node_ = nullptr;
  bool queued_ = false;
};

ComponentRuntime *current_component_runtime();
void set_current_component_runtime(ComponentRuntime *runtime);

class ComponentRenderScope {
public:
  explicit ComponentRenderScope(ComponentRuntime &runtime)
      : previous_(current_component_runtime()) {
    runtime.begin_render();
    set_current_component_runtime(&runtime);
  }

  ~ComponentRenderScope() { set_current_component_runtime(previous_); }

private:
  ComponentRuntime *previous_;
};

} // namespace detail

/// A lightweight handle into a component-owned state slot.
/// Hooks are identified by call order, so every render must call use_state in
/// the same unconditional order.
///
/// A handle may legitimately outlive its component: the natural way to write an
/// asynchronous continuation is to capture one, and nothing guarantees the node
/// is still mounted when the worker finishes. Such a handle is inert rather
/// than dangling -- `get` still reads the last value it held, and `set` writes
/// it without invalidating a tree the component has already left. It costs one
/// shared reference per handle and one branch per write, which buys away an
/// entire class of use-after-free that no assertion could have caught.
template <class T> class State {
public:
  /// Reads are checked as strictly as writes. `operator T` makes an accidental
  /// cross-thread read the easiest mistake of the two to write -- `int n =
  /// count;` inside a worker closure compiles in silence -- and it races with
  /// the UI thread exactly as a write would.
  const T &get() const {
    assert_owner_thread_();
    return slot_->value;
  }

  template <class U> void set(U &&value) const {
    assert_owner_thread_();
    slot_->value = std::forward<U>(value);
    invalidate_();
  }

  template <class Fn> void update(Fn &&fn) const {
    assert_owner_thread_();
    std::invoke(std::forward<Fn>(fn), slot_->value);
    invalidate_();
  }

  const T &operator*() const { return get(); }
  operator T() const { return get(); }

  void operator++(int) const
    requires requires(T value) { ++value; }
  {
    assert_owner_thread_();
    ++slot_->value;
    invalidate_();
  }

  void operator--(int) const
    requires requires(T value) { --value; }
  {
    assert_owner_thread_();
    --slot_->value;
    invalidate_();
  }

  const State &operator++() const
    requires requires(T value) { ++value; }
  {
    assert_owner_thread_();
    ++slot_->value;
    invalidate_();
    return *this;
  }

  const State &operator--() const
    requires requires(T value) { --value; }
  {
    assert_owner_thread_();
    --slot_->value;
    invalidate_();
    return *this;
  }

  /// False once the component that owns this slot has unmounted. Reading and
  /// writing stay well defined either way; this only reports whether a write
  /// will still reach the tree.
  [[nodiscard]] bool mounted() const noexcept {
    return storage_->runtime != nullptr;
  }

private:
  template <class U> friend State<std::decay_t<U>> use_state(U &&initial);

  State(detail::StateSlot<T> *slot,
        std::shared_ptr<detail::HookStorage> storage)
      : slot_(slot), storage_(std::move(storage)) {}

  void assert_owner_thread_() const noexcept {
    if (detail::ComponentRuntime *runtime = storage_->runtime)
      runtime->assert_owner_thread();
  }

  void invalidate_() const {
    if (detail::ComponentRuntime *runtime = storage_->runtime)
      runtime->invalidate();
  }

  detail::StateSlot<T> *slot_;
  std::shared_ptr<detail::HookStorage> storage_;
};

template <class T> State<std::decay_t<T>> use_state(T &&initial) {
  using Value = std::decay_t<T>;
  detail::ComponentRuntime *runtime = detail::current_component_runtime();
  assert(runtime && "use_state may only be called while rendering a component");
  runtime->assert_owner_thread();
  return State<Value>(runtime->use<Value>(std::forward<T>(initial)),
                      runtime->storage());
}

} // namespace voidui
