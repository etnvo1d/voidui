#include "voidui/core/style/stylesheet.h"

#include <algorithm>

namespace voidui {

std::uint32_t StyleSheet::add(Selector selector, StyleDeclaration declaration,
                              StyleOrigin origin) {
  const auto index = static_cast<std::uint32_t>(rules_.size());
  StyleRule rule;
  rule.selector = std::move(selector);
  rule.declaration = std::move(declaration);
  rule.origin = origin;
  rule.order = index;
  rules_.push_back(std::move(rule));
  index_rule_(index);
  return index;
}

void StyleSheet::append(const StyleSheet &other) {
  for (const StyleRule &rule : other.rules_)
    add(rule.selector, rule.declaration, rule.origin);
  diagnostics_.insert(diagnostics_.end(), other.diagnostics_.begin(),
                      other.diagnostics_.end());
  for (const auto &[name, keyframes] : other.keyframes_)
    keyframes_.insert_or_assign(name, keyframes);
  for (const FontFaceRule &face : other.font_faces_)
    add_font_face(face);
}

void StyleSheet::append(const StyleSheet &other, StyleOrigin origin) {
  for (const StyleRule &rule : other.rules_)
    add(rule.selector, rule.declaration, origin);
  diagnostics_.insert(diagnostics_.end(), other.diagnostics_.begin(),
                      other.diagnostics_.end());
  for (const auto &[name, keyframes] : other.keyframes_)
    keyframes_.insert_or_assign(name, keyframes);
  for (const FontFaceRule &face : other.font_faces_)
    add_font_face(face);
}

void StyleSheet::add_keyframes(Keyframes keyframes) {
  keyframes.tracks.clear();
  for (const Keyframe &frame : keyframes.frames) {
    for (const StyleDeclaration::Entry &entry : frame.declaration.entries()) {
      auto track =
          std::find_if(keyframes.tracks.begin(), keyframes.tracks.end(),
                       [&](const KeyframeTrack &candidate) {
                         return candidate.property == entry.property;
                       });
      if (track == keyframes.tracks.end()) {
        keyframes.tracks.push_back({entry.property, {}});
        track = std::prev(keyframes.tracks.end());
      }
      if (!track->values.empty() && track->values.back().offset == frame.offset)
        track->values.back().value = entry.value;
      else
        track->values.push_back({frame.offset, entry.value});
    }
  }
  keyframes_.insert_or_assign(keyframes.name, std::move(keyframes));
}

void StyleSheet::add_font_face(FontFaceRule rule) {
  // A later rule for the same family and weight replaces an earlier one, the
  // way a later declaration replaces an earlier one.
  for (FontFaceRule &existing : font_faces_) {
    if (existing.weight == rule.weight && existing.family == rule.family) {
      existing = std::move(rule);
      return;
    }
  }
  font_faces_.push_back(std::move(rule));
}

const Keyframes *StyleSheet::find_keyframes(std::string_view name) const {
  const auto found = keyframes_.find(std::string(name));
  return found == keyframes_.end() ? nullptr : &found->second;
}

void StyleSheet::clear() {
  rules_.clear();
  by_id_.clear();
  by_class_.clear();
  by_part_.clear();
  by_type_.clear();
  universal_.clear();
  keyframes_.clear();
  font_faces_.clear();
  diagnostics_.clear();
}

void StyleSheet::index_rule_(std::uint32_t index) {
  const Selector &selector = rules_[index].selector;
  if (selector.empty()) {
    universal_.push_back(index);
    return;
  }

  // Filed under the narrowest thing the key selector requires. A node only has
  // to look in the buckets matching what it actually is, so adding rules that
  // mention other widgets costs nothing at resolve time.
  const CompoundSelector &key = selector.key();
  if (key.part != kNoAtom)
    by_part_[key.part].push_back(index);
  else if (key.id != kNoAtom)
    by_id_[key.id].push_back(index);
  else if (!key.classes.empty())
    by_class_[key.classes.front()].push_back(index);
  else if (key.type.has_value())
    by_type_[*key.type].push_back(index);
  else
    universal_.push_back(index);
}

void StyleSheet::collect_candidates(const StyleNode &node,
                                    std::vector<std::uint32_t> &out) const {
  out.clear();

  auto append_bucket =
      [&out](const std::unordered_map<Atom, std::vector<std::uint32_t>> &map,
             Atom key) {
        if (key == kNoAtom)
          return;
        auto it = map.find(key);
        if (it != map.end())
          out.insert(out.end(), it->second.begin(), it->second.end());
      };

  if (node.is_internal) {
    // Only ::part() rules can reach an internal node, so nothing else is even
    // considered for it.
    append_bucket(by_part_, node.part);
  } else {
    append_bucket(by_id_, node.id);
    for (Atom klass : node.classes)
      append_bucket(by_class_, klass);

    auto by_type = by_type_.find(node.type);
    if (by_type != by_type_.end())
      out.insert(out.end(), by_type->second.begin(), by_type->second.end());

    out.insert(out.end(), universal_.begin(), universal_.end());
  }

  // A rule filed under two of the node's classes would otherwise be considered
  // twice; source order is the natural key, so sorting also puts the candidate
  // list in cascade order before specificity is applied.
  std::sort(out.begin(), out.end());
  out.erase(std::unique(out.begin(), out.end()), out.end());
}

} // namespace voidui
