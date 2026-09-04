// Reports what happens to one image URI at every stage it passes through.
//
// The image path has four places a picture can quietly fail to appear -- no
// network provider installed, a request that did not come back, bytes no
// decoder claims, and a decoder that claimed them and could not finish -- and
// from the outside all four look identical: an empty box. This says which.
//
//   voidui_image_probe https://example.com/a.png
//   voidui_image_probe res://icons/user.png

#include "voidui/core/http.h"
#include "voidui/core/resource.h"
#include "voidui/paint/image_codec.h"

#include <cstdio>
#include <cstring>
#include <string>

using namespace voidui;

namespace {

void print_magic(std::span<const std::byte> data) {
  const std::size_t n = std::min<std::size_t>(data.size(), 16);
  std::printf("  first %zu bytes:", n);
  for (std::size_t i = 0; i < n; ++i)
    std::printf(" %02X", static_cast<unsigned>(data[i]));
  std::printf("\n  as text:        ");
  for (std::size_t i = 0; i < n; ++i) {
    const unsigned char c = static_cast<unsigned char>(data[i]);
    std::printf("%c", c >= 32 && c < 127 ? c : '.');
  }
  std::printf("\n");
}

} // namespace

int main(int argc, char **argv) {
  if (argc < 2) {
    std::printf("usage: voidui_image_probe <uri>\n");
    return 2;
  }

  const std::string reference = argv[1];

  std::printf("decoders registered:");
  for (const std::string_view name : ImageCodecs::global().decoders())
    std::printf(" %.*s", static_cast<int>(name.size()), name.data());
  std::printf("\n\n");

  const ResourceResult<ResourceUri> uri = ResourceUri::parse(reference);
  if (!uri) {
    std::printf("uri:     REFUSED (%.*s)\n",
                static_cast<int>(to_string(uri.error()).size()),
                to_string(uri.error()).data());
    return 1;
  }
  std::printf("uri:     %s\n", uri->to_string().c_str());
  std::printf("scheme:  %.*s%s\n", static_cast<int>(uri->scheme().size()),
              uri->scheme().data(), uri->is_network() ? " (network)" : "");

  if (uri->is_network()) {
    HttpOptions options;
    if (std::shared_ptr<ResourceProvider> network = http_provider(options)) {
      Resources::global().set_network_provider(std::move(network));
      std::printf("network: provider installed\n");
    } else {
      std::printf("network: NO PROVIDER on this platform\n");
    }
  }

  const ResourceResult<Blob> blob = Resources::global().open(*uri);
  if (!blob) {
    const std::string_view why = to_string(blob.error());
    std::printf("read:    FAILED (%.*s)\n", static_cast<int>(why.size()),
                why.data());
    return 1;
  }
  std::printf("read:    %zu bytes\n", blob->size());
  print_magic(blob->bytes());

  const ImageResult<ImageInfo> info = ImageCodecs::global().probe(blob->bytes());
  if (!info) {
    const std::string_view why = to_string(info.error());
    std::printf("probe:   FAILED (%.*s)\n", static_cast<int>(why.size()),
                why.data());
  } else {
    std::printf("probe:   %dx%d, alpha=%s\n", info->width, info->height,
                info->has_alpha ? "yes" : "no");
  }

  const ImageResult<std::shared_ptr<Image>> full =
      ImageCodecs::global().decode(blob->bytes());
  if (!full) {
    const std::string_view why = to_string(full.error());
    std::printf("decode:  FAILED (%.*s)\n", static_cast<int>(why.size()),
                why.data());
    return 1;
  }
  std::printf("decode:  %dx%d from a %dx%d source, %zu bytes\n", (*full)->width(),
              (*full)->height(), (*full)->source_width(), (*full)->source_height(),
              (*full)->byte_size());

  DecodeOptions fitted;
  fitted.max_width = 240;
  fitted.max_height = 240;
  const ImageResult<std::shared_ptr<Image>> thumb =
      ImageCodecs::global().decode(blob->bytes(), fitted);
  if (thumb)
    std::printf("at 240:  %dx%d, %zu bytes\n", (*thumb)->width(),
                (*thumb)->height(), (*thumb)->byte_size());

  return 0;
}
