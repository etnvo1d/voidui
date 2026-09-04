#include "voidui/paint/font.h"

#include "paint/font_impl.h"

#include "voidui/paint/font_registry.h"

#include <algorithm>
#include <atomic>
#include <fstream>
#include <mutex>
#include <unordered_map>

namespace voidui {

namespace {

/// HarfBuzz reports positions in 26.6 fixed point once the font scale is set in
/// the same units, so one constant converts everything back to logical pixels.
constexpr float kFixed26_6 = 1.0f / 64.0f;

/// The CSS generic family names. Kept here rather than in the provider so a
/// platform implementation only has to answer for the ones it has, not know
/// which strings are generics in the first place.
bool is_generic_family(std::string_view name) {
  return name == "system-ui" || name == "ui-sans-serif" ||
         name == "ui-serif" || name == "ui-monospace" || name == "ui-rounded" ||
         name == "sans-serif" || name == "serif" || name == "monospace" ||
         name == "cursive" || name == "fantasy" || name == "emoji" ||
         name == "math" || name == "fangsong";
}

std::uint64_t next_font_id() {
  static std::atomic<std::uint64_t> counter{1};
  return counter.fetch_add(1, std::memory_order_relaxed);
}

Blob read_file(const std::string &path) {
  std::ifstream file(path, std::ios::binary | std::ios::ate);
  if (!file)
    return {};

  const std::streamoff size = file.tellg();
  if (size <= 0)
    return {};

  std::vector<std::uint8_t> data(static_cast<std::size_t>(size));
  file.seekg(0);
  if (!file.read(reinterpret_cast<char *>(data.data()), size))
    return {};

  return Blob::own(std::move(data));
}

/// Names a set of bytes in the face cache.
///
/// The address is enough: FontData holds the Blob, so two live blobs can never
/// share an address, and a key whose bytes have gone away has an expired entry
/// that the next lookup drops.
std::string blob_cache_key(const Blob &bytes, int face_index) {
  return "blob:" + std::to_string(reinterpret_cast<std::uintptr_t>(bytes.data())) +
         ":" + std::to_string(bytes.size()) + "#" + std::to_string(face_index);
}

} // namespace

FontData::~FontData() {
  if (hb_face)
    hb_face_destroy(hb_face);
  if (blob)
    hb_blob_destroy(blob);
  if (face)
    FT_Done_Face(face);
  if (library)
    FT_Done_FreeType(library);
}

FontImpl::~FontImpl() {
  if (hb_font)
    hb_font_destroy(hb_font);
}

namespace {

std::mutex &font_cache_mutex() {
  static std::mutex mutex;
  return mutex;
}

std::unordered_map<std::string, std::weak_ptr<FontData>> &font_cache() {
  static std::unordered_map<std::string, std::weak_ptr<FontData>> cache;
  return cache;
}

} // namespace

std::shared_ptr<FontData> find_font_data(const std::string &key) {
  if (key.empty())
    return nullptr;

  std::lock_guard<std::mutex> lock(font_cache_mutex());
  auto &cache = font_cache();

  if (auto it = cache.find(key); it != cache.end()) {
    if (std::shared_ptr<FontData> existing = it->second.lock())
      return existing;
    cache.erase(it);
  }

  return nullptr;
}

std::shared_ptr<FontData> create_font_data(const std::string &key, Blob bytes,
                                           int face_index) {
  if (bytes.empty())
    return nullptr;

  auto data = std::make_shared<FontData>();
  data->bytes = std::move(bytes);
  data->id = next_font_id();

  if (FT_Init_FreeType(&data->library) != 0)
    return nullptr;

  if (FT_New_Memory_Face(data->library,
                         reinterpret_cast<const FT_Byte *>(data->bytes.data()),
                         static_cast<FT_Long>(data->bytes.size()),
                         static_cast<FT_Long>(face_index), &data->face) != 0)
    return nullptr;

  // The hb_blob borrows the Blob's bytes, which outlive it: both die with
  // FontData and members are destroyed in reverse declaration order.
  data->blob =
      hb_blob_create(reinterpret_cast<const char *>(data->bytes.data()),
                     static_cast<unsigned>(data->bytes.size()),
                     HB_MEMORY_MODE_READONLY, nullptr, nullptr);
  data->hb_face = hb_face_create(data->blob, static_cast<unsigned>(face_index));

  if (!key.empty()) {
    std::lock_guard<std::mutex> lock(font_cache_mutex());
    font_cache()[key] = data;
  }

  return data;
}

Font::Font(std::unique_ptr<FontImpl> impl, float size, std::uint64_t id)
    : impl_(std::move(impl)), size_(size), id_(id) {}

Font::~Font() = default;

std::shared_ptr<Font> Font::from_data_(std::shared_ptr<FontData> data,
                                       float size) {
  if (!data)
    return nullptr;

  auto impl = std::make_unique<FontImpl>();
  impl->data = std::move(data);
  impl->hb_font = hb_font_create(impl->data->hb_face);

  // Scaling in 26.6 units of the logical size makes shaped positions come out
  // in logical pixels, independent of the display scale.
  const int scale = static_cast<int>(size * 64.0f + 0.5f);
  hb_font_set_scale(impl->hb_font, scale, scale);

  const std::uint64_t id = impl->data->id;
  return std::shared_ptr<Font>(new Font(std::move(impl), size, id));
}

std::shared_ptr<Font> Font::from_memory(std::vector<std::uint8_t> data,
                                        float size, int face_index) {
  if (data.empty() || size <= 0.0f)
    return nullptr;
  return from_blob(Blob::own(std::move(data)), size, face_index);
}

std::shared_ptr<Font> Font::from_blob(Blob bytes, float size, int face_index) {
  if (bytes.empty() || size <= 0.0f)
    return nullptr;

  const std::string key = blob_cache_key(bytes, face_index);
  if (std::shared_ptr<FontData> existing = find_font_data(key))
    return Font::from_data_(std::move(existing), size);

  return Font::from_data_(create_font_data(key, std::move(bytes), face_index),
                          size);
}

std::shared_ptr<Font> Font::from_file(const std::string &path, float size,
                                      int face_index) {
  if (size <= 0.0f)
    return nullptr;

  const std::string key = path + "#" + std::to_string(face_index);

  // Reuse the face if it is already loaded; only touch the disk otherwise. A
  // CJK system font is around 20 MB, so this is the difference between a few
  // type sizes costing 20 MB and costing 100.
  if (std::shared_ptr<FontData> existing = find_font_data(key))
    return Font::from_data_(std::move(existing), size);

  Blob data = read_file(path);
  if (data.empty())
    return nullptr;

  return Font::from_data_(create_font_data(key, std::move(data), face_index),
                          size);
}

std::shared_ptr<Font>
Font::from_first_available(const std::vector<std::string> &paths, float size) {
  for (const std::string &path : paths) {
    if (std::shared_ptr<Font> font = from_file(path, size))
      return font;
  }
  return nullptr;
}

float Font::ascent() const {
  hb_font_extents_t extents{};
  hb_font_get_h_extents(impl_->hb_font, &extents);
  return static_cast<float>(extents.ascender) * kFixed26_6;
}

float Font::descent() const {
  hb_font_extents_t extents{};
  hb_font_get_h_extents(impl_->hb_font, &extents);
  return -static_cast<float>(extents.descender) * kFixed26_6;
}

float Font::line_height() const {
  hb_font_extents_t extents{};
  hb_font_get_h_extents(impl_->hb_font, &extents);
  return static_cast<float>(extents.ascender - extents.descender +
                            extents.line_gap) *
         kFixed26_6;
}

GlyphRun Font::shape(std::string_view utf8) const {
  GlyphRun run;
  if (utf8.empty())
    return run;

  hb_buffer_t *buffer = hb_buffer_create();
  hb_buffer_add_utf8(buffer, utf8.data(), static_cast<int>(utf8.size()), 0,
                     static_cast<int>(utf8.size()));

  // Script, language and direction are inferred from the codepoints, which is
  // right for a single run of one language.
  hb_buffer_guess_segment_properties(buffer);
  hb_shape(impl_->hb_font, buffer, nullptr, 0);

  unsigned count = 0;
  const hb_glyph_info_t *infos = hb_buffer_get_glyph_infos(buffer, &count);
  const hb_glyph_position_t *positions =
      hb_buffer_get_glyph_positions(buffer, &count);

  run.glyphs.reserve(count);

  float x = 0.0f;
  float y = 0.0f;
  for (unsigned i = 0; i < count; ++i) {
    PositionedGlyph glyph;
    glyph.id = infos[i].codepoint;
    glyph.cluster = infos[i].cluster;
    glyph.offset = Point<float>(
        x + static_cast<float>(positions[i].x_offset) * kFixed26_6,
        y - static_cast<float>(positions[i].y_offset) * kFixed26_6);
    run.glyphs.push_back(glyph);

    x += static_cast<float>(positions[i].x_advance) * kFixed26_6;
    y -= static_cast<float>(positions[i].y_advance) * kFixed26_6;
  }

  hb_buffer_destroy(buffer);

  run.font = shared_from_this();
  run.advance = x;
  return run;
}

float Font::measure(std::string_view utf8) const { return shape(utf8).advance; }

bool Font::has_color_glyphs() const {
  return impl_ && impl_->data && impl_->data->face &&
         FT_HAS_COLOR(impl_->data->face);
}

//------------------------------------------------------------------------------

std::shared_ptr<FontStack> FontStack::from_font(std::shared_ptr<Font> font,
                                                std::string family,
                                                std::string locale,
                                                FontWeight weight) {
  if (!font)
    return nullptr;

  auto stack = std::shared_ptr<FontStack>(new FontStack());
  stack->base_ = std::move(font);
  stack->family_ = std::move(family);
  stack->size_ = stack->base_->size();
  stack->weight_ = weight;
  stack->locale_ = locale.empty() ? FontProvider::system().default_locale()
                                  : std::move(locale);
  return stack;
}

std::shared_ptr<FontStack> FontStack::create(std::string family, float size,
                                             std::string locale,
                                             FontWeight weight) {
  if (family.empty())
    return nullptr;

  // The application's own fonts come first. A stylesheet naming a family the
  // machine has never had installed is the case this exists for, and taking
  // the shipped copy over an installed one of the same name is deliberate: an
  // application that packs a font means that font.
  if (const std::optional<RegisteredFace> face =
          FontRegistry::global().find(family, weight)) {
    if (!face->bytes.empty()) {
      if (std::shared_ptr<Font> font =
              Font::from_blob(face->bytes, size, face->face_index))
        return from_font(std::move(font), std::move(family), std::move(locale),
                         weight);
    } else if (!face->alias.empty()) {
      // An alias -- what `src: local(...)` leaves behind -- names a family the
      // machine has. It resolves through the platform and never back through
      // the registry, so two aliases pointing at each other cannot loop. The
      // stack keeps the resolved name, because that is the one the platform's
      // per-glyph fallback has to be asked about.
      FontProvider &platform = FontProvider::system();
      if (platform.available()) {
        if (const std::optional<FontFile> file =
                platform.resolve(face->alias, weight)) {
          if (std::shared_ptr<Font> font =
                  Font::from_file(file->path, size, file->face_index))
            return from_font(std::move(font), face->alias, std::move(locale),
                             weight);
        }
      }
    }
  }

  FontProvider &provider = FontProvider::system();
  if (!provider.available())
    return nullptr;

  const std::optional<FontFile> file = provider.resolve(family, weight);
  if (!file)
    return nullptr;

  std::shared_ptr<Font> font =
      Font::from_file(file->path, size, file->face_index);
  if (!font)
    return nullptr;

  return from_font(std::move(font), std::move(family), std::move(locale),
                   weight);
}

std::shared_ptr<FontStack> FontStack::create(const FontFamilyList &families,
                                             float size, std::string locale,
                                             FontWeight weight) {
  for (const std::string &name : families.families()) {
    // A generic stands for whatever the platform offers under that heading. It
    // is asked of the provider rather than resolved here because the answer is
    // "Consolas" on Windows and something else everywhere else.
    std::string family = name;
    if (is_generic_family(name)) {
      FontProvider &provider = FontProvider::system();
      family = provider.available() ? provider.generic_family(name)
                                    : std::string();
      if (family.empty())
        continue;
    }

    if (std::shared_ptr<FontStack> stack =
            create(std::move(family), size, locale, weight))
      return stack;
  }

  // Nothing in the list was found. The platform's UI font is a better answer
  // than no text at all, and it is what an empty `font-family` means anyway.
  return system_ui(size, std::move(locale), weight);
}

std::shared_ptr<FontStack> FontStack::system_ui(float size, std::string locale,
                                                FontWeight weight) {
  FontProvider &provider = FontProvider::system();
  if (!provider.available())
    return nullptr;

  return create(provider.default_ui_family(), size, std::move(locale), weight);
}

std::shared_ptr<FontStack> FontStack::cached(std::string family, float size,
                                             std::string locale,
                                             FontWeight weight) {
  struct Key {
    std::string family;
    std::string locale;
    float size = 0.0f;
    FontWeight weight = FontWeight::Normal;

    bool operator==(const Key &) const = default;
  };
  struct KeyHash {
    std::size_t operator()(const Key &key) const {
      std::size_t hash = std::hash<std::string>{}(key.family);
      hash ^= std::hash<std::string>{}(key.locale) + 0x9e3779b9 + (hash << 6);
      hash ^= std::hash<float>{}(key.size) + 0x9e3779b9 + (hash << 6);
      hash ^= std::hash<std::uint16_t>{}(font_weight_value(key.weight)) +
              0x9e3779b9 + (hash << 6);
      return hash;
    }
  };
  static std::unordered_map<Key, std::weak_ptr<FontStack>, KeyHash> cache;

  Key key{std::move(family), std::move(locale), size, weight};
  if (auto found = cache.find(key); found != cache.end()) {
    if (auto existing = found->second.lock())
      return existing;
    cache.erase(found);
  }

  auto stack = key.family.empty()
                   ? system_ui(size, key.locale, weight)
                   : create(key.family, size, key.locale, weight);
  cache.emplace(std::move(key), stack);
  return stack;
}

std::shared_ptr<FontStack> FontStack::cached(const FontFamilyList &families,
                                             float size, std::string locale,
                                             FontWeight weight) {
  // A single-family list is by far the common case and shares the string cache
  // with every other caller; only a real list needs one of its own.
  if (families.empty())
    return cached(std::string(), size, std::move(locale), weight);
  if (families.size() == 1)
    return cached(families.primary(), size, std::move(locale), weight);

  struct Key {
    FontFamilyList families;
    std::string locale;
    float size = 0.0f;
    FontWeight weight = FontWeight::Normal;

    bool operator==(const Key &other) const {
      return size == other.size && weight == other.weight &&
             locale == other.locale && families == other.families;
    }
  };
  struct KeyHash {
    std::size_t operator()(const Key &key) const {
      std::size_t hash = static_cast<std::size_t>(key.families.hash());
      hash ^= std::hash<std::string>{}(key.locale) + 0x9e3779b9 + (hash << 6);
      hash ^= std::hash<float>{}(key.size) + 0x9e3779b9 + (hash << 6);
      hash ^= std::hash<std::uint16_t>{}(font_weight_value(key.weight)) +
              0x9e3779b9 + (hash << 6);
      return hash;
    }
  };
  static std::unordered_map<Key, std::weak_ptr<FontStack>, KeyHash> cache;

  Key key{families, std::move(locale), size, weight};
  if (auto found = cache.find(key); found != cache.end()) {
    if (auto existing = found->second.lock())
      return existing;
    cache.erase(found);
  }

  auto stack = create(key.families, size, key.locale, weight);
  cache.emplace(std::move(key), stack);
  return stack;
}

std::shared_ptr<Font> FontStack::face_for_(const FontFile &file) const {
  if (file.path.empty())
    return base_;

  const std::string key = file.path + "#" + std::to_string(file.face_index);
  if (auto it = faces_.find(key); it != faces_.end())
    return it->second;

  std::shared_ptr<Font> font =
      Font::from_file(file.path, size_, file.face_index);
  faces_.emplace(key, font);
  return font;
}

std::vector<GlyphRun> FontStack::shape(std::string_view utf8) const {
  std::vector<GlyphRun> runs;
  if (utf8.empty() || !base_)
    return runs;

  FontProvider &provider = FontProvider::system();
  if (!provider.available()) {
    runs.push_back(base_->shape(utf8));
    return runs;
  }

  // Walk the string, asking the platform which face covers each stretch. Every
  // run's offsets are biased by the pen so far, so all of them share one origin
  // and the caller draws them at a single point.
  float pen = 0.0f;
  std::size_t offset = 0;

  while (offset < utf8.size()) {
    const std::optional<FontRun> piece =
        provider.fallback(utf8.substr(offset), family_, locale_, weight_);
    if (!piece || piece->length == 0)
      break;

    const std::size_t length = std::min(piece->length, utf8.size() - offset);
    if (std::shared_ptr<Font> font = face_for_(piece->file)) {
      GlyphRun run = font->shape(utf8.substr(offset, length));
      // Clusters come back relative to the piece; rebase them onto the whole
      // string so a caller can map any glyph to its source byte.
      for (PositionedGlyph &glyph : run.glyphs) {
        glyph.offset.x += pen;
        glyph.cluster += static_cast<std::uint32_t>(offset);
      }
      pen += run.advance;
      runs.push_back(std::move(run));
    }

    offset += length;
  }

  return runs;
}

float FontStack::measure(std::string_view utf8) const {
  float total = 0.0f;
  for (const GlyphRun &run : shape(utf8))
    total += run.advance;
  return total;
}

float FontStack::ascent() const { return base_ ? base_->ascent() : 0.0f; }
float FontStack::descent() const { return base_ ? base_->descent() : 0.0f; }
float FontStack::line_height() const {
  return base_ ? base_->line_height() : 0.0f;
}

} // namespace voidui
