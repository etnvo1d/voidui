#include "voidui/core/style/property.h"

#include <cassert>

namespace voidui {

PropertyRegistry &PropertyRegistry::instance() {
  // Function-local static: constructed on first use, which is what makes
  // registration from static initialisers in arbitrary translation units safe.
  static PropertyRegistry registry;
  return registry;
}

PropertyIndex
PropertyRegistry::register_property(PropertyDescriptor descriptor) {
  std::lock_guard<std::mutex> guard(mutex_);

  auto existing = by_name_.find(descriptor.name);
  if (existing != by_name_.end()) {
    // Registering the same name twice is expected: a header instantiated in
    // several translation units, or two DLLs whose RTTI identities differ.
    // Same value type means same property, so hand back the original index.
    const PropertyDescriptor &previous = descriptors_[existing->second];
    assert(previous.value_type == descriptor.value_type &&
           "two style properties share a name but not a value type");
    assert(previous.invalidation == descriptor.invalidation &&
           "two style properties share a name but not an invalidation kind");
    assert((previous.interpolate != nullptr) ==
               (descriptor.interpolate != nullptr) &&
           "two style properties share a name but not interpolation support");
    (void)previous;
    return existing->second;
  }

  const auto index = static_cast<PropertyIndex>(descriptors_.size());
  descriptors_.push_back(std::move(descriptor));
  by_name_.emplace(descriptors_.back().name, index);
  return index;
}

PropertyIndex PropertyRegistry::find(std::string_view name) const {
  std::lock_guard<std::mutex> guard(mutex_);
  auto it = by_name_.find(std::string(name));
  return it == by_name_.end() ? kInvalidPropertyIndex : it->second;
}

const PropertyDescriptor &
PropertyRegistry::describe(PropertyIndex index) const {
  std::lock_guard<std::mutex> guard(mutex_);
  return descriptors_[index];
}

std::size_t PropertyRegistry::size() const {
  std::lock_guard<std::mutex> guard(mutex_);
  return descriptors_.size();
}

std::vector<std::string> PropertyRegistry::names() const {
  std::lock_guard<std::mutex> guard(mutex_);
  std::vector<std::string> result;
  result.reserve(descriptors_.size());
  for (const PropertyDescriptor &descriptor : descriptors_)
    result.push_back(descriptor.name);
  return result;
}

WidgetTypeRegistry &WidgetTypeRegistry::instance() {
  static WidgetTypeRegistry registry;
  return registry;
}

void WidgetTypeRegistry::register_type(std::string name, std::type_index type) {
  std::lock_guard<std::mutex> guard(mutex_);
  by_name_.insert_or_assign(name, type);
  by_type_.insert_or_assign(type, std::move(name));
}

const std::type_index *WidgetTypeRegistry::find(std::string_view name) const {
  std::lock_guard<std::mutex> guard(mutex_);
  auto it = by_name_.find(std::string(name));
  return it == by_name_.end() ? nullptr : &it->second;
}

std::string_view WidgetTypeRegistry::name_of(std::type_index type) const {
  std::lock_guard<std::mutex> guard(mutex_);
  auto it = by_type_.find(type);
  return it == by_type_.end() ? std::string_view{}
                              : std::string_view(it->second);
}

} // namespace voidui
