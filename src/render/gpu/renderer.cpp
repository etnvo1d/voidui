#include "render/gpu/renderer.h"

#include "render/rasterizer.h"
#include "voidui/core/pixel_snap.h"
#include "voidui/shaders/mask.h"
#include "voidui/shaders/quad.h"

#include <SDL3/SDL_log.h>

#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstring>
#include <variant>

namespace voidui {

namespace {

// Must stay in step with shaders/quad.slang.
constexpr std::uint32_t kFlagFill = 1u << 0;
constexpr std::uint32_t kFlagStroke = 1u << 1;
constexpr std::uint32_t kFlagGradient = 1u << 2;
constexpr std::uint32_t kFlagShadow = 1u << 3;
constexpr std::uint32_t kFlagConic = 1u << 4;
constexpr std::uint32_t kFlagStrokeGradient = 1u << 5;
constexpr std::uint32_t kFlagShadowInset = 1u << 6;

// Must stay in step with shaders/mask.slang.
constexpr std::uint32_t kFlagMaskChannel = 1u << 0;

/// Four corners of a triangle strip; the vertex shaders derive them from
/// SV_VertexID, so no position buffer is ever uploaded.
constexpr std::uint32_t kVerticesPerQuad = 4;

/// One 2048-square R8 page holds a few thousand glyphs or a good many path
/// masks -- 4 MB. Images get a second page of the same size in RGBA8 (16 MB),
/// allocated lazily so a UI that draws no images pays nothing for it.
constexpr int kAtlasSize = 2048;

/// Beyond this an image gets a texture of its own rather than eating most of a
/// shared page.
constexpr int kMaxAtlasImage = 512;

struct SceneUniforms {
  float viewport[2];
  float pixel_size;
  float clip_count;
  float clip_rect[kMaxClipRRects][4];
  float clip_radii[kMaxClipRRects][4];
};
static_assert(sizeof(SceneUniforms) == 80);

// Instance size is the whole point of the stop table, and a regression here is
// the kind that goes unnoticed until a long list gets slow.
static_assert(sizeof(QuadInstance) == 104);
static_assert(sizeof(MaskInstance) == 56);

struct ShaderBlobs {
  const void *vertex = nullptr;
  std::size_t vertex_size = 0;
  const void *fragment = nullptr;
  std::size_t fragment_size = 0;
};

bool quad_blobs(ShaderBlobs &out) {
  namespace ns = shaders::quad;
#if defined(VOIDUI_SHADER_DXBC)
  out = {ns::vertex_dxbc, sizeof(ns::vertex_dxbc), ns::fragment_dxbc,
         sizeof(ns::fragment_dxbc)};
#elif defined(VOIDUI_SHADER_SPIRV)
  out = {ns::vertex_spirv, sizeof(ns::vertex_spirv), ns::fragment_spirv,
         sizeof(ns::fragment_spirv)};
#elif defined(VOIDUI_SHADER_MSL)
  out = {ns::vertex_msl, sizeof(ns::vertex_msl), ns::fragment_msl,
         sizeof(ns::fragment_msl)};
#else
  return false;
#endif
  return true;
}

bool mask_blobs(ShaderBlobs &out) {
  namespace ns = shaders::mask;
#if defined(VOIDUI_SHADER_DXBC)
  out = {ns::vertex_dxbc, sizeof(ns::vertex_dxbc), ns::fragment_dxbc,
         sizeof(ns::fragment_dxbc)};
#elif defined(VOIDUI_SHADER_SPIRV)
  out = {ns::vertex_spirv, sizeof(ns::vertex_spirv), ns::fragment_spirv,
         sizeof(ns::fragment_spirv)};
#elif defined(VOIDUI_SHADER_MSL)
  out = {ns::vertex_msl, sizeof(ns::vertex_msl), ns::fragment_msl,
         sizeof(ns::fragment_msl)};
#else
  return false;
#endif
  return true;
}

/// The one place an authored colour is clipped into the sRGB gamut and
/// quantised. Everything upstream of here keeps float components and whichever
/// space it was written in.
void write_color(std::uint8_t (&out)[4], Color color, float opacity) {
  const Rgba8 rgba =
      color.with_alpha(color.a * std::clamp(opacity, 0.0f, 1.0f)).to_rgba8();
  out[0] = rgba.r;
  out[1] = rgba.g;
  out[2] = rgba.b;
  out[3] = rgba.a;
}

/// Solid colour standing in for a brush where a gradient cannot be expressed.
Color representative_color(const Brush &brush) {
  if (const Color *color = std::get_if<Color>(&brush))
    return *color;
  if (const LinearGradient *gradient = std::get_if<LinearGradient>(&brush))
    return gradient->start_color();
  if (const ConicGradient *gradient = std::get_if<ConicGradient>(&brush)) {
    const auto stops = gradient->stops();
    return stops.empty() ? Color::TRANSPARENT : stops.front().color;
  }
  return Color::TRANSPARENT;
}

/// How far a stroke of `width` pushes the silhouette outward. The shader only
/// knows how to draw an inner band, so centred and outer strokes are expressed
/// by growing the shape and letting that band land where it should.
float stroke_outset(float width, StrokeAlign align) {
  switch (align) {
  case StrokeAlign::Inside:
    return 0.0f;
  case StrokeAlign::Center:
    return width * 0.5f;
  case StrokeAlign::Outside:
    return width;
  }
  return 0.0f;
}

/// The whole-pixel device displacement a pure translation contributes.
///
/// Rounded on its own, and applied to geometry that was snapped without it,
/// rather than folded in before the snap. The two agree whenever the arithmetic
/// is exact, and only this order survives when it is not. A coordinate that
/// lands on a rounding tie is decided by its last bit, and layout produces such
/// coordinates constantly -- a label at y=10 is device 12.5 at 1.25x, and one
/// device pixel below it is 13.5, another tie. Fold the shift in first and the
/// float addition settles both, independently, in whichever direction it
/// happens to land: the label rounds back onto the row it started on while the
/// background it sits on moves, and the button comes apart. Added afterwards,
/// the shift is an integer and cannot round anywhere at all.
Point<float> device_translation(const Transform &transform, float scale) {
  return Point<float>(round_half_up(transform.e * scale),
                      round_half_up(transform.f * scale));
}

/// Grid-preserving geometry is snapped onto the device pixel grid before it
/// becomes a quad. Genuinely transformed geometry keeps its subpixel position
/// for smooth motion.
///
/// The shader's coverage ramp is exactly one device pixel wide, so a box whose
/// edges sit mid-pixel is resolved as two rows at half coverage instead of one
/// solid row. That reads as a thin, washed-out border, and because the phase
/// shifts with every scroll and resize, the same border changes weight from one
/// frame to the next.
///
/// The rect that comes back is the one an instance is actually built from, and
/// `transform` is left carrying only the part the vertex shader has to apply.
/// Both matter to a caller emitting two related shapes -- a shadow and the box
/// it is knocked out of -- which has to measure one against the other.
///
/// A pure translation -- a hover lift, a slide-in, a shake -- is carried on the
/// rect rather than handed to the shader, because it cannot move the geometry
/// off the grid, only along it. Doing otherwise is what made `translateY(-2px)`
/// wash out a border that was crisp at rest.
///
/// Both edges take the same whole-pixel displacement, so their difference is
/// untouched and abutting boxes still tile.
///
/// Only a scale or a rotation survives this to reach the vertex shader, and
/// that one keeps its subpixel position and is antialiased analytically rather
/// than snapping between animation steps.
Rect<float> resolve_rect(Rect<float> rect, float scale, Transform &transform) {
  if (transform.is_translation()) {
    const Point<float> shift = device_translation(transform, scale);
    const float pixel = scale > 0.0f ? 1.0f / scale : 1.0f;
    rect = snap_rect_to_pixel(rect, scale);
    rect.origin.x += shift.x * pixel;
    rect.origin.y += shift.y * pixel;
    transform.e = 0.0f;
    transform.f = 0.0f;
  }
  return rect;
}

/// The four corner radii an outset of `outset` leaves behind, each held inside
/// the half-extent it has to fit within. A negative outset -- an inset shadow's
/// hole, shrunk by its spread -- squares the corners off as it closes in.
void write_radii(float (&out)[4], Radius radius, float outset,
                 float half_width, float half_height) {
  const float limit = std::max(std::min(half_width, half_height), 0.0f);
  const auto grow = [limit, outset](float value) {
    return std::clamp(value > 0.0f ? value + outset : value, 0.0f, limit);
  };
  out[0] = grow(radius.left_top);
  out[1] = grow(radius.right_top);
  out[2] = grow(radius.right_bottom);
  out[3] = grow(radius.left_bottom);
}

QuadInstance make_instance(Rect<float> rect, Radius radius, float outset,
                           float scale, Transform transform) {
  QuadInstance instance{};

  rect = resolve_rect(rect, scale, transform);

  const float half_width = rect.size.width * 0.5f + outset;
  const float half_height = rect.size.height * 0.5f + outset;

  const Point<float> center =
      transform.apply(Point<float>(rect.origin.x + rect.size.width * 0.5f,
                                   rect.origin.y + rect.size.height * 0.5f));
  instance.center[0] = center.x;
  instance.center[1] = center.y;
  instance.half_size[0] = std::max(half_width, 0.0f);
  instance.half_size[1] = std::max(half_height, 0.0f);

  write_radii(instance.radii, radius, outset, instance.half_size[0],
              instance.half_size[1]);

  instance.transform[0] = transform.a;
  instance.transform[1] = transform.b;
  instance.transform[2] = transform.c;
  instance.transform[3] = transform.d;

  return instance;
}

/// The second rounded rect a shadow instance carries: for an outer shadow the
/// border box that is knocked out of it, for an inner one the hole it fades
/// towards. It rides in the gradient slots, which no shadow uses, so a shadow
/// still costs exactly one quad and the vertex layout is unchanged.
///
/// `delta` is measured in the instance's own shape space, so it is the offset
/// between the two centres *before* the transform -- the linear map is applied
/// to the pair together in the shader.
void write_shadow_shape(QuadInstance &instance, Point<float> delta,
                        Size<float> size, Radius radius, float outset) {
  const float half_width = std::max(size.width * 0.5f + outset, 0.0f);
  const float half_height = std::max(size.height * 0.5f + outset, 0.0f);
  instance.gradient_a[0] = delta.x;
  instance.gradient_a[1] = delta.y;
  instance.gradient_b[0] = half_width;
  instance.gradient_b[1] = half_height;
  write_radii(instance.gradient_meta, radius, outset, half_width, half_height);
}

/// Builds a textured quad from a rect stated in the command's own coordinate
/// space, plus the transform that carries it to the screen.
///
/// The rect arrives already snapped where snapping applies; this only restates
/// it as the centre-and-half-extents pair the vertex shader wants, so the
/// transform has somewhere to live.
MaskInstance make_mask_instance(const Rect<float> &rect,
                                const Transform &transform) {
  MaskInstance instance{};

  const Point<float> center =
      transform.apply(Point<float>(rect.origin.x + rect.size.width * 0.5f,
                                   rect.origin.y + rect.size.height * 0.5f));
  instance.center[0] = center.x;
  instance.center[1] = center.y;
  instance.half_size[0] = rect.size.width * 0.5f;
  instance.half_size[1] = rect.size.height * 0.5f;

  instance.transform[0] = transform.a;
  instance.transform[1] = transform.b;
  instance.transform[2] = transform.c;
  instance.transform[3] = transform.d;

  return instance;
}

/// Whether two instances outline the same shape, and so may be shaded together.
bool same_geometry(const QuadInstance &a, const QuadInstance &b) {
  return std::memcmp(a.center, b.center, sizeof(a.center)) == 0 &&
         std::memcmp(a.half_size, b.half_size, sizeof(a.half_size)) == 0 &&
         std::memcmp(a.radii, b.radii, sizeof(a.radii)) == 0 &&
         std::memcmp(a.transform, b.transform, sizeof(a.transform)) == 0;
}

void hash_bytes(std::uint64_t &state, const void *data, std::size_t size) {
  const auto *bytes = static_cast<const std::uint8_t *>(data);
  for (std::size_t i = 0; i < size; ++i) {
    state ^= bytes[i];
    state *= 1099511628211ull; // FNV-1a
  }
}

/// Identity of a rasterised mask: the geometry, the transform it was rasterised
/// under, and everything about the stroke that changes its outline.
std::uint64_t mask_key(const Path &path, const Transform &to_device,
                       const DrawCommand &command) {
  std::uint64_t state = 14695981039346656037ull;

  // The outline identifies itself. Reading every point back out on every frame
  // that redraws a path is the whole cost of this lookup otherwise, and a path
  // maintains the hash as it is built, so this is one 64-bit mix regardless of
  // how many contours the shape has.
  const std::uint64_t outline = path.content_hash();
  hash_bytes(state, &outline, sizeof(outline));
  hash_bytes(state, &to_device, sizeof(to_device));
  hash_bytes(state, &command.kind, sizeof(command.kind));
  hash_bytes(state, &command.fill_rule, sizeof(command.fill_rule));
  hash_bytes(state, &command.stroke_width, sizeof(command.stroke_width));
  hash_bytes(state, &command.stroke_align, sizeof(command.stroke_align));
  hash_bytes(state, &command.cap, sizeof(command.cap));
  hash_bytes(state, &command.join, sizeof(command.join));
  hash_bytes(state, &command.miter_limit, sizeof(command.miter_limit));
  return state;
}

} // namespace

GpuRenderer::GpuRenderer(rhi::Device &device)
    : device_(device), quad_buffer_(device.create_buffer()),
      mask_buffer_(device.create_buffer()) {}

GpuRenderer::~GpuRenderer() { device_.wait_idle(); }

std::unique_ptr<GpuRenderer> GpuRenderer::create(rhi::Device &device) {
  std::unique_ptr<GpuRenderer> renderer(new GpuRenderer(device));

  if (!renderer->quad_buffer_ || !renderer->mask_buffer_ ||
      !renderer->build_pipelines_())
    return nullptr;

  // Not lazy like the atlases: every quad batch binds it, gradient or not, so
  // there is no frame in which it is absent.
  renderer->gradients_ = GradientTable::create(device);
  if (!renderer->gradients_)
    return nullptr;

  return renderer;
}

std::unique_ptr<rhi::Pipeline> GpuRenderer::build_pipeline_(
    const void *vertex_code, std::size_t vertex_size, const void *fragment_code,
    std::size_t fragment_size, const rhi::VertexAttribute *attributes,
    std::uint32_t attribute_count, std::uint32_t stride, bool sampled_texture) {
  const rhi::PipelineDesc desc{
      {vertex_code, vertex_size, "vs_main"},
      {fragment_code, fragment_size, "fs_main"},
      attributes,
      attribute_count,
      stride,
      sizeof(SceneUniforms),
      sampled_texture,
  };
  return device_.create_pipeline(desc);
}

bool GpuRenderer::build_pipelines_() {
  ShaderBlobs quad;
  ShaderBlobs mask;
  if (!quad_blobs(quad) || !mask_blobs(mask)) {
    SDL_Log("voidui: no shader bytecode baked for the %s driver",
            device_.driver());
    return false;
  }

  const rhi::VertexAttribute quad_attributes[]{
      {0, rhi::VertexFormat::Float2, offsetof(QuadInstance, center)},
      {1, rhi::VertexFormat::Float2, offsetof(QuadInstance, half_size)},
      {2, rhi::VertexFormat::Float4, offsetof(QuadInstance, radii)},
      {3, rhi::VertexFormat::Float2, offsetof(QuadInstance, gradient_a)},
      {4, rhi::VertexFormat::Float2, offsetof(QuadInstance, gradient_b)},
      {5, rhi::VertexFormat::Float4, offsetof(QuadInstance, transform)},
      {6, rhi::VertexFormat::UByte4Norm, offsetof(QuadInstance, fill_color)},
      {7, rhi::VertexFormat::UByte4Norm, offsetof(QuadInstance, stroke_color)},
      {8, rhi::VertexFormat::Float2, offsetof(QuadInstance, stroke_blur)},
      {9, rhi::VertexFormat::Float4, offsetof(QuadInstance, gradient_meta)},
      {10, rhi::VertexFormat::Float, offsetof(QuadInstance, gradient_v)},
      {11, rhi::VertexFormat::UInt, offsetof(QuadInstance, flags)},
  };

  const rhi::VertexAttribute mask_attributes[]{
      {0, rhi::VertexFormat::Float2, offsetof(MaskInstance, center)},
      {1, rhi::VertexFormat::Float2, offsetof(MaskInstance, half_size)},
      {2, rhi::VertexFormat::Float4, offsetof(MaskInstance, transform)},
      {3, rhi::VertexFormat::Float4, offsetof(MaskInstance, uv)},
      {4, rhi::VertexFormat::UByte4Norm, offsetof(MaskInstance, color)},
      {5, rhi::VertexFormat::UInt, offsetof(MaskInstance, flags)},
  };

  quad_pipeline_ = build_pipeline_(quad.vertex, quad.vertex_size, quad.fragment,
                                   quad.fragment_size, quad_attributes,
                                   SDL_arraysize(quad_attributes),
                                   sizeof(QuadInstance), true);
  mask_pipeline_ = build_pipeline_(mask.vertex, mask.vertex_size, mask.fragment,
                                   mask.fragment_size, mask_attributes,
                                   SDL_arraysize(mask_attributes),
                                   sizeof(MaskInstance), true);

  if (!quad_pipeline_ || !mask_pipeline_)
    return false;

  SDL_Log("voidui: GPU backend ready, driver %s", device_.driver());
  return true;
}

void GpuRenderer::resize(int pixel_width, int pixel_height, float logical_width,
                         float logical_height, float display_scale) {
  const bool scale_changed = display_scale != display_scale_;

  logical_width_ = logical_width;
  logical_height_ = logical_height;
  pixel_width_ = pixel_width;
  pixel_height_ = pixel_height;

  // Logical units covered by one device pixel; the shaders use this as their
  // antialiasing width.
  display_scale_ = display_scale;
  pixel_size_ = 1.0f / display_scale_;

  // Masks and glyphs are rasterised at device resolution, so a scale change
  // invalidates every one of them -- on both pages. Recycling only the coverage
  // page used to strand the colour page's emoji: their cache entries went away
  // with the glyph cache, but the shelves they sat on were never reclaimed, so
  // every DPI change leaked that space for the rest of the session.
  if (scale_changed) {
    recycle_coverage_atlas_();
    recycle_color_atlas_();
  }
}

bool GpuRenderer::apply_brush_(QuadInstance &instance, const Brush &brush,
                               float opacity) {
  ResolvedGradient resolved;
  bool conic = false;

  if (const LinearGradient *gradient = std::get_if<LinearGradient>(&brush)) {
    // The gradient box is the shape as it will be painted, which is why this
    // runs after make_instance has snapped it and applied any stroke outset.
    const Size<float> box(instance.half_size[0] * 2.0f,
                          instance.half_size[1] * 2.0f);
    const auto [start, end] = gradient->axis_for(box);
    instance.gradient_a[0] = start.x;
    instance.gradient_a[1] = start.y;
    instance.gradient_b[0] = end.x;
    instance.gradient_b[1] = end.y;
    resolved.count = static_cast<std::uint32_t>(
        gradient->resolve_stops(box, resolved.colors, resolved.positions));
    resolved.interpolation = gradient->effective_interpolation();
  } else if (const ConicGradient *wheel =
                 std::get_if<ConicGradient>(&brush)) {
    instance.gradient_meta[0] = wheel->angle();
    resolved.count = static_cast<std::uint32_t>(
        wheel->resolve_stops(resolved.colors, resolved.positions));
    resolved.interpolation = wheel->effective_interpolation();
    conic = true;
  } else {
    return false;
  }

  float row = 0.0f;
  if (resolved.count == 0 || !gradients_->row_for(resolved, frame_, row))
    return false;

  instance.gradient_v = row;
  instance.gradient_meta[1] = static_cast<float>(resolved.count);
  instance.gradient_meta[2] = std::clamp(opacity, 0.0f, 1.0f);
  // The stops in the row are already in this space; the shader converts the
  // mixed result back out of it.
  instance.gradient_meta[3] =
      static_cast<float>(resolved.interpolation.space);
  instance.flags |= kFlagGradient;
  if (conic)
    instance.flags |= kFlagConic;
  return true;
}

void GpuRenderer::recycle_coverage_atlas_() {
  if (atlas_)
    atlas_->clear();
  mask_cache_.clear();
  if (glyphs_)
    glyphs_->clear();
}

void GpuRenderer::recycle_color_atlas_() {
  if (color_atlas_)
    color_atlas_->clear();

  // Only the entries that actually held shelf space on this page. A standalone
  // image owns its own texture and is untouched by the page being recycled;
  // dropping its cache entry would strand that texture in standalone_textures_
  // with nothing left to release it, and the next draw of the same image would
  // find its id already taken.
  for (auto it = image_cache_.begin(); it != image_cache_.end();) {
    if (it->second.standalone)
      ++it;
    else
      it = image_cache_.erase(it);
  }

  // Over-broad: this drops coverage glyphs too, when only the colour entries
  // hold slots on this page. Correct, and a page recycle is rare enough that
  // splitting the cache by page would be optimising the wrong thing.
  if (glyphs_)
    glyphs_->clear();
}

void GpuRenderer::apply_pending_recycles_() {
  if (recycle_coverage_) {
    recycle_coverage_atlas_();
    recycle_coverage_ = false;
  }
  if (recycle_color_) {
    recycle_color_atlas_();
    recycle_color_ = false;
  }
}

void GpuRenderer::evict_standalone_() {
  // Long enough that nothing the GPU still holds can be referencing one: at
  // most two frames are ever in flight.
  constexpr std::uint64_t kIdleFrames = 120;
  if (frame_ < kIdleFrames)
    return;

  const std::uint64_t cutoff = frame_ - kIdleFrames;
  for (auto it = image_cache_.begin(); it != image_cache_.end();) {
    if (!it->second.standalone || it->second.last_used > cutoff) {
      ++it;
      continue;
    }
    standalone_textures_.erase(it->first);
    it = image_cache_.erase(it);
  }
}

Atlas *GpuRenderer::ensure_color_atlas_() {
  if (!color_atlas_) {
    color_atlas_ =
        Atlas::create(device_, kAtlasSize, rhi::TextureFormat::Rgba8Unorm, 4,
                      rhi::Filter::Linear);
  }
  return color_atlas_.get();
}

bool GpuRenderer::ensure_coverage_atlas_() {
  if (atlas_)
    return true;

  atlas_ = Atlas::create(device_, kAtlasSize, rhi::TextureFormat::R8Unorm, 1,
                         rhi::Filter::Nearest);
  if (!atlas_)
    return false;

  // Glyphs and path masks share this page: a screen of text and a handful of
  // icons then cost a single texture and, usually, a single batch.
  glyphs_ = std::make_unique<GlyphCache>();
  return true;
}

void GpuRenderer::append_(Batch::Kind kind, std::uint32_t clip_index,
                          rhi::Texture *texture, rhi::Sampler *sampler) {
  // The sampler is part of the key: the same page is point-sampled for upright
  // text and bilinear for transformed text, and merging across that would draw
  // one of them through the wrong filter.
  if (!batches_.empty() && batches_.back().kind == kind &&
      batches_.back().clip_index == clip_index &&
      batches_.back().texture == texture &&
      batches_.back().sampler == sampler) {
    batches_.back().count++;
    return;
  }

  Batch batch;
  batch.kind = kind;
  batch.clip_index = clip_index;
  batch.texture = texture;
  batch.sampler = sampler;
  batch.first = static_cast<std::uint32_t>(
      kind == Batch::Kind::Quad ? quads_.size() : masks_.size());
  batch.count = 1;
  batches_.push_back(batch);
}

const GpuRenderer::ImagePlacement *
GpuRenderer::place_image_(const Image &image) {
  if (auto it = image_cache_.find(image.id()); it != image_cache_.end()) {
    it->second.last_used = frame_;
    return &it->second;
  }

  const int width = image.width();
  const int height = image.height();
  const std::span<const std::uint8_t> source = image.pixels();

  // The shader composites premultiplied colour. A decoder produces that on its
  // own worker thread and those pixels are staged exactly as they arrived;
  // straight ones -- generated by hand, or from a codec that cannot premultiply
  // itself -- are converted here, once per image rather than once per fragment.
  Blob owned;
  const std::uint8_t *pixels = source.data();
  if (image.format() == PixelFormat::Rgba8Straight) {
    const std::size_t texels = static_cast<std::size_t>(width) * height;
    std::vector<std::uint8_t> premultiplied(texels * 4);
    for (std::size_t i = 0; i < texels; ++i) {
      const std::uint8_t *src = source.data() + i * 4;
      const unsigned a = src[3];
      premultiplied[i * 4 + 0] = static_cast<std::uint8_t>(src[0] * a / 255);
      premultiplied[i * 4 + 1] = static_cast<std::uint8_t>(src[1] * a / 255);
      premultiplied[i * 4 + 2] = static_cast<std::uint8_t>(src[2] * a / 255);
      premultiplied[i * 4 + 3] = static_cast<std::uint8_t>(a);
    }
    owned = Blob::own(std::move(premultiplied));
    pixels = reinterpret_cast<const std::uint8_t *>(owned.data());
  }

  ImagePlacement placement;
  placement.last_used = frame_;

  if (width <= kMaxAtlasImage && height <= kMaxAtlasImage) {
    if (color_full_ || !ensure_color_atlas_())
      return nullptr;

    const AtlasSlot slot = color_atlas_->allocate(width, height);
    if (!slot.valid) {
      // Recycling here would pull the shelves out from under the images and
      // emoji already queued behind their old texture coordinates, so the page
      // is marked for a recycle at the end of the frame and this image simply
      // goes unpainted until the next one.
      color_full_ = true;
      recycle_color_ = true;
      return nullptr;
    }

    color_atlas_->stage(slot, pixels, width * 4);
    color_atlas_->uv_for(slot, placement.uv);
    placement.texture = color_atlas_->texture();
    placement.sampler = color_atlas_->sampler();
  } else {
    // Standalone textures still borrow the shared page's sampler.
    if (!ensure_color_atlas_())
      return nullptr;

    std::unique_ptr<rhi::Texture> texture = device_.create_texture(
        static_cast<std::uint32_t>(width), static_cast<std::uint32_t>(height),
        rhi::TextureFormat::Rgba8Unorm);
    if (!texture)
      return nullptr;

    rhi::Texture *texture_ptr = texture.get();
    standalone_textures_.insert_or_assign(image.id(), std::move(texture));
    pending_standalone_.push_back(
        {texture_ptr, width, height, owned.empty() ? image.storage() : owned});

    placement.standalone = true;
    placement.texture = texture_ptr;
    placement.sampler = color_atlas_->sampler();
  }

  return &image_cache_.emplace(image.id(), placement).first->second;
}

const GpuRenderer::CachedMask *
GpuRenderer::mask_for_(const DisplayList &list, const DrawCommand &command,
                       Point<float> &device_offset) {
  const Path &path = list.path(command.path_index);

  // Masks live in device pixels, so the scale folds into the transform they are
  // rasterised under.
  Transform to_device = Transform::scale(display_scale_, display_scale_)
                            .concat(command.transform);

  // Where the shape sits must not reach the key -- only what shape it is and
  // how it is scaled and rotated. Baked in, the translation rasterises the same
  // outline again, and takes another atlas slot, for every distinct offset: an
  // animated one burns a slot per frame until the page fills, at which point
  // every glyph and mask sharing it is thrown away and rebuilt. Rounded to
  // whole device pixels and carried on the blit instead, it costs nothing,
  // keeps one entry per outline per scale, and keeps the mask texel-on-pixel
  // under the point sampler.
  //
  // This holds for any linear part, not just the identity one: coverage under
  // L is the same coverage wherever a whole number of pixels puts it. That is
  // what lets a list of forty icons -- each scaled from its own viewBox to the
  // same size, each at its own fractional centring offset -- share one mask
  // between them rather than rasterise forty.
  device_offset =
      Point<float>(round_half_up(to_device.e), round_half_up(to_device.f));
  to_device.e = 0.0f;
  to_device.f = 0.0f;

  const std::uint64_t key = mask_key(path, to_device, command);
  if (auto it = mask_cache_.find(key); it != mask_cache_.end())
    return &it->second;

  // The page already filled up earlier in this frame; nothing else will fit
  // until it is recycled, and recycling has to wait for the end of the frame.
  if (coverage_full_ || !ensure_coverage_atlas_())
    return nullptr;

  Mask mask;
  if (command.kind == CommandKind::FillPath) {
    mask = rasterize_path(path, to_device, command.fill_rule);
  } else {
    Pen pen;
    pen.width = command.stroke_width;
    pen.cap = command.cap;
    pen.join = command.join;
    pen.miter_limit = command.miter_limit;
    pen.align = command.stroke_align;

    const float scale = std::max(to_device.approximate_scale(), 1e-4f);
    mask = rasterize_path(stroke_to_fill(path, pen, 0.1f / scale), to_device,
                          FillRule::NonZero);
  }

  if (mask.empty())
    return nullptr;

  // A mask larger than an empty page is not a full-page problem and must not be
  // answered with one: recycling would drop every glyph and mask on the page
  // and still fail to place it. This distinction is the whole bug -- the
  // failure used to raise a flag that only a DPI change ever lowered, so one
  // oversized path silently stopped every path in the session from drawing.
  if (!atlas_->can_fit(mask.width, mask.height))
    return nullptr;

  const AtlasSlot slot = atlas_->allocate(mask.width, mask.height);
  if (!slot.valid) {
    // A full page is recycled wholesale, and the instances already queued this
    // frame point into it -- so the recycle is deferred past submission and
    // this mask waits for the next frame.
    coverage_full_ = true;
    recycle_coverage_ = true;
    return nullptr;
  }

  atlas_->stage(slot, mask.pixels.data(), mask.width);

  CachedMask cached;
  cached.slot = slot;
  cached.origin_x = mask.origin_x;
  cached.origin_y = mask.origin_y;
  cached.width = mask.width;
  cached.height = mask.height;

  return &mask_cache_.emplace(key, cached).first->second;
}

void GpuRenderer::flush_standalone_() {
  if (pending_standalone_.empty())
    return;

  for (PendingTexture &item : pending_standalone_) {
    const rhi::TextureUpload upload{0,
                                    0,
                                    static_cast<std::uint32_t>(item.width),
                                    static_cast<std::uint32_t>(item.height),
                                    0,
                                    static_cast<std::uint32_t>(item.width * 4)};
    device_.upload_texture(*item.texture, item.pixels.data(),
                           static_cast<std::uint32_t>(item.pixels.size()),
                           &upload, 1);
  }

  pending_standalone_.clear();
}

void GpuRenderer::build_batches_(const DisplayList &list) {
  quads_.clear();
  masks_.clear();
  batches_.clear();
  coverage_full_ = false;
  color_full_ = false;

  for (const DrawCommand &command : list.commands()) {
    switch (command.kind) {
    case CommandKind::FillRRect: {
      QuadInstance instance = make_instance(command.rect, command.radius, 0.0f,
                                            display_scale_, command.transform);
      instance.flags = kFlagFill;
      if (!apply_brush_(instance, command.brush, command.opacity)) {
        write_color(instance.fill_color, representative_color(command.brush),
                    command.opacity);
      }
      append_(Batch::Kind::Quad, command.clip_index);
      quads_.push_back(instance);
      break;
    }

    case CommandKind::StrokeRRect: {
      // Snapping the outline is only half the job. The shader puts the stroke's
      // inner edge at `d + stroke_width`, so a width that is not a whole number
      // of device pixels reintroduces the very partial row that snapping the
      // outline just removed: a 1.0 logical border at 1.5x covers 1.5 device
      // pixels and resolves as one solid row plus one at quarter coverage.
      // Held at one pixel so a hairline thins rather than disappears.
      const float transform_scale =
          std::max(command.transform.approximate_scale(), 1e-4f);
      const float width =
          std::max(round_half_up(command.stroke_width * transform_scale *
                                 display_scale_),
                   1.0f) *
          pixel_size_ / transform_scale;
      const float outset = stroke_outset(width, command.stroke_align);
      QuadInstance instance =
          make_instance(command.rect, command.radius, outset, display_scale_,
                        command.transform);
      instance.flags = kFlagStroke;
      instance.stroke_blur[0] = width;
      if (apply_brush_(instance, command.brush, command.opacity))
        instance.flags |= kFlagStrokeGradient;
      write_color(instance.stroke_color, representative_color(command.brush),
                  command.opacity);

      // Fuse with the fill that came immediately before when it traces the same
      // outline in the same batch. Shading such a pair twice and letting the
      // blender combine the two coverages is what tints a thin border with the
      // fill colour wherever the border's own coverage is partial: pronounced
      // on the curved parts of a rounded rect, absent on the axis-aligned ones,
      // which reads as a border of uneven weight.
      if (!quads_.empty() && !batches_.empty() &&
          batches_.back().kind == Batch::Kind::Quad &&
          batches_.back().clip_index == command.clip_index) {
        QuadInstance &previous = quads_.back();
        const bool two_gradients = (previous.flags & kFlagGradient) != 0 &&
                                   (instance.flags & kFlagStrokeGradient) != 0;
        if (!two_gradients &&
            (previous.flags & (kFlagStroke | kFlagShadow)) == 0 &&
            same_geometry(previous, instance)) {
          previous.flags |= kFlagStroke;
          previous.stroke_blur[0] = instance.stroke_blur[0];
          std::memcpy(previous.stroke_color, instance.stroke_color,
                      sizeof(previous.stroke_color));
          if ((instance.flags & kFlagStrokeGradient) != 0) {
            // A solid fill and gradient stroke share one gradient payload. The
            // shader selects it only for the stroke band, so the fill remains
            // independent while both edges are analytically shaded once.
            std::memcpy(previous.gradient_a, instance.gradient_a,
                        sizeof(previous.gradient_a));
            std::memcpy(previous.gradient_b, instance.gradient_b,
                        sizeof(previous.gradient_b));
            std::memcpy(previous.gradient_meta, instance.gradient_meta,
                        sizeof(previous.gradient_meta));
            previous.gradient_v = instance.gradient_v;
            previous.flags |= instance.flags & (kFlagGradient | kFlagConic |
                                                kFlagStrokeGradient);
          }
          break;
        }
      }

      append_(Batch::Kind::Quad, command.clip_index);
      quads_.push_back(instance);
      break;
    }

    case CommandKind::Shadow: {
      // Two rounded rects, both of them needed in the shader. `box` is the
      // reference box the shadow belongs to -- the border box for an outer
      // shadow, the padding box for an inner one -- and `silhouette` is the
      // shape actually blurred: the box displaced by the offset, then grown by
      // the spread outward or shrunk by it inward.
      const Shadow &shadow = command.shadow;
      Transform transform = command.transform;
      const Rect<float> box = resolve_rect(command.rect, display_scale_,
                                           transform);

      Rect<float> silhouette = box;
      silhouette.origin.x += shadow.offset.x;
      silhouette.origin.y += shadow.offset.y;
      if (transform.is_translation())
        silhouette = snap_rect_to_pixel(silhouette, display_scale_);

      const auto center_of = [](const Rect<float> &rect) {
        return Point<float>(rect.origin.x + rect.size.width * 0.5f,
                            rect.origin.y + rect.size.height * 0.5f);
      };
      const Point<float> box_center = center_of(box);
      const Point<float> silhouette_center = center_of(silhouette);

      QuadInstance instance;
      if (shadow.inset) {
        // The quad is the box, because that is what the shadow is confined to,
        // and the blurred hole is the second shape.
        instance = make_instance(box, command.radius, 0.0f, display_scale_,
                                 transform);
        write_shadow_shape(instance,
                           Point<float>(silhouette_center.x - box_center.x,
                                        silhouette_center.y - box_center.y),
                           silhouette.size, command.radius, -shadow.spread);
        instance.flags = kFlagShadow | kFlagShadowInset;
      } else {
        // The quad is the blurred silhouette, and the box is knocked out of it
        // -- css-backgrounds-3 does not paint an outer shadow inside the shape
        // that casts it, which is what made one show through a translucent
        // background as if it were an inner shadow.
        instance = make_instance(silhouette, command.radius, shadow.spread,
                                 display_scale_, transform);
        write_shadow_shape(instance,
                           Point<float>(box_center.x - silhouette_center.x,
                                        box_center.y - silhouette_center.y),
                           box.size, command.radius, 0.0f);
        instance.flags = kFlagShadow;
      }

      instance.stroke_blur[1] = std::max(shadow.blur, 0.0f);
      write_color(instance.fill_color, shadow.color, command.opacity);

      append_(Batch::Kind::Quad, command.clip_index);
      quads_.push_back(instance);
      break;
    }

    case CommandKind::FillPath:
    case CommandKind::StrokePath: {
      Point<float> device_offset;
      const CachedMask *cached = mask_for_(list, command, device_offset);
      if (!cached)
        break;

      // The mask was rasterised with the command's linear transform already
      // folded in, so its quad carries none: it is a device-aligned blit,
      // displaced by whatever whole-pixel translation was held back from the
      // raster so the cache could be shared across offsets.
      const Rect<float> dst(
          (static_cast<float>(cached->origin_x) + device_offset.x) *
              pixel_size_,
          (static_cast<float>(cached->origin_y) + device_offset.y) *
              pixel_size_,
          static_cast<float>(cached->width) * pixel_size_,
          static_cast<float>(cached->height) * pixel_size_);
      MaskInstance instance = make_mask_instance(dst, Transform{});
      atlas_->uv_for(cached->slot, instance.uv);
      write_color(instance.color, representative_color(command.brush),
                  command.opacity);
      instance.flags = kFlagMaskChannel;

      append_(Batch::Kind::Mask, command.clip_index, atlas_->texture(),
              atlas_->sampler());
      masks_.push_back(instance);
      break;
    }

    case CommandKind::Image: {
      if (!command.image)
        break;

      const ImagePlacement *placement = place_image_(*command.image);
      if (!placement)
        break;

      // Snapped for the same reason a fill is, and with a bonus: an image drawn
      // at its native size lands texel-on-pixel instead of resampling. A pure
      // translation is folded in first and snapped along with it, so a moved
      // image is still an unresampled blit. Only a scaled or rotated one is off
      // the device grid to begin with; it keeps its subpixel position and is
      // placed by the vertex shader.
      Transform to_screen = command.transform;
      Rect<float> dst = command.rect;
      if (to_screen.is_translation()) {
        const Point<float> shift =
            device_translation(to_screen, display_scale_);
        dst = snap_rect_to_pixel(dst, display_scale_);
        dst.origin.x += shift.x * pixel_size_;
        dst.origin.y += shift.y * pixel_size_;
        to_screen.e = 0.0f;
        to_screen.f = 0.0f;
      }

      MaskInstance instance = make_mask_instance(dst, to_screen);

      // The placement spans the whole image wherever it landed -- a slot on the
      // shared page, or a texture of its own. The command's source region is a
      // fraction of the image, so it is interpolated into that span rather than
      // written over it.
      const float du = placement->uv[2] - placement->uv[0];
      const float dv = placement->uv[3] - placement->uv[1];
      instance.uv[0] = placement->uv[0] + command.source[0] * du;
      instance.uv[1] = placement->uv[1] + command.source[1] * dv;
      instance.uv[2] = placement->uv[0] + command.source[2] * du;
      instance.uv[3] = placement->uv[1] + command.source[3] * dv;

      write_color(instance.color, representative_color(command.brush),
                  command.opacity);
      instance.flags = 0; // sample RGBA rather than a coverage channel

      append_(Batch::Kind::Mask, command.clip_index, placement->texture,
              placement->sampler);
      masks_.push_back(instance);
      break;
    }

    case CommandKind::Glyphs: {
      const GlyphRun &run = command.text_layout
                                ? command.text_layout->runs()[command.run_index]
                                : list.run(command.run_index);
      if (!run.font || !ensure_coverage_atlas_())
        break;

      const Font &font = *run.font;
      const Transform &transform = command.transform;

      // A pure translation counts as upright, and this is the whole reason a
      // button that lifts on hover keeps its label crisp. The linear part is
      // identity, so the run still lies along a horizontal baseline on the same
      // pixel grid: the offset just moves where on that grid it starts, and
      // `transform.apply` below carries it into the run origin that is rounded
      // there. Treating it as a general transform instead cost the run its
      // whole-pixel baseline, its quarter-pixel horizontal buckets and its
      // point sampler -- three separate ways to blur text that had not changed
      // shape at all.
      const bool upright = transform.is_translation();
      const float transform_scale =
          std::max(transform.approximate_scale(), 1e-4f);
      const int device_size = static_cast<int>(
          font.size() * display_scale_ * transform_scale + 0.5f);
      if (device_size <= 0)
        break;

      std::uint8_t color[4];
      write_color(color, representative_color(command.brush), command.opacity);

      // Device pixels per unit of the run's own coordinate space. The glyph is
      // rasterised at that combined size, so its bitmap converts back into run
      // units through the same factor.
      const float glyph_scale = display_scale_ * transform_scale;

      // The run's origin lands on the device grid once, and every glyph is
      // placed relative to it. Snapping each glyph's absolute position instead
      // is what let runs drift against one another: with a fractional line
      // pitch, neighbouring runs round in different directions, so the gap
      // between two lines flickers between two values as the block scrolls.
      //
      // The translation is rounded apart from the origin rather than added into
      // it first; `snap_with_shift` says why, and a label that stayed behind
      // its button through a whole press is what it costs otherwise.
      const float origin_x = snap_with_shift(command.rect.origin.x,
                                             transform.e, display_scale_);
      const float origin_y = snap_with_shift(command.rect.origin.y,
                                             transform.f, display_scale_);

      for (const PositionedGlyph &glyph : run.glyphs) {
        // Pen position in device pixels, split into a whole pixel and the
        // quarter-pixel the glyph is rasterised at. An affine transform
        // distributes over the sum, so applying the linear part to the offset
        // and adding it to the mapped origin is the same point as before.
        //
        // None of it applies once the run is transformed: the glyph does not
        // sit on the device grid at all then, and quantising to it would only
        // shear the spacing.
        float whole_x = 0.0f;
        float device_y = 0.0f;
        int subpixel = 0;
        if (upright) {
          const Point<float> offset = transform.apply_vector(glyph.offset);
          const float device_x = origin_x + offset.x * display_scale_;
          device_y = origin_y + offset.y * display_scale_;

          // Round to the nearest quarter pixel and let the carry fall into the
          // whole-pixel part. Truncating the fraction instead biases every
          // glyph left by up to a quarter pixel and puts the bucket boundary on
          // the integers, where one ulp of noise flips a crisp stem into a
          // blurred one -- the same reason the baseline below rounds rather
          // than floors.
          const float quantized =
              std::floor(device_x * GlyphCache::kSubpixelSteps + 0.5f);
          whole_x = std::floor(quantized / GlyphCache::kSubpixelSteps);
          subpixel = static_cast<int>(quantized -
                                      whole_x * GlyphCache::kSubpixelSteps);
        }

        GlyphCache::PageFull full = GlyphCache::PageFull::None;
        const GlyphCache::Entry *entry = glyphs_->get(
            font, glyph.id, device_size, subpixel, *atlas_,
            font.has_color_glyphs() ? ensure_color_atlas_() : nullptr, &full);

        // Same deferral as a path mask: the page cannot be recycled while the
        // glyphs already queued this frame still point into it. Text that did
        // not fit is missing until the next frame -- which it now reaches at
        // all: nothing but a path mask used to be able to trigger a recycle, so
        // a page filled by text stayed full for the rest of the session.
        if (full == GlyphCache::PageFull::Coverage) {
          coverage_full_ = true;
          recycle_coverage_ = true;
        } else if (full == GlyphCache::PageFull::Color) {
          color_full_ = true;
          recycle_color_ = true;
        }

        if (!entry || entry->width == 0 || entry->height == 0)
          continue;

        Atlas &page = entry->color ? *color_atlas_ : *atlas_;

        Rect<float> dst;
        if (upright) {
          // Position goes through the biased conversion; the extents are whole
          // device pixels either way and do not decide a coverage boundary.
          dst = Rect<float>(
              device_to_logical(whole_x + static_cast<float>(entry->left),
                                display_scale_),
              device_to_logical(std::floor(device_y + 0.5f) -
                                    static_cast<float>(entry->top),
                                display_scale_),
              static_cast<float>(entry->width) * pixel_size_,
              static_cast<float>(entry->height) * pixel_size_);
        } else {
          // Laid out in the run's own space and carried to the screen by the
          // vertex shader, so the quad turns with the text instead of standing
          // upright along a slanted baseline. The bitmap is still rasterised
          // axis-aligned, so a rotated run is hinted for the wrong orientation
          // -- visible under a magnifier, and a great deal better than glyphs
          // that ignore the rotation outright.
          const float inv = 1.0f / glyph_scale;
          dst = Rect<float>(command.rect.origin.x + glyph.offset.x +
                                static_cast<float>(entry->left) * inv,
                            command.rect.origin.y + glyph.offset.y -
                                static_cast<float>(entry->top) * inv,
                            static_cast<float>(entry->width) * inv,
                            static_cast<float>(entry->height) * inv);
        }

        MaskInstance instance =
            make_mask_instance(dst, upright ? Transform{} : transform);
        page.uv_for(entry->slot, instance.uv);
        std::memcpy(instance.color, color, sizeof(color));

        // A colour glyph carries its own premultiplied pixels, so the tint only
        // modulates its alpha; a coverage glyph is tinted outright.
        instance.flags = entry->color ? 0u : kFlagMaskChannel;

        // Texel-on-pixel only holds for upright text. Point-sampling a coverage
        // bitmap through a rotation is visibly ragged, so a transformed run
        // resamples bilinearly instead.
        append_(Batch::Kind::Mask, command.clip_index, page.texture(),
                upright ? page.sampler() : page.smooth_sampler());
        masks_.push_back(instance);
      }
      break;
    }
    }
  }
}

void GpuRenderer::render(const DisplayList &list, Color clear_color) {
  std::uint32_t frame_width = 0;
  std::uint32_t frame_height = 0;
  if (!device_.begin_frame(frame_width, frame_height))
    return;

  pixel_width_ = static_cast<int>(frame_width);
  pixel_height_ = static_cast<int>(frame_height);
  logical_width_ = static_cast<float>(frame_width) * pixel_size_;
  logical_height_ = static_cast<float>(frame_height) * pixel_size_;

  build_batches_(list);

  const bool have_quads =
      !quads_.empty() &&
      device_.write_buffer(
          *quad_buffer_, quads_.data(),
          static_cast<std::uint32_t>(quads_.size() * sizeof(QuadInstance)));
  const bool have_masks =
      !masks_.empty() &&
      device_.write_buffer(
          *mask_buffer_, masks_.data(),
          static_cast<std::uint32_t>(masks_.size() * sizeof(MaskInstance)));

  if (atlas_)
    atlas_->flush();
  if (color_atlas_)
    color_atlas_->flush();
  gradients_->flush();
  flush_standalone_();

  device_.begin_render(std::clamp(clear_color.r, 0.0f, 1.0f),
                       std::clamp(clear_color.g, 0.0f, 1.0f),
                       std::clamp(clear_color.b, 0.0f, 1.0f),
                       std::clamp(clear_color.a, 0.0f, 1.0f));

  Batch::Kind bound = Batch::Kind::Quad;
  rhi::Texture *bound_texture = nullptr;
  rhi::Sampler *bound_sampler = nullptr;
  bool anything_bound = false;

  for (const Batch &batch : batches_) {
    if (batch.count == 0)
      continue;
    if (batch.kind == Batch::Kind::Quad && !have_quads)
      continue;
    if (batch.kind == Batch::Kind::Mask && !have_masks)
      continue;

    if (!anything_bound || bound != batch.kind) {
      if (batch.kind == Batch::Kind::Quad) {
        device_.bind_pipeline(*quad_pipeline_);
        device_.bind_vertex_buffer(*quad_buffer_);
      } else {
        device_.bind_pipeline(*mask_pipeline_);
        device_.bind_vertex_buffer(*mask_buffer_);
      }
      // Both pipelines sample a texture, and a pipeline switch drops whatever
      // the previous one had bound.
      bound_texture = nullptr;
      bound_sampler = nullptr;
      bound = batch.kind;
      anything_bound = true;
    }

    // A quad batch reads its stops from the one shared table; a mask batch
    // reads the atlas page its instances landed on.
    rhi::Texture *texture = batch.kind == Batch::Kind::Quad
                                ? gradients_->texture()
                                : batch.texture;
    rhi::Sampler *sampler = batch.kind == Batch::Kind::Quad
                                ? gradients_->sampler()
                                : batch.sampler;

    if (texture != bound_texture || sampler != bound_sampler) {
      if (!texture || !sampler)
        continue;

      // Nothing is bound when this fails, and whatever the last batch bound is
      // still in place -- so the draw has to be skipped rather than issued
      // against the wrong texture.
      if (!device_.bind_texture(*texture, *sampler)) {
        bound_texture = nullptr;
        bound_sampler = nullptr;
        continue;
      }
      bound_texture = texture;
      bound_sampler = sampler;
    }

    const ClipState &clip = list.clips()[batch.clip_index];

    SceneUniforms uniforms{};
    uniforms.viewport[0] = logical_width_;
    uniforms.viewport[1] = logical_height_;
    uniforms.pixel_size = pixel_size_;
    uniforms.clip_count = static_cast<float>(clip.rrect_count);

    for (std::uint32_t i = 0; i < clip.rrect_count; ++i) {
      const ClipRRect &r = clip.rrects[i];
      uniforms.clip_rect[i][0] = r.rect.origin.x;
      uniforms.clip_rect[i][1] = r.rect.origin.y;
      uniforms.clip_rect[i][2] = r.rect.size.width;
      uniforms.clip_rect[i][3] = r.rect.size.height;
      uniforms.clip_radii[i][0] = r.radius.left_top;
      uniforms.clip_radii[i][1] = r.radius.right_top;
      uniforms.clip_radii[i][2] = r.radius.right_bottom;
      uniforms.clip_radii[i][3] = r.radius.left_bottom;
    }

    device_.set_uniforms(&uniforms, sizeof(uniforms));

    // The scissor is the conservative half of the clip and costs nothing; the
    // rounded refinement happens in the shader.
    const int left = std::clamp(
        static_cast<int>(std::floor(clip.scissor.origin.x * display_scale_)), 0,
        pixel_width_);
    const int top = std::clamp(
        static_cast<int>(std::floor(clip.scissor.origin.y * display_scale_)), 0,
        pixel_height_);
    const int right =
        std::clamp(static_cast<int>(std::ceil(
                       (clip.scissor.origin.x + clip.scissor.size.width) *
                       display_scale_)),
                   0, pixel_width_);
    const int bottom =
        std::clamp(static_cast<int>(std::ceil(
                       (clip.scissor.origin.y + clip.scissor.size.height) *
                       display_scale_)),
                   0, pixel_height_);
    device_.set_scissor(static_cast<std::uint32_t>(left),
                        static_cast<std::uint32_t>(top),
                        static_cast<std::uint32_t>(right - left),
                        static_cast<std::uint32_t>(bottom - top));

    device_.draw(kVerticesPerQuad, batch.count, 0, batch.first);
  }

  device_.end_render();
  device_.end_frame();

  // Page recycles and texture releases both wait until here, where no instance
  // referring to them is still queued and the frame they belonged to has been
  // handed to the driver.
  apply_pending_recycles_();
  evict_standalone_();
  ++frame_;
}

} // namespace voidui
