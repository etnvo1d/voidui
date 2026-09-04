// WIC-backed image decoder.
//
// WIC is the same machinery Explorer's thumbnails and every Windows imaging
// application go through, so an application built on it decodes exactly the
// formats the user's system already opens -- including whatever codecs they
// have installed -- without linking a single third-party library.
//
// The reason to prefer it over a bundled decoder is not convenience, though.
// It is that WIC scales while it decodes: asked for a small result from a large
// JPEG, the codec drops whole DCT ratios instead of producing every pixel and
// throwing most of them away. That is the difference between 48 MB and 120 KB
// for a photograph shown as a thumbnail, and it is the largest single saving
// available anywhere in the image path.

#define WIN32_LEAN_AND_MEAN
// Or `std::min` becomes `std::((a) < (b) ? ...)`: windows.h defines min and max
// as macros, and this file does arithmetic on sizes.
#define NOMINMAX
#include <windows.h>

#include <wincodec.h>
#include <wrl/client.h>

#include <SDL3/SDL_stdinc.h>

#include "voidui/paint/image_codec.h"

#include <algorithm>
#include <cstring>
#include <cstdio>
#include <new>
#include <vector>

namespace voidui {

namespace {

using Microsoft::WRL::ComPtr;

/// COM, per thread, for as long as the thread lives.
///
/// Decoding happens on the pool in `voidui::async`, on whichever worker picks
/// the job up, so every one of them needs an apartment. A thread that already
/// has one -- the UI thread, which SDL initialises -- reports RPC_E_CHANGED_MODE
/// and keeps the apartment it has; uninitialising that on the way out would
/// close an apartment this file did not open.
class Apartment {
public:
  Apartment() {
    const HRESULT hr = CoInitializeEx(nullptr, COINIT_MULTITHREADED);
    owned_ = SUCCEEDED(hr);
  }

  ~Apartment() {
    if (owned_)
      CoUninitialize();
  }

  Apartment(const Apartment &) = delete;
  Apartment &operator=(const Apartment &) = delete;

private:
  bool owned_ = false;
};

/// The factory is cheap to keep and not cheap to create, and WIC objects are
/// free-threaded, so one per thread is the right granularity: no sharing to
/// synchronise, no CoCreateInstance on the hot path.
///
/// Declared in this order deliberately. Members are destroyed in reverse, so
/// the factory is released before the apartment it was created in closes.
struct ThreadState {
  Apartment apartment;
  ComPtr<IWICImagingFactory> factory;
};

IWICImagingFactory *factory() {
  static thread_local ThreadState state;
  if (!state.factory) {
    if (FAILED(CoCreateInstance(CLSID_WICImagingFactory, nullptr,
                               CLSCTX_INPROC_SERVER,
                               IID_PPV_ARGS(&state.factory))))
      return nullptr;
  }
  return state.factory.Get();
}

bool starts_with(std::span<const std::byte> data, const unsigned char *magic,
                 std::size_t length) {
  if (data.size() < length)
    return false;
  return std::memcmp(data.data(), magic, length) == 0;
}

/// The formats WIC ships with, by magic number.
///
/// Sniffing an explicit list rather than handing WIC every byte stream and
/// asking whether it copes: this decoder shares a table with whatever else the
/// application registers, and one that claimed everything would sit in front of
/// a specialised decoder added at the same priority and never let it run.
bool sniff_container(std::span<const std::byte> data) {
  static const unsigned char png[] = {0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A};
  static const unsigned char jpeg[] = {0xFF, 0xD8, 0xFF};
  static const unsigned char gif87[] = {'G', 'I', 'F', '8', '7', 'a'};
  static const unsigned char gif89[] = {'G', 'I', 'F', '8', '9', 'a'};
  static const unsigned char bmp[] = {'B', 'M'};
  static const unsigned char tiff_le[] = {'I', 'I', 0x2A, 0x00};
  static const unsigned char tiff_be[] = {'M', 'M', 0x00, 0x2A};
  static const unsigned char ico[] = {0x00, 0x00, 0x01, 0x00};
  static const unsigned char dds[] = {'D', 'D', 'S', ' '};

  if (starts_with(data, png, sizeof(png)) ||
      starts_with(data, jpeg, sizeof(jpeg)) ||
      starts_with(data, gif87, sizeof(gif87)) ||
      starts_with(data, gif89, sizeof(gif89)) ||
      starts_with(data, bmp, sizeof(bmp)) ||
      starts_with(data, tiff_le, sizeof(tiff_le)) ||
      starts_with(data, tiff_be, sizeof(tiff_be)) ||
      starts_with(data, ico, sizeof(ico)) || starts_with(data, dds, sizeof(dds)))
    return true;

  // RIFF containers: only the WEBP form is an image.
  if (data.size() >= 12 && std::memcmp(data.data(), "RIFF", 4) == 0 &&
      std::memcmp(data.data() + 8, "WEBP", 4) == 0)
    return true;

  // ISO base media: HEIC, HEIF and AVIF all announce themselves in the brand of
  // an `ftyp` box, which sits at offset 4.
  if (data.size() >= 12 && std::memcmp(data.data() + 4, "ftyp", 4) == 0) {
    const char *brand = reinterpret_cast<const char *>(data.data()) + 8;
    if (std::memcmp(brand, "heic", 4) == 0 || std::memcmp(brand, "heix", 4) == 0 ||
        std::memcmp(brand, "heim", 4) == 0 || std::memcmp(brand, "heis", 4) == 0 ||
        std::memcmp(brand, "hevc", 4) == 0 || std::memcmp(brand, "mif1", 4) == 0 ||
        std::memcmp(brand, "msf1", 4) == 0 || std::memcmp(brand, "avif", 4) == 0 ||
        std::memcmp(brand, "avis", 4) == 0)
      return true;
  }

  return false;
}

/// Media Foundation's "no transform can decode this stream". Not in any WIC
/// header, so it is spelt out.
///
/// This is what a missing codec looks like from here, and it arrives late. WIC
/// decodes lazily: the container is parsed when the frame is opened and the
/// payload only when its pixels are asked for, so a format whose container
/// Windows understands and whose payload it cannot decode reports a size, an
/// alpha channel and a pixel format quite happily, then fails at the copy.
///
/// AVIF is the case that matters. The ISO base media parser ships with the OS;
/// the AV1 decoder is a separate Store package, and without it every `.avif`
/// gets exactly this far and no further.
constexpr HRESULT kMfCodecNotFound = static_cast<HRESULT>(0xC00D5212);

/// What a WIC failure actually means.
///
/// Worth distinguishing, because "Windows has no codec for this" and "these
/// bytes are broken" send a reader somewhere completely different -- one is
/// answered by installing something, the other by looking at the file.
ImageError to_image_error(HRESULT hr) {
  // The raw code, behind the same switch the rest of the image path logs
  // under. Windows has a great many ways to say no, and this is the only way
  // to find out which one it meant.
  static const bool log = [] {
    const char *env = SDL_getenv("VOIDUI_LOG_IMAGE");
    return env && SDL_strcmp(env, "0") != 0;
  }();
  if (log)
    std::fprintf(stderr, "voidui: wic hresult 0x%08lX\n",
                 static_cast<unsigned long>(hr));

  switch (hr) {
  case WINCODEC_ERR_COMPONENTNOTFOUND:
  case WINCODEC_ERR_UNKNOWNIMAGEFORMAT:
  case WINCODEC_ERR_UNSUPPORTEDPIXELFORMAT:
  case WINCODEC_ERR_UNSUPPORTEDOPERATION:
  case kMfCodecNotFound:
    return ImageError::Unsupported;
  case E_OUTOFMEMORY:
    return ImageError::OutOfMemory;
  default:
    return ImageError::Malformed;
  }
}

/// Opens the first frame. Every path here needs exactly this much.
HRESULT open_frame(IWICImagingFactory *wic, std::span<const std::byte> data,
                   ComPtr<IWICBitmapFrameDecode> &frame) {
  ComPtr<IWICStream> stream;
  HRESULT hr = wic->CreateStream(&stream);
  if (FAILED(hr))
    return hr;

  // InitializeFromMemory takes a non-const pointer and does not write through
  // it; the buffer belongs to the Blob the caller is holding open across this
  // whole call.
  hr = stream->InitializeFromMemory(
      const_cast<BYTE *>(reinterpret_cast<const BYTE *>(data.data())),
      static_cast<DWORD>(data.size()));
  if (FAILED(hr))
    return hr;

  ComPtr<IWICBitmapDecoder> decoder;
  hr = wic->CreateDecoderFromStream(stream.Get(), nullptr,
                                    WICDecodeMetadataCacheOnDemand, &decoder);
  if (FAILED(hr))
    return hr;

  return decoder->GetFrame(0, &frame);
}

/// The size to decode to: `source` shrunk to fit the caller's box, aspect
/// preserved, never enlarged.
void fit(UINT source_width, UINT source_height, int max_width, int max_height,
         UINT &out_width, UINT &out_height) {
  out_width = source_width;
  out_height = source_height;

  double scale = 1.0;
  if (max_width > 0 && source_width > static_cast<UINT>(max_width))
    scale = std::min(scale, static_cast<double>(max_width) / source_width);
  if (max_height > 0 && source_height > static_cast<UINT>(max_height))
    scale = std::min(scale, static_cast<double>(max_height) / source_height);

  if (scale >= 1.0)
    return;

  // Rounded up, then clamped to the source: a box one pixel narrower than the
  // image must not round its way back to the full width, and a very wide, very
  // short image must not round its height to zero.
  out_width = std::clamp<UINT>(
      static_cast<UINT>(source_width * scale + 0.5), 1u, source_width);
  out_height = std::clamp<UINT>(
      static_cast<UINT>(source_height * scale + 0.5), 1u, source_height);
}

class WicDecoder final : public ImageDecoder {
public:
  std::string_view name() const override { return "wic"; }

  bool sniff(std::span<const std::byte> prefix) const override {
    return sniff_container(prefix);
  }

  ImageResult<ImageInfo> probe(std::span<const std::byte> data) const override {
    IWICImagingFactory *wic = factory();
    if (!wic)
      return std::unexpected(ImageError::Unsupported);

    ComPtr<IWICBitmapFrameDecode> frame;
    if (FAILED(open_frame(wic, data, frame)))
      return std::unexpected(ImageError::Malformed);

    UINT width = 0;
    UINT height = 0;
    if (FAILED(frame->GetSize(&width, &height)) || width == 0 || height == 0)
      return std::unexpected(ImageError::Malformed);

    ImageInfo info;
    info.width = static_cast<int>(width);
    info.height = static_cast<int>(height);

    // Asking the format itself rather than matching GUIDs: WIC already knows
    // which of its several dozen pixel formats carry alpha, and an installed
    // codec can introduce one this file has never heard of.
    WICPixelFormatGUID format{};
    if (SUCCEEDED(frame->GetPixelFormat(&format))) {
      ComPtr<IWICComponentInfo> component;
      ComPtr<IWICPixelFormatInfo2> pixel_format;
      if (SUCCEEDED(wic->CreateComponentInfo(format, &component)) &&
          SUCCEEDED(component.As(&pixel_format))) {
        BOOL transparency = FALSE;
        if (SUCCEEDED(pixel_format->SupportsTransparency(&transparency)))
          info.has_alpha = transparency != FALSE;
      }
    }

    return info;
  }

  ImageResult<std::shared_ptr<Image>>
  decode(std::span<const std::byte> data,
         const DecodeOptions &options) const override {
    IWICImagingFactory *wic = factory();
    if (!wic)
      return std::unexpected(ImageError::Unsupported);

    ComPtr<IWICBitmapFrameDecode> frame;
    if (FAILED(open_frame(wic, data, frame)))
      return std::unexpected(ImageError::Malformed);

    UINT source_width = 0;
    UINT source_height = 0;
    if (FAILED(frame->GetSize(&source_width, &source_height)) ||
        source_width == 0 || source_height == 0)
      return std::unexpected(ImageError::Malformed);

    // Checked against the header, before anything is allocated for it. A
    // hundred bytes of untrusted data can declare 64000x64000, and the only
    // safe answer is to refuse rather than to try.
    if (options.size_limit > 0 &&
        (source_width > static_cast<UINT>(options.size_limit) ||
         source_height > static_cast<UINT>(options.size_limit)))
      return std::unexpected(ImageError::TooLarge);

    UINT width = 0;
    UINT height = 0;
    fit(source_width, source_height, options.max_width, options.max_height, width,
        height);

    ComPtr<IWICBitmapSource> source;
    if (width != source_width || height != source_height) {
      ComPtr<IWICBitmapScaler> scaler;
      HRESULT hr = wic->CreateBitmapScaler(&scaler);
      if (FAILED(hr))
        return std::unexpected(to_image_error(hr));

      // This is where the saving actually happens, and it happens inside WIC:
      // the scaler asks its source for IWICBitmapSourceTransform, and a JPEG
      // frame answers by decoding at the nearest whole DCT ratio -- an eighth,
      // a quarter, a half -- before any resampling is done at all. Asking for
      // the final size directly is what lets it choose.
      hr = scaler->Initialize(frame.Get(), width, height,
                              WICBitmapInterpolationModeFant);
      if (FAILED(hr))
        return std::unexpected(to_image_error(hr));

      source = scaler;
    } else {
      source = frame;
    }

    // Premultiplying here means premultiplying on this worker thread, over
    // pixels that are already in cache from the conversion. The alternative is
    // a full extra pass and a full extra buffer on the thread that uploads.
    ComPtr<IWICFormatConverter> converter;
    HRESULT hr = wic->CreateFormatConverter(&converter);
    if (FAILED(hr))
      return std::unexpected(to_image_error(hr));

    const WICPixelFormatGUID target = options.premultiply
                                          ? GUID_WICPixelFormat32bppPRGBA
                                          : GUID_WICPixelFormat32bppRGBA;
    hr = converter->Initialize(source.Get(), target, WICBitmapDitherTypeNone,
                               nullptr, 0.0, WICBitmapPaletteTypeMedianCut);
    if (FAILED(hr))
      return std::unexpected(to_image_error(hr));

    const std::size_t stride = static_cast<std::size_t>(width) * 4;
    const std::size_t bytes = stride * height;

    std::vector<std::uint8_t> pixels;
    try {
      pixels.resize(bytes);
    } catch (const std::bad_alloc &) {
      return std::unexpected(ImageError::OutOfMemory);
    }

    // Where the real work happens. WIC decodes lazily, so a format whose
    // container parses and whose payload has no codec gets all the way here
    // before it fails -- which is exactly the AVIF case.
    hr = converter->CopyPixels(nullptr, static_cast<UINT>(stride),
                               static_cast<UINT>(bytes), pixels.data());
    if (FAILED(hr))
      return std::unexpected(to_image_error(hr));

    std::shared_ptr<Image> image = Image::from_pixels(
        Blob::own(std::move(pixels)), static_cast<int>(width),
        static_cast<int>(height),
        options.premultiply ? PixelFormat::Rgba8Premultiplied
                            : PixelFormat::Rgba8Straight,
        static_cast<int>(source_width), static_cast<int>(source_height));
    if (!image)
      return std::unexpected(ImageError::Malformed);

    return image;
  }
};

} // namespace

std::shared_ptr<ImageDecoder> platform_image_decoder() {
  return std::make_shared<WicDecoder>();
}

} // namespace voidui
