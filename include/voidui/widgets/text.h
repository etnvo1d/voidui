#pragma once

#include <cmath>
#include <functional>
#include <string>
#include <utility>

#include "voidui/core/context.h"
#include "voidui/core/state.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"
#include "voidui/paint/text_layout.h"

namespace voidui {

/// A paragraph of text.
///
/// The expensive part of text is shaping: it crosses into HarfBuzz and, for
/// anything the base face cannot render, into the platform's font-fallback
/// machinery. So the shaped result is built once and kept, and `layout()`
/// rebuilds it only when something that changes the outcome changes -- the
/// string, the font, the alignment, or the width it must wrap to.
///
/// Drawing is then free of shaping entirely: the display list references the
/// layout through a shared pointer, so a paragraph redrawn every frame costs
/// one refcount bump rather than a copy of every glyph in it.
class Text : public Widget {
public:
  VOIDUI_STYLE_SCOPE(Text, "text")

  explicit Text(std::string text) : text_(std::move(text)) {}

  Text(std::string text, std::shared_ptr<FontStack> fonts)
      : text_(std::move(text)) {
    font(std::move(fonts));
  }

  VOIDUI_FLUENT_METHOD(
      content, (std::string text), if (text_ != text) {
        text_ = std::move(text);
        layout_.reset();
      })

  /// The families to shape with, most preferred first. Written into the inline
  /// style, so `font-family` from a stylesheet and this setter meet in one
  /// cascade.
  VOIDUI_FLUENT_METHOD(font_family, (FontFamilyList families),
                       set_style<styles::FontFamily>(std::move(families));)

  /// The same, taking the family off a stack that is already loaded. Only its
  /// family name is used -- the size comes from `font-size`, which is what lets
  /// one family serve every size in the application.
  VOIDUI_FLUENT_METHOD(font, (const std::shared_ptr<FontStack> &fonts),
                       set_style<styles::FontFamily>(
                           fonts ? FontFamilyList::of({fonts->family()})
                                 : FontFamilyList{});)

  /// Writes the inline style rather than a field of its own. The colour is
  /// the inherited `color` property, so a colour set on an ancestor
  /// reaches this paragraph unless something closer overrides it.
  VOIDUI_FLUENT_METHOD(color, (Color value),
                       set_style<styles::Foreground>(Brush(value));)

  VOIDUI_FLUENT_METHOD(line_height, (float value),
                       set_style<styles::LineHeight>(value);)

  VOIDUI_FLUENT_METHOD(font_size, (float value),
                       set_style<styles::FontSize>(value);)

  VOIDUI_FLUENT_METHOD(font_weight, (FontWeight value),
                       set_style<styles::FontWeight>(value);)

  VOIDUI_FLUENT_METHOD(
      align, (TextAlign align), if (align_ != align) {
        align_ = align;
        layout_.reset();
      })

  /// When false the paragraph is laid out on one line (plus any explicit
  /// newlines) and overflows its box rather than wrapping.
  VOIDUI_FLUENT_METHOD(
      wrap, (bool wrap), if (wrap_ != wrap) {
        wrap_ = wrap;
        layout_.reset();
      })

  /// Zero means unlimited.
  VOIDUI_FLUENT_METHOD(
      max_lines, (int lines), if (max_lines_ != lines) {
        max_lines_ = lines;
        layout_.reset();
      })

  VOIDUI_WIDGET_SIZE_STYLE

  const std::string &content() const { return text_; }
  const std::shared_ptr<const TextLayout> &text_layout() const {
    return layout_;
  }

  std::unique_ptr<Widget> clone() const override {
    auto copy = std::make_unique<Text>(text_);
    copy->align_ = align_;
    copy->wrap_ = wrap_;
    copy->max_lines_ = max_lines_;
    // The layout is immutable once built, so the copy may share it -- along
    // with the scale it was built for, or the copy would rebuild it needlessly.
    copy->device_scale_ = device_scale_;
    copy->line_height_ = line_height_;
    copy->layout_ = layout_;
    return copy;
  }

  void inherit_runtime(const Widget &previous) override {
    const auto &text = static_cast<const Text &>(previous);
    if (text_ == text.text_ && align_ == text.align_ && wrap_ == text.wrap_ &&
        max_lines_ == text.max_lines_) {
      fonts_ = text.fonts_;
      layout_ = text.layout_;
      device_scale_ = text.device_scale_;
      line_height_ = text.line_height_;
    }
  }

  void register_children(Registrar &) override {}

  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    // Shaping depends on the resolved font, so the style is consulted before
    // the layout cache is asked whether it is still valid.
    adopt_font_(ctx.style);
    // Held for `draw`, which has no layout context of its own.
    device_scale_ = ctx.device_scale();
    const auto whitespace = ctx.style.get<styles::WhiteSpace>();
    ensure_layout_(whitespace == WhiteSpace::Nowrap ||
                           whitespace == WhiteSpace::Pre
                       ? 0.0f
                       : wrap_width_for_(constraints));

    const Size<float> intrinsic =
        layout_ ? layout_->size() : Size<float>(0.0f, 0.0f);
    return constraints.resolve(ctx.style.layout_size(), intrinsic);
  }

  void draw(const DrawContext &ctx, Painter &painter) override {
    adopt_font_(ctx.style);

    // Only a guard for a widget drawn without being laid out. Re-wrapping to
    // the resolved box width here would be a feedback loop: that width is the
    // paragraph's own widest line, which is narrower than the limit it wrapped
    // to, so every frame would wrap to a different number than the last.
    if (!layout_)
      ensure_layout_(0.0f);

    // The layout aligns its lines against each other; placing the block inside
    // a box wider than itself is this widget's job, and costs nothing.
    float x = ctx.bounds.origin.x;
    const float slack = ctx.bounds.size.width - layout_->size().width;
    if (slack > 0.0f) {
      if (align_ == TextAlign::Center)
        x += slack * 0.5f;
      else if (align_ == TextAlign::Right)
        x += slack;
    }

    if (ctx.has_selection) {
      paint_selection(ctx.bounds, ctx.selection_begin, ctx.selection_end,
                      ctx.style.get<styles::SelectionColor>(), painter);
    }

    painter.draw_text_layout(Point<float>(x, ctx.bounds.origin.y), layout_,
                             Paint(ctx.style.get<styles::Foreground>()));
  }

  EventResult on_event(Event &) override { return EventResult::Unhandled; }

  bool supports_text_selection() const override { return true; }

  std::uint32_t selection_hit_test(Point<float> point,
                                   Rect<float> bounds) const override {
    if (!layout_)
      return 0;
    const float origin_x = layout_origin_x_(bounds);
    return layout_->hit_test(
        Point<float>(point.x - origin_x, point.y - bounds.origin.y));
  }

  std::pair<std::uint32_t, std::uint32_t>
  selection_word_at(std::uint32_t offset) const override {
    return layout_ ? layout_->word_at(offset)
                   : std::pair<std::uint32_t, std::uint32_t>{0, 0};
  }

  std::string_view selection_text() const override { return text_; }

  void paint_selection(Rect<float> bounds, std::uint32_t begin,
                       std::uint32_t end, Color color, Painter &painter) const {
    if (!layout_ || begin == end)
      return;
    const Point<float> origin{layout_origin_x_(bounds), bounds.origin.y};
    for (std::size_t line = 0; line < layout_->lines().size(); ++line) {
      Rect<float> highlight;
      if (!layout_->selection_rect(line, begin, end, highlight))
        continue;
      highlight.origin.x += origin.x;
      highlight.origin.y += origin.y;
      painter.fill_rect(highlight, Paint(color));
    }
  }

private:
  float layout_origin_x_(Rect<float> bounds) const {
    float x = bounds.origin.x;
    if (!layout_)
      return x;
    const float slack = bounds.size.width - layout_->size().width;
    if (slack > 0.0f) {
      if (align_ == TextAlign::Center)
        x += slack * 0.5f;
      else if (align_ == TextAlign::Right)
        x += slack;
    }
    return x;
  }

  /// Rebuilds the font only when the family list, size, weight, or line height
  /// moved, so the common case -- a layout pass where nothing about the style
  /// changed -- is three comparisons and no work.
  ///
  /// The list is compared rather than the resolved stack's family, because the
  /// two are not the same question: `"Inter", sans-serif` and `sans-serif` can
  /// resolve to one stack on a machine without Inter and to two on a machine
  /// with it.
  void adopt_font_(const ComputedStyle &style) {
    const float size = style.get<styles::FontSize>();
    const float line_height = style.get<styles::LineHeight>();
    const FontWeight weight = style.get<styles::FontWeight>();
    const FontFamilyList &families = style.get<styles::FontFamily>();

    if (fonts_ && fonts_->size() == size && fonts_->weight() == weight &&
        families_ == families && line_height_ == line_height)
      return;

    // Stacks are shared process-wide by (families, locale, size, weight): a
    // stack owns loaded faces and a fallback chain, so a list of a thousand
    // labels at one size must not build a thousand of them.
    families_ = families;
    fonts_ = FontStack::cached(families, size, {}, weight);
    line_height_ = line_height;
    layout_.reset();
  }

  float wrap_width_for_(const Constraints &constraints) const {
    if (!wrap_)
      return 0.0f;

    // An unbounded constraint is the same as not wrapping at all.
    return std::isfinite(constraints.max_width) ? constraints.max_width : 0.0f;
  }

  void ensure_layout_(float wrap_width) {
    // The width and the display scale are the only inputs that change from one
    // layout pass to the next; everything else invalidates the cache at the
    // point it is set. The scale is in the key because the layout rounds its
    // line box to it -- without this a window dragged to a second monitor would
    // keep the first one's metrics forever.
    if (layout_ && layout_->wrap_width() == wrap_width &&
        layout_->device_scale() == device_scale_)
      return;

    layout_ = TextLayout::build(fonts_, text_, wrap_width, align_, max_lines_,
                                device_scale_, line_height_);
  }

  std::string text_;
  /// Both derived from the resolved style; never set directly.
  FontFamilyList families_;
  std::shared_ptr<FontStack> fonts_;
  std::shared_ptr<const TextLayout> layout_;

  TextAlign align_ = TextAlign::Left;
  float device_scale_ = 1.0f;
  float line_height_ = 0.0f;
  bool wrap_ = true;
  int max_lines_ = 0;
};

[[nodiscard]] inline Text text(std::string content) {
  return Text(std::move(content));
}

template <class T>
  requires std::is_arithmetic_v<T>
[[nodiscard]] Text text(const State<T> &content) {
  return Text(std::to_string(content.get()));
}

[[nodiscard]] inline Text text(const State<std::string> &content) {
  return Text(content.get());
}

} // namespace voidui
