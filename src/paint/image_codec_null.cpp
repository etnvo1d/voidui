// Stand-in for platforms whose image decoder is not written yet.
//
// macOS would use ImageIO (CGImageSourceCreateWithData, then
// CGImageSourceCreateThumbnailAtIndex with kCGImageSourceThumbnailMaxPixelSize,
// which is that platform's equivalent of decoding straight to the display
// size). Linux has no system imaging layer to inherit, so it wants bundled
// codecs -- libspng and libjpeg-turbo for the two formats that matter, the
// latter also able to scale by DCT ratios during decode.
//
// Either fits behind ImageDecoder unchanged: an application on a platform
// without one can register its own into `ImageCodecs::global()` today.

#include "voidui/paint/image_codec.h"

namespace voidui {

std::shared_ptr<ImageDecoder> platform_image_decoder() { return nullptr; }

} // namespace voidui
