#pragma once

#include <array>
#include <cstdint>
#include <memory>
#include <unordered_map>
#include <vector>

#include "rhi/device.h"
#include "voidui/core/gradient.h"

namespace voidui {

/// Gradient stops in the form the GPU consumes: colors, positions already
/// resolved to 0..1 along the gradient line, and the space the ramp between
/// them is mixed in.
///
/// Deliberately free of the brush that produced it. A linear gradient's CSS
/// angle and a conic gradient's turn resolve to the same shape, so both kinds
/// share one table and one lookup.
struct ResolvedGradient {
  std::array<Color, kMaxGradientStops> colors{
      Color::TRANSPARENT, Color::TRANSPARENT, Color::TRANSPARENT,
      Color::TRANSPARENT, Color::TRANSPARENT, Color::TRANSPARENT,
      Color::TRANSPARENT, Color::TRANSPARENT};
  std::array<float, kMaxGradientStops> positions{};
  std::uint32_t count = 0;

  /// Always concrete by the time it reaches here -- the brush folds an
  /// unspecified method against its own stops before handing it over.
  ColorInterpolationMethod interpolation{};
};

/// Every distinct set of gradient stops on screen, in one small texture.
///
/// This lives here rather than in the vertex stream because the stream is the
/// wrong place for it: eight colors and eight positions are 64 bytes and ten
/// vertex attributes, and *every* quad paid them -- the solid fills, the
/// strokes, the shadows, none of which carry a gradient at all. A row lookup
/// costs one float, and the fragment shader trades a sixteen-way branch ladder
/// for four texture fetches.
///
/// Rows are fixed-size and independent, so a full table evicts the least
/// recently used row rather than being recycled wholesale the way the shelf-
/// packed atlases must be. Nothing referenced by the frame being built is ever
/// evicted, so no instance is left pointing at a row that changed under it.
class GradientTable {
public:
  /// Two texels carry the eight positions; the rest carry one color each.
  static constexpr int kPositionTexels = 2;
  static constexpr int kWidth =
      kPositionTexels + static_cast<int>(kMaxGradientStops);
  static constexpr int kRows = 256;

  static std::unique_ptr<GradientTable> create(rhi::Device &device);
  ~GradientTable();

  GradientTable(const GradientTable &) = delete;
  GradientTable &operator=(const GradientTable &) = delete;

  /// The `v` coordinate sampling the row that holds these stops, uploading
  /// them on first use. False when every row is already claimed by `frame`,
  /// which takes several hundred distinct gradients in one frame.
  bool row_for(const ResolvedGradient &gradient, std::uint64_t frame,
               float &out_v);

  /// Uploads every row staged since the last call.
  bool flush();

  rhi::Texture *texture() const { return texture_.get(); }
  rhi::Sampler *sampler() const { return sampler_.get(); }

private:
  struct Row {
    int index = 0;
    std::uint64_t last_used = 0;
  };

  GradientTable(rhi::Device &device, std::unique_ptr<rhi::Texture> texture,
                std::unique_ptr<rhi::Sampler> sampler);

  int claim_row_(std::uint64_t frame);
  void write_row_(int row, const ResolvedGradient &gradient);

  rhi::Device &device_;
  std::unique_ptr<rhi::Texture> texture_;
  std::unique_ptr<rhi::Sampler> sampler_;

  std::unordered_map<std::uint64_t, Row> rows_;
  int used_rows_ = 0;

  std::vector<float> scratch_;
  std::vector<int> pending_;
};

} // namespace voidui
