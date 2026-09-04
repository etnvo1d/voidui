#pragma once

#include <cstddef>
#include <cstdint>
#include <memory>

struct SDL_Window;

namespace voidui::rhi {

enum class TextureFormat : std::uint8_t { R8Unorm, Rgba8Unorm, Rgba32Float };

/// Bytes one texel of `format` occupies, which is also the alignment a copy
/// into it has to respect.
constexpr std::uint32_t texel_size(TextureFormat format) {
  switch (format) {
  case TextureFormat::R8Unorm:
    return 1;
  case TextureFormat::Rgba8Unorm:
    return 4;
  case TextureFormat::Rgba32Float:
    return 16;
  }
  return 4;
}
enum class Filter : std::uint8_t { Nearest, Linear };
enum class VertexFormat : std::uint8_t {
  Float,
  Float2,
  Float4,
  UByte4Norm,
  UInt
};

struct VertexAttribute {
  std::uint32_t location = 0;
  VertexFormat format = VertexFormat::Float2;
  std::uint32_t offset = 0;
};

struct ShaderCode {
  const void *data = nullptr;
  std::size_t size = 0;
  const char *entrypoint = nullptr;
};

struct PipelineDesc {
  ShaderCode vertex;
  ShaderCode fragment;
  const VertexAttribute *attributes = nullptr;
  std::uint32_t attribute_count = 0;
  std::uint32_t stride = 0;
  std::uint32_t uniform_size = 0;
  bool sampled_texture = false;
};

struct TextureUpload {
  std::uint32_t x = 0;
  std::uint32_t y = 0;
  std::uint32_t width = 0;
  std::uint32_t height = 0;
  std::uint32_t offset = 0;
  std::uint32_t row_pitch = 0;
};

class Buffer {
public:
  struct Impl;
  ~Buffer();
  Buffer(const Buffer &) = delete;
  Buffer &operator=(const Buffer &) = delete;

private:
  explicit Buffer(std::unique_ptr<Impl> impl);
  std::unique_ptr<Impl> impl_;
  friend class Device;
};

class Texture {
public:
  struct Impl;
  ~Texture();
  Texture(const Texture &) = delete;
  Texture &operator=(const Texture &) = delete;

private:
  explicit Texture(std::unique_ptr<Impl> impl);
  std::unique_ptr<Impl> impl_;
  friend class Device;
};

class Sampler {
public:
  struct Impl;
  ~Sampler();
  Sampler(const Sampler &) = delete;
  Sampler &operator=(const Sampler &) = delete;

private:
  explicit Sampler(std::unique_ptr<Impl> impl);
  std::unique_ptr<Impl> impl_;
  friend class Device;
};

class Pipeline {
public:
  struct Impl;
  ~Pipeline();
  Pipeline(const Pipeline &) = delete;
  Pipeline &operator=(const Pipeline &) = delete;

private:
  explicit Pipeline(std::unique_ptr<Impl> impl);
  std::unique_ptr<Impl> impl_;
  friend class Device;
};

/// Small rendering interface implemented once per native graphics API.
/// A device owns one window and records at most one frame at a time.
class Device {
public:
  struct Impl;

  static std::unique_ptr<Device> create(SDL_Window *window);
  ~Device();
  Device(const Device &) = delete;
  Device &operator=(const Device &) = delete;

  std::unique_ptr<Buffer> create_buffer();
  std::unique_ptr<Texture> create_texture(std::uint32_t width,
                                          std::uint32_t height,
                                          TextureFormat format);
  std::unique_ptr<Sampler> create_sampler(Filter filter);
  std::unique_ptr<Pipeline> create_pipeline(const PipelineDesc &desc);

  bool begin_frame(std::uint32_t &width, std::uint32_t &height);
  bool write_buffer(Buffer &buffer, const void *data, std::uint32_t size);
  bool upload_texture(Texture &texture, const void *data, std::uint32_t size,
                      const TextureUpload *uploads, std::uint32_t upload_count);
  void begin_render(float red, float green, float blue, float alpha);
  void bind_pipeline(Pipeline &pipeline);
  void bind_vertex_buffer(Buffer &buffer);
  /// False when the binding could not be made, in which case nothing was
  /// bound. Callers must skip the draw rather than issue it -- whatever was
  /// bound before is still bound, and drawing with it paints the wrong texture.
  [[nodiscard]] bool bind_texture(Texture &texture, Sampler &sampler);
  void set_uniforms(const void *data, std::uint32_t size);
  void set_scissor(std::uint32_t x, std::uint32_t y, std::uint32_t width,
                   std::uint32_t height);
  void draw(std::uint32_t vertex_count, std::uint32_t instance_count,
            std::uint32_t first_vertex, std::uint32_t first_instance);
  void end_render();
  void end_frame();
  void wait_idle();
  const char *driver() const;

private:
  explicit Device(std::unique_ptr<Impl> impl);
  std::unique_ptr<Impl> impl_;
};

} // namespace voidui::rhi
