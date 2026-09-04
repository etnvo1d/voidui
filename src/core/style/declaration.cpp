#include "voidui/core/style/declaration.h"

#include <algorithm>

namespace voidui {

TokenTable::TokenTable() { names_.emplace_back(); }

TokenTable &TokenTable::instance() {
  static TokenTable table;
  return table;
}

TokenId TokenTable::intern(std::string_view name) {
  if (name.empty())
    return kNoToken;
  std::lock_guard<std::mutex> guard(mutex_);
  auto it = by_name_.find(name);
  if (it != by_name_.end())
    return it->second;
  const auto token = static_cast<TokenId>(names_.size());
  names_.emplace_back(name);
  by_name_.emplace(std::string_view(names_.back()), token);
  return token;
}

TokenId TokenTable::find(std::string_view name) const {
  if (name.empty())
    return kNoToken;
  std::lock_guard<std::mutex> guard(mutex_);
  auto it = by_name_.find(name);
  return it == by_name_.end() ? kNoToken : it->second;
}

std::string_view TokenTable::name(TokenId token) const {
  std::lock_guard<std::mutex> guard(mutex_);
  if (token == kNoToken || token >= names_.size())
    return {};
  return names_[token];
}

void StyleDeclaration::assign(PropertyIndex property, DeclaredValue value) {
  if (property == kInvalidPropertyIndex)
    return;

  auto position = std::lower_bound(
      entries_.begin(), entries_.end(), property,
      [](const Entry &entry, PropertyIndex key) { return entry.property < key; });

  if (position != entries_.end() && position->property == property) {
    position->value = std::move(value);
    return;
  }
  entries_.insert(position, Entry{property, std::move(value)});
}

const DeclaredValue *StyleDeclaration::find(PropertyIndex property) const {
  auto position = std::lower_bound(
      entries_.begin(), entries_.end(), property,
      [](const Entry &entry, PropertyIndex key) { return entry.property < key; });
  if (position == entries_.end() || position->property != property)
    return nullptr;
  return &position->value;
}

void StyleDeclaration::merge_over(const StyleDeclaration &other) {
  for (const Entry &entry : other.entries_)
    assign(entry.property, entry.value);
}

} // namespace voidui
