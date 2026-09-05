#include "voidui/core/style/parser.h"
#include "voidui/paint/font.h"
#include "voidui/paint/font_registry.h"
#include "voidui/paint/text_layout.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iterator>

using namespace voidui;

namespace {
void check(bool condition, const char *message) {
  if (!condition) {
    std::fprintf(stderr, "font self-test failed: %s\n", message);
    std::exit(1);
  }
}
bool near(float a, float b) { return std::abs(a - b) < 0.001f; }
bool missing(const GlyphRun &run) {
  return std::any_of(run.glyphs.begin(), run.glyphs.end(),
                     [](const auto &glyph) { return glyph.id == 0; });
}

void same_glyphs(const GlyphRun &actual, const GlyphRun &expected,
                 float pen = 0, std::uint32_t offset = 0) {
  check(actual.font == expected.font, "shaping preserves the supplied face");
  check(near(actual.advance, expected.advance), "run advance matches its face");
  check(actual.glyphs.size() == expected.glyphs.size(), "glyph count matches");
  for (std::size_t i = 0; i < actual.glyphs.size(); ++i) {
    const auto &a = actual.glyphs[i];
    const auto &b = expected.glyphs[i];
    check(a.id == b.id && a.cluster == b.cluster + offset &&
              near(a.offset.x, b.offset.x + pen) && near(a.offset.y, b.offset.y),
          "glyph ids, positions and UTF-8 clusters match the shaped source");
  }
}

void supported(const std::shared_ptr<FontStack> &stack) {
  for (const char *text : {"iiiiWWWW", "office AV ffi", "Cafe\u0301"}) {
    const auto direct = stack->base()->shape(text);
    check(!missing(direct), "test face supports the Latin sample");
    const auto runs = stack->shape(text);
    check(runs.size() == 1, "supported text stays in one base-font run");
    same_glyphs(runs.front(), direct);
    check(near(stack->measure(text), direct.advance), "measurement uses base face");
    const auto layout = TextLayout::build(stack, text, 800);
    check(near(layout->size().width, direct.advance), "layout width uses base face");
    for (const auto &run : layout->runs())
      check(run.font == stack->base(), "layout paints the supplied face");
  }
}

void mixed(const std::shared_ptr<FontStack> &stack, std::string middle,
            bool expect_fallback, bool single_cluster = false) {
  const std::string prefix = "AV office ";
  const std::string suffix = " WW end";
  const std::string text = prefix + middle + suffix;
  const auto runs = stack->shape(text);
  check(!runs.empty(), "mixed text produces glyphs");
  float pen = 0;
  bool fallback = false, saw_tail = false;
  for (const auto &run : runs) {
    check(!run.glyphs.empty(), "mixed runs contain glyphs");
    const auto first = std::min_element(
        run.glyphs.begin(), run.glyphs.end(),
        [](const auto &a, const auto &b) { return a.cluster < b.cluster; });
    const auto next = &run == &runs.back()
                          ? text.size()
                          : std::min_element(
                                (&run + 1)->glyphs.begin(), (&run + 1)->glyphs.end(),
                                [](const auto &a, const auto &b) {
                                  return a.cluster < b.cluster;
                                })->cluster;
    same_glyphs(run, run.font->shape(
                         std::string_view(text).substr(first->cluster,
                                                       next - first->cluster)),
                pen, first->cluster);
    for (const auto &glyph : run.glyphs) {
      check(glyph.cluster < text.size() &&
                (static_cast<unsigned char>(text[glyph.cluster]) & 0xc0) != 0x80,
            "cluster offsets point to original UTF-8 character boundaries");
      if (single_cluster && glyph.cluster >= prefix.size() &&
          glyph.cluster < prefix.size() + middle.size())
        check(glyph.cluster == prefix.size(),
              "fallback never splits a combining or emoji shaping cluster");
      if (glyph.cluster < prefix.size() ||
          glyph.cluster >= prefix.size() + middle.size())
        check(run.font == stack->base(), "supported neighbors retain base face");
      if (glyph.cluster >= prefix.size() + middle.size())
        saw_tail = true;
    }
    if (run.font != stack->base()) {
      fallback = true;
      if (expect_fallback)
        check(!missing(run), "fallback covers Chinese and emoji without tofu");
    }
    pen += run.advance;
  }
  check(saw_tail, "unmapped characters never discard the paragraph tail");
  if (expect_fallback)
    check(fallback, "missing text uses a system fallback face");

  const auto layout = TextLayout::build(stack, text, 0);
  check(near(layout->size().width, pen), "layout measures the same mixed runs");
  Rect<float> caret, selection;
  const auto end = static_cast<std::uint32_t>(text.size());
  check(layout->caret_rect(end, caret) && near(caret.origin.x, pen),
        "end caret matches the rendered text width");
  check(layout->hit_test({pen + 1, 0}) == end,
        "hit testing returns the original UTF-8 end offset");
  check(layout->selection_rect(0, 0, end, selection) &&
            near(selection.size.width, pen),
        "selection covers exactly the measured text");
}
} // namespace

int main(int argc, char **argv) {
  std::vector<std::string> paths;
  for (int i = 1; i < argc; ++i)
    paths.emplace_back(argv[i]);
  if (paths.empty())
    for (const char *path : {"C:/Windows/Fonts/cour.ttf",
                             "C:/Windows/Fonts/couri.ttf",
                             "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
                             "/System/Library/Fonts/Menlo.ttc"})
      if (std::filesystem::is_regular_file(path))
        paths.emplace_back(path);
  if (paths.empty()) {
    std::puts("font self-test skipped: no test font installed");
    return 0;
  }

  for (const auto &path : paths) {
    std::printf("testing %s\n", path.c_str());
    auto face = Font::from_file(path, 22);
    check(face != nullptr, "test font loads");
    // A deliberately different installed family must never replace this face.
    auto stack = FontStack::from_font(face, "Arial", "zh-CN");
    supported(stack);
    supported(FontStack::from_font(face));
    const bool platform = FontProvider::system().available();
    mixed(stack, "\u4f60\u597d", platform);
    mixed(stack, "\U0001f600", platform);
    mixed(stack, "\U0001f469\u200d\U0001f4bb", platform, true);
    mixed(stack, "1\ufe0f\u20e3", platform, true);
    mixed(stack, "\U0010ffff", false);

    std::ifstream file(path, std::ios::binary);
    std::vector<std::uint8_t> bytes((std::istreambuf_iterator<char>(file)), {});
    Blob blob = Blob::own(std::move(bytes));
    supported(FontStack::from_font(Font::from_blob(blob, 22)));
    FontRegistry::global().add("VoidUI Shaping Test", blob);
    auto registered = FontStack::create("VoidUI Shaping Test", 22, "zh-CN");
    check(registered != nullptr, "registered family resolves");
    supported(registered);
    mixed(registered, "\u4f60\u597d\U0001f600", platform);
    FontRegistry::global().remove("VoidUI Shaping Test");

    auto pack = std::make_shared<MemoryProvider>();
    pack->add("face.ttf", blob);
    const auto mount = Resources::global().mount("font-shaping-test", pack);
    const auto sheet = StyleParser::parse(
        "@font-face { font-family: 'VoidUI VSS Shaping'; src: url('face.ttf'); }",
        "res://font-shaping-test/test.vss");
    check(sheet.sheet && sheet.diagnostics.empty(), "@font-face parses");
    std::vector<std::string> problems;
    register_font_faces(*sheet.sheet, &problems);
    check(problems.empty(), "@font-face registers the supplied bytes");
    auto styled = FontStack::create("VoidUI VSS Shaping", 22, "zh-CN");
    check(styled != nullptr, "VSS font family resolves");
    supported(styled);
    FontRegistry::global().remove("VoidUI VSS Shaping");
    check(Resources::global().unmount(mount), "test font resource unmounts");
  }
  std::puts("font self-test passed");
}
