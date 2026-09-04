#pragma once

#include <cstdint>
#include <memory>
#include <string>
#include <unordered_map>
#include <vector>

#include "voidui/core/resource.h"
#include "voidui/core/style/declaration.h"
#include "voidui/core/style/selector.h"
#include "voidui/core/typography.h"

namespace voidui {

/// Cascade origin, in ascending priority. Origin outranks specificity, which
/// is what lets a widget ship sensible defaults that any application rule beats
/// without having to out-specify it.
enum class StyleOrigin : std::uint8_t {
  /// Shipped with the component itself.
  WidgetDefault = 0,
  /// Contributed by the active theme's own rules, if it has any.
  Theme = 1,
  /// The application's stylesheet.
  User = 2,
  /// Set directly on one widget instance.
  Inline = 3,
};

struct StyleRule {
  Selector selector;
  StyleDeclaration declaration;
  StyleOrigin origin = StyleOrigin::User;

  /// Source order, the final tie-breaker of the cascade.
  std::uint32_t order = 0;
};

struct Keyframe {
  float offset = 0.0f;
  StyleDeclaration declaration;
};

struct KeyframeValue {
  float offset = 0.0f;
  DeclaredValue value;
};

struct KeyframeTrack {
  PropertyIndex property = kInvalidPropertyIndex;
  std::vector<KeyframeValue> values;
};

struct Keyframes {
  std::string name;
  std::vector<Keyframe> frames;
  std::vector<KeyframeTrack> tracks;
};

/// A collection of rules, indexed for fast candidate lookup.
///
/// Without the index, styling a node would test it against every rule in the
/// sheet. The buckets narrow that to the rules whose key selector mentions
/// something the node actually has, which keeps resolve time flat as a
/// stylesheet grows.
/// One entry of a `@font-face` `src` list, in the order written.
struct FontFaceSource {
  /// `local(...)`: a family the machine is expected to have already, which may
  /// be a different name from the one being defined.
  bool local = false;

  /// The family named by a `local()` source. Empty for `url()`.
  std::string family;

  /// Where a `url()` source points, already resolved against the stylesheet
  /// that wrote it -- so a sheet loaded from `res://` cannot name a file.
  ResourceUri uri;

  /// The `format()` hint as written. Kept for diagnostics only: whether a
  /// source can be used is decided by whether its bytes actually parse as a
  /// face, which is the same answer without a table of what this build
  /// happens to have been compiled to read.
  std::string format;
};

/// An `@font-face` rule: a family name, the weight it supplies, and where the
/// bytes come from.
///
/// The rule lives on the sheet rather than going straight into the font
/// registry, because parsing a sheet must not change what the application is
/// drawing with. A sheet that is parsed and thrown away -- a hot reload that
/// failed, a probe -- registers nothing; the fonts appear when the sheet does.
struct FontFaceRule {
  std::string family;
  FontWeight weight = FontWeight::Normal;
  std::vector<FontFaceSource> sources;
};

class StyleSheet {
public:
  StyleSheet() = default;

  std::uint32_t add(Selector selector, StyleDeclaration declaration,
                    StyleOrigin origin = StyleOrigin::User);

  /// Appends every rule of `other`, preserving relative source order.
  void append(const StyleSheet &other);

  /// Appends every rule while replacing its origin. Component-owned sheets
  /// use this path so their defaults cannot outrank application rules.
  void append(const StyleSheet &other, StyleOrigin origin);

  void add_keyframes(Keyframes keyframes);
  const Keyframes *find_keyframes(std::string_view name) const;

  void add_font_face(FontFaceRule rule);
  const std::vector<FontFaceRule> &font_faces() const { return font_faces_; }

  void clear();

  const StyleRule &rule(std::uint32_t index) const { return rules_[index]; }
  const std::vector<StyleRule> &rules() const { return rules_; }
  std::size_t size() const { return rules_.size(); }

  /// Fills `out` with the indices of rules whose key selector could apply to
  /// `node`. Cheap and approximate; the caller still runs the full match.
  void collect_candidates(const StyleNode &node,
                          std::vector<std::uint32_t> &out) const;

  /// Diagnostics produced while parsing, kept so a hot reload can surface them
  /// without throwing away the rules that did parse.
  const std::vector<std::string> &diagnostics() const { return diagnostics_; }
  void add_diagnostic(std::string message) {
    diagnostics_.push_back(std::move(message));
  }

private:
  void index_rule_(std::uint32_t index);

  std::vector<StyleRule> rules_;

  std::unordered_map<Atom, std::vector<std::uint32_t>> by_id_;
  std::unordered_map<Atom, std::vector<std::uint32_t>> by_class_;
  std::unordered_map<Atom, std::vector<std::uint32_t>> by_part_;
  std::unordered_map<std::type_index, std::vector<std::uint32_t>> by_type_;
  std::vector<std::uint32_t> universal_;
  std::unordered_map<std::string, Keyframes> keyframes_;
  std::vector<FontFaceRule> font_faces_;

  std::vector<std::string> diagnostics_;
};

} // namespace voidui
