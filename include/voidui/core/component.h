#pragma once

#include <concepts>
#include <functional>
#include <memory>
#include <type_traits>
#include <utility>

#include "voidui/core/state.h"
#include "voidui/core/widget.h"

namespace voidui {

/// The non-templated component surface consumed by WidgetTree.
class ComponentBase : public Widget {
public:
  void register_children(Registrar &) final {}
  Size<float> layout(Constraints constraints, LayoutContext &ctx) final;
  void draw(const DrawContext &, Painter &) final {}
  EventResult on_event(Event &) final { return EventResult::Unhandled; }

  std::unique_ptr<Widget> render(detail::ComponentRuntime &runtime) {
    detail::ComponentRenderScope scope(runtime);
    return render_component();
  }

protected:
  virtual std::unique_ptr<Widget> render_component() = 0;
};

template <class Builder> class Component final : public ComponentBase {
public:
  explicit Component(Builder builder) : builder_(std::move(builder)) {}

  std::unique_ptr<Widget> clone() const override {
    return std::make_unique<Component>(builder_);
  }

protected:
  std::unique_ptr<Widget> render_component() override {
    auto declaration = std::invoke(builder_);
    return transfer_widget(std::move(declaration));
  }

private:
  Builder builder_;
};

template <class Builder>
  requires WidgetClass<std::invoke_result_t<std::decay_t<Builder> &>> &&
           std::copy_constructible<std::decay_t<Builder>>
[[nodiscard]] auto component(Builder &&builder) {
  using StoredBuilder = std::decay_t<Builder>;
  return Component<StoredBuilder>(std::forward<Builder>(builder));
}

} // namespace voidui
