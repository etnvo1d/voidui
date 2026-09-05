#include "voidui/widgets/select.h"
#include "voidui/core/component.h"
#include "voidui/core/window.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"

using namespace voidui;
int main() {
  Window window("VoidUI / Select", 900, 680);
  window.watch_styles("examples/select.vss");
  window.run(component([] {
    auto region = use_state(std::string("asia"));
    const std::vector<SelectOption> regions = {
        {"asia", "Asia Pacific"},      {"europe", "Europe"},
        {"us", "United States"},       {"ca", "Canada"},
        {"sa", "South America", true}, {"af", "Africa"},
        {"me", "Middle East"},         {"oceania", "Oceania"}};
    std::vector<SelectOption> teams;
    for (int i = 1; i <= 40; ++i)
      teams.push_back({std::to_string(i), "Workspace " + std::to_string(i)});
    return column(
               text("COMPONENTS / FORMS")
                   .font_size(12)
                   .color(Color(100, 116, 139)),
               text("A place for every choice")
                   .font_size(32)
                   .font_weight(FontWeight::Bold),
               text("A single select with a clear focus, keyboard control, and "
                    "room to open.")
                   .color(Color(100, 116, 139)),
               row(column(text("Deployment region").font_size(14),
                          select(regions, region).width(330),
                          text("Selected: " + region.get())
                              .font_size(12)
                              .color(Color(100, 116, 139)))
                       .gap(10),
                   column(
                       text("Workspace").font_size(14),
                       select(teams)
                           .placeholder("Choose a workspace")
                           .width(330)
                           .picker_max_height(230),
                       text("40 options. Type to jump, or use the arrow keys.")
                           .font_size(12)
                           .color(Color(100, 116, 139)))
                       .gap(10))
                   .gap(30)
                   .margin({12, 0}),
               column(
                   text("Inside a scrollable viewport")
                       .font_size(16)
                       .font_weight(FontWeight::Bold),
                   text("Open the field below. The menu extends beyond this "
                        "container.")
                       .font_size(13)
                       .color(Color(100, 116, 139)),
                   scrollable(column(row(select(regions, region).width(310),
                                         text("Scroll area / 80 px tall")
                                             .font_size(12)
                                             .color(Color(100, 116, 139)))
                                         .gap(20),
                                     text("This content scrolls; the dropdown "
                                          "stays in the overlay layer."),
                                     column().height(140))
                                  .gap(20))
                       .height(80)
                       .width(690)
                       .add_class("viewport"))
                   .gap(12)
                   .padding(22)
                   .add_class("card"),
               row(column(text("Unavailable").font_size(13),
                          select(regions).value("europe").disabled(true).width(
                              210))
                       .gap(10),
                   column(text("Minimal appearance").font_size(13),
                          select(regions)
                              .value("us")
                              .appearance(SelectAppearance::None)
                              .width(210))
                       .gap(10),
                   column(
                       text("Custom accent").font_size(13),
                       select(regions).value("ca").width(210).add_class("teal"))
                       .gap(10))
                   .gap(30),
               text("Tab to focus   /   Space to open   /   Enter to choose   "
                    "/   Esc to cancel")
                   .font_size(12)
                   .color(Color(100, 116, 139)),
               select(regions)
                   .value("asia")
                   .width(220)
                   .position(Position::Fixed)
                   .right(30)
                   .bottom(20))
        .gap(18)
        .padding(36)
        .size({Length::Fill{}, Length::Fill{}})
        .add_class("page");
  }));
}
