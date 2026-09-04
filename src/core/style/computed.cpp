#include "voidui/core/style.h"

#include <algorithm>

namespace voidui {

const PropertyValue *ComputedStyle::find(PropertyIndex property) const {
  auto position =
      std::lower_bound(values_.begin(), values_.end(), property,
                       [](const std::pair<PropertyIndex, PropertyValue> &entry,
                          PropertyIndex key) { return entry.first < key; });
  if (position == values_.end() || position->first != property)
    return nullptr;
  return &position->second;
}

void ComputedStyle::set(PropertyIndex property, PropertyValue value) {
  if (property == kInvalidPropertyIndex)
    return;
  auto position =
      std::lower_bound(values_.begin(), values_.end(), property,
                       [](const std::pair<PropertyIndex, PropertyValue> &entry,
                          PropertyIndex key) { return entry.first < key; });
  if (position != values_.end() && position->first == property) {
    position->second = std::move(value);
    return;
  }
  values_.insert(position, {property, std::move(value)});
}

void ComputedStyle::remove(PropertyIndex property) {
  auto position =
      std::lower_bound(values_.begin(), values_.end(), property,
                       [](const std::pair<PropertyIndex, PropertyValue> &entry,
                          PropertyIndex key) { return entry.first < key; });
  if (position != values_.end() && position->first == property)
    values_.erase(position);
}

void ComputedStyle::finalize() {
  // set() keeps values_ sorted, so this only has to fold the hash. Order is
  // therefore already canonical: two styles with the same content hash the
  // same regardless of the order the cascade produced them in.
  std::uint64_t hash = 0xcbf29ce484222325ULL;
  for (const auto &[property, value] : values_) {
    hash = style_hash_combine(hash, property);
    hash = style_hash_combine(hash, value.hash());
  }
  hash_ = hash;
  layout_size_ = {get<styles::Width>(), get<styles::Height>()};
  layout_margin_ = Spacing<MarginValue>(
      get<styles::MarginLeft>(), get<styles::MarginTop>(),
      get<styles::MarginRight>(), get<styles::MarginBottom>());
}

bool ComputedStyle::operator==(const ComputedStyle &other) const {
  if (hash_ != other.hash_ || values_.size() != other.values_.size())
    return false;
  for (std::size_t i = 0; i < values_.size(); ++i) {
    if (values_[i].first != other.values_[i].first)
      return false;
    if (!(values_[i].second == other.values_[i].second))
      return false;
  }
  return true;
}

ComputedStyle ComputedStyle::inherited_subset() const {
  ComputedStyle result;
  const PropertyRegistry &registry = PropertyRegistry::instance();
  result.values_.reserve(values_.size());
  for (const auto &entry : values_) {
    if (registry.describe(entry.first).inherited)
      result.values_.push_back(entry);
  }
  result.finalize();
  return result;
}

std::shared_ptr<const ComputedStyle>
ComputedStyleCache::intern(ComputedStyle style) {
  auto &bucket = buckets_[style.hash()];

  for (std::size_t i = 0; i < bucket.size();) {
    std::shared_ptr<const ComputedStyle> existing = bucket[i].lock();
    if (!existing) {
      // Opportunistic cleanup: the node that used this style is gone.
      bucket[i] = bucket.back();
      bucket.pop_back();
      continue;
    }
    if (*existing == style) {
      ++statistics_.hits;
      return existing;
    }
    ++i;
  }

  auto shared = std::make_shared<const ComputedStyle>(std::move(style));
  bucket.push_back(shared);
  ++statistics_.interned;
  return shared;
}

void ComputedStyleCache::collect() {
  for (auto it = buckets_.begin(); it != buckets_.end();) {
    auto &bucket = it->second;
    bucket.erase(
        std::remove_if(bucket.begin(), bucket.end(),
                       [](const std::weak_ptr<const ComputedStyle> &e) {
                         return e.expired();
                       }),
        bucket.end());
    it = bucket.empty() ? buckets_.erase(it) : std::next(it);
  }
}

} // namespace voidui
