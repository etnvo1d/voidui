#include "voidui/widgets/input.h"

#include "voidui/core/pixel_snap.h"

#include <SDL3/SDL_clipboard.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>

namespace voidui {
namespace detail {
namespace {

constexpr std::size_t no_child = static_cast<std::size_t>(-1);

/// The caret's nominal thickness, before it is rounded onto the device grid.
constexpr float kCaretThickness = 1.0f;

float available_axis(const Length &length, float minimum, float maximum) {
  if (const auto *fixed = std::get_if<Length::Fixed>(&length.value))
    return std::clamp(fixed->value, minimum, maximum);
  return maximum;
}

float subtract_if_finite(float value, float amount) {
  return std::isfinite(value) ? std::max(value - amount, 0.0f) : value;
}

} // namespace

void TextControl::set_value_(std::string value) {
  clear_composition_();
  value_ = value;
  declared_value_ = std::move(value);
  value_explicit_ = true;
  anchor_ = focus_ = static_cast<std::uint32_t>(value_.size());
  caret_moved_();
  layout_.reset();
}

void TextControl::set_placeholder_(std::string value) {
  if (placeholder_ == value)
    return;
  placeholder_ = std::move(value);
  if (value_.empty())
    layout_.reset();
}

void TextControl::set_inline_start_(std::unique_ptr<Widget> value) {
  inline_start_ = std::move(value);
}

void TextControl::set_inline_end_(std::unique_ptr<Widget> value) {
  inline_end_ = std::move(value);
}

void TextControl::set_block_start_(std::unique_ptr<Widget> value) {
  block_start_ = std::move(value);
}

void TextControl::set_block_end_(std::unique_ptr<Widget> value) {
  block_end_ = std::move(value);
}

void TextControl::copy_to_(TextControl &target) const {
  target.value_ = value_;
  target.declared_value_ = declared_value_;
  target.placeholder_ = placeholder_;
  target.composition_text_ = composition_text_;
  target.value_explicit_ = value_explicit_;
  target.anchor_ = anchor_;
  target.focus_ = focus_;
  target.composition_begin_ = composition_begin_;
  target.composition_end_ = composition_end_;
  target.composition_cursor_ = composition_cursor_;
  target.composition_selection_end_ = composition_selection_end_;
  target.on_change_ = on_change_;
  target.on_submit_ = on_submit_;
  if (inline_start_)
    target.inline_start_ = clone_widget(*inline_start_);
  if (inline_end_)
    target.inline_end_ = clone_widget(*inline_end_);
  if (block_start_)
    target.block_start_ = clone_widget(*block_start_);
  if (block_end_)
    target.block_end_ = clone_widget(*block_end_);
}

void TextControl::register_children(Registrar &registrar) {
  std::size_t index = 0;
  auto take = [&](std::unique_ptr<Widget> &child, const char *part,
                  std::size_t &stored_index) {
    stored_index = no_child;
    if (!child)
      return;
    stored_index = index++;
    registrar.take_internal_child(std::move(child), part);
  };

  take(block_start_, "block-start", block_start_index_);
  take(inline_start_, "inline-start", inline_start_index_);
  take(inline_end_, "inline-end", inline_end_index_);
  take(block_end_, "block-end", block_end_index_);
}

void TextControl::adopt_font_(const ComputedStyle &style) {
  const float size = style.get<styles::FontSize>();
  const float line_height = style.get<styles::LineHeight>();
  const FontWeight weight = style.get<styles::FontWeight>();
  const FontFamilyList &families = style.get<styles::FontFamily>();

  if (fonts_ && fonts_->size() == size && fonts_->weight() == weight &&
      families_ == families && line_height_ == line_height)
    return;

  families_ = families;
  fonts_ = FontStack::cached(families, size, {}, weight);
  line_height_ = line_height;
  layout_.reset();
}

void TextControl::ensure_layout_(float width) {
  std::string composed;
  if (has_composition_()) {
    composed = value_;
    composed.replace(composition_begin_, composition_end_ - composition_begin_,
                     composition_text_);
  }
  const std::string &shown =
      has_composition_() ? composed : (value_.empty() ? placeholder_ : value_);
  const float wrap_width = multiline_ ? width : 0.0f;
  if (layout_ && layout_text_ == shown && layout_width_ == wrap_width &&
      layout_->device_scale() == device_scale_)
    return;

  layout_text_ = shown;
  layout_width_ = wrap_width;
  layout_ = TextLayout::build(fonts_, shown, wrap_width, TextAlign::Left, 0,
                              device_scale_, line_height_);
}

Size<float> TextControl::layout(Constraints constraints, LayoutContext &ctx) {
  adopt_font_(ctx.style);
  device_scale_ = ctx.device_scale();

  const float border = std::max(ctx.style.get<styles::BorderWidth>(), 0.0f);
  const Spacing<float> chrome =
      ctx.style.get<styles::Padding>() + Spacing<float>(border);
  const float horizontal_chrome = chrome.left + chrome.right;
  const float vertical_chrome = chrome.top + chrome.bottom;
  const Size<Length> &styled_size = ctx.style.layout_size();
  const float available_outer_width = available_axis(
      styled_size.width, constraints.min_width, constraints.max_width);
  const float available_content_width =
      subtract_if_finite(available_outer_width, horizontal_chrome);
  const float infinity = std::numeric_limits<float>::infinity();

  std::array<Size<float>, 4> child_sizes{};
  auto measure = [&](std::size_t index) {
    if (index != no_child)
      child_sizes[index] = ctx.constrain_child(
          index, Constraints{0.0f, available_content_width, 0.0f, infinity});
  };
  measure(block_start_index_);
  measure(inline_start_index_);
  measure(inline_end_index_);
  measure(block_end_index_);

  const float inline_gap = std::max(inline_gap_(ctx.style), 0.0f);
  const float block_gap = std::max(block_gap_(ctx.style), 0.0f);
  const float start_width = inline_start_index_ == no_child
                                ? 0.0f
                                : child_sizes[inline_start_index_].width;
  const float end_width = inline_end_index_ == no_child
                              ? 0.0f
                              : child_sizes[inline_end_index_].width;
  const float inline_gaps =
      (inline_start_index_ != no_child ? inline_gap : 0.0f) +
      (inline_end_index_ != no_child ? inline_gap : 0.0f);
  const float editor_limit = subtract_if_finite(
      available_content_width, start_width + end_width + inline_gaps);

  ensure_layout_(editor_limit);
  const float line_height = line_box_();
  const Size<float> text_size = layout_ ? layout_->size() : Size<float>{};
  const float editor_intrinsic_height =
      multiline_ ? std::max(text_size.height, line_height) : line_height;
  const float center_height = std::max(
      {editor_intrinsic_height,
       inline_start_index_ == no_child
           ? 0.0f
           : child_sizes[inline_start_index_].height,
       inline_end_index_ == no_child ? 0.0f
                                     : child_sizes[inline_end_index_].height});
  const float center_width =
      start_width + end_width + inline_gaps + text_size.width;

  float intrinsic_content_width = center_width;
  float intrinsic_content_height = center_height;
  if (block_start_index_ != no_child) {
    intrinsic_content_width = std::max(intrinsic_content_width,
                                       child_sizes[block_start_index_].width);
    intrinsic_content_height +=
        child_sizes[block_start_index_].height + block_gap;
  }
  if (block_end_index_ != no_child) {
    intrinsic_content_width =
        std::max(intrinsic_content_width, child_sizes[block_end_index_].width);
    intrinsic_content_height +=
        child_sizes[block_end_index_].height + block_gap;
  }

  const Size<float> control_size = constraints.resolve(
      styled_size, {intrinsic_content_width + horizontal_chrome,
                    intrinsic_content_height + vertical_chrome});
  const float content_width =
      std::max(control_size.width - horizontal_chrome, 0.0f);
  const float content_height =
      std::max(control_size.height - vertical_chrome, 0.0f);
  const float editor_width =
      std::max(content_width - start_width - end_width - inline_gaps, 0.0f);
  ensure_layout_(editor_width);

  float top = 0.0f;
  if (block_start_index_ != no_child) {
    ctx.place_child(block_start_index_, {chrome.left, chrome.top});
    top = child_sizes[block_start_index_].height + block_gap;
  }
  float bottom = 0.0f;
  if (block_end_index_ != no_child) {
    bottom = child_sizes[block_end_index_].height + block_gap;
    ctx.place_child(block_end_index_,
                    {chrome.left, chrome.top + content_height -
                                      child_sizes[block_end_index_].height});
  }

  const float middle_height = std::max(content_height - top - bottom, 0.0f);
  const float middle_y = chrome.top + top;
  float x = chrome.left;
  if (inline_start_index_ != no_child) {
    ctx.place_child(
        inline_start_index_,
        {x, middle_y + std::max(middle_height -
                                    child_sizes[inline_start_index_].height,
                                0.0f) *
                           0.5f});
    x += start_width + inline_gap;
  }

  editor_bounds_ = {{x, middle_y}, {editor_width, middle_height}};
  x += editor_width;
  if (inline_end_index_ != no_child) {
    x += inline_gap;
    ctx.place_child(
        inline_end_index_,
        {x, middle_y +
                std::max(middle_height - child_sizes[inline_end_index_].height,
                         0.0f) *
                    0.5f});
  }

  // Scrolling the caret into view belongs to painting, not here. It moves no
  // box and resizes nothing, and putting it here is what left the caret behind
  // whenever it moved without a relayout -- every arrow key, Home, End and
  // click, all of which ask only for a repaint.
  reveal_caret_();

  return control_size;
}

Rect<float> TextControl::children_clip(Rect<float> bounds) const {
  return bounds;
}

float TextControl::line_box_() const {
  if (!fonts_)
    return 0.0f;
  return snap_to_pixel(line_height_ > 0.0f ? line_height_
                                           : fonts_->line_height(),
                       device_scale_);
}

float TextControl::caret_width_() const {
  const float scale = device_scale_ > 0.0f ? device_scale_ : 1.0f;
  return std::max(1.0f, round_half_up(kCaretThickness * scale)) / scale;
}

Rect<float>
TextControl::global_editor_bounds_(Point<float> control_origin) const {
  return {{control_origin.x + editor_bounds_.origin.x,
           control_origin.y + editor_bounds_.origin.y},
          editor_bounds_.size};
}

Point<float> TextControl::text_origin_(Point<float> control_origin) const {
  const Rect<float> editor = global_editor_bounds_(control_origin);
  // An empty field with no placeholder has a layout of no lines and zero
  // height. Centring against that would put its caret half way down the box
  // and hang it out of the bottom; centring against the line box the text
  // would have occupied puts the caret exactly where the first character will
  // appear.
  const float height = layout_ && !layout_->lines().empty()
                           ? layout_->size().height
                           : line_box_();
  const float y = multiline_
                      ? editor.origin.y - vertical_scroll_
                      : editor.origin.y +
                            std::max(editor.size.height - height, 0.0f) * 0.5f;
  return {editor.origin.x - horizontal_scroll_, y};
}

Rect<float> TextControl::caret_local_() const {
  Rect<float> caret;
  // While the field is empty the layout holds the placeholder, whose caret
  // table describes text the caret is not in. The caret is at the start of a
  // line box of its own.
  if (layout_ && has_composition_() &&
      layout_->caret_rect(composition_caret_(), caret))
    return caret;
  if (!value_.empty() && layout_ && layout_->caret_rect(focus_, caret))
    return caret;
  return {0.0f, 0.0f, 0.0f, line_box_()};
}

Rect<float> TextControl::caret_rect_(Point<float> control_origin) const {
  const Rect<float> editor = global_editor_bounds_(control_origin);
  const Point<float> origin = text_origin_(control_origin);
  const Rect<float> local = caret_local_();
  const float scale = device_scale_ > 0.0f ? device_scale_ : 1.0f;

  // Built in whole device pixels rather than snapped afterwards. The renderer
  // rounds a rect's left and right edges independently, so a caret one logical
  // pixel wide is one device pixel at some x and two at the next -- at 125% it
  // changes thickness as it walks through a word. A width that is already a
  // whole number of pixels survives that rounding unchanged.
  const float width = std::max(1.0f, round_half_up(kCaretThickness * scale));
  float height = std::max(1.0f, round_half_up(local.size.height * scale));
  float left = round_half_up((origin.x + local.origin.x) * scale);
  float top = round_half_up((origin.y + local.origin.y) * scale);

  // Held inside the editor's clip with ceil and floor, not with the clip's own
  // rounding: the clip is evaluated analytically in the shader against the
  // unrounded rectangle, so a caret that merely rounds to the same pixel as
  // the edge still comes back half faded.
  const float min_left = std::ceil(editor.origin.x * scale);
  const float max_left =
      std::floor((editor.origin.x + editor.size.width) * scale) - width;
  const float min_top = std::ceil(editor.origin.y * scale);
  const float max_bottom =
      std::floor((editor.origin.y + editor.size.height) * scale);

  left = std::clamp(left, min_left, std::max(min_left, max_left));
  height = std::max(1.0f, std::min(height, max_bottom - min_top));
  top = std::clamp(top, min_top, std::max(min_top, max_bottom - height));

  const float pixel = 1.0f / scale;
  return {device_to_logical(left, scale), device_to_logical(top, scale),
          width * pixel, height * pixel};
}

void TextControl::reveal_caret_() {
  const Rect<float> caret = caret_local_();
  if (multiline_) {
    const float view = editor_bounds_.size.height;
    const float bottom = caret.origin.y + caret.size.height;
    if (caret.origin.y < vertical_scroll_)
      vertical_scroll_ = caret.origin.y;
    else if (bottom > vertical_scroll_ + view)
      vertical_scroll_ = bottom - view;
    const float content = layout_ ? layout_->size().height : 0.0f;
    vertical_scroll_ =
        std::clamp(vertical_scroll_, 0.0f, std::max(content - view, 0.0f));
    horizontal_scroll_ = 0.0f;
    return;
  }

  const float width = caret_width_();
  const float view = editor_bounds_.size.width;
  const float right = caret.origin.x + width;
  if (caret.origin.x < horizontal_scroll_)
    horizontal_scroll_ = caret.origin.x;
  else if (right > horizontal_scroll_ + view)
    horizontal_scroll_ = right - view;

  // The caret stands just past the last glyph, so the scrollable width has to
  // account for it as well. Clamping to the text's own width alone puts the
  // caret exactly on the right edge of the clip at the end of a full field,
  // where it is cut away entirely and the field looks like it lost focus.
  const float content = (value_.empty() && !has_composition_()) || !layout_
                            ? 0.0f
                            : layout_->size().width;
  horizontal_scroll_ = std::clamp(horizontal_scroll_, 0.0f,
                                  std::max(content + width - view, 0.0f));
  vertical_scroll_ = 0.0f;
}

void TextControl::caret_moved_() {
  ++caret_revision_;
  goal_x_ = -1.0f;
}

void TextControl::draw(const DrawContext &ctx, Painter &painter) {
  const Radius radius = ctx.style.get<styles::BorderRadius>();
  painter.fill_rrect(ctx.bounds, radius,
                     Paint(ctx.style.get<styles::Background>()));
  const float border = ctx.style.get<styles::BorderWidth>();
  if (border > 0.0f) {
    painter.stroke_rrect(ctx.bounds, radius,
                         Paint(ctx.style.get<styles::BorderColor>()),
                         Pen(border, StrokeAlign::Inside));
  }
  if (editor_bounds_.size.width <= 0.0f || editor_bounds_.size.height <= 0.0f)
    return;

  const bool focused = ctx.status.is_focused();
  if (focused != caret_focused_) {
    caret_focused_ = focused;
    // A field that regains focus shows a solid caret first, rather than
    // resuming a blink phase that has been running while it was invisible.
    caret_moved_();
  }

  reveal_caret_();
  const Rect<float> editor = global_editor_bounds_(ctx.bounds.origin);
  const Point<float> origin = text_origin_(ctx.bounds.origin);

  painter.save();
  painter.clip_rect(editor);
  if (layout_ && !layout_->lines().empty()) {
    auto fill_range = [&](std::uint32_t begin, std::uint32_t end,
                          const Brush &brush) {
      for (std::size_t i = 0; i < layout_->lines().size(); ++i) {
        Rect<float> selection;
        if (!layout_->selection_rect(i, begin, end, selection))
          continue;
        selection.origin.x += origin.x;
        selection.origin.y += origin.y;
        painter.fill_rect(selection, Paint(brush));
      }
    };

    if (has_composition_()) {
      fill_range(composition_display_begin_() + composition_cursor_,
                 composition_display_begin_() + composition_selection_end_,
                 ctx.style.get<styles::SelectionColor>());
    } else if (ctx.has_selection && !value_.empty()) {
      fill_range(ctx.selection_begin, ctx.selection_end,
                 ctx.style.get<styles::SelectionColor>());
    }

    const Brush &text_color = value_.empty() && !has_composition_()
                                  ? placeholder_color_(ctx.style)
                                  : ctx.style.get<styles::Foreground>();
    painter.draw_text_layout(origin, layout_, Paint(text_color));

    if (has_composition_()) {
      // The underline distinguishes transient IME text without handing its
      // typography back to the native composition window.
      const float thickness = caret_width_();
      for (std::size_t i = 0; i < layout_->lines().size(); ++i) {
        Rect<float> underline;
        if (!layout_->selection_rect(i, composition_display_begin_(),
                                     composition_display_end_(), underline))
          continue;
        underline.origin.x += origin.x;
        underline.origin.y += origin.y + underline.size.height - thickness;
        underline.size.height = thickness;
        painter.fill_rect(underline, Paint(caret_color_(ctx.style)));
      }
    }
  }

  if (focused && (!ctx.has_selection || has_composition_()) && draw_caret_(ctx))
    painter.fill_rect(caret_rect_(ctx.bounds.origin),
                      Paint(caret_color_(ctx.style)));
  painter.restore();
}

bool TextControl::draw_caret_(const DrawContext &ctx) {
  const double now = ctx.now();
  if (painted_caret_revision_ != caret_revision_) {
    painted_caret_revision_ = caret_revision_;
    blink_epoch_ = now;
  }

  const float period = caret_blink_(ctx.style);
  if (!(period > 0.0f))
    return true; // `caret-blink: 0` means a caret that stays put.

  const double half = static_cast<double>(period);
  const double elapsed = std::max(now - blink_epoch_, 0.0);
  const double phases = elapsed / half;

  // Woken for the next toggle and nothing in between: two frames a second,
  // against the sixty that asking for a plain repaint every frame would cost.
  ctx.request_frame_at(blink_epoch_ + (std::floor(phases) + 1.0) * half);
  return std::fmod(phases, 2.0) < 1.0;
}

std::optional<TextInputArea>
TextControl::text_input_area(Rect<float> bounds) const {
  if (editor_bounds_.size.width <= 0.0f || editor_bounds_.size.height <= 0.0f)
    return std::nullopt;

  const Rect<float> editor = global_editor_bounds_(bounds.origin);
  const Rect<float> caret = caret_rect_(bounds.origin);

  // SDL places native composition UI inside this rectangle and excludes it
  // when positioning the candidate list. A single visible line keeps both UI
  // pieces beside the caret in a textarea instead of below the whole control.
  const Rect<float> line{editor.origin.x, caret.origin.y, editor.size.width,
                         caret.size.height};
  return TextInputArea{line, caret.origin.x - line.origin.x};
}

std::uint32_t TextControl::selection_hit_test(Point<float> point,
                                              Rect<float> bounds) const {
  if (!layout_ || (value_.empty() && !has_composition_()))
    return 0;

  // Clamped into the editor rather than rejected. A press only reaches this
  // widget when it landed on the field itself, so a press on its padding -- or
  // on a label in one of its block slots -- means the nearest end of the text.
  // Returning the caret unchanged instead left a click just past the last
  // character doing nothing at all.
  const Rect<float> editor = global_editor_bounds_(bounds.origin);
  const Point<float> origin = text_origin_(bounds.origin);
  const Point<float> inside{
      std::clamp(point.x, editor.origin.x, editor.origin.x + editor.size.width),
      std::clamp(point.y, editor.origin.y,
                 editor.origin.y + editor.size.height)};
  const std::uint32_t offset =
      layout_->hit_test({inside.x - origin.x, inside.y - origin.y});
  if (!has_composition_() || offset <= composition_display_begin_())
    return offset;
  if (offset < composition_display_end_())
    return composition_begin_;
  return offset - static_cast<std::uint32_t>(composition_text_.size()) +
         (composition_end_ - composition_begin_);
}

std::pair<std::uint32_t, std::uint32_t>
TextControl::selection_word_at(std::uint32_t offset) const {
  return value_.empty() || !layout_ || has_composition_()
             ? std::pair<std::uint32_t, std::uint32_t>{0, 0}
             : layout_->word_at(offset);
}

void TextControl::selection_changed(std::uint32_t anchor, std::uint32_t focus) {
  const auto size = static_cast<std::uint32_t>(value_.size());
  anchor = std::min(anchor, size);
  focus = std::min(focus, size);
  if (anchor_ == anchor && focus_ == focus)
    return;
  anchor_ = anchor;
  focus_ = focus;
  caret_moved_();
}

std::size_t TextControl::previous_codepoint_(std::string_view text,
                                             std::size_t offset) {
  if (offset == 0)
    return 0;
  --offset;
  while (offset > 0 &&
         (static_cast<unsigned char>(text[offset]) & 0xc0u) == 0x80u)
    --offset;
  return offset;
}

std::size_t TextControl::next_codepoint_(std::string_view text,
                                         std::size_t offset) {
  if (offset >= text.size())
    return text.size();
  ++offset;
  while (offset < text.size() &&
         (static_cast<unsigned char>(text[offset]) & 0xc0u) == 0x80u)
    ++offset;
  return offset;
}

std::size_t TextControl::byte_offset_for_codepoint_(std::string_view text,
                                                    std::int32_t index) {
  if (index < 0)
    return text.size();
  std::size_t offset = 0;
  while (index-- > 0 && offset < text.size())
    offset = next_codepoint_(text, offset);
  return offset;
}

void TextControl::clear_composition_() {
  if (composition_text_.empty())
    return;
  composition_text_.clear();
  composition_begin_ = composition_end_ = 0;
  composition_cursor_ = composition_selection_end_ = 0;
  layout_.reset();
  caret_moved_();
}

void TextControl::update_composition_(const TextEditingEvent &editing,
                                      Event &event) {
  if (editing.text().empty()) {
    if (has_composition_()) {
      clear_composition_();
      event.request_layout();
    }
    return;
  }

  if (!has_composition_()) {
    composition_begin_ = std::min(anchor_, focus_);
    composition_end_ = std::max(anchor_, focus_);
  }

  const std::string &text = editing.text();
  const std::size_t cursor = byte_offset_for_codepoint_(text, editing.start());
  const std::int64_t selection_end =
      editing.start() < 0 ? -1
                          : static_cast<std::int64_t>(editing.start()) +
                                std::max<std::int32_t>(editing.length(), 0);
  const std::size_t selected = byte_offset_for_codepoint_(
      text, selection_end > std::numeric_limits<std::int32_t>::max()
                ? std::numeric_limits<std::int32_t>::max()
                : static_cast<std::int32_t>(selection_end));

  composition_text_ = text;
  composition_cursor_ = static_cast<std::uint32_t>(cursor);
  composition_selection_end_ = static_cast<std::uint32_t>(selected);
  layout_.reset();
  caret_moved_();
  event.request_layout();
}

void TextControl::notify_change_() {
  layout_.reset();
  if (on_change_)
    on_change_(value_);
}

void TextControl::replace_selection_(std::string_view text, Event &event) {
  const bool cleared_composition = has_composition_();
  clear_composition_();
  std::string filtered;
  if (!multiline_ && text.find_first_of("\r\n") != std::string_view::npos) {
    filtered.reserve(text.size());
    for (char ch : text)
      if (ch != '\r' && ch != '\n')
        filtered.push_back(ch);
    text = filtered;
  }
  if (text.empty() && anchor_ == focus_) {
    if (cleared_composition)
      event.request_layout();
    return;
  }

  const std::size_t begin = std::min(anchor_, focus_);
  const std::size_t end = std::max(anchor_, focus_);
  value_.replace(begin, end - begin, text);
  anchor_ = focus_ = static_cast<std::uint32_t>(begin + text.size());
  caret_moved_();
  notify_change_();
  event.request_layout();
}

void TextControl::erase_selection_(Event &event) {
  if (anchor_ == focus_)
    return;
  replace_selection_({}, event);
}

void TextControl::set_caret_(std::uint32_t offset, bool extend) {
  if (!extend)
    anchor_ = offset;
  focus_ = offset;
  caret_moved_();
}

void TextControl::move_horizontal_(bool right, bool extend, Event &event) {
  std::uint32_t next = focus_;
  if (!extend && anchor_ != focus_) {
    next = right ? std::max(anchor_, focus_) : std::min(anchor_, focus_);
  } else {
    next =
        static_cast<std::uint32_t>(right ? next_codepoint_(value_, focus_)
                                         : previous_codepoint_(value_, focus_));
  }
  set_caret_(next, extend);
  event.request_paint();
}

void TextControl::move_line_(bool down, bool extend, Event &event) {
  if (!multiline_ || !layout_ || value_.empty()) {
    move_horizontal_(down, extend, event);
    return;
  }
  Rect<float> caret;
  if (!layout_->caret_rect(focus_, caret))
    return;

  // Aim at the column this run of vertical moves started from, not at wherever
  // the last short line left the caret: otherwise stepping through one brief
  // line narrows the column for every line below it, permanently.
  const float goal = goal_x_ >= 0.0f ? goal_x_ : caret.origin.x;
  const float y = caret.origin.y +
                  (down ? caret.size.height * 1.5f : -caret.size.height * 0.5f);
  const float height = layout_->size().height;

  if (y < 0.0f) {
    // Already on the first line, so Up goes to the very start, as it does in
    // every other editor. Clamping to line zero instead left the key dead.
    set_caret_(0, extend);
  } else if (y >= height) {
    set_caret_(static_cast<std::uint32_t>(value_.size()), extend);
  } else {
    set_caret_(layout_->hit_test({goal, y}), extend);
    goal_x_ = goal;
  }
  event.request_paint();
}

EventResult TextControl::on_event(Event &event) {
  if (auto result =
          event.dispatch<TextEditingEvent>([&](TextEditingEvent &editing) {
            update_composition_(editing, editing);
            return EventResult::Handled;
          });
      result == EventResult::Handled)
    return result;

  if (auto result = event.dispatch<TextInputEvent>([&](TextInputEvent &input) {
        replace_selection_(input.text(), input);
        return EventResult::Handled;
      });
      result == EventResult::Handled)
    return result;

  if (auto result =
          event.dispatch<MousePressedEvent>([&](MousePressedEvent &press) {
            if (press.button() != MouseButton::Left)
              return EventResult::Unhandled;
            if (has_composition_()) {
              clear_composition_();
              press.request_layout();
            }
            return EventResult::Handled;
          });
      result == EventResult::Handled)
    return result;

  return event.dispatch<KeyPressedEvent>([&](KeyPressedEvent &key) {
    const Keycode code = key.keycode();
    const bool extend = key.modifiers().shift();
    const bool primary = key.modifiers().primary();

    if ((primary && code == Keycode::V) || code == Keycode::Paste) {
      char *clipboard = SDL_GetClipboardText();
      if (clipboard) {
        if (*clipboard != '\0')
          replace_selection_(clipboard, key);
        SDL_free(clipboard);
      }
      return EventResult::Handled;
    }
    if ((primary && code == Keycode::X) || code == Keycode::Cut) {
      if (anchor_ != focus_) {
        const std::size_t begin = std::min(anchor_, focus_);
        const std::size_t end = std::max(anchor_, focus_);
        const std::string selected = value_.substr(begin, end - begin);
        SDL_SetClipboardText(selected.c_str());
        erase_selection_(key);
      }
      return EventResult::Handled;
    }
    if (code == Keycode::Backspace || code == Keycode::KpBackspace) {
      if (anchor_ != focus_) {
        erase_selection_(key);
      } else if (focus_ > 0) {
        anchor_ =
            static_cast<std::uint32_t>(previous_codepoint_(value_, focus_));
        replace_selection_({}, key);
      }
      return EventResult::Handled;
    }
    if (code == Keycode::Delete) {
      if (anchor_ != focus_) {
        erase_selection_(key);
      } else if (focus_ < value_.size()) {
        anchor_ = static_cast<std::uint32_t>(next_codepoint_(value_, focus_));
        replace_selection_({}, key);
      }
      return EventResult::Handled;
    }
    if (code == Keycode::Left) {
      move_horizontal_(false, extend, key);
      return EventResult::Handled;
    }
    if (code == Keycode::Right) {
      move_horizontal_(true, extend, key);
      return EventResult::Handled;
    }
    if (code == Keycode::Up || code == Keycode::Down) {
      move_line_(code == Keycode::Down, extend, key);
      return EventResult::Handled;
    }
    if (code == Keycode::Home || code == Keycode::End) {
      std::uint32_t next =
          code == Keycode::Home ? 0 : static_cast<std::uint32_t>(value_.size());
      if (multiline_ && layout_) {
        Rect<float> caret;
        if (layout_->caret_rect(focus_, caret))
          next = layout_->hit_test({code == Keycode::Home
                                        ? -1.0f
                                        : std::numeric_limits<float>::max(),
                                    caret.origin.y + caret.size.height * 0.5f});
      }
      set_caret_(next, extend);
      key.request_paint();
      return EventResult::Handled;
    }
    if (code == Keycode::Return || code == Keycode::Return2 ||
        code == Keycode::KpEnter) {
      if (multiline_)
        replace_selection_("\n", key);
      else if (on_submit_)
        on_submit_(value_);
      return EventResult::Handled;
    }
    return EventResult::Unhandled;
  });
}

void TextControl::inherit_runtime(const Widget &previous) {
  const auto &control = static_cast<const TextControl &>(previous);
  if (!value_explicit_ || declared_value_ == control.declared_value_)
    value_ = control.value_;

  // Comparing the live text, not the declaration, is what separates a rebuild
  // that merely re-declares what the user already typed -- which is what the
  // `State<std::string>` binding does on every keystroke -- from one that
  // hands the field genuinely different text. The first keeps the caret and
  // the blink phase; the second leaves the caret where `set_value_` put it, at
  // the end of the new text.
  if (value_ != control.value_)
    return;

  const auto size = static_cast<std::uint32_t>(value_.size());
  anchor_ = std::min(control.anchor_, size);
  focus_ = std::min(control.focus_, size);
  composition_text_ = control.composition_text_;
  composition_begin_ = control.composition_begin_;
  composition_end_ = control.composition_end_;
  composition_cursor_ = control.composition_cursor_;
  composition_selection_end_ = control.composition_selection_end_;
  horizontal_scroll_ = control.horizontal_scroll_;
  vertical_scroll_ = control.vertical_scroll_;
  goal_x_ = control.goal_x_;
  device_scale_ = control.device_scale_;
  line_height_ = control.line_height_;

  // Carried across so the caret does not flash back to solid on every rebuild,
  // which in a bound field means on every keystroke.
  blink_epoch_ = control.blink_epoch_;
  caret_revision_ = control.caret_revision_;
  painted_caret_revision_ = control.painted_caret_revision_;
  caret_focused_ = control.caret_focused_;

  if (placeholder_ == control.placeholder_) {
    fonts_ = control.fonts_;
    layout_ = control.layout_;
    layout_text_ = control.layout_text_;
    layout_width_ = control.layout_width_;
  }
}

} // namespace detail

std::shared_ptr<const StyleSheet> Input::default_stylesheet() const {
  static const auto sheet =
      // A shadcn ui style input
      StyleParser::parse(R"vss(
input {
  width: 240px;
	border-radius: 8px;
	border-color: oklch(0.922 0 0);
  border-width: 1px;
	font-size: 14px;
	line-height: 20px;
	height: 36px;
	padding: 4px 12px;
	color: black;
  background: white;
	placeholder-color: oklch(0.556 0 0);
	transition-property: color, box-shadow;
	transition-duration: 150ms;
	transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
	box-shadow: 0 1px 2px 0 rgb(0 0 0 / 5%);
}

input:focus {
	border-color: oklch(0.708 0 0);
	box-shadow:
		0 0 0 3px oklch(0.708 0 0 / 50%),
		0 1px 2px 0 rgb(0 0 0 / 5%);
}

input::part(inline-start) {
	color: oklch(0.556 0 0);
}

  )vss",
                         "input.default.vss", StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}

std::unique_ptr<Widget> Input::clone() const {
  auto copy = std::make_unique<Input>();
  copy_to_(*copy);
  return copy;
}

std::shared_ptr<const StyleSheet> Textarea::default_stylesheet() const {
  // A shadcn ui style textarea
  static const auto sheet =
      StyleParser::parse(R"vss(
textarea {
	width: 240px;
	height: 240px;
	border-width: 1px;
	border-color: oklch(0.922 0 0);
	border-radius: 8px;
	padding: 8px 12px;
	background: transparent;
	placeholder-color: oklch(0.556 0 0);
	transition-property: color, box-shadow;
	transition-duration: 150ms;
	transition-timing-function: cubic-bezier(0.4, 0, 0.2, 1);
	box-shadow: 0 1px 2px 0 rgb(0 0 0 / 5%);
}

textarea:focus {
	border-color: oklch(0.708 0 0);
	box-shadow:
		0 0 0 3px oklch(0.708 0 0 / 50%),
		0 1px 2px 0 rgb(0 0 0 / 5%);
}
  )vss",
                         "textarea.default.vss", StyleOrigin::WidgetDefault)
          .sheet;
  return sheet;
}

std::unique_ptr<Widget> Textarea::clone() const {
  auto copy = std::make_unique<Textarea>();
  copy_to_(*copy);
  return copy;
}

} // namespace voidui
