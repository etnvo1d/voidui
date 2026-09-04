#include "voidui/core/component.h"
#include "voidui/core/http.h"
#include "voidui/core/window.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/image.h"
#include "voidui/widgets/input.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/scrollable.h"
#include "voidui/widgets/svg.h"
#include "voidui/widgets/text.h"
#include <filesystem>

namespace fs = std::filesystem;
using namespace voidui;

auto view() {
  return component([] {
    auto url = use_state(std::string());
    return column()
        .add(
            column(
                image("https://i2.hdslb.com/bfs/new_dyn/"
                      "57cd29e6f900bd157cbcf53f12177608319966746.jpg@270w_"
                      "360h_1s.webp")
                    .width(240)
                    .height(240),
                input(url)
                    .placeholder("example.com")
                    .inline_start(
                        svg_markup(
                            R"(<svg xmlns="http://www.w3.org/2000/svg" height="24px" viewBox="0 -960 960 960" width="24px" fill="#e3e3e3"><path d="M784-120 532-372q-30 24-69 38t-83 14q-109 0-184.5-75.5T120-580q0-109 75.5-184.5T380-840q109 0 184.5 75.5T640-580q0 44-14 83t-38 69l252 252-56 56ZM380-400q75 0 127.5-52.5T560-580q0-75-52.5-127.5T380-760q-75 0-127.5 52.5T200-580q0 75 52.5 127.5T380-400Z"/></svg>)")
                            .add_class("svg")),
                textarea().placeholder("Type your message here."))
                .margin(Margin::Auto{})
                .gap(8))
        .width(Length::Fill{})
        .height(Length::Fill{});
  });
}

int main() {
  voidui::Window window("Galary", 800, 600);
  auto root = std::filesystem::temp_directory_path() / "voidui-galary-example";
  HttpOptions http;
  http.cache_directory = (root / "http-cache").string();
  if (std::shared_ptr<ResourceProvider> network = http_provider(http))
    Resources::global().set_network_provider(std::move(network));

  // The file-backed equivalent, which also re-reads on every save while
  // VOIDUI_HOT_RELOAD is on:
  //
  //   window.watch_styles("assets/app.vss", "assets/dark.vtheme");
  window.watch_styles(
      (fs::path(__FILE__).parent_path() / "galary.vss").string());

  window.run(view());
  return 0;
}
