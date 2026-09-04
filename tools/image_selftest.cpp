// Exercise of the CPU-side image layer and the recording end of image drawing:
// pixel storage and its keep-alive, the premultiplication contract, the codec
// registry's dispatch and ordering, and the source-region normalisation that
// `object-fit: cover` is built on.
//
// Nothing here touches a device. The point is that everything a decoder and a
// widget rely on can be checked without one -- what reaches the GPU is a
// DisplayList, so that is what gets inspected.
#include "voidui/core/async/executor.h"
#include "voidui/paint/display_list.h"
#include "voidui/paint/image.h"
#include "voidui/paint/image_cache.h"
#include "voidui/paint/image_codec.h"
#include "voidui/paint/painter.h"
#include "voidui/widgets/image.h"

#include "image_fixtures.h"

#include <chrono>
#include <cmath>
#include <cstdio>
#include <cstdint>
#include <cstring>
#include <span>
#include <string>
#include <thread>
#include <vector>

using namespace voidui;

namespace {

int failures = 0;

void check(bool condition, const char *what) {
  std::printf("%s: %s\n", condition ? "ok  " : "FAIL", what);
  if (!condition)
    ++failures;
}

bool close(float a, float b) { return std::fabs(a - b) < 1e-4f; }

/// A solid image of a known size; the contents never matter here, only the
/// dimensions the source region is normalised against.
std::shared_ptr<Image> solid(int width, int height, std::uint8_t alpha = 255) {
  std::vector<std::uint8_t> pixels(static_cast<std::size_t>(width) * height * 4);
  for (std::size_t i = 0; i < pixels.size(); i += 4) {
    pixels[i + 0] = 200;
    pixels[i + 1] = 100;
    pixels[i + 2] = 50;
    pixels[i + 3] = alpha;
  }
  return Image::from_rgba8(std::move(pixels), width, height);
}

std::span<const std::byte> bytes_of(const unsigned char *data, std::size_t size) {
  return {reinterpret_cast<const std::byte *>(data), size};
}

/// Claims a two-byte signature of its own invention, so the registry's dispatch
/// and ordering can be checked without depending on which real codecs a
/// platform happens to provide.
class FakeDecoder final : public ImageDecoder {
public:
  FakeDecoder(std::string name, unsigned char tag, int size)
      : name_(std::move(name)), tag_(tag), size_(size) {}

  std::string_view name() const override { return name_; }

  bool sniff(std::span<const std::byte> prefix) const override {
    return prefix.size() >= 2 &&
           static_cast<unsigned char>(prefix[0]) == 'V' &&
           static_cast<unsigned char>(prefix[1]) == tag_;
  }

  ImageResult<ImageInfo> probe(std::span<const std::byte>) const override {
    return ImageInfo{size_, size_, false};
  }

  ImageResult<std::shared_ptr<Image>>
  decode(std::span<const std::byte>, const DecodeOptions &) const override {
    if (size_ == 0)
      return std::unexpected(ImageError::Malformed);
    return solid(size_, size_);
  }

private:
  std::string name_;
  unsigned char tag_ = 0;
  int size_ = 0;
};

const DrawCommand *only_image(const DisplayList &list) {
  const DrawCommand *found = nullptr;
  for (const DrawCommand &command : list.commands()) {
    if (command.kind != CommandKind::Image)
      continue;
    if (found)
      return nullptr;
    found = &command;
  }
  return found;
}

} // namespace

int main() {
  // -- storage and format ----------------------------------------------------

  {
    std::shared_ptr<Image> image = solid(4, 3);
    check(image != nullptr, "a well-formed rgba8 buffer makes an image");
    check(image->width() == 4 && image->height() == 3,
          "an image reports the dimensions it was built with");
    check(image->pixels().size() == 4u * 3u * 4u,
          "pixels are tightly packed at four bytes per texel");
    check(image->byte_size() == 4u * 3u * 4u,
          "byte_size is what holding the image costs");
    check(image->format() == PixelFormat::Rgba8Straight,
          "from_rgba8 declares its pixels straight, not premultiplied");

    // The renderer's upload path holds this Blob across the frame boundary
    // rather than copying the pixels, so it has to keep them alive on its own.
    Blob kept = image->storage();
    const void *address = kept.data();
    image.reset();
    check(kept.data() == address && kept.size() == 4u * 3u * 4u,
          "the pixel storage outlives the image that produced it");
  }

  {
    std::vector<std::uint8_t> pixels(2u * 2u * 4u, 0u);
    check(Image::from_rgba8(pixels, 0, 4) == nullptr,
          "a zero dimension is refused rather than making an empty image");
    check(Image::from_rgba8(pixels, 4, 4) == nullptr,
          "a buffer smaller than the declared size is refused");

    // A decoder hands over premultiplied pixels so the upload path can stage
    // them as they stand; the format is how it says so.
    std::shared_ptr<Image> decoded = Image::from_pixels(
        Blob::own(std::move(pixels)), 2, 2, PixelFormat::Rgba8Premultiplied);
    check(decoded && decoded->format() == PixelFormat::Rgba8Premultiplied,
          "from_pixels carries the format its caller declared");
  }

  {
    std::shared_ptr<Image> a = solid(2, 2);
    std::shared_ptr<Image> b = solid(2, 2);
    check(a->id() != b->id(),
          "identical pixels decoded twice are still two ids, and two uploads");
  }

  // -- recording: the whole image --------------------------------------------

  {
    DisplayList list;
    Painter painter(list, Size<float>(200.0f, 200.0f));
    std::shared_ptr<Image> image = solid(100, 50);
    painter.draw_image(image, Rect<float>(10.0f, 20.0f, 30.0f, 40.0f));

    const DrawCommand *command = only_image(list);
    check(command != nullptr, "drawing an image records exactly one command");
    check(command && close(command->rect.origin.x, 10.0f) &&
              close(command->rect.size.width, 30.0f),
          "the destination is recorded in local coordinates");
    check(command && close(command->source[0], 0.0f) &&
              close(command->source[1], 0.0f) && close(command->source[2], 1.0f) &&
              close(command->source[3], 1.0f),
          "an uncropped draw covers the whole source");
  }

  {
    DisplayList list;
    Painter painter(list, Size<float>(200.0f, 200.0f));
    painter.draw_image(nullptr, Rect<float>(0.0f, 0.0f, 10.0f, 10.0f));
    painter.draw_image(solid(4, 4), Rect<float>(0.0f, 0.0f, 0.0f, 10.0f));
    check(list.commands().empty(),
          "a null image and a collapsed destination record nothing");
  }

  // -- recording: a source region --------------------------------------------

  {
    DisplayList list;
    Painter painter(list, Size<float>(200.0f, 200.0f));
    std::shared_ptr<Image> image = solid(100, 50);

    // The centre half of a 100x50 image: the shape `object-fit: cover` asks for
    // when a wide picture has to fill a narrower box.
    painter.draw_image(image, Rect<float>(0.0f, 0.0f, 50.0f, 50.0f),
                       Rect<float>(25.0f, 0.0f, 50.0f, 50.0f));

    const DrawCommand *command = only_image(list);
    check(command && close(command->source[0], 0.25f) &&
              close(command->source[2], 0.75f),
          "a source region is normalised against the image's own width");
    check(command && close(command->source[1], 0.0f) &&
              close(command->source[3], 1.0f),
          "and against its height independently");
  }

  {
    DisplayList list;
    Painter painter(list, Size<float>(200.0f, 200.0f));
    std::shared_ptr<Image> image = solid(100, 50);

    // A fit that rounds outward can name a fraction of a pixel past the edge.
    // Clamping keeps that drawable; refusing it would be the worse answer.
    painter.draw_image(image, Rect<float>(0.0f, 0.0f, 50.0f, 50.0f),
                       Rect<float>(-10.0f, -10.0f, 200.0f, 200.0f));

    const DrawCommand *command = only_image(list);
    check(command && close(command->source[0], 0.0f) &&
              close(command->source[1], 0.0f) && close(command->source[2], 1.0f) &&
              close(command->source[3], 1.0f),
          "a source region reaching past the image is clamped to it");
  }

  {
    DisplayList list;
    Painter painter(list, Size<float>(200.0f, 200.0f));
    std::shared_ptr<Image> image = solid(100, 50);

    painter.draw_image(image, Rect<float>(0.0f, 0.0f, 10.0f, 10.0f),
                       Rect<float>(10.0f, 10.0f, 0.0f, 10.0f));
    painter.draw_image(image, Rect<float>(0.0f, 0.0f, 10.0f, 10.0f),
                       Rect<float>(200.0f, 0.0f, 50.0f, 10.0f));
    check(list.commands().empty(),
          "a source region that is empty, or entirely outside, records nothing");
  }

  // -- recording: transform and opacity --------------------------------------

  {
    DisplayList list;
    Painter painter(list, Size<float>(200.0f, 200.0f));
    std::shared_ptr<Image> image = solid(20, 20);

    Paint paint(Color(255, 255, 255));
    paint.opacity = 0.5f;

    painter.save();
    painter.opacity(0.5f);
    painter.translate(7.0f, 9.0f);
    painter.draw_image(image, Rect<float>(0.0f, 0.0f, 20.0f, 20.0f),
                       Rect<float>(0.0f, 0.0f, 10.0f, 20.0f), paint);
    painter.restore();

    const DrawCommand *command = only_image(list);
    check(command && close(command->opacity, 0.25f),
          "the painter's opacity and the paint's multiply, as for any command");
    check(command && command->transform.is_translation() &&
              close(command->transform.e, 7.0f) &&
              close(command->transform.f, 9.0f),
          "the source overload carries the current transform like every other");
    check(command && close(command->source[2], 0.5f),
          "and still crops");
  }

  // -- the codec registry ----------------------------------------------------

  {
    ImageCodecs codecs;
    const unsigned char a[] = {'V', 'a', 0, 0};
    const unsigned char b[] = {'V', 'b', 0, 0};
    const unsigned char z[] = {'?', '?', 0, 0};

    check(!codecs.decode(bytes_of(a, sizeof(a))).has_value() &&
              codecs.decode(bytes_of(a, sizeof(a))).error() ==
                  ImageError::Unsupported,
          "an empty registry reports unsupported rather than crashing");

    codecs.add(std::make_shared<FakeDecoder>("a", 'a', 8));
    codecs.add(std::make_shared<FakeDecoder>("b", 'b', 16));

    ImageResult<std::shared_ptr<Image>> first = codecs.decode(bytes_of(a, sizeof(a)));
    check(first.has_value() && (*first)->width() == 8,
          "a decode goes to the decoder whose signature matches");

    ImageResult<std::shared_ptr<Image>> second = codecs.decode(bytes_of(b, sizeof(b)));
    check(second.has_value() && (*second)->width() == 16,
          "and a different signature reaches a different decoder");

    check(!codecs.decode(bytes_of(z, sizeof(z))).has_value(),
          "bytes no decoder claims are unsupported");
    check(!codecs.decode({}).has_value(),
          "an empty buffer is unsupported, not a null dereference");

    ImageResult<ImageInfo> info = codecs.probe(bytes_of(b, sizeof(b)));
    check(info.has_value() && info->width == 16,
          "probe dispatches the same way decode does");
  }

  {
    ImageCodecs codecs;
    const unsigned char a[] = {'V', 'a', 0, 0};

    codecs.add(std::make_shared<FakeDecoder>("first", 'a', 1));
    codecs.add(std::make_shared<FakeDecoder>("second", 'a', 2));
    ImageResult<std::shared_ptr<Image>> tie = codecs.decode(bytes_of(a, sizeof(a)));
    check(tie.has_value() && (*tie)->width() == 1,
          "two decoders at one priority keep the order they were added in");

    codecs.add(std::make_shared<FakeDecoder>("preferred", 'a', 3), 10);
    ImageResult<std::shared_ptr<Image>> ranked =
        codecs.decode(bytes_of(a, sizeof(a)));
    check(ranked.has_value() && (*ranked)->width() == 3,
          "a higher priority overtakes decoders already registered");

    const std::vector<std::string_view> names = codecs.decoders();
    check(names.size() == 3 && names[0] == "preferred" && names[1] == "first",
          "the listing reports them in the order they will be tried");
  }

  {
    ImageCodecs codecs;
    const unsigned char a[] = {'V', 'a', 0, 0};
    codecs.add(std::make_shared<FakeDecoder>("broken", 'a', 0));
    codecs.add(std::make_shared<FakeDecoder>("healthy", 'a', 4), -1);

    ImageResult<std::shared_ptr<Image>> result = codecs.decode(bytes_of(a, sizeof(a)));
    check(!result.has_value() && result.error() == ImageError::Malformed,
          "the decoder that claimed the bytes owns the failure, without "
          "falling through");
  }

  // -- the platform decoder --------------------------------------------------
  //
  // Skipped where a build has none, so this test stays meaningful on a platform
  // whose codec is still the stub.

  if (platform_image_decoder()) {
    const ImageCodecs &codecs = ImageCodecs::global();
    const std::span<const std::byte> tiny =
        bytes_of(fixtures::kTinyPng, sizeof(fixtures::kTinyPng));

    ImageResult<ImageInfo> info = codecs.probe(tiny);
    check(info.has_value() && info->width == 2 && info->height == 2,
          "the platform decoder reads a png's size out of its header");
    check(info.has_value() && info->has_alpha,
          "and reports that an rgba png carries alpha");

    DecodeOptions straight;
    straight.premultiply = false;
    ImageResult<std::shared_ptr<Image>> plain = codecs.decode(tiny, straight);
    check(plain.has_value() && (*plain)->width() == 2 && (*plain)->height() == 2,
          "a png decodes to its declared size");

    if (plain.has_value()) {
      const std::span<const std::uint8_t> p = (*plain)->pixels();
      check((*plain)->format() == PixelFormat::Rgba8Straight,
            "an unpremultiplied decode says so");
      check(p[0] == 255 && p[1] == 0 && p[2] == 0 && p[3] == 255,
            "channels arrive in rgba order, not the platform's bgra");
      check(p[4] == 0 && p[5] == 255 && p[6] == 0,
            "and the second texel is the second texel, not a row apart");
      check(p[12] == 255 && p[13] == 255 && p[14] == 255 && p[15] == 128,
            "a half-transparent texel keeps its colour when not premultiplied");
    }

    ImageResult<std::shared_ptr<Image>> premultiplied = codecs.decode(tiny);
    check(premultiplied.has_value() &&
              (*premultiplied)->format() == PixelFormat::Rgba8Premultiplied,
          "premultiplying is the default, and is declared");

    if (premultiplied.has_value()) {
      const std::span<const std::uint8_t> p = (*premultiplied)->pixels();
      // 255 * 128/255 == 128, so the white texel darkens to its own alpha.
      check(p[12] == 128 && p[13] == 128 && p[14] == 128 && p[15] == 128,
            "and the decoder, not the upload path, is what did the multiplying");
      check(p[0] == 255 && p[3] == 255,
            "an opaque texel is untouched by premultiplication");
    }

    // The whole reason to prefer a platform codec: the result is small because
    // it was never large, not because it was shrunk afterwards.
    const std::span<const std::byte> solid64 =
        bytes_of(fixtures::kSolid64Png, sizeof(fixtures::kSolid64Png));

    DecodeOptions fitted;
    fitted.max_width = 16;
    fitted.max_height = 16;
    ImageResult<std::shared_ptr<Image>> small = codecs.decode(solid64, fitted);
    check(small.has_value() && (*small)->width() == 16 && (*small)->height() == 16,
          "a decode bounded to a smaller box comes back at that size");
    check(small.has_value() && (*small)->byte_size() == 16u * 16u * 4u,
          "and costs what its own size costs, not what the source would have");

    DecodeOptions loose;
    loose.max_width = 4096;
    ImageResult<std::shared_ptr<Image>> native = codecs.decode(solid64, loose);
    check(native.has_value() && (*native)->width() == 64,
          "a box larger than the image does not enlarge it");

    DecodeOptions one_axis;
    one_axis.max_width = 32;
    ImageResult<std::shared_ptr<Image>> ratio = codecs.decode(solid64, one_axis);
    check(ratio.has_value() && (*ratio)->width() == 32 && (*ratio)->height() == 32,
          "bounding one axis scales the other with it");

    DecodeOptions tight;
    tight.size_limit = 8;
    ImageResult<std::shared_ptr<Image>> refused = codecs.decode(solid64, tight);
    check(!refused.has_value() && refused.error() == ImageError::TooLarge,
          "a size limit is checked against the header, before any allocation");

    const unsigned char garbage[] = {0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A,
                                     0x0A, 'n', 'o', 'p', 'e'};
    ImageResult<std::shared_ptr<Image>> broken =
        codecs.decode(bytes_of(garbage, sizeof(garbage)));
    check(!broken.has_value() && broken.error() == ImageError::Malformed,
          "bytes that pass the signature and fail the decode report malformed");

    const unsigned char text[] = {'h', 'e', 'l', 'l', 'o', ' ', 't',
                                  'h', 'e', 'r', 'e', '!'};
    ImageResult<std::shared_ptr<Image>> not_an_image =
        codecs.decode(bytes_of(text, sizeof(text)));
    check(!not_an_image.has_value() &&
              not_an_image.error() == ImageError::Unsupported,
          "and something that is not an image at all reports unsupported");
  } else {
    std::printf("skip: no platform decoder in this build\n");
  }

  // -- fitting -----------------------------------------------------------------
  //
  // A 200x100 picture in a 100x100 box: wide against square, so every fit lands
  // somewhere different. Against a box of the picture's own shape they would all
  // agree, which is exactly why that is the wrong thing to test with.

  {
    const Size<float> source(200.0f, 100.0f);
    const Rect<float> box(10.0f, 20.0f, 100.0f, 100.0f);

    const ImagePlacement fill = fit_image(source, box, ObjectFit::Fill);
    check(close(fill.destination.size.width, 100.0f) &&
              close(fill.destination.size.height, 100.0f) &&
              close(fill.source.size.width, 200.0f),
          "fill takes the whole box and the whole source, aspect be damned");

    const ImagePlacement contain = fit_image(source, box, ObjectFit::Contain);
    check(close(contain.destination.size.width, 100.0f) &&
              close(contain.destination.size.height, 50.0f),
          "contain shrinks to the tighter axis");
    check(close(contain.destination.origin.y, 20.0f + 25.0f),
          "and centres what is left over");
    check(close(contain.source.size.width, 200.0f) &&
              close(contain.source.size.height, 100.0f),
          "contain crops nothing");

    const ImagePlacement cover = fit_image(source, box, ObjectFit::Cover);
    check(close(cover.destination.size.width, 100.0f) &&
              close(cover.destination.size.height, 100.0f),
          "cover fills the box");
    check(close(cover.source.size.width, 100.0f) &&
              close(cover.source.size.height, 100.0f),
          "and pays for it out of the source, not the destination");
    check(close(cover.source.origin.x, 50.0f) && close(cover.source.origin.y, 0.0f),
          "the crop is centred on the axis that overflowed, and only that one");

    const ImagePlacement top_left =
        fit_image(source, box, ObjectFit::Cover, ImageAlignment::top_left());
    check(close(top_left.source.origin.x, 0.0f),
          "alignment decides which part of a crop survives");

    const ImagePlacement none = fit_image(source, box, ObjectFit::None);
    check(close(none.destination.size.width, 100.0f) &&
              close(none.source.size.width, 100.0f),
          "none shows a box-sized window onto the source");
    check(close(none.destination.size.height, 100.0f) &&
              close(none.source.size.height, 100.0f),
          "on both axes");

    const ImagePlacement scale_down =
        fit_image(Size<float>(40.0f, 20.0f), box, ObjectFit::ScaleDown);
    check(close(scale_down.destination.size.width, 40.0f),
          "scale-down leaves a picture smaller than its box alone");
    const ImagePlacement shrunk = fit_image(source, box, ObjectFit::ScaleDown);
    check(close(shrunk.destination.size.width, 100.0f),
          "and still shrinks one that is larger");

    check(fit_image(Size<float>(0.0f, 10.0f), box, ObjectFit::Cover).empty &&
              fit_image(source, Rect<float>(0.0f, 0.0f, 0.0f, 10.0f),
                        ObjectFit::Cover)
                  .empty,
          "a degenerate picture or box places nothing");
  }

  // -- decode-size bucketing ---------------------------------------------------

  {
    check(image_size_bucket(0) == 0 && image_size_bucket(-4) == 0,
          "an unbounded axis stays unbounded");
    check(image_size_bucket(1) == 32 && image_size_bucket(32) == 32,
          "everything small shares one bucket");
    check(image_size_bucket(33) == 64 && image_size_bucket(64) == 64,
          "and above it they round up to 64");
    check(image_size_bucket(65) == 128, "rounding up, never down");
    check(image_size_bucket(1025) == 1280 && image_size_bucket(1500) == 1536,
          "the step widens past 1024, where a bucket is a rounding error");
    check(image_size_bucket(200) == image_size_bucket(201) &&
              image_size_bucket(200) == image_size_bucket(255),
          "a window dragged a pixel wider asks for the size it already has");
  }

  // -- source identity ---------------------------------------------------------

  {
    check(ImageSource().empty(), "a default source is empty");
    check(ImageSource::uri("").empty(),
          "a reference naming nothing is empty rather than a load that fails");

    const ImageSource a = ImageSource::uri("res://a.png");
    const ImageSource b = ImageSource::uri("res://b.png");
    const ImageSource a_again = ImageSource::uri("res://./a.png");
    check(!a.empty() && a.key() != b.key(),
          "two different names are two different pictures");
    check(a.key() == a_again.key(),
          "and two spellings of one name are one, because the uri normalises");

    const std::shared_ptr<Image> pixels = solid(4, 4);
    check(ImageSource::ready(pixels).key() == ImageSource::ready(pixels).key(),
          "an already-decoded image keys on itself");
    check(ImageSource::bytes(Blob::own(std::vector<std::uint8_t>{1, 2, 3}), 7)
                  .key() !=
              ImageSource::bytes(Blob::own(std::vector<std::uint8_t>{1, 2, 3}), 8)
                  .key(),
          "byte sources key on the tag their caller supplied");
  }

  // -- the cache ---------------------------------------------------------------
  //
  // Driven headless. The cache hands results back through the UI queue rather
  // than blocking, so a test needs an executor to drain -- not a window, which
  // is the point: the hand-off is a property of the executor, not of SDL.

  {
    async::UiExecutor executor;
    async::detail::UiExecutorScope scope(executor);

    auto memory = std::make_shared<MemoryProvider>();
    memory->add("tiny.png",
                Blob::borrow(bytes_of(fixtures::kTinyPng, sizeof(fixtures::kTinyPng))));
    memory->add("solid.png", Blob::borrow(bytes_of(fixtures::kSolid64Png,
                                                   sizeof(fixtures::kSolid64Png))));
    memory->add("junk.png", Blob::own(std::vector<std::uint8_t>{'n', 'o', 'p', 'e'}));

    Resources resources;
    resources.mount("", memory);

    ImageCache cache;
    cache.set_resources(&resources);

    // Pumps until `handle` settles. A real application never does this -- the
    // event loop drains for it and the invalidator asks for the frame.
    const auto settle = [&executor](ImageHandle &handle) {
      for (int i = 0; i < 2000 && handle.loading(); ++i) {
        executor.drain(0.0);
        if (handle.loading())
          std::this_thread::sleep_for(std::chrono::milliseconds(1));
      }
      return !handle.loading();
    };

    ImageRequest request;
    ImageHandle handle = cache.acquire(ImageSource::uri("res://solid.png"), request);
    check(handle.loading(), "a fresh acquire is loading, not blocking");
    check(settle(handle) && handle.ready(), "and lands once the queue is drained");
    check(handle.image() && handle.image()->width() == 64,
          "an unbounded request decodes the picture at its own size");

    // The line that matters in a list: the second caller attaches rather than
    // starting a second read, a second decode and a second upload.
    ImageHandle again = cache.acquire(ImageSource::uri("res://solid.png"), request);
    check(again.ready() && again.image() == handle.image(),
          "a second acquire of the same picture is the same Image object");
    check(cache.size() == 1, "and one entry, not two");

    ImageRequest small;
    small.max_width = 16;
    small.max_height = 16;
    ImageHandle thumbnail =
        cache.acquire(ImageSource::uri("res://solid.png"), small);
    check(settle(thumbnail) && thumbnail.ready(),
          "the same picture at another size loads on its own");
    check(thumbnail.image() && thumbnail.image()->width() == 32,
          "at the bucket its size rounds up to, never smaller than asked for");
    check(thumbnail.image() != handle.image() && cache.size() == 2,
          "so the decode size is part of the identity, not a hint");
    check(thumbnail.image()->source_width() == 64,
          "and the reduction still reports what the source measured");

    ImageRequest rounded = small;
    rounded.max_width = 20;
    rounded.max_height = 20;
    ImageHandle bucketed =
        cache.acquire(ImageSource::uri("res://solid.png"), rounded);
    check(bucketed.ready() && bucketed.image() == thumbnail.image(),
          "two sizes in one bucket are one entry");

    ImageHandle missing = cache.acquire(ImageSource::uri("res://absent.png"), request);
    check(settle(missing) && missing.failed() &&
              missing.error() == ImageError::NotFound,
          "a picture that is not there fails rather than hanging");

    ImageHandle junk = cache.acquire(ImageSource::uri("res://junk.png"), request);
    check(settle(junk) && junk.failed(), "and so does one that will not decode");
    check(cache.size() == 4,
          "a failed entry stays while a handle still needs to report its error");

    missing = ImageHandle();
    junk = ImageHandle();
    check(cache.size() == 2,
          "and is forgotten the moment nobody is asking, so a retry can happen");

    // Retention: dropping the last handle keeps the pixels, up to the budget.
    const std::size_t held = handle.image()->byte_size();
    handle = ImageHandle();
    again = ImageHandle();
    check(cache.bytes_retained() == held,
          "the last handle going hands the pixels to the budget, not the floor");
    check(cache.size() == 2, "and the entry stays, ready for the next caller");

    ImageHandle back = cache.acquire(ImageSource::uri("res://solid.png"), request);
    check(back.ready() && cache.bytes_retained() == 0,
          "asking again takes it straight back out of retention");

    back = ImageHandle();
    cache.set_byte_budget(held / 2);
    check(cache.bytes_retained() == 0,
          "lowering the budget below what is retained evicts at once");

    cache.set_byte_budget(64 * 1024 * 1024);
    thumbnail = ImageHandle();
    bucketed = ImageHandle();
    check(cache.bytes_retained() > 0, "and raising it lets retention resume");
    cache.clear();
    check(cache.bytes_retained() == 0 && cache.size() == 0,
          "clear drops everything nothing is holding");

    // Already-decoded pixels are not cache traffic at all.
    ImageHandle direct =
        cache.acquire(ImageSource::ready(solid(8, 8)), request);
    check(direct.ready() && cache.size() == 0,
          "an image handed over ready is never an entry");

    ImageHandle nothing = cache.acquire(ImageSource(), request);
    check(nothing.failed() && nothing.error() == ImageError::NotFound,
          "an empty source fails immediately rather than loading forever");

    // -- remote --------------------------------------------------------------
    //
    // A provider of our own rather than a socket. What is being checked is that
    // a URL travels the same road as a resource: the widget names it, the read
    // happens on the blocking lane, the decode on a worker, and the same cache
    // dedupes it. Which stack fetches the bytes is the provider's business.

    auto network = std::make_shared<MemoryProvider>();
    network->add("https://example.com/solid.png",
                 Blob::borrow(bytes_of(fixtures::kSolid64Png,
                                       sizeof(fixtures::kSolid64Png))));
    resources.set_network_provider(network);

    ImageHandle remote =
        cache.acquire(ImageSource::uri("https://example.com/solid.png"), request);
    check(settle(remote) && remote.ready() && remote.image()->width() == 64,
          "a picture named by url loads exactly as one named by resource does");

    ImageHandle remote_again =
        cache.acquire(ImageSource::uri("https://example.com/solid.png"), request);
    check(remote_again.image() == remote.image(),
          "and is deduplicated on the same terms");

    ImageHandle absent =
        cache.acquire(ImageSource::uri("https://example.com/gone.png"), request);
    check(settle(absent) && absent.failed() &&
              absent.error() == ImageError::NotFound,
          "a url that is not there fails rather than hanging a row forever");

    check(ImageSource::uri("https://example.com/a.png").key() !=
              ImageSource::uri("http://example.com/a.png").key(),
          "and http is not https, even to the cache");
  }

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
