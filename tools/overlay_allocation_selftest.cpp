#include "voidui/core/widget_tree.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/modal.h"
#include "voidui/widgets/text.h"
#include <cstdio>
#include <cstdlib>
#include <new>

static bool count_allocations = false;
static std::size_t allocations = 0;
void *operator new(std::size_t size) {
  if (count_allocations)
    ++allocations;
  if (void *ptr = std::malloc(size ? size : 1))
    return ptr;
  throw std::bad_alloc();
}
void operator delete(void *ptr) noexcept { std::free(ptr); }
void operator delete(void *ptr, std::size_t) noexcept { std::free(ptr); }

int main() {
  using namespace voidui;
  auto child = transfer_widget(modal(text("deep")).open(true));
  for (int i = 0; i < 31; ++i) {
    auto content = column(text("layer"));
    content.add(std::move(child));
    child = transfer_widget(modal(std::move(content)).open(true));
  }
  WidgetTree tree(std::move(child));
  DisplayList list;
  Painter painter(list, {800, 600});
  tree.advance_animations(1);
  for (int i = 0; i < 4; ++i) {
    if (tree.needs_layout())
      tree.layout({800, 600});
    list.clear();
    tree.render(painter);
  }
  count_allocations = true;
  for (int i = 0; i < 100; ++i) {
    list.clear();
    tree.render(painter);
  }
  count_allocations = false;
  std::printf("32 stacked modals, 100 steady frames: %zu allocations\n",
              allocations);
  if (allocations != 0 || tree.needs_paint())
    return 1;
  for (int i = 0; i < 32; ++i) {
    KeyPressedEvent escape(Keycode::Escape);
    tree.process_event(escape);
  }
  list.clear();
  tree.render(painter);
  std::printf("after closing all layers: %zu commands\n",
              list.commands().size());
  return list.commands().empty() ? 0 : 1;
}
