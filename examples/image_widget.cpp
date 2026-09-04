// The ImageView widget, over the real loading path.
//
// The PNGs are written to a temporary directory at startup and read back
// through the resource layer, so this exercises what an application actually
// does -- mount, read on the blocking lane, decode on a worker, cache, draw --
// rather than handing the widget pixels it already had.
//
// The point the layout is arranged to show is the decode size. The photograph
// is 1600x1000, which is 6.4 MB of pixels; every view of it here is a few
// hundred logical units wide, and none of them ever holds more than a few
// hundred pixels' worth. Run with VOIDUI_LOG_IMAGE=1 to see what was decoded.

#include "voidui/core/http.h"
#include "voidui/core/resource.h"
#include "voidui/core/window.h"
#include "voidui/paint/image_cache.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/image.h"
#include "voidui/widgets/row.h"

#include <cmath>
#include <cstdint>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <string>
#include <vector>

using namespace voidui;

namespace {

// -- a minimal PNG writer
// ------------------------------------------------------
//
// Deflate's stored-block form is uncompressed, which makes a writer short
// enough to justify itself here: it keeps a multi-megabyte fixture out of the
// repository and lets the example generate a genuinely large source image,
// which is the only way to demonstrate what decoding to the display size saves.

std::uint32_t crc32_of(const std::uint8_t *data, std::size_t size,
                       std::uint32_t crc = 0xFFFFFFFFu) {
  static std::uint32_t table[256];
  static const bool built = [] {
    for (std::uint32_t i = 0; i < 256; ++i) {
      std::uint32_t c = i;
      for (int k = 0; k < 8; ++k)
        c = (c & 1) ? 0xEDB88320u ^ (c >> 1) : c >> 1;
      table[i] = c;
    }
    return true;
  }();
  (void)built;

  for (std::size_t i = 0; i < size; ++i)
    crc = table[(crc ^ data[i]) & 0xFF] ^ (crc >> 8);
  return crc;
}

void put_be32(std::vector<std::uint8_t> &out, std::uint32_t value) {
  out.push_back(static_cast<std::uint8_t>(value >> 24));
  out.push_back(static_cast<std::uint8_t>(value >> 16));
  out.push_back(static_cast<std::uint8_t>(value >> 8));
  out.push_back(static_cast<std::uint8_t>(value));
}

void put_chunk(std::vector<std::uint8_t> &out, const char tag[4],
               const std::vector<std::uint8_t> &data) {
  put_be32(out, static_cast<std::uint32_t>(data.size()));

  const std::size_t start = out.size();
  out.insert(out.end(), tag, tag + 4);
  out.insert(out.end(), data.begin(), data.end());

  const std::uint32_t crc =
      crc32_of(out.data() + start, out.size() - start) ^ 0xFFFFFFFFu;
  put_be32(out, crc);
}

/// A zlib stream of stored (uncompressed) deflate blocks.
std::vector<std::uint8_t> stored_zlib(const std::vector<std::uint8_t> &raw) {
  std::vector<std::uint8_t> out;
  out.push_back(0x78); // CMF: deflate, 32K window
  out.push_back(0x01); // FLG: no dictionary, fastest

  std::size_t offset = 0;
  while (offset < raw.size() || raw.empty()) {
    const std::size_t chunk = std::min<std::size_t>(raw.size() - offset, 65535);
    const bool last = offset + chunk >= raw.size();

    out.push_back(last ? 1 : 0);
    out.push_back(static_cast<std::uint8_t>(chunk & 0xFF));
    out.push_back(static_cast<std::uint8_t>(chunk >> 8));
    out.push_back(static_cast<std::uint8_t>(~chunk & 0xFF));
    out.push_back(static_cast<std::uint8_t>((~chunk >> 8) & 0xFF));
    out.insert(out.end(), raw.begin() + offset, raw.begin() + offset + chunk);

    offset += chunk;
    if (last)
      break;
  }

  // Adler-32 of the uncompressed data.
  std::uint32_t a = 1;
  std::uint32_t b = 0;
  for (const std::uint8_t byte : raw) {
    a = (a + byte) % 65521;
    b = (b + a) % 65521;
  }
  put_be32(out, (b << 16) | a);
  return out;
}

template <class Fn>
bool write_png(const std::filesystem::path &path, int width, int height,
               Fn &&pixel) {
  std::vector<std::uint8_t> raw;
  raw.reserve(static_cast<std::size_t>(height) * (1 + width * 4));

  for (int y = 0; y < height; ++y) {
    raw.push_back(0); // per-row filter: none
    for (int x = 0; x < width; ++x) {
      const std::array<std::uint8_t, 4> texel = pixel(x, y);
      raw.insert(raw.end(), texel.begin(), texel.end());
    }
  }

  std::vector<std::uint8_t> header;
  put_be32(header, static_cast<std::uint32_t>(width));
  put_be32(header, static_cast<std::uint32_t>(height));
  header.push_back(8); // bit depth
  header.push_back(6); // colour type: RGBA
  header.push_back(0); // deflate
  header.push_back(0); // adaptive filtering
  header.push_back(0); // no interlace

  std::vector<std::uint8_t> png{0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A};
  put_chunk(png, "IHDR", header);
  put_chunk(png, "IDAT", stored_zlib(raw));
  put_chunk(png, "IEND", {});

  std::ofstream file(path, std::ios::binary);
  if (!file)
    return false;
  file.write(reinterpret_cast<const char *>(png.data()),
             static_cast<std::streamsize>(png.size()));
  return file.good();
}

// -- the fixtures
// --------------------------------------------------------------

std::uint8_t byte_of(float unit) {
  return static_cast<std::uint8_t>(std::clamp(unit, 0.0f, 1.0f) * 255.0f +
                                   0.5f);
}

/// Big enough that decoding it natively for a thumbnail would be absurd, and
/// detailed enough that a resample is visible if one happens.
constexpr int kPhotoWidth = 1600;
constexpr int kPhotoHeight = 1000;

std::array<std::uint8_t, 4> photo_texel(int x, int y) {
  const float u = static_cast<float>(x) / kPhotoWidth;
  const float v = static_cast<float>(y) / kPhotoHeight;
  const float a = 0.5f + 0.5f * std::sin(u * 5.0f + std::cos(v * 3.0f) * 1.5f);
  const float b = 0.5f + 0.5f * std::sin((u + v) * 4.0f);
  return {byte_of(0.15f + 0.7f * a), byte_of(0.25f + 0.5f * b),
          byte_of(0.78f - 0.5f * v), 255};
}

constexpr int kBadgeSize = 96;

/// Transparent outside its circle, so the alpha round trip through the decoder
/// and the premultiplied upload is visible against whatever sits behind it.
std::array<std::uint8_t, 4> badge_texel(int x, int y) {
  const float centre = kBadgeSize * 0.5f;
  const float dx = static_cast<float>(x) - centre;
  const float dy = static_cast<float>(y) - centre;
  const float d = std::min(1.0f, std::sqrt(dx * dx + dy * dy) / centre);
  const float ring = 0.5f + 0.5f * std::sin(d * 12.0f);
  return {byte_of(0.95f - 0.5f * ring), byte_of(0.35f + 0.4f * ring),
          byte_of(0.55f + 0.35f * d), byte_of((1.0f - d) * 6.0f)};
}

/// Writes the fixtures somewhere the resource layer can be pointed at, and
/// mounts that directory under `res://`.
///
/// A directory provider is exactly what a development build uses for its real
/// assets, so nothing in the widgets below knows this is a fixture: they name
/// `res://photo.png` and would keep working unchanged against a shipped pack.
bool install_assets(std::filesystem::path &root) {
  std::error_code error;
  root = std::filesystem::temp_directory_path(error) / "voidui-image-example";
  if (error)
    return false;

  std::filesystem::create_directories(root, error);
  if (error)
    return false;

  if (!write_png(root / "photo.png", kPhotoWidth, kPhotoHeight, photo_texel))
    return false;
  if (!write_png(root / "badge.png", kBadgeSize, kBadgeSize, badge_texel))
    return false;

  Resources::global().mount("", directory_provider(root.string()));

  // Installing this is all it takes for `https://...` to work everywhere
  // `res://...` does -- the widget, the cache and the decoder are unchanged,
  // because a URL is just another name for bytes. The cache directory keeps the
  // *encoded* bytes between runs, so a second launch of a real application
  // paints its images without a single request.
  //
  // Nothing in this example actually reaches the network: it names one
  // unreachable URL, to show that a request that fails leaves the placeholder
  // standing rather than a hole or a spin. Point it at a real address to watch
  // the fetch happen.
  HttpOptions http;
  http.cache_directory = (root / "http-cache").string();
  if (std::shared_ptr<ResourceProvider> network = http_provider(http))
    Resources::global().set_network_provider(std::move(network));

  return true;
}

// -- the interface
// -------------------------------------------------------------

/// A square box for a 16:10 picture, which is the only way the fits differ:
/// against a box of the source's own shape every one of them produces exactly
/// the same rectangle.
ImageView photo(ObjectFit fit) {
  return image("res://photo.png")
      .fit(fit)
      .placeholder(Color(228, 231, 238))
      .fade(0.18f)
      // Reserves the row's height before the pixels arrive, so the list does
      // not jump as loads land in whatever order the pool finishes them.
      .natural_size(Size<float>(140.0f, 140.0f))
      .width(Length::Fixed{140.0f})
      .height(Length::Fixed{140.0f})
      .margin(Margin(MarginValue(8.0f)));
}

std::unique_ptr<Widget> build() {
  // One row per fit, all naming the same file. The cache turns that into one
  // read and one decode per distinct decode size, not one per widget.
  // Contain letterboxes, Cover crops the sides, Fill squashes, None shows a
  // 140x140 window onto the full 1600x1000 -- and is the one fit that has to
  // decode the picture at full resolution to mean anything.
  auto fits = row(photo(ObjectFit::Contain), photo(ObjectFit::Cover),
                  photo(ObjectFit::Fill), photo(ObjectFit::None));

  // Cover plus a full border radius is the avatar case, and the one that would
  // otherwise want two clips: the crop is in the source rect, so only the
  // rounding needs one.
  auto avatars = row(image("res://badge.png")
                         .fit(ObjectFit::Cover)
                         .fade(0.25f)
                         .width(Length::Fixed{72.0f})
                         .height(Length::Fixed{72.0f}),
                     image("res://photo.png")
                         .fit(ObjectFit::Cover)
                         .placeholder(Color(228, 231, 238))
                         .fade(0.25f)
                         .width(Length::Fixed{72.0f})
                         .height(Length::Fixed{72.0f}),
                     // A source that does not exist: the placeholder stays, and
                     // nothing spins waiting for it.
                     image("res://missing.png")
                         .placeholder(Color(244, 214, 214))
                         .width(Length::Fixed{72.0f})
                         .height(Length::Fixed{72.0f}),
                     // And a remote one that will not answer, which fails the
                     // same way through the same handle -- the widget does not
                     // know or care which of the two it was.
                     image("https://localhost:1/unreachable.png")
                         .placeholder(Color(244, 232, 214))
                         .width(Length::Fixed{72.0f})
                         .height(Length::Fixed{72.0f}));

  auto hero = image("res://photo.png")
                  .fit(ObjectFit::Cover)
                  .placeholder(Color(228, 231, 238))
                  .fade(0.3f)
                  .width(Length::Fill{})
                  .height(Length::Fixed{180.0f});

  return transfer_widget(
      column(std::move(hero), std::move(fits), std::move(avatars)));
}

} // namespace

int main() {
  std::filesystem::path root;
  if (!install_assets(root)) {
    std::printf("could not write the example's images\n");
    return 1;
  }
  std::printf("assets in %s\n", root.string().c_str());
  std::printf("photo.png is %dx%d, %.1f MB of pixels decoded natively\n",
              kPhotoWidth, kPhotoHeight,
              kPhotoWidth * kPhotoHeight * 4.0 / (1024.0 * 1024.0));

  Window window("VoidUI Images", 720, 520);
  window.run(build());

  std::printf("cache held %zu entries, %.2f MB retained\n",
              ImageCache::global().size(),
              ImageCache::global().bytes_retained() / (1024.0 * 1024.0));
  return 0;
}
