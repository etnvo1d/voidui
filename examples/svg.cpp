// Exercises the SVG path end to end.
//
// The three ideas worth watching for:
//
//   * The icons in the top row are *one* file each, drawn at four sizes. The
//     document is parsed once and is size-independent, so the only thing that
//     changes between them is the painter's transform.
//   * Every icon in the second row is the *same* markup. What differs is the
//     VSS rule matching it: `fill`, `stroke`, `stroke-width` and friends are
//     ordinary inherited CSS properties, so a document that states none of them
//     takes them from whatever the cascade says -- including on `:hover`, and
//     including through a `transition`.
//   * The third row is what the parser has to get right: arcs, a gradient
//     defined once and referenced twice, nested transforms, dash patterns, and
//     `fill-rule` on a self-intersecting outline.

#include "voidui/core/window.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/svg.h"
#include "voidui/widgets/text.h"

using namespace voidui;

namespace {

/// A stroked icon that states nothing about colour. Every presentation
/// property it leaves out is one the stylesheet gets to decide, which is what
/// makes the second row possible from a single string.
const char *kBell = R"SVG(
<svg viewBox="0 0 24 24" fill="none" stroke-linecap="round"
     stroke-linejoin="round">
  <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/>
  <path d="M13.7 21a2 2 0 0 1-3.4 0"/>
</svg>
)SVG";

/// Two tones stated by the file itself, so the cascade must leave them alone.
const char *kFolder = R"SVG(
<svg viewBox="0 0 24 24">
  <path fill="#f4b942" d="M2 6a2 2 0 0 1 2-2h5l2 3h7a2 2 0 0 1 2 2v9a2 2 0 0
       1-2 2H4a2 2 0 0 1-2-2z"/>
  <path fill="#ffffff" fill-opacity="0.35" d="M2 10h20v3H2z"/>
</svg>
)SVG";

/// Everything the geometry side has to handle at once.
const char *kShowcase = R"SVG(
<svg viewBox="0 0 120 120">
  <defs>
    <linearGradient id="sky" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#38bdf8"/>
      <stop offset="1" stop-color="#a855f7"/>
    </linearGradient>
    <linearGradient id="fade" xlink:href="#sky" x1="0" y1="0" x2="1" y2="0"/>
    <clipPath id="frame"><rect x="4" y="4" width="112" height="112"/></clipPath>
  </defs>

  <g clip-path="url(#frame)">
    <rect x="6" y="6" width="48" height="34" rx="8" fill="url(#sky)"/>
    <rect x="62" y="6" width="52" height="34" rx="17" fill="url(#fade)"/>

    <!-- An arc sweep, which the parser turns into cubics. -->
    <path d="M8 62 A22 22 0 0 1 52 62" fill="none" stroke="#111827"
          stroke-width="4" stroke-linecap="round"/>

    <!-- A self-intersecting star, filled even-odd so the middle drops out. -->
    <path fill-rule="evenodd" fill="#f43f5e"
          d="M88 44 L98 76 L70 56 L106 56 L78 76 Z"/>

    <!-- Nested transforms, baked into the geometry at parse time. -->
    <g transform="translate(10 86) rotate(-8)">
      <g transform="scale(1.4 1.4)">
        <rect width="24" height="16" rx="3" fill="#111827"/>
      </g>
    </g>

    <!-- A dashed outline, cut into geometry once rather than every frame. -->
    <path d="M62 92 h50" stroke="#111827" stroke-width="5"
          stroke-dasharray="10 6" stroke-linecap="round" fill="none"/>
  </g>
</svg>
)SVG";

const char *kStyleSheet = R"SVG(
column, row { background: #f8fafc; }

.title {
  color: #0f172a;
  font-size: 13;
}

/* The bell states no colours at all, so these five properties reach it
   through the ordinary cascade -- and animate through the ordinary
   transition machinery. */
.bell {
  width: 44px;
  height: 44px;
  stroke: #64748b;
  stroke-width: 1.8;
  fill: none;
  transition: stroke 160ms ease, stroke-width 160ms ease;
}

.bell:hover {
  stroke: #2563eb;
  stroke-width: 2.6;
}

/* `fill: currentColor` in a document binds to whatever `color` resolves to,
   which is how one file serves a whole palette. */
.accent  { stroke: #16a34a; }
.warn    { stroke: #f97316; }
.danger  { stroke: #e11d48; stroke-dasharray: 3 2; }

/* The folder states its own two tones, so nothing here reaches them. */
.folder { width: 44px; height: 44px; fill: #e11d48; }

.showcase { width: 240px; height: 240px; }

.icon-16 { width: 16px;  height: 16px;  stroke: #334155; fill: none; }
.icon-24 { width: 24px;  height: 24px;  stroke: #334155; fill: none; }
.icon-48 { width: 48px;  height: 48px;  stroke: #334155; fill: none; }
.icon-96 { width: 96px;  height: 96px;  stroke: #334155; fill: none; }
)SVG";

Text label(const char *what) { return text(what).add_class("title"); }

} // namespace

int main() {
  Window window("VoidUI SVG", 720, 560);

  auto sheet = StyleParser::parse(kStyleSheet, "svg-example.vss");
  window.set_stylesheet(sheet.sheet);

  // One document, four sizes. The parse, the paths and -- because the renderer
  // keys coverage on the outline rather than on where it sits -- most of the
  // rasterisation are shared.
  auto sizes = row(svg_markup(kBell).add_class("icon-16"),
                   svg_markup(kBell).add_class("icon-24"),
                   svg_markup(kBell).add_class("icon-48"),
                   svg_markup(kBell).add_class("icon-96"))
                   .gap(16.0f);

  // One document, five styles. Hover any of them.
  auto tinted = row(svg_markup(kBell).add_class("bell"),
                    svg_markup(kBell).add_class("bell").add_class("accent"),
                    svg_markup(kBell).add_class("bell").add_class("warn"),
                    svg_markup(kBell).add_class("bell").add_class("danger"),
                    svg_markup(kFolder).add_class("folder"))
                    .gap(16.0f);

  auto view = column(label("one document, four sizes"), std::move(sizes),
                     label("one document, five stylesheet rules (hover me)"),
                     std::move(tinted),
                     label("arcs, gradients, clips, transforms, dashes"),
                     svg_markup(kShowcase).add_class("showcase"))
                  .gap(12.0f)
                  .padding(Padding(20.0f));

  window.run(view);
  return 0;
}
