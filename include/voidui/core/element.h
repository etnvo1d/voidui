#pragma once

#include <memory>

#include "voidui/core/widget.h"

namespace voidui {

class Element {
public:
private:
  std::unique_ptr<Widget> widget_;
};

} // namespace voidui