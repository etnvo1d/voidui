#pragma once

#include <compare>
#include <cstdint>
#include <deque>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <typeindex>
#include <unordered_map>
#include <vector>

#include "voidui/core/style/declaration.h"
#include "voidui/core/style/property.h"

namespace voidui {

class ComputedStyle;

/// An interned string identity. Zero is "none".
///
/// Class and id names are compared constantly during matching, so they are
/// folded to integers once at parse time. This also keeps StyleNode small.
using Atom = std::uint32_t;

inline constexpr Atom kNoAtom = 0;

class AtomTable {
public:
  static AtomTable &instance();

  Atom intern(std::string_view text);

  /// kNoAtom when the text was never interned. Matching uses this so an
  /// unknown class name in a stylesheet costs nothing at run time.
  Atom find(std::string_view text) const;

  std::string_view text(Atom atom) const;

private:
  AtomTable() { texts_.emplace_back(); }

  mutable std::mutex mutex_;
  // A deque, not a vector: the by_text_ keys are views into these strings,
  // and short-string storage would move with a vector reallocation.
  std::deque<std::string> texts_;
  std::unordered_map<std::string_view, Atom> by_text_;
};

/// Status bits a selector can require. Mirrors WidgetStatus, but as a mask:
/// `button:hover` must still match while the button is also focused, so the
/// test is "all required bits present", never equality.
struct StatusBits {
  static constexpr std::uint8_t kHovered = 1 << 0;
  static constexpr std::uint8_t kActive = 1 << 1;
  static constexpr std::uint8_t kFocused = 1 << 2;
};

struct Specificity {
  std::uint16_t ids = 0;
  std::uint16_t classes = 0; // classes, parts and status pseudo-classes
  std::uint16_t types = 0;

  auto operator<=>(const Specificity &) const = default;

  Specificity &operator+=(const Specificity &other) {
    ids = static_cast<std::uint16_t>(ids + other.ids);
    classes = static_cast<std::uint16_t>(classes + other.classes);
    types = static_cast<std::uint16_t>(types + other.types);
    return *this;
  }
};

enum class Combinator : std::uint8_t {
  /// No combinator: this is the leftmost compound of a selector.
  None,
  /// `parent > child`
  Child,
  /// `ancestor descendant`
  Descendant,
};

/// A 128-bit Bloom filter of everything above a node.
///
/// This is what makes descendant selectors cheap. Before walking a single
/// parent pointer the matcher asks whether the ancestor chain could possibly
/// contain `.sidebar`; when the answer is no -- which it is the overwhelming
/// majority of the time -- the rule is rejected outright. Building it is free:
/// a child starts from a copy of its parent's filter.
class AncestorBloom {
public:
  void add(std::uint32_t hash) {
    bits_[0] |= slot_(hash);
    bits_[1] |= slot_(hash >> 16);
  }

  bool may_contain(std::uint32_t hash) const {
    return (bits_[0] & slot_(hash)) != 0 && (bits_[1] & slot_(hash >> 16)) != 0;
  }

  void clear() { bits_[0] = bits_[1] = 0; }

private:
  static std::uint64_t slot_(std::uint32_t hash) {
    return std::uint64_t{1} << (hash & 63u);
  }

  std::uint64_t bits_[2]{0, 0};
};

std::uint32_t style_atom_hash(Atom atom);
std::uint32_t style_type_hash(std::type_index type);

/// One node in the style tree.
///
/// Kept separate from the widget tree's Node so the whole style system builds
/// and can be tested without pulling in widgets, painting or windowing. A
/// widget-tree node simply owns one of these and links the pointers.
struct StyleNode {
  /// Concrete widget type, from typeid(*widget).
  std::type_index type = std::type_index(typeid(void));

  Atom id = kNoAtom;

  /// Sorted, so two nodes with the same classes compare and hash identically.
  std::vector<Atom> classes;

  /// Non-zero when this node is a component's internal child that the component
  /// has deliberately exposed, reachable as `host::part(name)`.
  Atom part = kNoAtom;

  /// True for a component's internal children. Outside selectors do not reach
  /// across this boundary except through ::part(), which is what stops a
  /// stray `.panel text { ... }` from rewriting the inside of every button.
  bool is_internal = false;

  /// Function components participate in layout and events but are invisible
  /// to selector relationships.
  bool is_transparent = false;

  std::uint8_t status = 0;

  StyleNode *parent = nullptr;
  std::vector<StyleNode *> children;

  AncestorBloom ancestor_bloom;

  /// Set on this instance alone. Highest origin, above every sheet rule.
  const StyleDeclaration *inline_declaration = nullptr;

  std::shared_ptr<const ComputedStyle> computed;

  bool has_class(Atom atom) const;

  /// The nearest ancestor that is not internal -- the component that owns this
  /// node's shadow subtree. Returns null for a node in the light tree.
  const StyleNode *shadow_host() const;

  void rebuild_bloom();
};

/// A simple selector: everything that applies to a single node with no
/// combinator between the pieces, such as `button.primary:hover`.
struct CompoundSelector {
  std::optional<std::type_index> type;
  Atom id = kNoAtom;
  std::vector<Atom> classes;
  std::uint8_t required_status = 0;

  /// Set by `host::part(name)`. The compound then describes the *host*, and
  /// the subject of the match is the internal child exposed under this name.
  Atom part = kNoAtom;

  bool is_universal() const {
    return !type.has_value() && id == kNoAtom && classes.empty() &&
           required_status == 0 && part == kNoAtom;
  }

  Specificity specificity() const;

  bool matches(const StyleNode &node) const;

  /// The narrowest thing this compound requires of an ancestor, for the Bloom
  /// pre-check. Zero when it requires nothing that the filter tracks.
  std::uint32_t bloom_hash() const;
};

struct SelectorPart {
  CompoundSelector compound;

  /// How this compound relates to the one on its left. `None` on the first.
  Combinator combinator = Combinator::None;
};

/// A full selector, stored left to right. The last part is the key selector --
/// the one that describes the node the rule actually applies to.
class Selector {
public:
  Selector() = default;
  explicit Selector(std::vector<SelectorPart> parts);

  const std::vector<SelectorPart> &parts() const { return parts_; }
  const CompoundSelector &key() const { return parts_.back().compound; }
  Specificity specificity() const { return specificity_; }
  bool empty() const { return parts_.empty(); }

  bool matches(const StyleNode &node) const;

  std::string to_string() const;

private:
  std::vector<SelectorPart> parts_;
  Specificity specificity_;
};

/// Fluent construction of a selector from C++, equivalent to the text form.
///
///   Selector::of<Button>().hovered()                     // button:hover
///   Selector::klass("panel").child_of<Window>()          // window > .panel
///   Selector::of<Text>().descendant_of_class("toolbar")  // .toolbar text
///
/// The `_of` suffix reads right to left, matching how the matcher works and
/// how the parts accumulate: each call prepends a compound.
class SelectorBuilder {
public:
  template <class T> static SelectorBuilder of() {
    SelectorBuilder builder;
    builder.parts_.push_back(
        {compound_of_type(std::type_index(typeid(T))), Combinator::None});
    return builder;
  }

  static SelectorBuilder klass(std::string_view name);
  static SelectorBuilder id(std::string_view name);
  static SelectorBuilder any();

  SelectorBuilder &with_class(std::string_view name);
  SelectorBuilder &hovered();
  SelectorBuilder &active();
  SelectorBuilder &focused();

  /// Selects an exposed internal child of the compound built so far.
  SelectorBuilder &part(std::string_view name);

  template <class T> SelectorBuilder &child_of() {
    return prepend_(compound_of_type(std::type_index(typeid(T))),
                    Combinator::Child);
  }

  template <class T> SelectorBuilder &descendant_of() {
    return prepend_(compound_of_type(std::type_index(typeid(T))),
                    Combinator::Descendant);
  }

  SelectorBuilder &child_of_class(std::string_view name);
  SelectorBuilder &descendant_of_class(std::string_view name);

  Selector build() const;
  operator Selector() const { return build(); }

private:
  static CompoundSelector compound_of_type(std::type_index type);
  SelectorBuilder &prepend_(CompoundSelector compound, Combinator combinator);

  std::vector<SelectorPart> parts_;
};

namespace Selectors {
template <class T> inline SelectorBuilder of() {
  return SelectorBuilder::of<T>();
}
inline SelectorBuilder klass(std::string_view name) {
  return SelectorBuilder::klass(name);
}
inline SelectorBuilder id(std::string_view name) {
  return SelectorBuilder::id(name);
}
inline SelectorBuilder any() { return SelectorBuilder::any(); }
} // namespace Selectors

} // namespace voidui
