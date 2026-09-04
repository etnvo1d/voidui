#pragma once

#include <cstdint>
#include <memory>
#include <unordered_map>
#include <utility>
#include <vector>

#include "voidui/core/layout.h"
#include "voidui/core/style/property.h"
#include "voidui/core/style/value.h"

namespace voidui {

/// The finished style of one node: every property that has a value, after the
/// cascade, inheritance and token resolution have run.
///
/// Immutable once finalised, and shared between every node that computes the
/// same result -- a list of a thousand identical rows holds one of these, not a
/// thousand. That sharing is also what makes repaint diffing a pointer
/// comparison.
class ComputedStyle {
public:
  const PropertyValue *find(PropertyIndex property) const;

  /// Type-safe read. Falls back to the property's declared default when unset,
  /// so a component never has to check.
  template <class P> const typename P::Value &get() const {
    if (const PropertyValue *value = find(P::index()))
      if (const auto *typed = value->as<typename P::Value>())
        return *typed;
    return P::default_value_ref();
  }

  /// True when the property was explicitly set rather than defaulted.
  template <class P> bool has() const { return find(P::index()) != nullptr; }

  std::uint64_t hash() const { return hash_; }

  /// Width and height are copied out when the style is finalized. Layout reads
  /// this hot pair directly instead of searching the generic property array on
  /// every frame.
  const Size<Length> &layout_size() const { return layout_size_; }

  /// The four independently cascading margin properties, packed once when the
  /// style is finalized. Layout reads one contiguous value per node.
  const Spacing<MarginValue> &layout_margin() const { return layout_margin_; }

  bool operator==(const ComputedStyle &other) const;

  const std::vector<std::pair<PropertyIndex, PropertyValue>> &values() const {
    return values_;
  }

  // -- Building ------------------------------------------------------------

  void set(PropertyIndex property, PropertyValue value);
  void remove(PropertyIndex property);

  /// Sorts and hashes. Must be called before the style is shared or compared.
  void finalize();

  /// Copies only the properties marked inherited, for a child to start from.
  ComputedStyle inherited_subset() const;

private:
  std::vector<std::pair<PropertyIndex, PropertyValue>> values_;
  Size<Length> layout_size_;
  Spacing<MarginValue> layout_margin_;
  std::uint64_t hash_ = 0;
};

/// De-duplicates computed styles so identical results share one allocation.
///
/// Entries are weak: once the last node using a style goes away, so does the
/// cache entry. Nothing has to be invalidated on a theme switch or a hot
/// reload -- new results simply intern to new (or existing) entries.
class ComputedStyleCache {
public:
  std::shared_ptr<const ComputedStyle> intern(ComputedStyle style);

  /// Drops expired buckets. Cheap; worth calling after a hot reload.
  void collect();

  std::size_t size() const { return buckets_.size(); }

  struct Statistics {
    std::uint64_t interned = 0;
    std::uint64_t hits = 0;
  };

  const Statistics &statistics() const { return statistics_; }

private:
  std::unordered_map<std::uint64_t,
                     std::vector<std::weak_ptr<const ComputedStyle>>>
      buckets_;
  Statistics statistics_;
};

} // namespace voidui
