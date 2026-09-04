// The Text widget: a wrapped, shaped paragraph that reshapes only when
// something changes the outcome.
//
// Run with VOIDUI_LOG_TEXT=1 to see every TextLayout::build. A window sitting
// still should produce one line, not one per frame; resizing should produce one
// more each time the width actually changes.

#include "voidui/core/window.h"
#include "voidui/paint/font.h"
#include "voidui/paint/font_registry.h"
#include "voidui/widgets/text.h"

using namespace voidui;

namespace {

/// The family this paragraph asks for.
///
/// `system-ui` is the whole answer wherever a platform font provider exists.
/// Where one does not, a face is registered under a name of our own and named
/// first -- which is the same mechanism a shipped font uses, and the reason the
/// widget only ever needs to know a family name rather than a loaded face.
FontFamilyList ui_family() {
  if (FontProvider::system().available())
    return FontFamilyList::of({"system-ui"});

  for (const char *path : {"C:/Windows/Fonts/msyh.ttc",
                           "C:/Windows/Fonts/segoeui.ttf",
                           "/System/Library/Fonts/SFNS.ttf",
                           "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"}) {
    const ResourceResult<ResourceUri> uri = ResourceUri::parse(path);
    if (uri && FontRegistry::global().add("Fallback", *uri))
      return FontFamilyList::of({"Fallback"});
  }
  return {};
}

const char *kParagraph =
    "VoidUI 的文本组件把整形结果缓存下来，只有在文字、字体、对齐方式或换行宽度"
    "真正改变时才重新构建。Shaping is the expensive step: it crosses into "
    "HarfBuzz, and for anything the base face cannot render it crosses into the "
    "platform's font-fallback machinery as well.\n"
    "换行同时支持西文按空格断行和中日韩逐字断行，并且不会把标点丢到行首。"
    "Drawing costs no shaping at all — the display list references the layout "
    "through a shared pointer, so a paragraph redrawn every frame copies not a "
    "single glyph.";

} // namespace

int main() {
  auto label = std::make_unique<Text>(kParagraph);
  label->font_family(ui_family())
      .font_size(15.0f)
      .color(Color(28, 30, 38))
      .align(TextAlign::Left)
      .wrap(true);

  Window window("VoidUI Text Widget", 420, 380);
  window.run(std::move(label));
  return 0;
}
