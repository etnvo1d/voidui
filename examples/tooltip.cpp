#include "voidui/widgets/tooltip.h"
#include "voidui/core/window.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"

using namespace voidui;
using namespace std::chrono_literals;

int main() {
  Window window("VoidUI overlay tooltips", 720, 440);
  window.set_stylesheet(StyleParser::parse(R"vss(
    .page { padding: 32px; background: #f4f5f7; }
    .viewport { background: white; border-width: 1px;
                border-color: #cbd5e1; border-radius: 8px; padding: 12px; }
    .accent::part(bubble) { background: #1d4ed8; }
  )vss",
                                           "tooltip.vss")
                            .sheet);

  auto view =
      column(
          text("Tooltips escape the scroll viewport"),
          text("Hover for 500 ms. Scroll or press Escape to dismiss."),
          scrollable(
              column(
                  row(tooltip(button("Save"), "Save the current document"),
                      tooltip(button("Share"),
                              "Create a link for your teammates")
                          .placement(OverlayPlacement::Right)
                          .add_class("accent"))
                      .gap(20),
                  tooltip(input().placeholder("Click to focus"),
                          "Focus also reveals a tooltip immediately."),
                  tooltip(
                      button("More information"),
                      "Long explanations wrap to the configured maximum width.")
                      .max_width(220)
                      .delay(250ms),
                  text("Scroll down to move the anchors out of view."),
                  column().height(180))
                  .gap(24))
              .height(140)
              .width(420)
              .add_class("viewport"),
          text("The bubble remains above this content."),
          tooltip(
              button("Near the window edge"),
              "This prefers the bottom, but flips up when space is limited.")
              .placement(OverlayPlacement::Bottom)
              .position(Position::Fixed)
              .right(16)
              .bottom(16))
          .gap(18)
          .add_class("page")
          .size({Length::Fill{}, Length::Fill{}});
  window.run(view);
}
