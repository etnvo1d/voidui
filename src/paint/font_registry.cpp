#include "voidui/paint/font_registry.h"

#include "voidui/core/style/stylesheet.h"
#include "voidui/paint/font.h"
#include "voidui/paint/font_provider.h"

#include <algorithm>

namespace voidui {
namespace {

std::string fold(std::string_view family) {
  std::string out;
  out.reserve(family.size());

  // Leading and trailing space is not part of a family name: `font-family:
  // "Inter" , system-ui` names Inter, not " Inter ".
  while (!family.empty() && (family.front() == ' ' || family.front() == '\t'))
    family.remove_prefix(1);
  while (!family.empty() && (family.back() == ' ' || family.back() == '\t'))
    family.remove_suffix(1);

  for (const char c : family)
    out += (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
  return out;
}

/// CSS font-matching, the weight half of it (css-fonts-4 §5.2).
///
/// The rule is not "nearest": below 400 a lighter face is preferred over a
/// nearer heavier one, above 500 the reverse, and 400..500 first looks upward
/// as far as 500. Getting this wrong is visible -- a family shipping 400 and
/// 700 would answer a request for 300 with the bold.
bool better_weight(std::uint16_t candidate, std::uint16_t best,
                   std::uint16_t want) {
  const auto rank = [want](std::uint16_t weight) {
    // Lower is better. The first component orders the preference groups, the
    // second the distance inside a group.
    if (weight == want)
      return std::pair<int, int>(0, 0);

    if (want >= 400 && want <= 500) {
      if (weight > want && weight <= 500)
        return std::pair<int, int>(1, weight - want);
      if (weight < want)
        return std::pair<int, int>(2, want - weight);
      return std::pair<int, int>(3, weight - want);
    }

    if (want < 400) {
      if (weight < want)
        return std::pair<int, int>(1, want - weight);
      return std::pair<int, int>(2, weight - want);
    }

    if (weight > want)
      return std::pair<int, int>(1, weight - want);
    return std::pair<int, int>(2, want - weight);
  };

  return rank(candidate) < rank(best);
}

} // namespace

FontRegistry &FontRegistry::global() {
  static FontRegistry instance;
  return instance;
}

void FontRegistry::add(std::string family, Blob bytes, FontWeight weight,
                       int face_index) {
  if (family.empty() || bytes.empty())
    return;

  std::string folded = fold(family);
  if (folded.empty())
    return;

  RegisteredFace face;
  face.bytes = std::move(bytes);
  face.face_index = face_index;
  face.weight = weight;

  for (Entry &entry : entries_) {
    if (entry.folded == folded && entry.face.weight == weight) {
      entry.face = std::move(face);
      return;
    }
  }

  entries_.push_back(
      Entry{std::move(family), std::move(folded), std::move(face)});
}

void FontRegistry::add_alias(std::string family, std::string target,
                             FontWeight weight) {
  if (family.empty() || target.empty())
    return;

  std::string folded = fold(family);
  if (folded.empty() || folded == fold(target))
    return; // an alias to itself would only be a slower lookup

  RegisteredFace face;
  face.alias = std::move(target);
  face.weight = weight;

  for (Entry &entry : entries_) {
    if (entry.folded == folded && entry.face.weight == weight) {
      entry.face = std::move(face);
      return;
    }
  }

  entries_.push_back(
      Entry{std::move(family), std::move(folded), std::move(face)});
}

bool FontRegistry::add(std::string family, const ResourceUri &source,
                       FontWeight weight, int face_index) {
  const ResourceResult<Blob> bytes = Resources::global().open(source);
  if (!bytes)
    return false;

  add(std::move(family), *bytes, weight, face_index);
  return true;
}

bool FontRegistry::contains(std::string_view family) const {
  const std::string folded = fold(family);
  return std::any_of(entries_.begin(), entries_.end(),
                     [&](const Entry &e) { return e.folded == folded; });
}

std::optional<RegisteredFace> FontRegistry::find(std::string_view family,
                                                 FontWeight weight) const {
  const std::string folded = fold(family);
  if (folded.empty())
    return std::nullopt;

  const std::uint16_t want = font_weight_value(weight);
  const RegisteredFace *best = nullptr;

  for (const Entry &entry : entries_) {
    if (entry.folded != folded)
      continue;
    if (!best || better_weight(font_weight_value(entry.face.weight),
                               font_weight_value(best->weight), want))
      best = &entry.face;
  }

  if (!best)
    return std::nullopt;
  return *best;
}

std::vector<std::string> FontRegistry::families() const {
  std::vector<std::string> names;
  for (const Entry &entry : entries_) {
    if (std::find(names.begin(), names.end(), entry.family) == names.end())
      names.push_back(entry.family);
  }
  return names;
}

bool FontRegistry::remove(std::string_view family) {
  const std::string folded = fold(family);
  const auto first = std::remove_if(
      entries_.begin(), entries_.end(),
      [&](const Entry &entry) { return entry.folded == folded; });
  if (first == entries_.end())
    return false;
  entries_.erase(first, entries_.end());
  return true;
}

void FontRegistry::clear() { entries_.clear(); }

void register_font_faces(const StyleSheet &sheet,
                         std::vector<std::string> *problems) {
  FontRegistry &registry = FontRegistry::global();
  FontProvider &provider = FontProvider::system();

  for (const FontFaceRule &rule : sheet.font_faces()) {
    bool satisfied = false;

    for (const FontFaceSource &source : rule.sources) {
      if (source.local) {
        if (!provider.available() ||
            !provider.resolve(source.family, rule.weight))
          continue;
        registry.add_alias(rule.family, source.family, rule.weight);
        satisfied = true;
        break;
      }

      const ResourceResult<Blob> bytes = Resources::global().open(source.uri);
      if (!bytes)
        continue;

      // Building the face is how a source is checked: a `format()` this build
      // cannot read, or a truncated file, fails here and the next source gets
      // its turn. The face is cached by its bytes, so this is also the load
      // the first paragraph would have paid for.
      if (!Font::from_blob(*bytes, 16.0f))
        continue;

      registry.add(rule.family, *bytes, rule.weight);
      satisfied = true;
      break;
    }

    if (!satisfied && problems)
      problems->push_back("no usable source for @font-face '" + rule.family +
                          "'");
  }
}

} // namespace voidui
