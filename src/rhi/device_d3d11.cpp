#include "rhi/device.h"

#include <SDL3/SDL_log.h>
#include <SDL3/SDL_properties.h>
#include <SDL3/SDL_video.h>

#define NOMINMAX
#include <d3d11.h>
#include <dxgi1_2.h>
#include <windows.h>
#include <wrl/client.h>

#include <algorithm>
#include <cstring>
#include <utility>
#include <vector>

namespace voidui::rhi {

using Microsoft::WRL::ComPtr;

namespace {

constexpr std::uint32_t kUniformCapacity = 256;

DXGI_FORMAT vertex_format(VertexFormat format) {
  switch (format) {
  case VertexFormat::Float:
    return DXGI_FORMAT_R32_FLOAT;
  case VertexFormat::Float2:
    return DXGI_FORMAT_R32G32_FLOAT;
  case VertexFormat::Float4:
    return DXGI_FORMAT_R32G32B32A32_FLOAT;
  case VertexFormat::UByte4Norm:
    return DXGI_FORMAT_R8G8B8A8_UNORM;
  case VertexFormat::UInt:
    return DXGI_FORMAT_R32_UINT;
  }
  return DXGI_FORMAT_UNKNOWN;
}

DXGI_FORMAT texture_format(TextureFormat format) {
  switch (format) {
  case TextureFormat::R8Unorm:
    return DXGI_FORMAT_R8_UNORM;
  case TextureFormat::Rgba8Unorm:
    return DXGI_FORMAT_R8G8B8A8_UNORM;
  case TextureFormat::Rgba32Float:
    return DXGI_FORMAT_R32G32B32A32_FLOAT;
  }
  return DXGI_FORMAT_R8G8B8A8_UNORM;
}

std::uint32_t grown_capacity(std::uint32_t current, std::uint32_t required) {
  return std::max(required, current ? current * 2 : 4096u);
}

} // namespace

struct Buffer::Impl {
  ComPtr<ID3D11Buffer> buffer;
  std::uint32_t capacity = 0;
};

struct Texture::Impl {
  ComPtr<ID3D11Texture2D> texture;
  ComPtr<ID3D11ShaderResourceView> view;
};

struct Sampler::Impl {
  ComPtr<ID3D11SamplerState> state;
};

struct Pipeline::Impl {
  ComPtr<ID3D11VertexShader> vertex;
  ComPtr<ID3D11PixelShader> fragment;
  ComPtr<ID3D11InputLayout> input_layout;
  std::uint32_t stride = 0;
};

struct Device::Impl {
  SDL_Window *window = nullptr;
  HWND hwnd = nullptr;
  ComPtr<ID3D11Device> device;
  ComPtr<ID3D11DeviceContext> context;
  ComPtr<IDXGISwapChain1> swapchain;
  ComPtr<ID3D11RenderTargetView> render_target;
  ComPtr<ID3D11Buffer> uniform_buffer;
  ComPtr<ID3D11RasterizerState> rasterizer;
  ComPtr<ID3D11BlendState> blend;
  std::uint32_t width = 0;
  std::uint32_t height = 0;
  std::uint32_t vertex_stride = 0;

  bool resize(std::uint32_t new_width, std::uint32_t new_height) {
    if (render_target && width == new_width && height == new_height)
      return true;

    context->OMSetRenderTargets(0, nullptr, nullptr);
    render_target.Reset();

    if (width != 0) {
      const HRESULT result = swapchain->ResizeBuffers(0, new_width, new_height,
                                                      DXGI_FORMAT_UNKNOWN, 0);
      if (FAILED(result)) {
        SDL_Log("voidui: IDXGISwapChain::ResizeBuffers failed (0x%08lx)",
                result);
        return false;
      }
    }

    ComPtr<ID3D11Texture2D> back_buffer;
    HRESULT result = swapchain->GetBuffer(0, IID_PPV_ARGS(&back_buffer));
    if (SUCCEEDED(result))
      result = device->CreateRenderTargetView(back_buffer.Get(), nullptr,
                                              &render_target);
    if (FAILED(result)) {
      SDL_Log("voidui: D3D11 back-buffer view creation failed (0x%08lx)",
              result);
      return false;
    }

    width = new_width;
    height = new_height;
    return true;
  }
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
  impl->hwnd = static_cast<HWND>(
      SDL_GetPointerProperty(SDL_GetWindowProperties(window),
                             SDL_PROP_WINDOW_WIN32_HWND_POINTER, nullptr));
  if (!impl->hwnd) {
    SDL_Log("voidui: SDL did not expose a Win32 window handle");
    return nullptr;
  }

  D3D_FEATURE_LEVEL level;
  const D3D_FEATURE_LEVEL requested[] = {D3D_FEATURE_LEVEL_11_0};
  HRESULT result = D3D11CreateDevice(nullptr, D3D_DRIVER_TYPE_HARDWARE, nullptr,
                                     D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                                     requested, 1, D3D11_SDK_VERSION,
                                     &impl->device, &level, &impl->context);
  if (FAILED(result)) {
    SDL_Log("voidui: D3D11CreateDevice failed (0x%08lx)", result);
    return nullptr;
  }

  ComPtr<IDXGIDevice> dxgi_device;
  ComPtr<IDXGIAdapter> adapter;
  ComPtr<IDXGIFactory2> factory;
  result = impl->device.As(&dxgi_device);
  if (SUCCEEDED(result))
    result = dxgi_device->GetAdapter(&adapter);
  if (SUCCEEDED(result))
    result = adapter->GetParent(IID_PPV_ARGS(&factory));
  if (FAILED(result)) {
    SDL_Log("voidui: DXGI factory discovery failed (0x%08lx)", result);
    return nullptr;
  }

  ComPtr<IDXGIDevice1> dxgi_device1;
  if (SUCCEEDED(dxgi_device.As(&dxgi_device1)))
    dxgi_device1->SetMaximumFrameLatency(2);

  RECT client{};
  GetClientRect(impl->hwnd, &client);
  const std::uint32_t width = std::max<LONG>(client.right - client.left, 1);
  const std::uint32_t height = std::max<LONG>(client.bottom - client.top, 1);

  DXGI_SWAP_CHAIN_DESC1 swapchain_desc{};
  swapchain_desc.Width = width;
  swapchain_desc.Height = height;
  swapchain_desc.Format = DXGI_FORMAT_B8G8R8A8_UNORM;
  swapchain_desc.SampleDesc.Count = 1;
  swapchain_desc.BufferUsage = DXGI_USAGE_RENDER_TARGET_OUTPUT;
  swapchain_desc.BufferCount = 2;
  swapchain_desc.Scaling = DXGI_SCALING_NONE;
  swapchain_desc.SwapEffect = DXGI_SWAP_EFFECT_FLIP_DISCARD;
  swapchain_desc.AlphaMode = DXGI_ALPHA_MODE_IGNORE;

  result = factory->CreateSwapChainForHwnd(impl->device.Get(), impl->hwnd,
                                           &swapchain_desc, nullptr, nullptr,
                                           &impl->swapchain);
  if (FAILED(result)) {
    SDL_Log("voidui: CreateSwapChainForHwnd failed (0x%08lx)", result);
    return nullptr;
  }
  factory->MakeWindowAssociation(impl->hwnd, DXGI_MWA_NO_ALT_ENTER);

  D3D11_BUFFER_DESC uniform_desc{};
  uniform_desc.ByteWidth = kUniformCapacity;
  uniform_desc.Usage = D3D11_USAGE_DYNAMIC;
  uniform_desc.BindFlags = D3D11_BIND_CONSTANT_BUFFER;
  uniform_desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;
  result =
      impl->device->CreateBuffer(&uniform_desc, nullptr, &impl->uniform_buffer);
  if (FAILED(result)) {
    SDL_Log("voidui: D3D11 uniform-buffer creation failed (0x%08lx)", result);
    return nullptr;
  }

  D3D11_RASTERIZER_DESC raster_desc{};
  raster_desc.FillMode = D3D11_FILL_SOLID;
  raster_desc.CullMode = D3D11_CULL_NONE;
  raster_desc.DepthClipEnable = TRUE;
  raster_desc.ScissorEnable = TRUE;
  result = impl->device->CreateRasterizerState(&raster_desc, &impl->rasterizer);
  if (FAILED(result)) {
    SDL_Log("voidui: D3D11 rasterizer-state creation failed (0x%08lx)", result);
    return nullptr;
  }

  D3D11_BLEND_DESC blend_desc{};
  blend_desc.RenderTarget[0].BlendEnable = TRUE;
  blend_desc.RenderTarget[0].SrcBlend = D3D11_BLEND_ONE;
  blend_desc.RenderTarget[0].DestBlend = D3D11_BLEND_INV_SRC_ALPHA;
  blend_desc.RenderTarget[0].BlendOp = D3D11_BLEND_OP_ADD;
  blend_desc.RenderTarget[0].SrcBlendAlpha = D3D11_BLEND_ONE;
  blend_desc.RenderTarget[0].DestBlendAlpha = D3D11_BLEND_INV_SRC_ALPHA;
  blend_desc.RenderTarget[0].BlendOpAlpha = D3D11_BLEND_OP_ADD;
  blend_desc.RenderTarget[0].RenderTargetWriteMask =
      D3D11_COLOR_WRITE_ENABLE_ALL;
  result = impl->device->CreateBlendState(&blend_desc, &impl->blend);
  if (FAILED(result)) {
    SDL_Log("voidui: D3D11 blend-state creation failed (0x%08lx)", result);
    return nullptr;
  }

  if (!impl->resize(width, height))
    return nullptr;

  SDL_Log("voidui: GPU backend ready, driver D3D11");
  return std::unique_ptr<Device>(new Device(std::move(impl)));
}

Device::~Device() {
  if (impl_ && impl_->context) {
    wait_idle();
    impl_->context->ClearState();
  }
}

std::unique_ptr<Buffer> Device::create_buffer() {
  return std::unique_ptr<Buffer>(new Buffer(std::make_unique<Buffer::Impl>()));
}

std::unique_ptr<Texture> Device::create_texture(std::uint32_t width,
                                                std::uint32_t height,
                                                TextureFormat format) {
  auto texture = std::make_unique<Texture::Impl>();
  D3D11_TEXTURE2D_DESC desc{};
  desc.Width = width;
  desc.Height = height;
  desc.MipLevels = 1;
  desc.ArraySize = 1;
  desc.Format = texture_format(format);
  desc.SampleDesc.Count = 1;
  desc.Usage = D3D11_USAGE_DEFAULT;
  desc.BindFlags = D3D11_BIND_SHADER_RESOURCE;

  HRESULT result =
      impl_->device->CreateTexture2D(&desc, nullptr, &texture->texture);
  if (SUCCEEDED(result))
    result = impl_->device->CreateShaderResourceView(texture->texture.Get(),
                                                     nullptr, &texture->view);
  if (FAILED(result)) {
    SDL_Log("voidui: D3D11 texture creation failed (0x%08lx)", result);
    return nullptr;
  }
  return std::unique_ptr<Texture>(new Texture(std::move(texture)));
}

std::unique_ptr<Sampler> Device::create_sampler(Filter filter) {
  auto sampler = std::make_unique<Sampler::Impl>();
  D3D11_SAMPLER_DESC desc{};
  desc.Filter = filter == Filter::Nearest
                    ? D3D11_FILTER_MIN_MAG_MIP_POINT
                    : D3D11_FILTER_MIN_MAG_LINEAR_MIP_POINT;
  desc.AddressU = D3D11_TEXTURE_ADDRESS_CLAMP;
  desc.AddressV = D3D11_TEXTURE_ADDRESS_CLAMP;
  desc.AddressW = D3D11_TEXTURE_ADDRESS_CLAMP;
  desc.MaxLOD = D3D11_FLOAT32_MAX;

  const HRESULT result =
      impl_->device->CreateSamplerState(&desc, &sampler->state);
  if (FAILED(result)) {
    SDL_Log("voidui: D3D11 sampler creation failed (0x%08lx)", result);
    return nullptr;
  }
  return std::unique_ptr<Sampler>(new Sampler(std::move(sampler)));
}

std::unique_ptr<Pipeline> Device::create_pipeline(const PipelineDesc &desc) {
  auto pipeline = std::make_unique<Pipeline::Impl>();
  HRESULT result = impl_->device->CreateVertexShader(
      desc.vertex.data, desc.vertex.size, nullptr, &pipeline->vertex);
  if (SUCCEEDED(result)) {
    result = impl_->device->CreatePixelShader(
        desc.fragment.data, desc.fragment.size, nullptr, &pipeline->fragment);
  }

  std::vector<D3D11_INPUT_ELEMENT_DESC> elements(desc.attribute_count);
  for (std::uint32_t i = 0; i < desc.attribute_count; ++i) {
    const VertexAttribute &source = desc.attributes[i];
    elements[i] = {"TEXCOORD", source.location, vertex_format(source.format),
                   0,          source.offset,   D3D11_INPUT_PER_INSTANCE_DATA,
                   1};
  }

  if (SUCCEEDED(result)) {
    result = impl_->device->CreateInputLayout(
        elements.data(), static_cast<UINT>(elements.size()), desc.vertex.data,
        desc.vertex.size, &pipeline->input_layout);
  }
  if (FAILED(result)) {
    SDL_Log("voidui: D3D11 pipeline creation failed (0x%08lx)", result);
    return nullptr;
  }

  pipeline->stride = desc.stride;
  return std::unique_ptr<Pipeline>(new Pipeline(std::move(pipeline)));
}

bool Device::begin_frame(std::uint32_t &width, std::uint32_t &height) {
  RECT client{};
  GetClientRect(impl_->hwnd, &client);
  const LONG client_width = client.right - client.left;
  const LONG client_height = client.bottom - client.top;
  if (client_width <= 0 || client_height <= 0)
    return false;

  width = static_cast<std::uint32_t>(client_width);
  height = static_cast<std::uint32_t>(client_height);
  if (!impl_->resize(width, height))
    return false;

  return true;
}

bool Device::write_buffer(Buffer &buffer, const void *data,
                          std::uint32_t size) {
  if (size > buffer.impl_->capacity) {
    D3D11_BUFFER_DESC desc{};
    desc.ByteWidth = grown_capacity(buffer.impl_->capacity, size);
    desc.Usage = D3D11_USAGE_DYNAMIC;
    desc.BindFlags = D3D11_BIND_VERTEX_BUFFER;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_WRITE;

    ComPtr<ID3D11Buffer> replacement;
    const HRESULT result =
        impl_->device->CreateBuffer(&desc, nullptr, &replacement);
    if (FAILED(result)) {
      SDL_Log("voidui: D3D11 vertex-buffer allocation failed (0x%08lx)",
              result);
      return false;
    }
    buffer.impl_->buffer = std::move(replacement);
    buffer.impl_->capacity = desc.ByteWidth;
  }

  D3D11_MAPPED_SUBRESOURCE mapped{};
  const HRESULT result = impl_->context->Map(
      buffer.impl_->buffer.Get(), 0, D3D11_MAP_WRITE_DISCARD, 0, &mapped);
  if (FAILED(result))
    return false;
  std::memcpy(mapped.pData, data, size);
  impl_->context->Unmap(buffer.impl_->buffer.Get(), 0);
  return true;
}

bool Device::upload_texture(Texture &texture, const void *data, std::uint32_t,
                            const TextureUpload *uploads,
                            std::uint32_t upload_count) {
  const auto *bytes = static_cast<const std::uint8_t *>(data);
  for (std::uint32_t i = 0; i < upload_count; ++i) {
    const TextureUpload &upload = uploads[i];
    D3D11_BOX box{upload.x,
                  upload.y,
                  0,
                  upload.x + upload.width,
                  upload.y + upload.height,
                  1};
    impl_->context->UpdateSubresource(texture.impl_->texture.Get(), 0, &box,
                                      bytes + upload.offset, upload.row_pitch,
                                      0);
  }
  return true;
}

void Device::begin_render(float red, float green, float blue, float alpha) {
  ID3D11RenderTargetView *target = impl_->render_target.Get();
  impl_->context->OMSetRenderTargets(1, &target, nullptr);
  impl_->context->RSSetState(impl_->rasterizer.Get());

  const D3D11_VIEWPORT viewport{0.0f,
                                0.0f,
                                static_cast<float>(impl_->width),
                                static_cast<float>(impl_->height),
                                0.0f,
                                1.0f};
  impl_->context->RSSetViewports(1, &viewport);
  const float clear[4]{red, green, blue, alpha};
  impl_->context->ClearRenderTargetView(target, clear);
}

void Device::bind_pipeline(Pipeline &pipeline) {
  impl_->context->IASetInputLayout(pipeline.impl_->input_layout.Get());
  impl_->context->IASetPrimitiveTopology(
      D3D11_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
  impl_->context->VSSetShader(pipeline.impl_->vertex.Get(), nullptr, 0);
  impl_->context->PSSetShader(pipeline.impl_->fragment.Get(), nullptr, 0);
  impl_->context->OMSetBlendState(impl_->blend.Get(), nullptr, 0xffffffffu);
  impl_->vertex_stride = pipeline.impl_->stride;
}

void Device::bind_vertex_buffer(Buffer &buffer) {
  ID3D11Buffer *native = buffer.impl_->buffer.Get();
  const UINT stride = impl_->vertex_stride;
  const UINT offset = 0;
  impl_->context->IASetVertexBuffers(0, 1, &native, &stride, &offset);
}

bool Device::bind_texture(Texture &texture, Sampler &sampler) {
  ID3D11ShaderResourceView *view = texture.impl_->view.Get();
  ID3D11SamplerState *state = sampler.impl_->state.Get();
  impl_->context->PSSetShaderResources(0, 1, &view);
  impl_->context->PSSetSamplers(0, 1, &state);
  return true;
}

void Device::set_uniforms(const void *data, std::uint32_t size) {
  D3D11_MAPPED_SUBRESOURCE mapped{};
  if (FAILED(impl_->context->Map(impl_->uniform_buffer.Get(), 0,
                                 D3D11_MAP_WRITE_DISCARD, 0, &mapped)))
    return;
  std::memcpy(mapped.pData, data, size);
  impl_->context->Unmap(impl_->uniform_buffer.Get(), 0);

  ID3D11Buffer *buffer = impl_->uniform_buffer.Get();
  impl_->context->VSSetConstantBuffers(1, 1, &buffer);
  impl_->context->PSSetConstantBuffers(1, 1, &buffer);
}

void Device::set_scissor(std::uint32_t x, std::uint32_t y, std::uint32_t width,
                         std::uint32_t height) {
  const D3D11_RECT rect{static_cast<LONG>(x), static_cast<LONG>(y),
                        static_cast<LONG>(x + width),
                        static_cast<LONG>(y + height)};
  impl_->context->RSSetScissorRects(1, &rect);
}

void Device::draw(std::uint32_t vertex_count, std::uint32_t instance_count,
                  std::uint32_t first_vertex, std::uint32_t first_instance) {
  impl_->context->DrawInstanced(vertex_count, instance_count, first_vertex,
                                first_instance);
}

void Device::end_render() {
  ID3D11RenderTargetView *none = nullptr;
  impl_->context->OMSetRenderTargets(1, &none, nullptr);
}

void Device::end_frame() { impl_->swapchain->Present(1, 0); }

void Device::wait_idle() {
  D3D11_QUERY_DESC desc{D3D11_QUERY_EVENT, 0};
  ComPtr<ID3D11Query> event;
  if (FAILED(impl_->device->CreateQuery(&desc, &event)))
    return;

  impl_->context->End(event.Get());
  impl_->context->Flush();
  while (impl_->context->GetData(event.Get(), nullptr, 0, 0) == S_FALSE)
    SwitchToThread();
}

const char *Device::driver() const { return "D3D11"; }

} // namespace voidui::rhi
