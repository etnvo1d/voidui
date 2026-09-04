#include "rhi/device.h"

#include <SDL3/SDL_log.h>
#include <SDL3/SDL_metal.h>
#include <SDL3/SDL_video.h>

#import <Metal/Metal.h>
#import <QuartzCore/CAMetalLayer.h>
#import <dispatch/dispatch.h>

#include <algorithm>
#include <cstring>
#include <utility>
#include <vector>

namespace voidui::rhi {

namespace {

constexpr std::uint32_t kFramesInFlight = 2;

MTLVertexFormat vertex_format(VertexFormat format) {
  switch (format) {
  case VertexFormat::Float:
    return MTLVertexFormatFloat;
  case VertexFormat::Float2:
    return MTLVertexFormatFloat2;
  case VertexFormat::Float4:
    return MTLVertexFormatFloat4;
  case VertexFormat::UByte4Norm:
    return MTLVertexFormatUChar4Normalized;
  case VertexFormat::UInt:
    return MTLVertexFormatUInt;
  }
}

MTLPixelFormat texture_format(TextureFormat format) {
  switch (format) {
  case TextureFormat::R8Unorm:
    return MTLPixelFormatR8Unorm;
  case TextureFormat::Rgba8Unorm:
    return MTLPixelFormatRGBA8Unorm;
  case TextureFormat::Rgba32Float:
    return MTLPixelFormatRGBA32Float;
  }
  return MTLPixelFormatRGBA8Unorm;
}

std::uint32_t grown_capacity(std::uint32_t current, std::uint32_t required) {
  return std::max(required, current ? current * 2 : 4096u);
}

std::uint32_t align_up(std::uint32_t value, std::uint32_t alignment) {
  return (value + alignment - 1) & ~(alignment - 1);
}

id<MTLLibrary> compile_library(id<MTLDevice> device, const ShaderCode &shader) {
  NSString *source = [[NSString alloc] initWithBytes:shader.data
                                              length:shader.size
                                            encoding:NSUTF8StringEncoding];
  NSError *error = nil;
  id<MTLLibrary> library = [device newLibraryWithSource:source
                                                options:nil
                                                  error:&error];
  if (!library)
    SDL_Log("voidui: Metal shader compilation failed: %s",
            error.localizedDescription.UTF8String);
  return library;
}

} // namespace

struct Buffer::Impl {
  id<MTLBuffer> buffers[kFramesInFlight];
  std::uint32_t capacities[kFramesInFlight]{};
  std::uint32_t slot = 0;
};

struct Texture::Impl {
  id<MTLTexture> texture;
};

struct Sampler::Impl {
  id<MTLSamplerState> state;
};

struct Pipeline::Impl {
  id<MTLRenderPipelineState> state;
  std::uint32_t stride = 0;
};

struct Device::Impl {
  struct StagingBuffer {
    id<MTLBuffer> buffer;
    std::uint32_t capacity = 0;
  };

  SDL_Window *window = nullptr;
  SDL_MetalView view = nullptr;
  CAMetalLayer *layer = nil;
  id<MTLDevice> device;
  id<MTLCommandQueue> queue;
  id<CAMetalDrawable> drawable;
  id<MTLCommandBuffer> commands;
  id<MTLBlitCommandEncoder> blit;
  id<MTLRenderCommandEncoder> encoder;
  dispatch_semaphore_t frame_slots[kFramesInFlight];
  std::vector<StagingBuffer> staging[kFramesInFlight];
  std::uint32_t staging_used = 0;
  std::uint32_t frame_index = 0;
  std::uint32_t width = 0;
  std::uint32_t height = 0;
};

Buffer::Buffer(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Buffer::~Buffer() = default;
Texture::Texture(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Texture::~Texture() = default;
Sampler::Sampler(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Sampler::~Sampler() = default;
Pipeline::Pipeline(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}
Pipeline::~Pipeline() = default;
Device::Device(std::unique_ptr<Impl> impl) : impl_(std::move(impl)) {}

std::unique_ptr<Device> Device::create(SDL_Window *window) {
  if (!window)
    return nullptr;

  auto impl = std::make_unique<Impl>();
  impl->window = window;
  impl->device = MTLCreateSystemDefaultDevice();
  if (!impl->device) {
    SDL_Log("voidui: Metal is not available on this system");
    return nullptr;
  }

  impl->view = SDL_Metal_CreateView(window);
  if (!impl->view) {
    SDL_Log("voidui: SDL_Metal_CreateView failed: %s", SDL_GetError());
    return nullptr;
  }

  impl->layer = (__bridge CAMetalLayer *)SDL_Metal_GetLayer(impl->view);
  impl->layer.device = impl->device;
  impl->layer.pixelFormat = MTLPixelFormatBGRA8Unorm;
  impl->layer.framebufferOnly = YES;
  impl->layer.maximumDrawableCount = kFramesInFlight;
  impl->queue = [impl->device newCommandQueue];
  for (dispatch_semaphore_t &slot : impl->frame_slots)
    slot = dispatch_semaphore_create(1);
  if (!impl->queue) {
    SDL_Log("voidui: Metal command-queue creation failed");
    SDL_Metal_DestroyView(impl->view);
    return nullptr;
  }

  SDL_Log("voidui: GPU backend ready, driver Metal");
  return std::unique_ptr<Device>(new Device(std::move(impl)));
}

Device::~Device() {
  if (!impl_)
    return;
  wait_idle();
  if (impl_->view)
    SDL_Metal_DestroyView(impl_->view);
}

void Device::wait_idle() {
  id<MTLCommandBuffer> drain = [impl_->queue commandBuffer];
  [drain commit];
  [drain waitUntilCompleted];
}

std::unique_ptr<Buffer> Device::create_buffer() {
  return std::unique_ptr<Buffer>(new Buffer(std::make_unique<Buffer::Impl>()));
}

std::unique_ptr<Texture> Device::create_texture(std::uint32_t width,
                                                std::uint32_t height,
                                                TextureFormat format) {
  MTLTextureDescriptor *desc = [MTLTextureDescriptor
      texture2DDescriptorWithPixelFormat:texture_format(format)
                                   width:width
                                  height:height
                               mipmapped:NO];
  desc.usage = MTLTextureUsageShaderRead;
  desc.storageMode = MTLStorageModePrivate;

  auto texture = std::make_unique<Texture::Impl>();
  texture->texture = [impl_->device newTextureWithDescriptor:desc];
  if (!texture->texture) {
    SDL_Log("voidui: Metal texture creation failed");
    return nullptr;
  }
  return std::unique_ptr<Texture>(new Texture(std::move(texture)));
}

std::unique_ptr<Sampler> Device::create_sampler(Filter filter) {
  MTLSamplerDescriptor *desc = [[MTLSamplerDescriptor alloc] init];
  const MTLSamplerMinMagFilter native_filter =
      filter == Filter::Nearest ? MTLSamplerMinMagFilterNearest
                                : MTLSamplerMinMagFilterLinear;
  desc.minFilter = native_filter;
  desc.magFilter = native_filter;
  desc.mipFilter = MTLSamplerMipFilterNotMipmapped;
  desc.sAddressMode = MTLSamplerAddressModeClampToEdge;
  desc.tAddressMode = MTLSamplerAddressModeClampToEdge;

  auto sampler = std::make_unique<Sampler::Impl>();
  sampler->state = [impl_->device newSamplerStateWithDescriptor:desc];
  if (!sampler->state)
    return nullptr;
  return std::unique_ptr<Sampler>(new Sampler(std::move(sampler)));
}

std::unique_ptr<Pipeline> Device::create_pipeline(const PipelineDesc &desc) {
  id<MTLLibrary> vertex_library = compile_library(impl_->device, desc.vertex);
  id<MTLLibrary> fragment_library =
      compile_library(impl_->device, desc.fragment);
  if (!vertex_library || !fragment_library)
    return nullptr;

  NSString *vertex_name =
      [NSString stringWithUTF8String:desc.vertex.entrypoint];
  NSString *fragment_name =
      [NSString stringWithUTF8String:desc.fragment.entrypoint];
  id<MTLFunction> vertex = [vertex_library newFunctionWithName:vertex_name];
  id<MTLFunction> fragment =
      [fragment_library newFunctionWithName:fragment_name];
  if (!vertex || !fragment) {
    SDL_Log("voidui: Metal shader entry point was not found");
    return nullptr;
  }

  MTLVertexDescriptor *vertices = [[MTLVertexDescriptor alloc] init];
  for (std::uint32_t i = 0; i < desc.attribute_count; ++i) {
    const VertexAttribute &attribute = desc.attributes[i];
    vertices.attributes[attribute.location].format =
        vertex_format(attribute.format);
    vertices.attributes[attribute.location].offset = attribute.offset;
    vertices.attributes[attribute.location].bufferIndex = 0;
  }
  vertices.layouts[0].stride = desc.stride;
  vertices.layouts[0].stepFunction = MTLVertexStepFunctionPerInstance;
  vertices.layouts[0].stepRate = 1;

  MTLRenderPipelineDescriptor *pipeline_desc =
      [[MTLRenderPipelineDescriptor alloc] init];
  pipeline_desc.vertexFunction = vertex;
  pipeline_desc.fragmentFunction = fragment;
  pipeline_desc.vertexDescriptor = vertices;

  MTLRenderPipelineColorAttachmentDescriptor *color =
      pipeline_desc.colorAttachments[0];
  color.pixelFormat = MTLPixelFormatBGRA8Unorm;
  color.blendingEnabled = YES;
  color.sourceRGBBlendFactor = MTLBlendFactorOne;
  color.destinationRGBBlendFactor = MTLBlendFactorOneMinusSourceAlpha;
  color.rgbBlendOperation = MTLBlendOperationAdd;
  color.sourceAlphaBlendFactor = MTLBlendFactorOne;
  color.destinationAlphaBlendFactor = MTLBlendFactorOneMinusSourceAlpha;
  color.alphaBlendOperation = MTLBlendOperationAdd;

  NSError *error = nil;
  auto pipeline = std::make_unique<Pipeline::Impl>();
  pipeline->state =
      [impl_->device newRenderPipelineStateWithDescriptor:pipeline_desc
                                                    error:&error];
  if (!pipeline->state) {
    SDL_Log("voidui: Metal pipeline creation failed: %s",
            error.localizedDescription.UTF8String);
    return nullptr;
  }
  pipeline->stride = desc.stride;
  return std::unique_ptr<Pipeline>(new Pipeline(std::move(pipeline)));
}

bool Device::begin_frame(std::uint32_t &width, std::uint32_t &height) {
  int pixel_width = 0;
  int pixel_height = 0;
  SDL_GetWindowSizeInPixels(impl_->window, &pixel_width, &pixel_height);
  if (pixel_width <= 0 || pixel_height <= 0)
    return false;

  const std::uint32_t slot = impl_->frame_index % kFramesInFlight;
  dispatch_semaphore_wait(impl_->frame_slots[slot], DISPATCH_TIME_FOREVER);
  impl_->width = static_cast<std::uint32_t>(pixel_width);
  impl_->height = static_cast<std::uint32_t>(pixel_height);
  impl_->layer.drawableSize = CGSizeMake(pixel_width, pixel_height);
  impl_->drawable = [impl_->layer nextDrawable];
  if (!impl_->drawable) {
    dispatch_semaphore_signal(impl_->frame_slots[slot]);
    return false;
  }

  impl_->commands = [impl_->queue commandBuffer];
  dispatch_semaphore_t frame_slot = impl_->frame_slots[slot];
  [impl_->commands addCompletedHandler:^(id<MTLCommandBuffer>) {
    dispatch_semaphore_signal(frame_slot);
  }];
  impl_->staging_used = 0;
  width = impl_->width;
  height = impl_->height;
  return true;
}

bool Device::write_buffer(Buffer &buffer, const void *data,
                          std::uint32_t size) {
  const std::uint32_t slot = impl_->frame_index % kFramesInFlight;
  if (size > buffer.impl_->capacities[slot]) {
    const std::uint32_t capacity =
        grown_capacity(buffer.impl_->capacities[slot], size);
    buffer.impl_->buffers[slot] =
        [impl_->device newBufferWithLength:capacity
                                   options:MTLResourceStorageModeShared];
    if (!buffer.impl_->buffers[slot])
      return false;
    buffer.impl_->capacities[slot] = capacity;
  }
  std::memcpy(buffer.impl_->buffers[slot].contents, data, size);
  buffer.impl_->slot = slot;
  return true;
}

bool Device::upload_texture(Texture &texture, const void *data, std::uint32_t,
                            const TextureUpload *uploads,
                            std::uint32_t upload_count) {
  const std::uint32_t slot = impl_->frame_index % kFramesInFlight;
  std::uint32_t staging_size = 0;
  for (std::uint32_t i = 0; i < upload_count; ++i)
    staging_size = align_up(staging_size, 256) +
                   align_up(uploads[i].row_pitch, 256) * uploads[i].height;

  std::vector<Impl::StagingBuffer> &buffers = impl_->staging[slot];
  if (impl_->staging_used == buffers.size())
    buffers.emplace_back();
  Impl::StagingBuffer &staging = buffers[impl_->staging_used++];
  if (staging_size > staging.capacity) {
    staging.capacity = grown_capacity(staging.capacity, staging_size);
    staging.buffer =
        [impl_->device newBufferWithLength:staging.capacity
                                   options:MTLResourceStorageModeShared];
    if (!staging.buffer)
      return false;
  }

  const auto *bytes = static_cast<const std::uint8_t *>(data);
  auto *mapped = static_cast<std::uint8_t *>(staging.buffer.contents);
  if (!impl_->blit)
    impl_->blit = [impl_->commands blitCommandEncoder];

  std::uint32_t offset = 0;
  for (std::uint32_t i = 0; i < upload_count; ++i) {
    const TextureUpload &upload = uploads[i];
    offset = align_up(offset, 256);
    const std::uint32_t row_pitch = align_up(upload.row_pitch, 256);
    for (std::uint32_t row = 0; row < upload.height; ++row) {
      std::memcpy(mapped + offset + row * row_pitch,
                  bytes + upload.offset + row * upload.row_pitch,
                  upload.row_pitch);
    }
    [impl_->blit copyFromBuffer:staging.buffer
                   sourceOffset:offset
              sourceBytesPerRow:row_pitch
            sourceBytesPerImage:row_pitch * upload.height
                     sourceSize:MTLSizeMake(upload.width, upload.height, 1)
                      toTexture:texture.impl_->texture
               destinationSlice:0
               destinationLevel:0
              destinationOrigin:MTLOriginMake(upload.x, upload.y, 0)];
    offset += row_pitch * upload.height;
  }
  return true;
}

void Device::begin_render(float red, float green, float blue, float alpha) {
  if (impl_->blit) {
    [impl_->blit endEncoding];
    impl_->blit = nil;
  }
  MTLRenderPassDescriptor *pass =
      [MTLRenderPassDescriptor renderPassDescriptor];
  pass.colorAttachments[0].texture = impl_->drawable.texture;
  pass.colorAttachments[0].loadAction = MTLLoadActionClear;
  pass.colorAttachments[0].storeAction = MTLStoreActionStore;
  pass.colorAttachments[0].clearColor =
      MTLClearColorMake(red, green, blue, alpha);
  impl_->encoder = [impl_->commands renderCommandEncoderWithDescriptor:pass];
  [impl_->encoder setViewport:MTLViewport{0.0, 0.0, double(impl_->width),
                                          double(impl_->height), 0.0, 1.0}];
}

void Device::bind_pipeline(Pipeline &pipeline) {
  [impl_->encoder setRenderPipelineState:pipeline.impl_->state];
}

void Device::bind_vertex_buffer(Buffer &buffer) {
  [impl_->encoder setVertexBuffer:buffer.impl_->buffers[buffer.impl_->slot]
                           offset:0
                          atIndex:0];
}

bool Device::bind_texture(Texture &texture, Sampler &sampler) {
  [impl_->encoder setFragmentTexture:texture.impl_->texture atIndex:0];
  [impl_->encoder setFragmentSamplerState:sampler.impl_->state atIndex:0];
  return true;
}

void Device::set_uniforms(const void *data, std::uint32_t size) {
  [impl_->encoder setVertexBytes:data length:size atIndex:1];
  [impl_->encoder setFragmentBytes:data length:size atIndex:1];
}

void Device::set_scissor(std::uint32_t x, std::uint32_t y, std::uint32_t width,
                         std::uint32_t height) {
  [impl_->encoder setScissorRect:MTLScissorRect{x, y, width, height}];
}

void Device::draw(std::uint32_t vertex_count, std::uint32_t instance_count,
                  std::uint32_t first_vertex, std::uint32_t first_instance) {
  [impl_->encoder drawPrimitives:MTLPrimitiveTypeTriangleStrip
                     vertexStart:first_vertex
                     vertexCount:vertex_count
                   instanceCount:instance_count
                    baseInstance:first_instance];
}

void Device::end_render() {
  [impl_->encoder endEncoding];
  impl_->encoder = nil;
}

void Device::end_frame() {
  [impl_->commands presentDrawable:impl_->drawable];
  [impl_->commands commit];
  impl_->commands = nil;
  impl_->drawable = nil;
  ++impl_->frame_index;
}

const char *Device::driver() const { return "Metal"; }

} // namespace voidui::rhi
