#pragma once

#include <optional>
#include <string>
#include <string_view>
#include <vector>

#include "voidui/core/resource.h"
#include "voidui/core/typography.h"

namespace voidui {

/// One face an application has supplied itself.
///
/// Either the bytes are here, or `alias` names a family the machine is expected
/// to have -- which is what `@font-face { src: local(...) }` means, and the
/// only way one name can stand for another.
struct RegisteredFace {
  Blob bytes;
  std::string alias;
  int face_index = 0;
  FontWeight weight = FontWeight::Normal;
};

class StyleSheet;

/// The fonts an application ships, looked up before the platform's.
///
/// A registered family is indistinguishable from an installed one at the point
/// of use: `font-family: "Inter"` finds it either way. That is what lets a
/// stylesheet name a font the machine does not have, and it is why the
/// registry is consulted first rather than last -- an application that packs
/// Inter means its copy of Inter, not whatever version the machine happens to
/// have installed under that name.
///
/// The registry holds bytes, not paths, because a shipped font is as likely to
/// live in .rdata as on disk. A Blob keeps them alive for as long as any face
/// reads them, which is what FreeType requires.
///
/// Like the rest of the font layer, this is used from the UI thread.
class FontRegistry {
public:
  static FontRegistry &global();

  /// Adds a face. Several faces may share a family, one per weight; which one
  /// a request gets follows the CSS weight-matching rules.
  ///
  /// Registering the same family and weight twice replaces the earlier face,
  /// so a hot-reloaded `@font-face` does not pile up copies of itself.
  void add(std::string family, Blob bytes,
           FontWeight weight = FontWeight::Normal, int face_index = 0);

  /// Reads `source` through the resource layer, then adds what it finds.
  bool add(std::string family, const ResourceUri &source,
           FontWeight weight = FontWeight::Normal, int face_index = 0);

  /// Makes `family` stand for `target`, which the platform is expected to
  /// resolve. Nothing is loaded here: an alias is a redirection, not a face.
  void add_alias(std::string family, std::string target,
                 FontWeight weight = FontWeight::Normal);

  bool contains(std::string_view family) const;

  /// The face to use for `family` at `weight`, or nothing when the application
  /// registered none. Family names match the way CSS matches them: ASCII
  /// case-insensitively, ignoring surrounding space.
  std::optional<RegisteredFace> find(std::string_view family,
                                     FontWeight weight) const;

  /// Every family registered, in the order first seen. For diagnostics.
  std::vector<std::string> families() const;

  bool remove(std::string_view family);
  void clear();

private:
  struct Entry {
    std::string family;    ///< as written, for `families()`
    std::string folded;    ///< lowercased, for matching
    RegisteredFace face;
  };

  std::vector<Entry> entries_;
};

/// Puts a stylesheet's `@font-face` rules into the global registry.
///
/// Separate from parsing on purpose: a sheet that is read and discarded -- a
/// hot reload that failed to parse, a probe -- must not change what the
/// application draws with. Call this when a sheet actually goes into effect.
///
/// Each rule takes the first source that works, in the order written, which is
/// what makes `src: local("Inter"), url(...)` mean "the installed copy if there
/// is one". A rule no source satisfies is reported through `problems`, when one
/// is given, and otherwise passes quietly -- the family then resolves the way
/// any unknown family does.
void register_font_faces(const StyleSheet &sheet,
                         std::vector<std::string> *problems = nullptr);

} // namespace voidui
