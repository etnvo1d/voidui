#include "voidui/core/component.h"
#include "voidui/core/window.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/row.h"
#include "voidui/widgets/text.h"

using namespace voidui;

std::string vss = R"(
button {
  background: #fff;
  border-color: conic-gradient(from 0deg,
    #ff3b3b, #ffcc00, #30e88c,
    #38bdf8, #a855f7, #ff3b3b);
  border-width: 2px;

  animation: rainbow-flow 3s linear infinite;
  transition: transform 200ms ease, box-shadow 200ms ease;
}

button:hover {
  box-shadow: 0 0 22px #a855f773;
}

button:active {
  transform: translateY(1px);
}

@keyframes rainbow-flow {
  to {
    border-color: conic-gradient(from 360deg,
      #ff3b3b, #ffcc00, #30e88c,
      #38bdf8, #a855f7, #ff3b3b);
  }
}
)";

auto counter() {
  return component([] {
    auto count = use_state(0);

    return column(button("Increment").on_click([count] { count++; }),
                  text(std::to_string(count)),
                  button("Decrement").on_click([count] { count--; }))
        .gap(8.0f);
  });
}

int main() {
  Window window("VoidUI Counter", 480, 240);
  auto sheet = voidui::StyleParser::parse(vss, "app.vss");
  auto view = row(counter(), counter()).gap(24.0f);
  window.set_stylesheet(sheet.sheet);
  window.run(view);
  return 0;
}
