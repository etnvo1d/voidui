#include "voidui/widgets/modal.h"
#include "voidui/core/component.h"
#include "voidui/core/window.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/tooltip.h"

using namespace voidui;

std::unique_ptr<Widget> dialog_level(int depth, State<bool> showing) {
  return transfer_widget(component([depth, showing] {
    auto child_open = use_state(false);
    auto menu_open = use_state(false);
    auto content =
        column(
            text("Dialog " + std::to_string(depth)),
            text("Tab stays here. Escape closes the topmost popup."),
            input().placeholder(
                "Focus returns here after closing the next dialog"),
            scrollable(column(tooltip(button("Hover for a tooltip"),
                                      "This tooltip belongs to dialog " +
                                          std::to_string(depth)),
                              column().height(140))
                           .gap(16))
                .height(70),
            column(button("Open menu").on_click([menu_open] {
              menu_open.set(true);
            }),
                   overlay(column(button("First item").on_click([menu_open] {
                             menu_open.set(false);
                           }),
                                  button("Second item").on_click([menu_open] {
                                    menu_open.set(false);
                                  }))
                               .gap(4))
                       .open(menu_open)
                       .close_on_escape(true)
                       .close_on_outside_press(true)
                       .background(Color(240, 245, 255))
                       .padding(8)),
            row(button("Open next dialog").on_click([child_open] {
              child_open.set(true);
            }),
                button("Close").on_click([showing] { showing.set(false); }))
                .gap(12))
            .gap(18);
    if (child_open.get())
      content.add(dialog_level(depth + 1, child_open));
    return modal(std::move(content))
        .open(showing)
        .width(520)
        .close_on_outside_press(true);
  }));
}

int main() {
  Window window("VoidUI nested modals", 820, 640);
  auto view = component([] {
    auto showing = use_state(false);
    auto count = use_state(0);
    auto page =
        column(text("Nested modals, menus and tooltips"),
               text("Each dialog can open another dialog. No z-index "
                    "configuration."),
               tooltip(button("Open dialog").on_click([showing] {
                 showing.set(true);
               }),
                       "Open the first modal dialog"),
               button("Background counter: " + std::to_string(count.get()))
                   .on_click([count] { count.set(count.get() + 1); }))
            .gap(20)
            .padding(32)
            .background(Color(244, 245, 247))
            .size({Length::Fill{}, Length::Fill{}});
    if (showing.get())
      page.add(dialog_level(1, showing));
    return page;
  });
  window.run(std::move(view));
}
