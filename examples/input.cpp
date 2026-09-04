#include "voidui/widgets/input.h"
#include "voidui/core/component.h"
#include "voidui/core/window.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/text.h"

using namespace voidui;

std::string vss = R"(
input, textarea {
  caret-color: #2563eb;
}

// A field that wants a steadier caret can slow the blink down, or stop it
// altogether with `caret-blink: 0`.
input.steady {
  caret-blink: 0;
  caret-color: #dc2626;
}
)";

auto form() {
  return component([] {
    auto address =
        use_state(std::string("https://example.com/a/very/long/path"));
    auto notes = use_state(std::string("one\ntwo\nthree"));

    // `add_class` is declared on `Widget`, so it hands back a `Widget&&` that
    // cannot be stored by value. Naming the field keeps it an `Input`.
    Input steady("no blink");
    steady.add_class("steady");

    return column(text("Address"),
                  input(address).inline_start(text("URL")).placeholder("host"),
                  text("Steady caret"), std::move(steady), text("Notes"),
                  textarea(notes), text("value: " + address.get()))
        .gap(8.0f);
  });
}

int main() {
  Window window("VoidUI Input", 520, 480);
  auto sheet = voidui::StyleParser::parse(vss, "app.vss");
  window.set_stylesheet(sheet.sheet);
  window.run(form());
  return 0;
}
