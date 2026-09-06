#include "voidui/widgets/table.h"

#include <string>
#include <vector>

#include "voidui/core/component.h"
#include "voidui/core/window.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/text.h"

using namespace voidui;

namespace {

struct Release {
  const char *name;
  const char *owner;
  const char *status;
  /// A class name, because a label with a space in it is not one.
  const char *state;
  const char *coverage;
  const char *shipped;
};

const std::vector<Release> kReleases = {
    {"voidui-core", "Ada", "Shipped", "shipped", "98%", "2026-08-14"},
    {"style-resolver", "Lin", "In review", "review", "94%", "2026-08-21"},
    {"text-shaping", "Rui", "Shipped", "shipped", "91%", "2026-08-28"},
    {"table-layout", "Nour", "In progress", "progress", "72%", "—"},
    {"gpu-renderer", "Ada", "Blocked", "blocked", "88%", "—"},
    {"resource-layer", "Jae", "Shipped", "shipped", "96%", "2026-09-01"},
};

/// A striped, collapsed-border table -- the shape most application tables
/// take. Every column but the first is sized by its content; the first takes
/// whatever is left, through `width: fill` on its `col`.
Table release_table() {
  auto grid =
      table(colgroup(col().width(Length::Fill{}).add_class("name-column"),
                     col(), col(), col(), col()),
            thead(tr(th("Package"), th("Owner"), th("Status"),
                     th("Coverage").add_class("numeric"),
                     th("Shipped").add_class("numeric"))))
          .add_class("releases")
          .width(Length::Fill{});

  auto body = tbody();
  for (const Release &release : kReleases) {
    body.add(tr(td(release.name).add_class("name"), td(release.owner),
                td(text(release.status)).add_class("status"),
                td(release.coverage).add_class("numeric"),
                td(release.shipped).add_class("numeric"))
                .add_class(release.state));
  }
  grid.add(std::move(body));
  grid.add(tfoot(tr(td("6 packages").colspan(3),
                    td("90% mean").add_class("numeric"),
                    td("3 shipped").add_class("numeric"))));
  return grid;
}

/// The other half of the vocabulary: separate borders with `border-spacing`,
/// a caption underneath, spans, and per-cell vertical alignment.
Table matrix_table() {
  return table(
             caption("Plan comparison — billed monthly"),
             thead(tr(th(), th("Free"), th("Team"), th("Enterprise"))),
             tbody(tr(th("Seats"), td("1"), td("Up to 25"), td("Unlimited")),
                   tr(th("Storage"), td("2 GB"), td("200 GB"), td("Custom")),
                   tr(th("Support"),
                      td("Community").colspan(2).add_class("merged"),
                      td("24/7, named contact")),
                   tr(th("Audit log").rowspan(2).add_class("tall"),
                      td("—"), td("30 days"), td("7 years")),
                   tr(td("—"), td("CSV export"), td("Streaming"))))
      .add_class("plans")
      .border_spacing(BorderSpacing(8.0f, 6.0f))
      .caption_side(CaptionSide::Bottom);
}

/// The same data through the fluent builder, with a fixed layout so the
/// columns hold their width whatever lands in them.
Table fixed_table() {
  return table()
      .headers({"Key", "Value"})
      .row("border-collapse", "collapse | separate")
      .row("border-spacing", "<h> <v>")
      .row("table-layout", "auto | fixed")
      .row("caption-side", "top | bottom")
      .row("empty-cells", "show | hide")
      .row("vertical-align", "baseline | top | middle | bottom")
      .add_class("reference")
      .table_layout(TableLayout::Fixed)
      .width(Length::Fill{});
}

} // namespace

int main() {
  Window window("VoidUI / Table", 1040, 860);
  window.watch_styles("examples/table.vss");
  window.run(component([] {
    return scrollable(
               column(text("COMPONENTS / DATA")
                          .font_size(12)
                          .color(Color(100, 116, 139)),
                      text("Tables, the way CSS spells them")
                          .font_size(32)
                          .font_weight(FontWeight::Bold),
                      text("thead, tbody, tfoot, tr, th, td, caption, "
                           "colgroup and col -- styled from VSS, laid out as "
                           "one grid.")
                          .color(Color(100, 116, 139)),
                      release_table(),
                      row(matrix_table(), fixed_table()).gap(28),
                      text("Hover a row. Stripes are :nth-child(even); the "
                           "status colour is a class on the row.")
                          .font_size(12)
                          .color(Color(100, 116, 139)))
                   .gap(20)
                   .padding(36)
                   .width(Length::Fill{})
                   .add_class("page"))
        .size({Length::Fill{}, Length::Fill{}});
  }));
}
