#pragma once

#include <algorithm>
#include <cmath>
#include <memory>
#include <string_view>
#include <utility>

#include "voidui/core/context.h"
#include "voidui/core/style.h"
#include "voidui/core/widget.h"
#include "voidui/paint/image_cache.h"

namespace voidui {

/// How a picture is fitted to a box that is not its own shape. The CSS
/// `object-fit` values, with the same meanings.
enum class ObjectFit : std::uint8_t {
  /// The whole picture, as large as fits, aspect preserved. Leaves empty space
  /// on the axis that runs out first.
  Contain,
  /// Fills the box, aspect preserved, overflow cropped. What an avatar or a
  /// header image wants.
  Cover,
  /// Stretched to the box, aspect ignored.
  Fill,
  /// Contain, except that a picture smaller than the box is left alone rather
  /// than blown up.
  ScaleDown,
  /// Natural size, cropped by the box.
  None,
};

/// Where a picture sits in a box bigger than it, and which part survives a crop
/// in one smaller. (0,0) is top-left, (1,1) bottom-right; the default is
/// centred, which is right for both jobs.
struct ImageAlignment {
  float x = 0.5f;
  float y = 0.5f;

  constexpr ImageAlignment() = default;
  constexpr ImageAlignment(float x, float y) : x(x), y(y) {}

  static constexpr ImageAlignment center() { return {0.5f, 0.5f}; }
  static constexpr ImageAlignment top_left() { return {0.0f, 0.0f}; }

  bool operator==(const ImageAlignment &) const = default;
};

/// The destination and source rectangles a fit produces.
struct ImagePlacement {
  Rect<float> destination{0.0f, 0.0f, 0.0f, 0.0f};
  Rect<float> source{0.0f, 0.0f, 0.0f, 0.0f}; ///< in source pixels
  bool empty = true;
};

/// Fits a picture of `source` pixels into `box`.
///
/// Free rather than a member because the answer depends on nothing else, and
/// because both the widget and anything drawing an image through a raw Painter
/// want the same arithmetic.
///
/// A crop is expressed by narrowing the source rather than by overflowing the
/// destination, which is why the caller never needs a clip for `Cover`: a clip
/// costs a scissor change and splits the batch, while a narrower source is the
/// same instance with different texture coordinates.
inline ImagePlacement fit_image(Size<float> source, Rect<float> box, ObjectFit fit,
                                ImageAlignment alignment = {}) {
  ImagePlacement placement;
  if (source.width <= 0.0f || source.height <= 0.0f || box.size.width <= 0.0f ||
      box.size.height <= 0.0f)
    return placement;

  placement.empty = false;
  placement.destination = box;
  placement.source = Rect<float>(0.0f, 0.0f, source.width, source.height);

  const float scale_x = box.size.width / source.width;
  const float scale_y = box.size.height / source.height;

  switch (fit) {
  case ObjectFit::Fill:
    return placement;

  case ObjectFit::Contain:
  case ObjectFit::ScaleDown: {
    float scale = std::min(scale_x, scale_y);
    if (fit == ObjectFit::ScaleDown)
      scale = std::min(scale, 1.0f);

    const float width = source.width * scale;
    const float height = source.height * scale;
    placement.destination = Rect<float>(
        box.origin.x + (box.size.width - width) * alignment.x,
        box.origin.y + (box.size.height - height) * alignment.y, width, height);
    return placement;
  }

  case ObjectFit::Cover: {
    // The larger scale is the one that leaves no gap; the axis that overflows
    // is trimmed out of the source instead of off the destination.
    const float scale = std::max(scale_x, scale_y);
    const float visible_width = std::min(source.width, box.size.width / scale);
    const float visible_height = std::min(source.height, box.size.height / scale);
    placement.source =
        Rect<float>((source.width - visible_width) * alignment.x,
                    (source.height - visible_height) * alignment.y,
                    visible_width, visible_height);
    return placement;
  }

  case ObjectFit::None: {
    // Natural size. Bigger than the box means a crop, smaller means padding,
    // and alignment decides both -- so the two are computed per axis rather
    // than branched on.
    const float visible_width = std::min(source.width, box.size.width);
    const float visible_height = std::min(source.height, box.size.height);
    placement.source =
        Rect<float>((source.width - visible_width) * alignment.x,
                    (source.height - visible_height) * alignment.y,
                    visible_width, visible_height);
    placement.destination = Rect<float>(
        box.origin.x + (box.size.width - visible_width) * alignment.x,
        box.origin.y + (box.size.height - visible_height) * alignment.y,
        visible_width, visible_height);
    return placement;
  }
  }

  return placement;
}

/// A picture.
///
/// Loading is asynchronous and shared: the widget names a source and a box, and
/// the cache decides whether that means a decode, a wait on one already running,
/// or a hash lookup. Nothing here blocks and nothing here spins -- a widget
/// still waiting draws its placeholder and asks for no frames at all, and the
/// load wakes the tree once when it lands.
///
/// The decode size is the interesting part. A widget knows how much room it has
/// only during measurement, so that is where the load starts, sized to the room
/// rather than to the picture. A thumbnail therefore never holds a full-size
/// decode even for a frame, which is the difference between a list of a hundred
/// photographs costing a few megabytes and costing several gigabytes.
class ImageView : public Widget {
public:
  VOIDUI_STYLE_SCOPE(ImageView, "image")

  ImageView() = default;

  explicit ImageView(ImageSource source) : source_(std::move(source)) {}

  /// Names a resource or a file: `res://icons/user.png`, `file:///c:/a.png`, or
  /// a bare path, which is read as a file the way a command line argument is.
  explicit ImageView(std::string_view uri)
      : source_(ImageSource::uri(uri)) {}

  /// Pixels the application already has. Nothing is loaded or cached.
  explicit ImageView(std::shared_ptr<const Image> image)
      : source_(ImageSource::ready(std::move(image))) {}

  VOIDUI_FLUENT_METHOD(
      source, (ImageSource value), if (value.key() != source_.key()) {
        source_ = std::move(value);
        handle_ = ImageHandle();
        fallback_ = ImageHandle();
        requested_width_ = 0;
        requested_height_ = 0;
      })

  VOIDUI_FLUENT_METHOD(fit, (ObjectFit value), fit_ = value;)
  VOIDUI_FLUENT_METHOD(alignment, (ImageAlignment value), alignment_ = value;)

  /// Source pixels per logical unit. Two for an `@2x` asset, so a 64x64 file
  /// lays out as 32x32 and stays sharp on a high-density display.
  VOIDUI_FLUENT_METHOD(density, (float value),
                       density_ = value > 0.0f ? value : 1.0f;)

  /// The size to lay out at before the picture arrives.
  ///
  /// Worth setting in a list. Without it a row is zero-high until its image
  /// lands, so a hundred rows arriving in whatever order the pool finishes them
  /// make the list jump under the reader a hundred times.
  VOIDUI_FLUENT_METHOD(natural_size, (Size<float> value), natural_ = value;)

  /// Painted under the picture, and alone while one is loading or has failed.
  VOIDUI_FLUENT_METHOD(placeholder, (Brush value), placeholder_ = std::move(value);)

  /// Seconds to fade in over once the pixels arrive. Zero shows them at once.
  ///
  /// Worth the frames it costs: a decode that lands mid-scroll pops, and a
  /// picture appearing instantly at full strength reads as a flash rather than
  /// as content arriving.
  VOIDUI_FLUENT_METHOD(fade, (float seconds),
                       fade_ = seconds > 0.0f ? seconds : 0.0f;)

  VOIDUI_WIDGET_SIZE_STYLE

  const ImageHandle &handle() const { return handle_; }
  ObjectFit object_fit() const { return fit_; }

  std::shared_ptr<const StyleSheet> default_stylesheet() const override {
    static const std::shared_ptr<const StyleSheet> defaults =
        StyleParser::parse("image { width: auto; height: auto; }",
                           "image.default.vss", StyleOrigin::WidgetDefault)
            .sheet;
    return defaults;
  }

  std::unique_ptr<Widget> clone() const override {
    auto copy = std::make_unique<ImageView>(source_);
    copy->fit_ = fit_;
    copy->alignment_ = alignment_;
    copy->density_ = density_;
    copy->natural_ = natural_;
    copy->placeholder_ = placeholder_;
    copy->fade_ = fade_;
    return copy;
  }

  /// Carries the load across a rebuild.
  ///
  /// Without this a component that re-renders for an unrelated reason drops its
  /// handle, and the entry -- which now has no holders -- is cancelled mid-load
  /// or evicted, only to be asked for again on the very next line. Text keeps
  /// its shaped layout across a rebuild for the same reason.
  void inherit_runtime(const Widget &previous) override {
    const auto &other = static_cast<const ImageView &>(previous);
    if (other.source_.key() != source_.key())
      return;

    handle_ = other.handle_;
    fallback_ = other.fallback_;
    requested_width_ = other.requested_width_;
    requested_height_ = other.requested_height_;
    appeared_ = other.appeared_;
  }

  void register_children(Registrar &) override {}

  Size<float> layout(Constraints constraints, LayoutContext &ctx) override {
    // Sized to the room, not to the picture. The room is known now and does not
    // depend on a load that has not finished, so this asks for one decode size
    // and keeps asking for the same one -- no second decode when the pixels
    // arrive and the box turns out to be a little smaller than the room.
    ensure_load_(room_for_(constraints, ctx.style), ctx.device_scale(),
                 ctx.invalidator());

    return constraints.resolve(ctx.style.layout_size(), intrinsic_size());
  }

  void draw(const DrawContext &ctx, Painter &painter) override {
    const Radius radius = ctx.style.get<styles::BorderRadius>();

    if (!placeholder_holds_nothing_()) {
      if (radius.left_top > 0.0f || radius.right_top > 0.0f ||
          radius.right_bottom > 0.0f || radius.left_bottom > 0.0f)
        painter.fill_rrect(ctx.bounds, radius, Paint(placeholder_));
      else
        painter.fill_rect(ctx.bounds, Paint(placeholder_));
    }

    const std::shared_ptr<const Image> image = display_image_();
    if (!image)
      return;

    const ImagePlacement placement =
        fit_image(natural_size_of_(*image), ctx.bounds, fit_, alignment_);
    if (placement.empty)
      return;

    // The source rect is in the decoded image's own pixels, while the fit was
    // computed in logical units -- the two differ by the density, and by
    // whatever the decoder actually returned when it could not hit the size
    // asked for exactly.
    const float to_pixels_x =
        static_cast<float>(image->width()) / natural_size_of_(*image).width;
    const float to_pixels_y =
        static_cast<float>(image->height()) / natural_size_of_(*image).height;

    Rect<float> source = placement.source;
    source.origin.x *= to_pixels_x;
    source.origin.y *= to_pixels_y;
    source.size.width *= to_pixels_x;
    source.size.height *= to_pixels_y;

    Paint paint(Color(255, 255, 255));
    paint.opacity = fade_opacity_(ctx);

    // Only a rounded box needs a clip. Cover has already cropped itself through
    // the source rect, and Contain never leaves its box at all.
    const bool rounded = radius.left_top > 0.0f || radius.right_top > 0.0f ||
                         radius.right_bottom > 0.0f || radius.left_bottom > 0.0f;
    if (rounded) {
      painter.save();
      painter.clip_rrect(ctx.bounds, radius);
    }

    painter.draw_image(image, placement.destination, source, paint);

    if (rounded)
      painter.restore();
  }

  EventResult on_event(Event &) override { return EventResult::Unhandled; }

  /// The size the picture wants, in logical units. Zero until it arrives,
  /// unless a natural size was declared.
  Size<float> intrinsic_size() const {
    if (const std::shared_ptr<const Image> image = display_image_())
      return natural_size_of_(*image);
    return natural_;
  }

private:
  std::shared_ptr<const Image> display_image_() const {
    if (auto image = handle_.image()) {
      fallback_ = ImageHandle();
      return image;
    }
    return fallback_.image();
  }

  /// The picture's own size in logical units.
  ///
  /// Read off the *source* dimensions, not the decoded ones. A thumbnail is
  /// decoded to fit its box, so the two differ by design -- and if this used
  /// the decoded size the widget's natural size would be a function of the box
  /// it was measured in, which is circular: `object-fit: none` would mean
  /// nothing, and an `auto`-sized image would settle at whatever size it
  /// happened to be asked for first.
  Size<float> natural_size_of_(const Image &image) const {
    return Size<float>(static_cast<float>(image.source_width()) / density_,
                       static_cast<float>(image.source_height()) / density_);
  }

  bool placeholder_holds_nothing_() const {
    const Color *color = std::get_if<Color>(&placeholder_);
    return color != nullptr && color->a == 0;
  }

  /// The logical box the picture could occupy at most.
  ///
  /// A definite style width is tighter than the constraint and is what the
  /// picture will actually get, so it wins. An indefinite one leaves the
  /// constraint, which may be infinite -- a column gives unbounded height --
  /// and an infinity here simply means "no limit on that axis", which is what
  /// the decoder wants to hear anyway.
  static Size<float> room_for_(const Constraints &constraints,
                               const ComputedStyle &style) {
    const Size<Length> declared = style.layout_size();

    float width = constraints.max_width;
    if (const auto *fixed = std::get_if<Length::Fixed>(&declared.width.value))
      width = std::min(width, fixed->value);

    float height = constraints.max_height;
    if (const auto *fixed = std::get_if<Length::Fixed>(&declared.height.value))
      height = std::min(height, fixed->value);

    return Size<float>(width, height);
  }

  void ensure_load_(Size<float> room, float device_scale,
                    const Invalidator &invalidator) {
    if (source_.empty())
      return;

    // `None` means "draw at native resolution", so bounding the decode to the
    // box would be answering a different question: the widget would crop a
    // reduction rather than the picture, and every pixel it showed would be an
    // upscale of one. It is the one fit that has to pay for a full decode.
    const bool native = fit_ == ObjectFit::None;
    const int width = native ? 0 : to_request_(room.width, device_scale);
    const int height = native ? 0 : to_request_(room.height, device_scale);

    // Bucketed before the comparison, or a window being dragged one pixel wider
    // would re-enter the cache every frame to be told the same answer.
    const int bucket_width = image_size_bucket(width);
    const int bucket_height = image_size_bucket(height);

    if (handle_.state() != ImageHandle::State::Empty &&
        bucket_width == requested_width_ && bucket_height == requested_height_)
      return;

    ImageRequest request;
    request.max_width = width;
    request.max_height = height;
    request.invalidator = invalidator;

    // A new decode size must not erase pixels already on screen. In particular,
    // Windows keeps painting inside its modal resize loop while worker results
    // wait for the normal event loop to resume. Keep one ready handle across as
    // many size changes as happen before a replacement is ready.
    if (handle_.ready())
      fallback_ = handle_;
    handle_ = ImageCache::global().acquire(source_, request);
    requested_width_ = bucket_width;
    requested_height_ = bucket_height;
  }

  /// A logical extent as the pixel count a decoder should aim at. An unbounded
  /// axis becomes zero, which is how "no limit" is spelled downstream.
  static int to_request_(float logical, float device_scale) {
    if (!std::isfinite(logical) || logical <= 0.0f)
      return 0;
    return static_cast<int>(logical * device_scale + 0.5f);
  }

  /// How far into the fade this frame is, and the request for the next one.
  float fade_opacity_(const DrawContext &ctx) const {
    if (fade_ <= 0.0f)
      return 1.0f;

    if (appeared_ < 0.0) {
      // First frame the pixels are drawn on. The clock is the tree's, not one
      // sampled here, so every widget fading in this frame agrees on when now
      // is.
      appeared_ = ctx.now();
    }

    const float elapsed = static_cast<float>(ctx.now() - appeared_);
    if (elapsed >= fade_)
      return 1.0f;

    ctx.request_frame();
    return std::clamp(elapsed / fade_, 0.0f, 1.0f);
  }

  ImageSource source_;
  ImageHandle handle_;
  // A handle keeps these visible pixels outside the cache's eviction budget.
  // Released as soon as measurement or drawing observes a ready replacement.
  mutable ImageHandle fallback_;

  ObjectFit fit_ = ObjectFit::Contain;
  ImageAlignment alignment_;
  float density_ = 1.0f;
  Size<float> natural_{0.0f, 0.0f};
  Brush placeholder_ = Color::TRANSPARENT;
  float fade_ = 0.0f;

  /// The bucketed size the current handle was acquired for. Kept so a layout
  /// pass that changes nothing costs a comparison rather than a hash lookup.
  int requested_width_ = 0;
  int requested_height_ = 0;

  /// When the picture was first painted, or negative before that. Mutable
  /// because it is written while drawing, which is the only moment that knows
  /// the pixels actually reached the screen.
  mutable double appeared_ = -1.0;
};

[[nodiscard]] inline ImageView image(std::string_view uri) {
  return ImageView(uri);
}

[[nodiscard]] inline ImageView image(std::shared_ptr<const Image> pixels) {
  return ImageView(std::move(pixels));
}

[[nodiscard]] inline ImageView image(ImageSource source) {
  return ImageView(std::move(source));
}

} // namespace voidui
