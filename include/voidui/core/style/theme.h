#pragma once

#include <memory>
#include <string>
#include <string_view>
#include <unordered_map>
#include <vector>

#include "voidui/core/style/declaration.h"
#include "voidui/core/style/property.h"
#include "voidui/core/style/value.h"

namespace voidui {

/// A set of semantic token bindings.
///
/// Themes are the *token* layer, kept strictly apart from the rule layer. A
/// stylesheet says `background: $surface.raised`; a theme says what
/// `surface.raised` is. Switching theme therefore costs one tree re-resolve
/// with no parsing and no allocation of rules -- the expensive half of the
/// system never moves.
///
/// A token's value is usually held as text and parsed on demand against
/// whichever property it lands in. That is what lets one token serve a Color
/// property and a Brush property without the theme author declaring a type.
class Theme {
public:
  Theme() = default;
  explicit Theme(std::string name) : name_(std::move(name)) {}

  const std::string &name() const { return name_; }
  void set_name(std::string name) { name_ = std::move(name); }

  /// Text form. Parsed lazily, per property that uses it, and memoised.
  void set(std::string_view token, std::string text);

  /// Typed form, for tokens built in C++. Skips parsing entirely.
  template <class T> void set_value(std::string_view token, T value) {
    set_property_value(token, PropertyValue(std::move(value)));
  }

  void set_property_value(std::string_view token, PropertyValue value);

  bool contains(std::string_view token) const;

  /// Themes may layer: a dark theme usually overrides a handful of tokens on
  /// top of a base. Lookup walks the chain; nothing is copied.
  void set_base(std::shared_ptr<const Theme> base);
  const std::shared_ptr<const Theme> &base() const { return base_; }

  /// Resolves `token` for the property at `property`, parsing the token text
  /// with that property's parser the first time the pair is seen.
  ///
  /// Returns null when the token is undefined, or when its text does not parse
  /// as the property's value type. Both are reported once and then treated as
  /// "property unset", so a broken theme degrades instead of crashing.
  const PropertyValue *resolve(TokenId token, PropertyIndex property) const;

  /// Bumped whenever a binding changes, so a resolver can tell that its cached
  /// results are stale.
  std::uint64_t revision() const { return revision_; }

  static std::shared_ptr<Theme> from_file(const std::string &path,
                                          std::string *error = nullptr);
  static std::shared_ptr<Theme> from_source(std::string_view source,
                                            std::string *error = nullptr);

  /// The colour behind the whole window.
  ///
  /// Read from the well-known token `app.background`. It lives in the theme
  /// rather than in a rule because there is no widget behind the window to
  /// hang a selector on -- the surface being painted is the frame itself.
  Color background(Color fallback) const;

  /// Every token name defined here or in any base, for tooling.
  std::vector<std::string> token_names() const;

private:
  struct Binding {
    std::string text;
    PropertyValue value; // set when the token was bound typed
    bool typed = false;
  };

  const Binding *lookup_(TokenId token) const;
  void invalidate_();

  std::string name_;
  std::shared_ptr<const Theme> base_;
  std::unordered_map<TokenId, Binding> bindings_;

  /// (token, property) -> parsed value. Cleared on any rebinding. Resolution
  /// runs on the UI thread only, hence no lock.
  mutable std::unordered_map<std::uint64_t, PropertyValue> cache_;
  mutable std::unordered_map<std::uint64_t, bool> failed_;

  std::uint64_t revision_ = 1;
};

} // namespace voidui
