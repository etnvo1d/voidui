// Probe: can the system tell us (a) its UI font and (b) which font to use for
// text the UI font cannot render? Compiled and run standalone; not part of the
// library.

#define WIN32_LEAN_AND_MEAN
#include <windows.h>

#include <dwrite_3.h>
#include <wrl/client.h>

#include <cstdio>
#include <string>

using Microsoft::WRL::ComPtr;

namespace {

std::string narrow(const wchar_t *w) {
  if (!w)
    return {};
  const int n = WideCharToMultiByte(CP_UTF8, 0, w, -1, nullptr, 0, nullptr, nullptr);
  std::string s(n > 0 ? n - 1 : 0, '\0');
  WideCharToMultiByte(CP_UTF8, 0, w, -1, s.data(), n, nullptr, nullptr);
  return s;
}

/// A text source over one immutable string, which is all MapCharacters needs.
class Source : public IDWriteTextAnalysisSource {
public:
  Source(const std::wstring &text, const wchar_t *locale)
      : text_(text), locale_(locale) {}

  HRESULT STDMETHODCALLTYPE QueryInterface(REFIID riid, void **object) override {
    if (!object)
      return E_POINTER;
    if (riid == __uuidof(IUnknown) || riid == __uuidof(IDWriteTextAnalysisSource)) {
      *object = static_cast<IDWriteTextAnalysisSource *>(this);
      return S_OK;
    }
    *object = nullptr;
    return E_NOINTERFACE;
  }
  ULONG STDMETHODCALLTYPE AddRef() override { return 1; }
  ULONG STDMETHODCALLTYPE Release() override { return 1; }

  HRESULT STDMETHODCALLTYPE GetTextAtPosition(UINT32 position, const WCHAR **text,
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
  HRESULT STDMETHODCALLTYPE GetTextBeforePosition(UINT32, const WCHAR **text,
                                                  UINT32 *length) override {
    *text = nullptr;
    *length = 0;
    return S_OK;
  }
  DWRITE_READING_DIRECTION STDMETHODCALLTYPE GetParagraphReadingDirection() override {
    return DWRITE_READING_DIRECTION_LEFT_TO_RIGHT;
  }
  HRESULT STDMETHODCALLTYPE GetLocaleName(UINT32 position, UINT32 *length,
                                          const WCHAR **name) override {
    *length = position < text_.size() ? static_cast<UINT32>(text_.size() - position) : 0;
    *name = locale_;
    return S_OK;
  }
  HRESULT STDMETHODCALLTYPE GetNumberSubstitution(UINT32 position, UINT32 *length,
                                                  IDWriteNumberSubstitution **s) override {
    *length = position < text_.size() ? static_cast<UINT32>(text_.size() - position) : 0;
    *s = nullptr;
    return S_OK;
  }

private:
  std::wstring text_;
  const wchar_t *locale_;
};

void report_font_file(IDWriteFont *font) {
  ComPtr<IDWriteFontFace> face;
  if (FAILED(font->CreateFontFace(&face)))
    return;

  UINT32 count = 0;
  face->GetFiles(&count, nullptr);
  if (count == 0)
    return;

  ComPtr<IDWriteFontFile> file;
  if (FAILED(face->GetFiles(&count, file.GetAddressOf())))
    return;

  const void *key = nullptr;
  UINT32 key_size = 0;
  ComPtr<IDWriteFontFileLoader> loader;
  if (FAILED(file->GetReferenceKey(&key, &key_size)) || FAILED(file->GetLoader(&loader)))
    return;

  ComPtr<IDWriteLocalFontFileLoader> local;
  if (FAILED(loader.As(&local)))
    return;

  UINT32 path_length = 0;
  if (FAILED(local->GetFilePathLengthFromKey(key, key_size, &path_length)))
    return;

  std::wstring path(path_length + 1, L'\0');
  if (SUCCEEDED(local->GetFilePathFromKey(key, key_size, path.data(), path_length + 1)))
    std::printf("      file        : %s\n", narrow(path.c_str()).c_str());

  std::printf("      face index  : %u\n", face->GetIndex());
}

void map(IDWriteFontFallback *fallback, IDWriteFontCollection *collection,
         const wchar_t *base_family, const std::wstring &text, const char *label,
         const wchar_t *locale) {
  Source source(text, locale);

  UINT32 mapped = 0;
  ComPtr<IDWriteFont> font;
  FLOAT scale = 1.0f;

  const HRESULT hr = fallback->MapCharacters(
      &source, 0, static_cast<UINT32>(text.size()), collection, base_family,
      DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL,
      &mapped, font.GetAddressOf(), &scale);

  std::printf("  %s\n", label);
  if (FAILED(hr) || !font) {
    std::printf("      -> no font mapped (hr=0x%08lx)\n", static_cast<unsigned long>(hr));
    return;
  }

  ComPtr<IDWriteFontFamily> family;
  ComPtr<IDWriteLocalizedStrings> names;
  if (SUCCEEDED(font->GetFontFamily(&family)) &&
      SUCCEEDED(family->GetFamilyNames(&names))) {
    UINT32 length = 0;
    names->GetStringLength(0, &length);
    std::wstring name(length + 1, L'\0');
    names->GetString(0, name.data(), length + 1);
    std::printf("      family      : %s   (covers %u of %zu chars)\n",
                narrow(name.c_str()).c_str(), mapped, text.size());
  }

  report_font_file(font.Get());
}

} // namespace

int main() {
  setvbuf(stdout, nullptr, _IONBF, 0);

  // (a) What does the OS say its UI font is?
  NONCLIENTMETRICSW metrics{};
  metrics.cbSize = sizeof(metrics);
  if (SystemParametersInfoW(SPI_GETNONCLIENTMETRICS, sizeof(metrics), &metrics, 0)) {
    std::printf("system UI font (SPI_GETNONCLIENTMETRICS lfMessageFont)\n");
    std::printf("  family : %s\n", narrow(metrics.lfMessageFont.lfFaceName).c_str());
    std::printf("  height : %ld  (negative = character height in logical units)\n\n",
                metrics.lfMessageFont.lfHeight);
  }

  // (b) Which font does the OS itself pick for text that font cannot render?
  ComPtr<IDWriteFactory2> factory;
  if (FAILED(DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED, __uuidof(IDWriteFactory2),
                                 reinterpret_cast<IUnknown **>(factory.GetAddressOf())))) {
    std::printf("DWriteCreateFactory failed\n");
    return 1;
  }

  ComPtr<IDWriteFontFallback> fallback;
  ComPtr<IDWriteFontCollection> collection;
  if (FAILED(factory->GetSystemFontFallback(&fallback)) || !fallback) {
    std::printf("GetSystemFontFallback failed\n");
    return 1;
  }
  if (FAILED(factory->GetSystemFontCollection(&collection, FALSE)) || !collection) {
    std::printf("GetSystemFontCollection failed\n");
    return 1;
  }

  std::printf("system UI font is what SPI reports; the fallback below shows what\n");
  std::printf("the OS itself would substitute for text that font cannot render.\n");

  // The same codepoints resolve to different faces depending on the locale --
  // Han unification means U+4F60 U+597D is valid Chinese *and* valid Japanese,
  // and DirectWrite needs the locale to tell them apart.
  const wchar_t *locales[] = {L"en-us", L"zh-CN", L"zh-TW", L"ja-JP", L"ko-KR"};
  for (const wchar_t *locale : locales) {
    std::printf("\n=== U+4F60 U+597D under locale %ls ===\n", locale);
    map(fallback.Get(), collection.Get(), L"Segoe UI", L"你好", "han", locale);
  }

  std::printf("\n=== other scripts under locale zh-CN ===\n");
  map(fallback.Get(), collection.Get(), L"Segoe UI", L"Hello", "latin", L"zh-CN");
  map(fallback.Get(), collection.Get(), L"Segoe UI", L"안녕", "korean", L"zh-CN");
  map(fallback.Get(), collection.Get(), L"Segoe UI", L"\U0001F600", "emoji", L"zh-CN");

  return 0;
}
