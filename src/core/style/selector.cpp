#include "voidui/core/style/selector.h"

#include <algorithm>

#include "voidui/core/style.h"
#include "voidui/core/style/computed.h"

namespace voidui {
namespace {

std::uint32_t mix32(std::uint64_t value) {
  value ^= value >> 33;
  value *= 0xff51afd7ed558ccdULL;
  value ^= value >> 29;
  value *= 0xc4ceb9fe1a85ec53ULL;
  value ^= value >> 32;
  return static_cast<std::uint32_t>(value);
}

/// The parent a selector is allowed to walk to.
///
/// Null at a shadow boundary: an internal node's host is not reachable by a
/// plain descendant or child combinator, which is what keeps an application's
/// `.panel text { ... }` from reaching inside every component that happens to
/// build itself out of Text. ::part() is the deliberate way through, and it
/// jumps to the host before any walking begins.
const StyleNode *light_parent(const StyleNode *node) {
  if (!node || (node->is_internal && !node->exposes_descendants))
    return nullptr;
  const StyleNode *parent = node->parent;
  while (parent && parent->is_transparent) {
    if (parent->is_internal)
      return nullptr;
    parent = parent->parent;
  }
  return parent;
}

} // namespace

AtomTable &AtomTable::instance() {
  static AtomTable table;
  return table;
}

Atom AtomTable::intern(std::string_view text) {
  if (text.empty())
    return kNoAtom;
  std::lock_guard<std::mutex> guard(mutex_);
  auto it = by_text_.find(text);
  if (it != by_text_.end())
    return it->second;
  const auto atom = static_cast<Atom>(texts_.size());
  texts_.emplace_back(text);
  by_text_.emplace(std::string_view(texts_.back()), atom);
  return atom;
}

Atom AtomTable::find(std::string_view text) const {
  if (text.empty())
    return kNoAtom;
  std::lock_guard<std::mutex> guard(mutex_);
  auto it = by_text_.find(text);
  return it == by_text_.end() ? kNoAtom : it->second;
}

std::string_view AtomTable::text(Atom atom) const {
  std::lock_guard<std::mutex> guard(mutex_);
  if (atom == kNoAtom || atom >= texts_.size())
    return {};
  return texts_[atom];
}

std::uint32_t style_atom_hash(Atom atom) {
  // Salted so that an atom and a type index with the same numeric value do not
  // collide in the Bloom filter.
  return mix32(0x9e3779b9ULL ^ (static_cast<std::uint64_t>(atom) << 1));
}

std::uint32_t style_type_hash(std::type_index type) {
  return mix32(0x51ed270bULL ^ static_cast<std::uint64_t>(type.hash_code()));
}

bool StyleNode::has_class(Atom atom) const {
  return std::binary_search(classes.begin(), classes.end(), atom);
}

const StyleNode *StyleNode::shadow_host() const {
  const StyleNode *current = this;
  while (current && current->is_internal)
    current = current->parent;
  return current == this ? nullptr : current;
}

void StyleNode::rebuild_bloom() {
  ancestor_bloom.clear();
  if (!parent)
    return;
  // Start from the parent's filter -- which already covers everything above it
  // -- and fold in the parent itself. Building the whole tree is therefore
  // linear, with one filter update per node.
  ancestor_bloom = parent->ancestor_bloom;
  const StyleNode *visible_parent = parent;
  while (visible_parent && visible_parent->is_transparent)
    visible_parent = visible_parent->parent;
  if (!visible_parent)
    return;
  if (visible_parent->id != kNoAtom)
    ancestor_bloom.add(style_atom_hash(visible_parent->id));
  for (Atom klass : visible_parent->classes)
    ancestor_bloom.add(style_atom_hash(klass));
  ancestor_bloom.add(style_type_hash(visible_parent->type));
}

Specificity CompoundSelector::specificity() const {
  Specificity result;
  if (id != kNoAtom)
    result.ids = 1;
  result.classes = static_cast<std::uint16_t>(classes.size() + nth.size());
  for (int bit = 0; bit < 8; ++bit)
    result.classes += static_cast<std::uint16_t>(
        ((required_status & (1u << bit)) != 0) +
        ((part_status & (1u << bit)) != 0));
  if (part != kNoAtom) {
    if (css_part) ++result.types;
    else ++result.classes;
  }
  if (type.has_value())
    ++result.types;
  return result;
}

bool CompoundSelector::matches(const StyleNode &node) const {
  if (type.has_value() && *type != node.type)
    return false;
  if (id != kNoAtom && node.id != id)
    return false;
  // Mask semantics, not equality: `button:hover` still applies while the button
  // is additionally focused.
  if ((node.status & required_status) != required_status)
    return false;
  for (Atom klass : classes)
    if (!node.has_class(klass))
      return false;
  for (const NthTest &test : nth)
    if (!test.matches(node.sibling_index, node.sibling_count))
      return false;
  return true;
}

std::uint32_t CompoundSelector::bloom_hash() const {
  if (id != kNoAtom)
    return style_atom_hash(id);
  if (!classes.empty())
    return style_atom_hash(classes.front());
  if (type.has_value())
    return style_type_hash(*type);
  return 0;
}

Selector::Selector(std::vector<SelectorPart> parts) : parts_(std::move(parts)) {
  for (const SelectorPart &part : parts_)
    specificity_ += part.compound.specificity();
}

namespace {

bool match_ancestors(const std::vector<SelectorPart> &parts, int index,
                     const StyleNode *current) {
  if (index < 0)
    return true;

  const Combinator combinator = parts[index + 1].combinator;
  const CompoundSelector &compound = parts[index].compound;

  if (combinator == Combinator::Child) {
    const StyleNode *parent = light_parent(current);
    if (!parent || !compound.matches(*parent))
      return false;
    return match_ancestors(parts, index - 1, parent);
  }

  // Descendant. The first matching ancestor is not necessarily the right one --
  // `a b > c` can need a further one up -- so this recurses rather than
  // committing greedily. Trees are shallow enough that the branching never
  // shows up in practice, and the Bloom pre-check has already thrown out the
  // rules that could not match at all.
  for (const StyleNode *ancestor = light_parent(current); ancestor;
       ancestor = light_parent(ancestor)) {
    if (compound.matches(*ancestor) &&
        match_ancestors(parts, index - 1, ancestor))
      return true;
  }
  return false;
}

} // namespace

bool Selector::matches(const StyleNode &node) const {
  if (parts_.empty() || node.is_transparent)
    return false;

  const CompoundSelector &key = parts_.back().compound;
  const StyleNode *context = nullptr;

  if (key.part != kNoAtom) {
    // `host::part(name)`. The subject is the exposed internal child; the rest
    // of the compound describes the host, and ancestor walking resumes there.
    if (!node.is_internal || node.part != key.part ||
        (node.status & key.part_status) != key.part_status)
      return false;
    const StyleNode *host = node.shadow_host();
    if (!host)
      return false;
    if ((host->status & key.required_status) != key.required_status)
      return false;
    if (key.type.has_value() && *key.type != host->type)
      return false;
    if (key.id != kNoAtom && host->id != key.id)
      return false;
    for (Atom klass : key.classes)
      if (!host->has_class(klass))
        return false;
    context = host;
  } else {
    if (node.is_internal)
      return false;
    if (!key.matches(node))
      return false;
    context = &node;
  }

  if (parts_.size() == 1)
    return true;

  // Reject on the ancestor filter before touching a single parent pointer.
  for (std::size_t i = 0; i + 1 < parts_.size(); ++i) {
    const std::uint32_t hash = parts_[i].compound.bloom_hash();
    if (hash != 0 && !context->ancestor_bloom.may_contain(hash))
      return false;
  }

  return match_ancestors(parts_, static_cast<int>(parts_.size()) - 2, context);
}

std::string Selector::to_string() const {
  std::string result;
  for (std::size_t i = 0; i < parts_.size(); ++i) {
    const SelectorPart &part = parts_[i];
    if (i > 0)
      result += part.combinator == Combinator::Child ? " > " : " ";

    const CompoundSelector &compound = part.compound;
    bool wrote = false;
    if (compound.type.has_value()) {
      const std::string_view name =
          WidgetTypeRegistry::instance().name_of(*compound.type);
      result += name.empty() ? std::string_view("<type>") : name;
      wrote = true;
    }
    if (compound.id != kNoAtom) {
      result += '#';
      result += AtomTable::instance().text(compound.id);
      wrote = true;
    }
    for (Atom klass : compound.classes) {
      result += '.';
      result += AtomTable::instance().text(klass);
      wrote = true;
    }
    if (compound.required_status & StatusBits::kHovered)
      result += ":hover";
    if (compound.required_status & StatusBits::kActive)
      result += ":active";
    if (compound.required_status & StatusBits::kFocused)
      result += ":focus";
    if (compound.required_status & StatusBits::kOpen) result += ":open";
    if (compound.required_status & StatusBits::kChecked) result += ":checked";
    if (compound.required_status & StatusBits::kDisabled) result += ":disabled";
    if (compound.required_status & StatusBits::kEnabled) result += ":enabled";
    for (const NthTest &test : compound.nth) {
      result += test.from_end ? ":nth-last-child(" : ":nth-child(";
      if (test.a != 0) {
        result += std::to_string(test.a);
        result += 'n';
        if (test.b != 0) {
          if (test.b > 0)
            result += '+';
          result += std::to_string(test.b);
        }
      } else {
        result += std::to_string(test.b);
      }
      result += ')';
      wrote = true;
    }
    if (compound.part != kNoAtom) {
      const auto name = AtomTable::instance().text(compound.part);
      if (compound.css_part) {
        result += name == "picker" ? "::picker(select)" : "::" + std::string(name);
      } else {
        result += "::part(";
        result += name;
        result += ')';
      }
      wrote = true;
    }
    if (compound.part_status & StatusBits::kHovered) result += ":hover";
    if (compound.part_status & StatusBits::kActive) result += ":active";
    if (compound.part_status & StatusBits::kFocused) result += ":focus";
    if (compound.part_status & StatusBits::kOpen) result += ":open";
    if (compound.part_status & StatusBits::kChecked) result += ":checked";
    if (compound.part_status & StatusBits::kDisabled) result += ":disabled";
    if (compound.part_status & StatusBits::kEnabled) result += ":enabled";
    if (!wrote && compound.required_status == 0)
      result += '*';
  }
  return result;
}

CompoundSelector SelectorBuilder::compound_of_type(std::type_index type) {
  CompoundSelector compound;
  compound.type = type;
  return compound;
}

SelectorBuilder SelectorBuilder::klass(std::string_view name) {
  SelectorBuilder builder;
  CompoundSelector compound;
  compound.classes.push_back(AtomTable::instance().intern(name));
  builder.parts_.push_back({std::move(compound), Combinator::None});
  return builder;
}

SelectorBuilder SelectorBuilder::id(std::string_view name) {
  SelectorBuilder builder;
  CompoundSelector compound;
  compound.id = AtomTable::instance().intern(name);
  builder.parts_.push_back({std::move(compound), Combinator::None});
  return builder;
}

SelectorBuilder SelectorBuilder::any() {
  SelectorBuilder builder;
  builder.parts_.push_back({CompoundSelector{}, Combinator::None});
  return builder;
}

SelectorBuilder &SelectorBuilder::with_class(std::string_view name) {
  CompoundSelector &compound = parts_.back().compound;
  const Atom atom = AtomTable::instance().intern(name);
  compound.classes.push_back(atom);
  std::sort(compound.classes.begin(), compound.classes.end());
  return *this;
}

SelectorBuilder &SelectorBuilder::hovered() {
  auto &key = parts_.back().compound;
  (key.part == kNoAtom ? key.required_status : key.part_status) |= StatusBits::kHovered;
  return *this;
}

SelectorBuilder &SelectorBuilder::active() {
  auto &key = parts_.back().compound;
  (key.part == kNoAtom ? key.required_status : key.part_status) |= StatusBits::kActive;
  return *this;
}

SelectorBuilder &SelectorBuilder::focused() {
  auto &key = parts_.back().compound;
  (key.part == kNoAtom ? key.required_status : key.part_status) |= StatusBits::kFocused;
  return *this;
}

#define VOIDUI_STATE_SELECTOR(method, bit) \
SelectorBuilder &SelectorBuilder::method() { \
  auto &key = parts_.back().compound; \
  (key.part == kNoAtom ? key.required_status : key.part_status) |= StatusBits::bit; \
  return *this; \
}
VOIDUI_STATE_SELECTOR(open, kOpen)
VOIDUI_STATE_SELECTOR(checked, kChecked)
VOIDUI_STATE_SELECTOR(disabled, kDisabled)
VOIDUI_STATE_SELECTOR(enabled, kEnabled)
#undef VOIDUI_STATE_SELECTOR
SelectorBuilder &SelectorBuilder::picker() { part("picker"); parts_.back().compound.css_part = true; return *this; }
SelectorBuilder &SelectorBuilder::picker_icon() { part("picker-icon"); parts_.back().compound.css_part = true; return *this; }
SelectorBuilder &SelectorBuilder::checkmark() { part("checkmark"); parts_.back().compound.css_part = true; return *this; }

SelectorBuilder &SelectorBuilder::nth_child(std::int32_t a, std::int32_t b) {
  parts_.back().compound.nth.push_back(NthTest{a, b, false});
  return *this;
}

SelectorBuilder &SelectorBuilder::nth_last_child(std::int32_t a,
                                                 std::int32_t b) {
  parts_.back().compound.nth.push_back(NthTest{a, b, true});
  return *this;
}

SelectorBuilder &SelectorBuilder::first_child() { return nth_child(0, 1); }

SelectorBuilder &SelectorBuilder::last_child() { return nth_last_child(0, 1); }

SelectorBuilder &SelectorBuilder::only_child() {
  return first_child().last_child();
}

SelectorBuilder &SelectorBuilder::part(std::string_view name) {
  parts_.back().compound.part = AtomTable::instance().intern(name);
  return *this;
}

SelectorBuilder &SelectorBuilder::prepend_(CompoundSelector compound,
                                           Combinator combinator) {
  parts_.front().combinator = combinator;
  parts_.insert(parts_.begin(), {std::move(compound), Combinator::None});
  return *this;
}

SelectorBuilder &SelectorBuilder::child_of_class(std::string_view name) {
  CompoundSelector compound;
  compound.classes.push_back(AtomTable::instance().intern(name));
  return prepend_(std::move(compound), Combinator::Child);
}

SelectorBuilder &SelectorBuilder::descendant_of_class(std::string_view name) {
  CompoundSelector compound;
  compound.classes.push_back(AtomTable::instance().intern(name));
  return prepend_(std::move(compound), Combinator::Descendant);
}

Selector SelectorBuilder::build() const { return Selector(parts_); }

void style_rebuild_blooms(StyleNode &root) {
  root.rebuild_bloom();
  for (StyleNode *child : root.children)
    if (child)
      style_rebuild_blooms(*child);
}

} // namespace voidui
