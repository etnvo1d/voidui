#pragma once

#include <cstdint>
#include <deque>
#include <mutex>
#include <string>
#include <unordered_map>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

#include "voidui/core/style/property.h"
#include "voidui/core/style/value.h"

namespace voidui {

/// A theme token name, interned. Zero is "none".
using TokenId = std::uint32_t;

inline constexpr TokenId kNoToken = 0;

class TokenTable {
public:
  static TokenTable &instance();

  TokenId intern(std::string_view name);
  TokenId find(std::string_view name) const;
  std::string_view name(TokenId token) const;

private:
  TokenTable();

  mutable std::mutex mutex_;
  std::deque<std::string> names_;
  std::unordered_map<std::string_view, TokenId> by_name_;
};

/// A reference to a theme token, written `$surface.raised` in a stylesheet.
///
/// Rules keep the reference rather than the value, which is the whole point of
/// the two-layer theme design: switching theme rebinds tokens and re-resolves,
/// and never re-parses a single rule.
struct TokenRef {
  TokenId token = kNoToken;
};

/// A value as it appears in a rule: either literal, or a token to look up.
using DeclaredValue = std::variant<PropertyValue, TokenRef>;

/// The set of property assignments in one rule.
///
/// Stored as a small array sorted by property index. Rules hold a handful of
/// properties, so a linear merge over sorted keys beats a hash map on both
/// speed and footprint.
class StyleDeclaration {
public:
  struct Entry {
    PropertyIndex property = kInvalidPropertyIndex;
    DeclaredValue value;
  };

  /// Type-safe assignment. This is the form component and application code
  /// uses; the property tag carries the value type, so a wrong type is a
  /// compile error rather than a silent no-op at run time.
  template <class P> StyleDeclaration &set(typename P::Value value) {
    assign(P::index(), DeclaredValue(PropertyValue(std::move(value))));
    return *this;
  }

  /// Binds a property to a theme token instead of a literal value.
  template <class P> StyleDeclaration &set_token(std::string_view name) {
    assign(P::index(), DeclaredValue(TokenRef{TokenTable::instance().intern(name)}));
    return *this;
  }

  void assign(PropertyIndex property, DeclaredValue value);

  const DeclaredValue *find(PropertyIndex property) const;

  /// Applies `other` on top of this one: every property `other` sets wins.
  void merge_over(const StyleDeclaration &other);

  const std::vector<Entry> &entries() const { return entries_; }
  bool empty() const { return entries_.empty(); }
  std::size_t size() const { return entries_.size(); }
  void clear() { entries_.clear(); }

private:
  std::vector<Entry> entries_;
};

} // namespace voidui
