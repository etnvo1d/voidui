#include "voidui/core/component.h"
#include "voidui/core/window.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/sidebar.h"

using namespace voidui;

int main() {
  Window window("VoidUI / universal sidebar", 1080, 720);
  window.run(component([] {
    auto open = use_state(true);
    auto extent = use_state(260.0f);
    auto edge = use_state(SidebarPlacement::Left);
    auto elastic = use_state(true);
    auto overlay = use_state(false);
    auto rail = use_state(false);
    auto placement_button = [edge](const char *label, SidebarPlacement value) {
      return button(label).on_click([edge, value] { edge.set(value); });
    };
    auto panel = column(
        text("WORKSPACE").font_size(14),
        text("Team HQ").font_size(28),
        button("Overview"), button("Projects"), button("Documents"),
        text("Drag the panel edge.\nShort pulls keep the size."),
        button("Close panel").on_click([open] { open.set(false); }))
        .padding(24).gap(16).background(Color(229, 241, 241));
    auto main = column(
        text("A sidebar on any edge").font_size(30),
        text("Pull past 48 px to reveal or resize. Pull inward past the minimum to collapse."),
        text("After opening, catch up to the new edge to widen. Reverse to shrink."),
        row(placement_button("Left", SidebarPlacement::Left),
            placement_button("Right", SidebarPlacement::Right),
            placement_button("Top", SidebarPlacement::Top),
            placement_button("Bottom", SidebarPlacement::Bottom)).gap(8),
        row(button(open.get() ? "Close" : "Open").on_click([open] { open.set(!open.get()); }),
            button(elastic.get() ? "Drag: elastic" : "Drag: immediate")
                .on_click([elastic] { elastic.set(!elastic.get()); })).gap(8),
        row(button(overlay.get() ? "Layout: overlay" : "Layout: docked")
                .on_click([overlay] { overlay.set(!overlay.get()); }),
            button(rail.get() ? "Closed: 56 px rail" : "Closed: edge only")
                .on_click([rail] { rail.set(!rail.get()); })).gap(8),
        text("Expanded size: " + std::to_string(static_cast<int>(extent.get())) + " px"),
        text("Keyboard: Tab to the edge. Arrows resize; Enter toggles; Escape cancels a drag."))
        .padding(32).gap(20).background(Color(248, 250, 252));
    return sidebar(scrollable(std::move(panel)).background(Color(229, 241, 241)),
                   scrollable(std::move(main)).background(Color(248, 250, 252)))
        .placement(edge.get()).open(open).extent(extent)
        .limits(150, 480).collapsed_extent(rail.get() ? 56.0f : 0.0f)
        .mode(overlay.get() ? SidebarMode::Overlay : SidebarMode::Docked)
        .drag_mode(elastic.get() ? SidebarDragMode::Elastic : SidebarDragMode::Immediate)
        .drag_threshold(48).collapse_threshold(40)
#ifndef NDEBUG
        .edge_visible(true)
#endif
        .handle_color(Color(30, 165, 165));
  }));
}
