#pragma once

#include <cstdint>
#include <memory>
#include <unordered_map>
#include <vector>

#include "render/atlas.h"
#include "render/glyph_cache.h"
#include "render/gradient_table.h"
#include "rhi/device.h"
#include "voidui/render/renderer.h"

namespace voidui {

/// Per-quad payload for the analytic pipeline: a rounded rect with an optional
/// fill (solid or gradient), an optional inner stroke, or a blurred shadow.
///
/// The old GL backend duplicated all of this across four vertices. Keeping the
/// payload instance-rate avoids that 4x bandwidth cost.
///
/// The stops themselves live in `GradientTable` rather than here. Eight colors
/// and eight positions are 64 bytes and ten vertex attributes, and every quad
/// carried them -- solid fills, strokes and shadows included, none of which
/// have a gradient at all. A row lookup is one float.
struct QuadInstance {
  float center[2];
  float half_size[2];
  float radii[4]; // top-left, top-right, bottom-right, bottom-left
  // Gradient line endpoints, in the shape's local 0..1 box coordinates.
  //
  // A shadow has no gradient and reuses these three fields for its second
  // rounded rect -- the box an outer shadow is knocked out of, or the hole an
  // inner one fades towards. `gradient_a` becomes that shape's centre relative
  // to this one's, `gradient_b` its half extents, and `gradient_meta` its four
  // radii. Overlaying them is what keeps a shadow at one quad and leaves the
  // vertex layout alone.
  float gradient_a[2];
  float gradient_b[2];
  // Linear part of the local-to-world transform: a, b, c, d. Translation is
  // already folded into center, which saves two floats per instance.
  float transform[4];
  std::uint8_t fill_color[4]; // also the shadow's color
  std::uint8_t stroke_color[4];
  float stroke_blur[2]; // x = stroke width, y = shadow sigma
  // x = conic angle, y = stop count, z = opacity, w unused. Opacity rides here
  // rather than being baked into the stops so that two elements sharing a
  // brush at different opacities still share one table row.
  float gradient_meta[4];
  float gradient_v; // row of the gradient stop table
  std::uint32_t flags;
};

/// Per-quad payload for the textured pipeline, shared by path coverage masks,
/// glyphs and images.
///
/// Stated the way `QuadInstance` is -- an already-transformed centre plus the
/// linear part -- rather than as an axis-aligned rect. A rect could not carry a
/// transform at all, which is why an image inside a rotated or scaled subtree
/// used to be drawn at its untransformed position, and why a rotated run of
/// text came out as upright glyphs strung along a slanted baseline.
struct MaskInstance {
  float center[2];    // transformed centre, logical units
  float half_size[2]; // half extents before the transform
  // Linear part of the local-to-world transform: a, b, c, d. Translation is
  // already folded into center.
  float transform[4];
  float uv[4]; // u0, v0, u1, v1
  std::uint8_t color[4];
  std::uint32_t flags;
};

class GpuRenderer final : public Renderer {
public:
  static std::unique_ptr<GpuRenderer> create(rhi::Device &device);
  ~GpuRenderer() override;

  GpuRenderer(const GpuRenderer &) = delete;
  GpuRenderer &operator=(const GpuRenderer &) = delete;

  void resize(int pixel_width, int pixel_height, float logical_width,
              float logical_height, float display_scale) override;

  void render(const DisplayList &list, Color clear_color) override;

private:
  /// A run of instances from one pipeline sharing a clip, and therefore one
  /// scissor rect and one uniform push. Batches stay in command order so the
  /// two pipelines interleave correctly.
  struct Batch {
    enum class Kind : std::uint8_t { Quad, Mask };

    Kind kind = Kind::Quad;
    std::uint32_t first = 0;
    std::uint32_t count = 0;
    std::uint32_t clip_index = 0;

    // Which page a Mask batch samples. Glyphs and path masks share the
    // coverage page; images live on their own, and an oversized image gets a
    // texture to itself.
    rhi::Texture *texture = nullptr;
    rhi::Sampler *sampler = nullptr;
  };

  /// A rasterised path held in the atlas, keyed by the path's contents and the
  /// transform it was rasterised under.
  struct CachedMask {
    AtlasSlot slot;
    int origin_x = 0;
    int origin_y = 0;
    int width = 0;
    int height = 0;
  };

  explicit GpuRenderer(rhi::Device &device);

  bool build_pipelines_();
  std::unique_ptr<rhi::Pipeline>
  build_pipeline_(const void *vertex, std::size_t vertex_size,
                  const void *fragment, std::size_t fragment_size,
                  const rhi::VertexAttribute *attributes,
                  std::uint32_t attribute_count, std::uint32_t stride,
                  bool sampled_texture);

  void build_batches_(const DisplayList &list);

  /// Points the instance at a gradient's stops and fills in its geometry.
  /// False for a solid brush, and for the vanishingly rare frame that has
  /// exhausted the stop table -- the caller then paints a representative solid
  /// colour rather than dropping the shape.
  bool apply_brush_(QuadInstance &instance, const Brush &brush, float opacity);

  void append_(Batch::Kind kind, std::uint32_t clip_index,
               rhi::Texture *texture = nullptr,
               rhi::Sampler *sampler = nullptr);

  /// Uploads an image on first use and returns where it landed.
  struct ImagePlacement {
    rhi::Texture *texture = nullptr;
    rhi::Sampler *sampler = nullptr;
    float uv[4]{0.0f, 0.0f, 1.0f, 1.0f};

    /// Standalone placements own a whole texture and are the ones worth
    /// reclaiming; an atlas placement holds shelf space its page can only free
    /// wholesale, and re-uploading it would just consume fresh space.
    bool standalone = false;
    std::uint64_t last_used = 0;
  };
  const ImagePlacement *place_image_(const Image &image);

  /// The coverage page backs both glyphs and path masks; neither may be in use,
  /// so it is created on first demand.
  bool ensure_coverage_atlas_();

  /// The colour page carries images and colour (emoji) glyphs. Also lazy: a UI
  /// with neither pays nothing for it.
  Atlas *ensure_color_atlas_();

  /// Recycles a shared page and every cache that allocates from it.
  ///
  /// The shelf packers cannot free individual entries, so a full page is
  /// dropped wholesale -- and anything still holding an `AtlasSlot` into it is
  /// left pointing at space that is about to be handed out again. Both pages
  /// are shared by more than one cache, which is exactly how that goes wrong
  /// quietly: clearing the coverage page without the glyph cache leaves every
  /// cached glyph sampling whatever mask lands on top of it. Route every clear
  /// through these two so the invariant lives in one place.
  ///
  /// Never call these while a display list is being turned into batches. The
  /// instances already queued carry texture coordinates into the page as it
  /// stands, and staged pixels that have not been flushed yet go out with it.
  /// `build_batches_` records the need instead and `apply_pending_recycles_`
  /// acts on it once the frame has been submitted.
  void recycle_coverage_atlas_();
  void recycle_color_atlas_();
  void apply_pending_recycles_();

  /// Releases standalone image textures nothing has drawn for a long while.
  ///
  /// Atlas-backed images are deliberately left alone: their page reclaims
  /// wholesale, so dropping one entry frees nothing and only costs a re-upload.
  void evict_standalone_();

  /// An image too large for a shared page owns its texture; the pixels wait
  /// here until there is a command buffer to record the copy on.
  ///
  /// The wait is why this holds a Blob rather than a pointer: premultiplied
  /// pixels belong to the Image itself and are merely shared until the copy is
  /// recorded, while straight ones were converted into a buffer this is the
  /// only owner of. A Blob spells both without a copy either way.
  struct PendingTexture {
    rhi::Texture *texture = nullptr;
    int width = 0;
    int height = 0;
    Blob pixels;
  };
  void flush_standalone_();
  /// `device_offset` receives the whole-pixel translation that was kept out of
  /// the mask's identity and so still has to be applied to its blit.
  const CachedMask *mask_for_(const DisplayList &list,
                              const DrawCommand &command,
                              Point<float> &device_offset);

  rhi::Device &device_;
  std::unique_ptr<rhi::Buffer> quad_buffer_;
  std::unique_ptr<rhi::Buffer> mask_buffer_;
  std::unique_ptr<Atlas> atlas_;
  std::unique_ptr<Atlas> color_atlas_;
  std::unique_ptr<GlyphCache> glyphs_;
  std::unique_ptr<GradientTable> gradients_;

  std::unordered_map<std::uint64_t, ImagePlacement> image_cache_;
  std::unordered_map<std::uint64_t, std::unique_ptr<rhi::Texture>>
      standalone_textures_;
  std::vector<PendingTexture> pending_standalone_;

  std::unique_ptr<rhi::Pipeline> quad_pipeline_;
  std::unique_ptr<rhi::Pipeline> mask_pipeline_;

  std::vector<QuadInstance> quads_;
  std::vector<MaskInstance> masks_;
  std::vector<Batch> batches_;

  std::unordered_map<std::uint64_t, CachedMask> mask_cache_;

  std::uint64_t frame_ = 0;

  // A page that filled up part way through this frame. Further entries are
  // simply dropped rather than retried, and the recycle waits for the end of
  // the frame. The flags reset every frame: the sticky one this replaces was
  // only ever cleared by a DPI change, so a single path too large for the page
  // stopped every path in the session from drawing again.
  bool coverage_full_ = false;
  bool color_full_ = false;
  bool recycle_coverage_ = false;
  bool recycle_color_ = false;

  float logical_width_ = 0.0f;
  float logical_height_ = 0.0f;
  float pixel_size_ = 1.0f;    // logical units per device pixel
  float display_scale_ = 1.0f; // device pixels per logical unit
  int pixel_width_ = 0;
  int pixel_height_ = 0;
};

} // namespace voidui
