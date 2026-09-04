#include "render/gradient_table.h"

#include <SDL3/SDL_log.h>

#include <algorithm>
#include <cstring>
#include <limits>

namespace voidui {

namespace {

constexpr int kFloatsPerTexel = 4;
constexpr int kFloatsPerRow = GradientTable::kWidth * kFloatsPerTexel;

std::uint64_t hash_gradient(const ResolvedGradient &gradient) {
  std::uint64_t state = 14695981039346656037ull;
  const auto mix = [&state](const void *data, std::size_t size) {
    const auto *bytes = static_cast<const std::uint8_t *>(data);
    for (std::size_t i = 0; i < size; ++i) {
      state ^= bytes[i];
      state *= 1099511628211ull; // FNV-1a
    }
  };

  // Both zeroes hash alike, so a stop at -0.0 does not claim its own row.
  const auto mix_float = [&mix](float value) {
    const float canonical = value == 0.0f ? 0.0f : value;
    mix(&canonical, sizeof(canonical));
  };

  mix(&gradient.count, sizeof(gradient.count));
  mix(&gradient.interpolation.space, sizeof(gradient.interpolation.space));
  mix(&gradient.interpolation.hue, sizeof(gradient.interpolation.hue));
  for (std::uint32_t i = 0; i < gradient.count; ++i) {
    // Field by field: Color pads after its space tag, and padding is not
    // guaranteed to hold anything in particular.
    const Color &color = gradient.colors[i];
    mix_float(color.r);
    mix_float(color.g);
    mix_float(color.b);
    mix_float(color.a);
    mix(&color.space, sizeof(color.space));
    mix_float(gradient.positions[i]);
  }
  return state;
}

} // namespace

GradientTable::GradientTable(rhi::Device &device,
                             std::unique_ptr<rhi::Texture> texture,
                             std::unique_ptr<rhi::Sampler> sampler)
    : device_(device), texture_(std::move(texture)),
      sampler_(std::move(sampler)) {}

GradientTable::~GradientTable() = default;

std::unique_ptr<GradientTable> GradientTable::create(rhi::Device &device) {
  std::unique_ptr<rhi::Texture> texture = device.create_texture(
      static_cast<std::uint32_t>(kWidth), static_cast<std::uint32_t>(kRows),
      rhi::TextureFormat::Rgba32Float);
  if (!texture) {
    SDL_Log("voidui: gradient table texture creation failed");
    return nullptr;
  }

  // Point sampling: the shader addresses individual texels, and every value in
  // here is a colour or a position that must arrive exactly as written.
  std::unique_ptr<rhi::Sampler> sampler =
      device.create_sampler(rhi::Filter::Nearest);
  if (!sampler) {
    SDL_Log("voidui: gradient table sampler creation failed");
    return nullptr;
  }

  return std::unique_ptr<GradientTable>(
      new GradientTable(device, std::move(texture), std::move(sampler)));
}

int GradientTable::claim_row_(std::uint64_t frame) {
  if (used_rows_ < kRows)
    return used_rows_++;

  // Full: retire the row nothing has drawn for longest. A row the frame being
  // built already points at is never a candidate -- rewriting it would change
  // the colours under instances that are already queued.
  std::uint64_t oldest = std::numeric_limits<std::uint64_t>::max();
  auto victim = rows_.end();
  for (auto it = rows_.begin(); it != rows_.end(); ++it) {
    if (it->second.last_used >= frame || it->second.last_used > oldest)
      continue;
    oldest = it->second.last_used;
    victim = it;
  }
  if (victim == rows_.end())
    return -1;

  const int row = victim->second.index;
  rows_.erase(victim);
  return row;
}

void GradientTable::write_row_(int row, const ResolvedGradient &gradient) {
  const std::size_t offset = scratch_.size();
  scratch_.resize(offset + kFloatsPerRow, 0.0f);
  float *texels = scratch_.data() + offset;

  // Texels 0 and 1 hold the eight positions; one colour follows per texel.
  for (std::uint32_t i = 0; i < kMaxGradientStops; ++i)
    texels[i] = i < gradient.count ? gradient.positions[i] : 0.0f;

  // Stops are converted into the gradient's own interpolation space once,
  // here, so the fragment shader can lerp two of them directly and convert
  // only the result. The alternative -- storing sRGB and converting both
  // endpoints per pixel -- costs two cube roots and six transfer functions on
  // every shaded pixel of every Oklab gradient on screen.
  const ColorInterpolationSpace space = gradient.interpolation.space;
  const bool polar = is_polar(space);

  std::array<std::array<float, 3>, kMaxGradientStops> components{};
  for (std::uint32_t i = 0; i < gradient.count; ++i)
    components[i] = gradient.colors[i].to(space);

  if (polar) {
    // A stop with no chroma has no hue worth honouring, so it takes one from
    // its neighbours instead of sweeping the wheel from an arbitrary angle.
    constexpr float kAchromatic = 1e-4f;
    for (std::uint32_t i = 1; i < gradient.count; ++i)
      if (components[i][1] <= kAchromatic)
        components[i][2] = components[i - 1][2];
    for (std::uint32_t i = gradient.count; i-- > 1;)
      if (components[i - 1][1] <= kAchromatic)
        components[i - 1][2] = components[i][2];

    // Unwrapping each hue against the one before it turns the arc CSS asks for
    // into a plain lerp, which is all the shader can afford to do.
    for (std::uint32_t i = 1; i < gradient.count; ++i)
      components[i][2] = adjust_hue(components[i - 1][2], components[i][2],
                                    gradient.interpolation.hue);
  }

  for (std::uint32_t i = 0; i < kMaxGradientStops; ++i) {
    float *texel = texels + (kPositionTexels + i) * kFloatsPerTexel;
    if (i >= gradient.count) {
      texel[0] = texel[1] = texel[2] = texel[3] = 0.0f;
      continue;
    }
    // Premultiplied, as CSS interpolates -- except the hue, which is an angle
    // and would be meaningless scaled.
    const float alpha = std::clamp(gradient.colors[i].a, 0.0f, 1.0f);
    texel[0] = components[i][0] * alpha;
    texel[1] = components[i][1] * alpha;
    texel[2] = polar ? components[i][2] : components[i][2] * alpha;
    texel[3] = alpha;
  }

  pending_.push_back(row);
}

bool GradientTable::row_for(const ResolvedGradient &gradient,
                            std::uint64_t frame, float &out_v) {
  const auto to_v = [](int row) {
    return (static_cast<float>(row) + 0.5f) / static_cast<float>(kRows);
  };

  const std::uint64_t key = hash_gradient(gradient);
  if (auto it = rows_.find(key); it != rows_.end()) {
    it->second.last_used = frame;
    out_v = to_v(it->second.index);
    return true;
  }

  const int row = claim_row_(frame);
  if (row < 0)
    return false;

  rows_.emplace(key, Row{row, frame});
  write_row_(row, gradient);
  out_v = to_v(row);
  return true;
}

bool GradientTable::flush() {
  if (pending_.empty())
    return true;

  std::vector<rhi::TextureUpload> uploads;
  uploads.reserve(pending_.size());
  for (std::size_t i = 0; i < pending_.size(); ++i) {
    uploads.push_back(
        {0, static_cast<std::uint32_t>(pending_[i]),
         static_cast<std::uint32_t>(kWidth), 1,
         static_cast<std::uint32_t>(i * kFloatsPerRow * sizeof(float)),
         static_cast<std::uint32_t>(kFloatsPerRow * sizeof(float))});
  }

  const bool uploaded = device_.upload_texture(
      *texture_, scratch_.data(),
      static_cast<std::uint32_t>(scratch_.size() * sizeof(float)),
      uploads.data(), static_cast<std::uint32_t>(uploads.size()));

  scratch_.clear();
  pending_.clear();
  return uploaded;
}

} // namespace voidui
