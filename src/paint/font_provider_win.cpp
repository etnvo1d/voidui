// DirectWrite-backed font provider.
//
// DirectWrite is the same machinery the shell and every Windows application
// uses, so an application built on it inherits exactly the substitutions the
// user already sees elsewhere -- including the locale-sensitive choice between
// Chinese, Japanese and Traditional Chinese forms of the same codepoints.

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <dwrite_3.h>
#include <wrl/client.h>

#include "voidui/paint/font_provider.h"

#include <string>
#include <vector>

namespace voidui {

namespace {

using Microsoft::WRL::ComPtr;

std::wstring widen(std::string_view utf8) {
  if (utf8.empty())
    return {};

  const int n = MultiByteToWideChar(CP_UTF8, 0, utf8.data(),
                                    static_cast<int>(utf8.size()), nullptr, 0);
  std::wstring wide(n > 0 ? n : 0, L'\0');
  if (n > 0)
    MultiByteToWideChar(CP_UTF8, 0, utf8.data(), static_cast<int>(utf8.size()),
                        wide.data(), n);
  return wide;
}

std::string narrow(const wchar_t *wide, int length = -1) {
  if (!wide)
    return {};

  const int n = WideCharToMultiByte(CP_UTF8, 0, wide, length, nullptr, 0,
                                    nullptr, nullptr);
  if (n <= 0)
    return {};

  std::string out(static_cast<std::size_t>(length < 0 ? n - 1 : n), '\0');
  WideCharToMultiByte(CP_UTF8, 0, wide, length, out.data(), n, nullptr,
                      nullptr);
  return out;
}

/// Number of UTF-8 bytes that encode the first `units` UTF-16 code units of
/// `utf8`. DirectWrite counts in UTF-16; everything above this layer counts in
/// UTF-8, and a run boundary has to land in the same place in both.
std::size_t utf16_units_to_utf8_bytes(std::string_view utf8,
                                      std::size_t units) {
  std::size_t bytes = 0;
  std::size_t consumed = 0;

  while (bytes < utf8.size() && consumed < units) {
    const unsigned char lead = static_cast<unsigned char>(utf8[bytes]);
    std::size_t width = 1;
    if (lead >= 0xF0)
      width = 4;
    else if (lead >= 0xE0)
      width = 3;
    else if (lead >= 0xC0)
      width = 2;

    // Anything outside the basic plane costs two UTF-16 units.
    consumed += width == 4 ? 2 : 1;
    bytes += width;
  }

  return bytes;
}

/// The minimum IDWriteTextAnalysisSource that MapCharacters needs: one
/// immutable string with one locale.
class AnalysisSource final : public IDWriteTextAnalysisSource {
public:
  AnalysisSource(const std::wstring &text, const std::wstring &locale)
      : text_(text), locale_(locale) {}

  HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid,
                                           void **object) override {
    if (!object)
      return E_POINTER;
    if (riid == __uuidof(IUnknown) ||
        riid == __uuidof(IDWriteTextAnalysisSource)) {
      *object = static_cast<IDWriteTextAnalysisSource *>(this);
      return S_OK;
    }
    *object = nullptr;
    return E_NOINTERFACE;
  }

  // Stack-allocated for the duration of one call; reference counting is moot.
  ULONG STDMETHODCALLTYPE AddRef() override { return 1; }
  ULONG STDMETHODCALLTYPE Release() override { return 1; }

  HRESULT STDMETHODCALLTYPE GetTextAtPosition(UINT32 position,
                                              const WCHAR **text,
                                              UINT32 *length) override {
    if (position >= text_.size()) {
      *text = nullptr;
      *length = 0;
    } else {
      *text = text_.c_str() + position;
      *length = static_cast<UINT32>(text_.size() - position);
    }
    return S_OK;
  }

  HRESULT STDMETHODCALLTYPE GetTextBeforePosition(UINT32 position,
                                                  const WCHAR **text,
                                                  UINT32 *length) override {
    if (position == 0 || position > text_.size()) {
      *text = nullptr;
      *length = 0;
    } else {
      *text = text_.c_str();
      *length = static_cast<UINT32>(position);
    }
    return S_OK;
  }

  DWRITE_READING_DIRECTION STDMETHODCALLTYPE
  GetParagraphReadingDirection() override {
    return DWRITE_READING_DIRECTION_LEFT_TO_RIGHT;
  }

  HRESULT STDMETHODCALLTYPE GetLocaleName(UINT32 position, UINT32 *length,
                                          const WCHAR **name) override {
    *length = position < text_.size()
                  ? static_cast<UINT32>(text_.size() - position)
                  : 0;
    *name = locale_.c_str();
    return S_OK;
  }

  HRESULT STDMETHODCALLTYPE
  GetNumberSubstitution(UINT32 position, UINT32 *length,
                        IDWriteNumberSubstitution **substitution) override {
    *length = position < text_.size()
                  ? static_cast<UINT32>(text_.size() - position)
                  : 0;
    *substitution = nullptr;
    return S_OK;
  }

private:
  std::wstring text_;
  std::wstring locale_;
};

std::optional<FontFile> file_for_face(IDWriteFontFace *face) {
  if (!face)
    return std::nullopt;

  UINT32 count = 0;
  if (FAILED(face->GetFiles(&count, nullptr)) || count == 0)
    return std::nullopt;

  ComPtr<IDWriteFontFile> file;
  count = 1;
  if (FAILED(face->GetFiles(&count, file.GetAddressOf())) || !file)
    return std::nullopt;

  const void *key = nullptr;
  UINT32 key_size = 0;
  ComPtr<IDWriteFontFileLoader> loader;
  if (FAILED(file->GetReferenceKey(&key, &key_size)) ||
      FAILED(file->GetLoader(&loader)))
    return std::nullopt;

  ComPtr<IDWriteLocalFontFileLoader> local;
  if (FAILED(loader.As(&local)))
    return std::nullopt;

  UINT32 length = 0;
  if (FAILED(local->GetFilePathLengthFromKey(key, key_size, &length)))
    return std::nullopt;

  std::wstring path(static_cast<std::size_t>(length) + 1, L'\0');
  if (FAILED(local->GetFilePathFromKey(key, key_size, path.data(), length + 1)))
    return std::nullopt;

  FontFile result;
  result.path = narrow(path.c_str());
  result.face_index = static_cast<int>(face->GetIndex());
  return result;
}

std::optional<FontFile> file_for_font(IDWriteFont *font) {
  if (!font)
    return std::nullopt;

  ComPtr<IDWriteFontFace> face;
  if (FAILED(font->CreateFontFace(&face)))
    return std::nullopt;

  return file_for_face(face.Get());
}

class DirectWriteProvider final : public FontProvider {
public:
  DirectWriteProvider() {
    if (FAILED(DWriteCreateFactory(
            DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory2),
            reinterpret_cast<IUnknown **>(factory_.GetAddressOf()))))
      return;

    factory_->GetSystemFontCollection(&collection_, FALSE);
    factory_->GetSystemFontFallback(&fallback_);
  }

  bool available() const override {
    return factory_ && collection_ && fallback_;
  }

  std::string default_ui_family() override {
    NONCLIENTMETRICSW metrics{};
    metrics.cbSize = sizeof(metrics);
    if (SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, sizeof(metrics),
                              &metrics, 0))
      return narrow(metrics.lfMessageFont.lfFaceName);
    return "Segoe UI";
  }

  std::string default_locale() override {
    wchar_t buffer[LOCALE_NAME_MAX_LENGTH]{};
    if (GetUserDefaultLocaleName(buffer, LOCALE_NAME_MAX_LENGTH) > 0)
      return narrow(buffer);
    return "en-us";
  }

  /// What the CSS generics mean on Windows.
  ///
  /// Every candidate is checked against the installed collection before it is
  /// returned, because none of these is guaranteed: Segoe UI Emoji arrived in
  /// Windows 8.1, and a stripped Server install may have neither Georgia nor
  /// Impact. An unmatched generic reports nothing and the caller moves on.
  std::string generic_family(std::string_view generic) override {
    static const std::pair<std::string_view, std::vector<const wchar_t *>>
        table[] = {
            {"serif", {L"Times New Roman", L"Georgia"}},
            {"ui-serif", {L"Georgia", L"Times New Roman"}},
            {"monospace", {L"Consolas", L"Courier New"}},
            {"ui-monospace", {L"Cascadia Mono", L"Consolas"}},
            {"cursive", {L"Segoe Script", L"Comic Sans MS"}},
            {"fantasy", {L"Impact"}},
            {"emoji", {L"Segoe UI Emoji"}},
            {"math", {L"Cambria Math"}},
            {"fangsong", {L"FangSong", L"仿宋"}},
        };

    // The sans-serif family of generics is the interface font, which the
    // machine has by definition.
    if (generic == "sans-serif" || generic == "system-ui" ||
        generic == "ui-sans-serif" || generic == "ui-rounded")
      return default_ui_family();

    for (const auto &[name, candidates] : table) {
      if (name != generic)
        continue;
      for (const wchar_t *candidate : candidates) {
        UINT32 index = 0;
        BOOL exists = FALSE;
        if (collection_ &&
            SUCCEEDED(collection_->FindFamilyName(candidate, &index, &exists)) &&
            exists)
          return narrow(candidate);
      }
      break;
    }
    return {};
  }

  std::optional<FontFile> resolve(std::string_view family,
                                  FontWeight weight) override {
    if (!available())
      return std::nullopt;

    const std::wstring wide = widen(family);
    UINT32 index = 0;
    BOOL exists = FALSE;
    if (FAILED(collection_->FindFamilyName(wide.c_str(), &index, &exists)) ||
        !exists)
      return std::nullopt;

    ComPtr<IDWriteFontFamily> font_family;
    if (FAILED(collection_->GetFontFamily(index, &font_family)))
      return std::nullopt;

    ComPtr<IDWriteFont> font;
    if (FAILED(font_family->GetFirstMatchingFont(
            static_cast<DWRITE_FONT_WEIGHT>(font_weight_value(weight)),
            DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL, &font)))
      return std::nullopt;

    return file_for_font(font.Get());
  }

  std::optional<FontRun> fallback(std::string_view utf8,
                                  std::string_view base_family,
                                  std::string_view locale,
                                  FontWeight weight) override {
    if (!available() || utf8.empty())
      return std::nullopt;

    const std::wstring text = widen(utf8);
    if (text.empty())
      return std::nullopt;

    AnalysisSource source(text, widen(locale));

    UINT32 mapped = 0;
    ComPtr<IDWriteFont> font;
    FLOAT scale = 1.0f;

    const std::wstring base = widen(base_family);
    if (FAILED(fallback_->MapCharacters(
            &source, 0, static_cast<UINT32>(text.size()), collection_.Get(),
            base.empty() ? nullptr : base.c_str(),
            static_cast<DWRITE_FONT_WEIGHT>(font_weight_value(weight)),
            DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, &mapped,
            font.GetAddressOf(), &scale)))
      return std::nullopt;

    if (mapped == 0)
      return std::nullopt;

    // No font covers these characters. Report the span anyway so the caller can
    // skip past it rather than looping on the same position forever.
    FontRun run;
    run.length = utf16_units_to_utf8_bytes(utf8, mapped);
    if (run.length == 0)
      return std::nullopt;

    if (std::optional<FontFile> file = file_for_font(font.Get()))
      run.file = *file;

    return run;
  }

private:
  ComPtr<IDWriteFactory2> factory_;
  ComPtr<IDWriteFontCollection> collection_;
  ComPtr<IDWriteFontFallback> fallback_;
};

} // namespace

FontProvider &FontProvider::system() {
  static DirectWriteProvider provider;
  return provider;
}

} // namespace voidui
