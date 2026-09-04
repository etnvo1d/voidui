#pragma once

#include <ft2build.h>
#include FT_FREETYPE_H

#include <hb.h>

#include <cstdint>
#include <memory>
#include <string>

#include "voidui/core/resource.h"

namespace voidui {

/// Everything about a face that does not depend on the size it is drawn at:
/// the file bytes, the FreeType face, and the HarfBuzz face.
///
/// Shared between every Font that names the same file, because these are the
/// expensive part -- a CJK system font is 10-20 MB, and loading one per size
/// would make a handful of type sizes cost more than the rest of the renderer
/// put together.
struct FontData {
  /// A Blob rather than a vector so a face baked into the binary is read in
  /// place: FreeType keeps a pointer into these bytes for the life of the face,
  /// and the Blob is what guarantees they stay put.
  Blob bytes;

  FT_Library library = nullptr;
  FT_Face face = nullptr;

  hb_blob_t *blob = nullptr;
  hb_face_t *hb_face = nullptr;

  /// Pixel size the FT_Face is currently configured for. Lives here rather than
  /// on Font because the face is shared, so a size change by one Font is
  /// visible to all of them.
  int ft_pixel_size = 0;

  /// Identifies the face in cache keys. Stable across the sizes that share it,
  /// so a glyph rasterised for one size does not shadow another's.
  std::uint64_t id = 0;

  ~FontData();
};

/// Per-size state: only the HarfBuzz font, whose scale carries the size.
struct FontImpl {
  std::shared_ptr<FontData> data;
  hb_font_t *hb_font = nullptr;

  ~FontImpl();
};

/// A face already in memory under this key, or null.
std::shared_ptr<FontData> find_font_data(const std::string &key);

/// Builds a face from `bytes` and, when `key` is non-empty, publishes it so the
/// next request for the same file and index reuses it.
std::shared_ptr<FontData> create_font_data(const std::string &key, Blob bytes,
                                           int face_index);

} // namespace voidui
