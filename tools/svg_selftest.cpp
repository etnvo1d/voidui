// End-to-end exercise of the SVG path: the VSS presentation properties, the
// document parser (XML, the `d` grammar, transforms, gradients, clips), the
// document cache, and the merge of "what the file said" with "what the
// stylesheet said" that makes one icon file take its colour from its
// surroundings.
//
// Links only voidui and asserts its way through every behaviour the design
// promises, so a regression shows up as a named failing line.

#include "voidui/core/style.h"
#include "voidui/paint/painter.h"
#include "voidui/paint/svg.h"
#include "voidui/paint/svg_cache.h"
#include "voidui/widgets/svg.h"

#include <cmath>
#include <cstdio>
#include <memory>
#include <string>

using namespace voidui;

namespace {

int failures = 0;

void check(bool condition, const char *what) {
  if (!condition) {
    ++failures;
    std::printf("  FAIL  %s\n", what);
  } else {
    std::printf("  ok    %s\n", what);
  }
}

bool near(float a, float b, float tolerance = 1e-3f) {
  return std::abs(a - b) <= tolerance;
}

bool same_color(Color a, Color b) {
  return near(a.r, b.r) && near(a.g, b.g) && near(a.b, b.b) && near(a.a, b.a);
}

std::shared_ptr<const SvgDocument> parse(const char *source) {
  SvgDocument::Result result = SvgDocument::parse(source);
  return result.document;
}

/// The point a path visits at `index`, for checking geometry without depending
/// on how many verbs produced it.
Point<float> point_at(const Path &path, std::size_t index) {
  return index < path.points().size() ? path.points()[index]
                                      : Point<float>(0.0f, 0.0f);
}

ComputedStyle finalized(std::initializer_list<
                        std::pair<PropertyIndex, PropertyValue>>
                            values) {
  ComputedStyle style;
  for (const auto &entry : values)
    style.set(entry.first, entry.second);
  style.finalize();
  return style;
}

} // namespace

int main() {
  std::printf("-- VSS reads the SVG presentation properties\n");
  {
    const StyleParser::Result parsed = StyleParser::parse(
        ".icon {"
        "  fill: currentColor;"
        "  stroke: #ff0000;"
        "  stroke-width: 1.5;"
        "  stroke-linecap: round;"
        "  stroke-linejoin: bevel;"
        "  stroke-miterlimit: 8;"
        "  stroke-dasharray: 4 2;"
        "  stroke-dashoffset: 1;"
        "  fill-rule: evenodd;"
        "  fill-opacity: 0.5;"
        "  stroke-opacity: 0.25;"
        "  paint-order: stroke fill;"
        "  vector-effect: non-scaling-stroke;"
        "}",
        "icons.vss");
    check(parsed.diagnostics.empty(),
          "every SVG presentation property is a known VSS property");
    check(parsed.sheet && parsed.sheet->size() == 1, "the rule survives");
  }

  {
    // `transition` resolves the property names it mentions against the
    // registry, so this is a separate path from reading a declaration.
    const StyleParser::Result parsed = StyleParser::parse(
        ".icon { transition: fill 120ms ease, stroke-width 80ms linear; }",
        "motion.vss");
    check(parsed.diagnostics.empty(),
          "an SVG property can be named in a transition shorthand");
  }

  {
    SvgPaint paint;
    check(parse_style_value("none", paint) && paint.is_none(),
          "fill: none parses");
    check(parse_style_value("currentColor", paint) &&
              paint.kind() == SvgPaint::Kind::CurrentColor,
          "currentColor is case-insensitive, as CSS keywords are");
    check(parse_style_value("rebeccapurple", paint) &&
              paint.kind() == SvgPaint::Kind::Solid,
          "a named colour is a paint");
    check(!parse_style_value("url(#grad)", paint),
          "a url() paint in a stylesheet is a diagnostic, not a black shape");

    SvgDashArray dashes;
    check(parse_style_value("4, 2", dashes) && dashes.count == 2 &&
              near(dashes.lengths[0], 4.0f) && near(dashes.lengths[1], 2.0f),
          "a dash list splits on commas and whitespace alike");
    check(parse_style_value("none", dashes) && dashes.empty(),
          "stroke-dasharray: none is an empty pattern");
    check(!parse_style_value("1 2 3 4 5 6 7", dashes),
          "a pattern past the inline limit is refused, not truncated");

    SvgPaintOrder order = SvgPaintOrder::FillStroke;
    check(parse_style_value("stroke fill", order) &&
              order == SvgPaintOrder::StrokeFill,
          "paint-order reads the order of fill and stroke");
    check(parse_style_value("markers fill stroke", order) &&
              order == SvgPaintOrder::FillStroke,
          "markers are accepted and ignored rather than failing the rule");
    check(parse_style_value("normal", order) &&
              order == SvgPaintOrder::FillStroke,
          "normal is fill then stroke");
  }

  {
    // Inheritance is what lets `fill` be set once on a container.
    const PropertyRegistry &registry = PropertyRegistry::instance();
    check(registry.describe(styles::Fill::index()).inherited,
          "fill inherits, as SVG says");
    check(registry.describe(styles::StrokeWidth::index()).inherited,
          "stroke-width inherits");
    check(!registry.describe(styles::VectorEffect::index()).inherited,
          "vector-effect does not inherit");
  }

  {
    SvgPaint from = SvgPaint::solid(Color(0, 0, 0));
    SvgPaint to = SvgPaint::solid(Color(255, 255, 255));
    SvgPaint out;
    check(interpolate_style_value(from, to, 0.5f, out) &&
              out.kind() == SvgPaint::Kind::Solid,
          "two solid paints interpolate, so `transition: fill` moves");
    check(!interpolate_style_value(SvgPaint::none(), to, 0.5f, out),
          "there is nothing to interpolate between none and a colour");
  }

  std::printf("\n-- the path grammar\n");
  {
    const std::shared_ptr<const SvgDocument> document = parse(
        "<svg viewBox='0 0 10 10'><path d='M1 2 l3 0 H8 V6 z'/></svg>");
    check(document && document->shapes().size() == 1, "one shape");
    const Path &path = *document->path(document->shapes()[0].path);
    check(path.verbs().size() == 5, "M, l, H, V and z are five verbs");
    check(near(point_at(path, 1).x, 4.0f) && near(point_at(path, 1).y, 2.0f),
          "a relative lineto is relative to the current point");
    check(near(point_at(path, 2).x, 8.0f) && near(point_at(path, 2).y, 2.0f),
          "H keeps y");
    check(near(point_at(path, 3).x, 8.0f) && near(point_at(path, 3).y, 6.0f),
          "V keeps x");
  }

  {
    // A repeated moveto argument is an implicit lineto -- the single most
    // common thing a minifier emits.
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg viewBox='0 0 10 10'><path d='M0 0 1 1 2 2'/></svg>");
    const Path &path = *document->path(document->shapes()[0].path);
    check(path.verbs().size() == 3 && path.verbs()[1] == PathVerb::Line,
          "a second moveto pair is a lineto");
  }

  {
    // `.5.5` is two numbers, and a sign starts one without a separator.
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg viewBox='0 0 10 10'><path d='M.5.5L1-1'/></svg>");
    const Path &path = *document->path(document->shapes()[0].path);
    check(near(point_at(path, 0).x, 0.5f) && near(point_at(path, 0).y, 0.5f),
          "two decimals run together are two numbers");
    check(near(point_at(path, 1).x, 1.0f) && near(point_at(path, 1).y, -1.0f),
          "a minus sign separates as well as negates");
  }

  {
    // A half-turn arc of radius 5 from (0,5) to (10,5) must reach (5,0) at the
    // top, whatever the cubics it was cut into.
    const std::shared_ptr<const SvgDocument> document = parse(
        "<svg viewBox='0 0 10 10'><path d='M0 5 A5 5 0 0 1 10 5'/></svg>");
    const Path &path = *document->path(document->shapes()[0].path);
    check(path.verbs().size() >= 3, "an arc becomes at least two cubics");
    const Rect<float> bounds = path.bounds();
    check(near(bounds.origin.y, 0.0f, 0.05f),
          "the sweep reaches the top of the circle");
    check(near(bounds.origin.x + bounds.size.width, 10.0f, 0.05f) &&
              near(bounds.origin.x, 0.0f, 0.05f),
          "and spans the full chord");
  }

  {
    const std::shared_ptr<const SvgDocument> document = parse(
        "<svg viewBox='0 0 20 20'>"
        "<rect x='1' y='1' width='4' height='4'/>"
        "<circle cx='10' cy='10' r='3'/>"
        "<ellipse cx='10' cy='10' rx='4' ry='2'/>"
        "<line x1='0' y1='0' x2='5' y2='5' stroke='#000'/>"
        "<polyline points='0,0 1,1 2,0' stroke='#000' fill='none'/>"
        "<polygon points='0,0 4,0 4,4'/>"
        "</svg>");
    check(document && document->shapes().size() == 6,
          "every basic shape element builds geometry");
  }

  std::printf("\n-- transforms are baked into the geometry\n");
  {
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg viewBox='0 0 20 20'>"
              "<g transform='translate(10 0) scale(2)'>"
              "<path d='M1 1 L2 2' stroke='#000' stroke-width='3'/>"
              "</g></svg>");
    const SvgShape &shape = document->shapes()[0];
    const Path &path = *document->path(shape.path);
    check(near(point_at(path, 0).x, 12.0f) && near(point_at(path, 0).y, 2.0f),
          "a group's transform is applied to the points once, at parse time");
    check(near(shape.stroke_scale, 2.0f),
          "and the scale it carried is remembered for the pen");
  }

  {
    // The same outline built twice must hash the same, or the renderer keeps
    // two mask entries for one shape.
    Path built;
    built.move_to(Point<float>(1.0f, 2.0f));
    built.line_to(Point<float>(3.0f, 4.0f));
    built.close();

    Path again;
    again.move_to(Point<float>(1.0f, 2.0f));
    again.line_to(Point<float>(3.0f, 4.0f));
    again.close();
    check(built.content_hash() == again.content_hash(),
          "an outline built twice hashes the same");

    Path moved = built;
    moved.apply_transform(Transform::translate(5.0f, 0.0f));
    check(moved.content_hash() != built.content_hash(),
          "and a transformed one does not");

    Path directly;
    directly.move_to(Point<float>(6.0f, 2.0f));
    directly.line_to(Point<float>(8.0f, 4.0f));
    directly.close();
    check(directly.content_hash() == moved.content_hash(),
          "a rehash after baking agrees with the same outline built by hand");
  }

  std::printf("\n-- the viewport\n");
  {
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg width='48' height='24' viewBox='-2 -2 24 12'"
              " preserveAspectRatio='xMinYMax slice'><rect width='1'"
              " height='1'/></svg>");
    check(near(document->intrinsic_size().width, 48.0f) &&
              near(document->intrinsic_size().height, 24.0f),
          "width and height are the intrinsic size");
    check(near(document->view_box().origin.x, -2.0f) &&
              near(document->view_box().size.width, 24.0f),
          "the view box is read including its origin");
    check(near(document->preserve_aspect_ratio().align_x, 0.0f) &&
              near(document->preserve_aspect_ratio().align_y, 1.0f) &&
              document->preserve_aspect_ratio().slice,
          "xMinYMax slice decodes into two alignments and a fit");
  }

  {
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg width='32' height='32'><rect width='4' height='4'/></svg>");
    check(near(document->view_box().size.width, 32.0f),
          "a file with no view box takes one from its stated size");
  }

  std::printf("\n-- presentation attributes and what they leave open\n");
  {
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg viewBox='0 0 10 10'>"
              "<g fill='#ff0000' opacity='0.5'>"
              "<path d='M0 0 L1 1'/>"
              "<path d='M1 1 L2 2' style='fill:#00ff00;fill-opacity:0.25'/>"
              "</g>"
              "<path d='M2 2 L3 3'/>"
              "</svg>");
    check(document->shapes().size() == 3, "three shapes");

    const SvgShapeStyle &inherited = document->shapes()[0].style;
    check((inherited.specified & kSvgFillSet) != 0 &&
              same_color(inherited.fill.color(), Color(255, 0, 0)),
          "a group's fill reaches its children and counts as stated");
    check(near(inherited.opacity, 0.5f),
          "a group's opacity multiplies into its children");

    const SvgShapeStyle &overridden = document->shapes()[1].style;
    check(same_color(overridden.fill.color(), Color(0, 255, 0)),
          "an inline style wins over the inherited value");
    check(near(overridden.fill_opacity, 0.25f), "and brings its own opacity");

    const SvgShapeStyle &open = document->shapes()[2].style;
    check((open.specified & kSvgFillSet) == 0,
          "a shape outside the group left its fill open for the cascade");
  }

  {
    // The `style` attribute beats the matching presentation attribute, which is
    // what CSS says and what every exporter relies on.
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg viewBox='0 0 10 10'>"
              "<path d='M0 0 L1 1' fill='#ff0000' style='fill:#0000ff'/>"
              "</svg>");
    check(same_color(document->shapes()[0].style.fill.color(),
                     Color(0, 0, 255)),
          "style= wins over the presentation attribute");
  }

  {
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg viewBox='0 0 10 10'>"
              "<path d='M0 0 L1 1' fill='none' stroke='none'/>"
              "<path d='M0 0 L1 1' fill='none'/>"
              "</svg>");
    check(document->shapes().size() == 1,
          "a shape the file declared entirely unpainted is dropped, and one "
          "that only said fill:none is kept for the cascade to stroke");
  }

  std::printf("\n-- gradients\n");
  {
    const std::shared_ptr<const SvgDocument> document = parse(
        "<svg viewBox='0 0 10 10'>"
        "<defs>"
        "<linearGradient id='base'>"
        "<stop offset='0' stop-color='#ff0000'/>"
        "<stop offset='100%' stop-color='#0000ff' stop-opacity='0.5'/>"
        "</linearGradient>"
        "<linearGradient id='tilted' xlink:href='#base' x1='0' y1='0' x2='0'"
        " y2='1'/>"
        "</defs>"
        "<rect width='10' height='10' fill='url(#tilted)'/>"
        "</svg>");
    check(document && document->shapes().size() == 1, "the shape survives");
    const SvgPaint &fill = document->shapes()[0].style.fill;
    check(fill.kind() == SvgPaint::Kind::Server,
          "url(#id) resolves to a paint server");

    const Brush &brush = document->brush(fill.server_index());
    const auto *gradient = std::get_if<LinearGradient>(&brush);
    check(gradient != nullptr, "and the server is a gradient brush");
    if (gradient) {
      check(gradient->stops().size() == 2,
            "the stops come from the referenced gradient");
      check(same_color(gradient->stops()[0].color, Color(255, 0, 0)),
            "first stop");
      check(near(gradient->stops()[1].color.a, 0.5f),
            "stop-opacity multiplies into the stop's alpha");
      check(near(gradient->end().y, 1.0f),
            "the referencing gradient's own geometry wins over the base's");
    }
  }

  {
    // A forward reference is legal: the file may use a gradient before it
    // defines one.
    const std::shared_ptr<const SvgDocument> document = parse(
        "<svg viewBox='0 0 10 10'>"
        "<rect width='10' height='10' fill='url(#late)'/>"
        "<defs><linearGradient id='late'><stop stop-color='#0f0'/>"
        "</linearGradient></defs>"
        "</svg>");
    check(document->shapes()[0].style.fill.kind() == SvgPaint::Kind::Server,
          "a gradient defined after its use still resolves");
  }

  std::printf("\n-- clip paths\n");
  {
    const std::shared_ptr<const SvgDocument> document = parse(
        "<svg viewBox='0 0 10 10'>"
        "<clipPath id='frame'><rect x='1' y='1' width='4' height='4'/>"
        "</clipPath>"
        "<g clip-path='url(#frame)'><rect width='10' height='10'/></g>"
        "</svg>");
    const SvgShape &shape = document->shapes()[0];
    check(shape.clip != kNoSvgIndex, "a rectangular clip is honoured");
    if (shape.clip != kNoSvgIndex) {
      const Rect<float> clip = document->clip(shape.clip);
      check(near(clip.origin.x, 1.0f) && near(clip.size.width, 4.0f),
            "and lands where the clipPath's rect did");
    }
  }

  {
    const std::shared_ptr<const SvgDocument> document = parse(
        "<svg viewBox='0 0 10 10'>"
        "<clipPath id='star'><circle cx='5' cy='5' r='3'/></clipPath>"
        "<g clip-path='url(#star)'><rect width='10' height='10'/></g>"
        "</svg>");
    check(document->shapes()[0].clip == kNoSvgIndex,
          "a clip the scissor cannot express is dropped, not guessed at");
  }

  std::printf("\n-- unsupported content is skipped, not fatal\n");
  {
    const std::shared_ptr<const SvgDocument> document = parse(
        "<?xml version='1.0'?>"
        "<!DOCTYPE svg PUBLIC '-//W3C//DTD SVG 1.1//EN' 'x.dtd'>"
        "<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 10 10'>"
        "<!-- a comment -->"
        "<title>An icon</title>"
        "<text x='0' y='0'>hello</text>"
        "<image href='x.png' width='1' height='1'/>"
        "<svg:rect width='4' height='4'/>"
        "</svg>");
    check(document != nullptr, "a prologue and a doctype do not stop the parse");
    check(document->shapes().size() == 1,
          "text and images are skipped; a namespaced rect still draws");
  }

  {
    SvgDocument::Result result = SvgDocument::parse("<html><body/></html>");
    check(!result && !result.error.empty(),
          "a document with no <svg> element reports why");
  }

  std::printf("\n-- dashing is geometry\n");
  {
    Path line;
    line.move_to(Point<float>(0.0f, 0.0f));
    line.line_to(Point<float>(10.0f, 0.0f));

    const float pattern[] = {2.0f, 2.0f};
    const Path dashed = dash_path(line, pattern, 0.0f, 0.01f);
    check(dashed.verbs().size() == 6,
          "a ten-unit line under a 2/2 pattern is three dashes");
    check(near(point_at(dashed, 1).x, 2.0f),
          "the first dash ends where the pattern says");
    check(near(point_at(dashed, 2).x, 4.0f),
          "and the second starts after the gap");

    const Path offset = dash_path(line, pattern, 2.0f, 0.01f);
    check(near(point_at(offset, 0).x, 2.0f),
          "stroke-dashoffset moves the pattern along the line");

    const float degenerate[] = {0.0f, 0.0f};
    check(dash_path(line, degenerate, 0.0f, 0.01f).verbs().size() ==
              line.verbs().size(),
          "a pattern that sums to nothing is no dashing at all");
  }

  std::printf("\n-- shared paths ride into the display list by reference\n");
  {
    auto shared = std::make_shared<Path>();
    shared->move_to(Point<float>(0.0f, 0.0f));
    shared->line_to(Point<float>(4.0f, 4.0f));
    shared->line_to(Point<float>(0.0f, 4.0f));
    shared->close();

    DisplayList list;
    Painter painter(list, Size<float>(100.0f, 100.0f));
    painter.fill_path(std::shared_ptr<const Path>(shared),
                      Paint(Color(0, 0, 0)));

    check(list.commands().size() == 1, "one command");
    check(&list.path(list.commands()[0].path_index) == shared.get(),
          "the display list points at the caller's path rather than a copy");
    check(shared.use_count() == 2, "and holds a reference to it");

    list.clear();
    check(shared.use_count() == 1, "which it lets go when the frame is over");
  }

  std::printf("\n-- the cache parses one document once\n");
  {
    const std::string markup =
        "<svg viewBox='0 0 8 8'><rect width='8' height='8'/></svg>";
    SvgHandle first = SvgCache::global().acquire(SvgSource::markup(markup));
    SvgHandle second = SvgCache::global().acquire(SvgSource::markup(markup));
    check(first.ready() && second.ready(),
          "markup the caller holds resolves without a round trip");
    check(first.document() == second.document(),
          "and the same markup is one document, not two");

    SvgHandle missing =
        SvgCache::global().acquire(SvgSource::markup("<not-svg/>"));
    check(missing.failed() && !missing.error().empty(),
          "a document that does not parse reports a failure with a reason");
  }

  std::printf("\n-- the document and the cascade meet\n");
  {
    const std::shared_ptr<const SvgDocument> document =
        parse("<svg viewBox='0 0 10 10'>"
              "<path d='M0 0 L1 1' fill='#ff0000'/>"
              "<path d='M0 0 L1 1'/>"
              "<path d='M0 0 L1 1' fill='currentColor'/>"
              "</svg>");

    const ComputedStyle style = finalized({
        {styles::Fill::index(), PropertyValue(SvgPaint::solid(Color(0, 0, 255)))},
        {styles::StrokeWidth::index(), PropertyValue(3.0f)},
        {styles::Foreground::index(), PropertyValue(Brush(Color(0, 255, 0)))},
    });

    const SvgResolvedStyle stated =
        resolve_svg_style(document->shapes()[0].style, *document, style);
    check(same_color(stated.fill.color(), Color(255, 0, 0)),
          "a fill the file stated is not overridden by the stylesheet");

    const SvgResolvedStyle open =
        resolve_svg_style(document->shapes()[1].style, *document, style);
    check(same_color(open.fill.color(), Color(0, 0, 255)),
          "a fill the file left open comes from the cascade");
    check(near(open.stroke_width, 3.0f),
          "and so does every other property it did not state");

    const SvgResolvedStyle current =
        resolve_svg_style(document->shapes()[2].style, *document, style);
    check(current.fill.kind() == SvgPaint::Kind::CurrentColor,
          "currentColor survives resolution and is bound at paint time");
  }

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
