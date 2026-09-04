#pragma once

#include <memory>
#include <string>
#include <string_view>
#include <vector>

#include "voidui/core/resource.h"
#include "voidui/core/style/stylesheet.h"
#include "voidui/core/style/theme.h"

namespace voidui {

struct StyleDiagnostic {
  std::string file;
  std::uint32_t line = 0;
  std::uint32_t column = 0;
  std::string message;

  std::string to_string() const;
};

/// Reader for the .vss stylesheet format.
///
/// The grammar is a deliberate CSS subset, so editors highlight it and users
/// already know it, plus one extension: `$token` for theme tokens.
///
///   /* comment */
///   button { background: $surface.raised; padding: 6 12; }
///   button:hover { background: $surface.raised.hover; }
///   window > .sidebar button { padding: $space.sm; }
///   .form input::part(placeholder) { color: $text.muted; }
///   .pulse { animation: pulse 800ms ease-in-out infinite alternate; }
///   @keyframes pulse { from { opacity: 0.4; } to { opacity: 1; } }
///
/// `@font-face` names a family and where to get it, so an application can ship
/// a font rather than hope the machine has one. Everything else keeps naming
/// the family and nothing else changes:
///
///   @font-face {
///     font-family: "Inter";
///     src: local("Inter"),
///          url("res://fonts/Inter-Regular.ttf") format("truetype");
///     font-weight: 400;
///   }
///   .title { font-family: "Inter", system-ui, sans-serif; }
///
/// A `url()` is resolved against the stylesheet that wrote it, which is what
/// keeps a sheet loaded from `res://` inside the resource namespace. `local()`
/// means "the installed copy, if this machine has one", and sources are tried
/// in the order written. Registration happens when the sheet is installed, not
/// when it is parsed.
///
/// A property name may be written unprefixed when it belongs to the rule's key
/// widget: inside `input { ... }`, `caret-color` resolves to
/// `input.caret-color`. Anywhere else the qualified name is required.
///
/// Transitions follow css-transitions-1 in full. The shorthand takes a
/// property (or `all`, or `none`), a duration, an easing function, a delay
/// -- the second of the two times, and the only one that may be negative --
/// and `allow-discrete`, in any order:
///
///   .card {
///     transition: opacity 200ms cubic-bezier(0.4, 0, 0.2, 1) 50ms,
///                 border-radius 120ms steps(4, jump-none);
///   }
///
/// It expands into `transition-property`, `transition-duration`,
/// `transition-timing-function`, `transition-delay` and
/// `transition-behavior`, which cascade on their own, so a later rule can
/// change one part of it and leave the rest standing:
///
///   .card.urgent { transition-duration: 60ms; }
///
/// A list shorter than `transition-property` repeats, exactly as in CSS, so
/// one duration covers every property named. The easing grammar is the whole
/// of css-easing-1 plus `linear()`: `linear`, `ease`, `ease-in`, `ease-out`,
/// `ease-in-out`, `step-start`, `step-end`, `cubic-bezier()`, `steps()` with
/// any of the five jump terms, and `linear(0, 0.25 75%, 1)`.
///
/// Property names inside a transition are scoped like any other: inside
/// `input { ... }`, `transition: caret-color 120ms` means `input.caret-color`.
///
/// Parsing never throws and never stops at the first error: an unreadable rule
/// is skipped with a diagnostic and the rest of the file is used. A hot reload
/// therefore degrades to "most of the file applied" instead of a blank window.
class StyleParser {
public:
  struct Result {
    std::shared_ptr<StyleSheet> sheet;
    std::vector<StyleDiagnostic> diagnostics;

    bool has_errors() const { return !diagnostics.empty(); }
  };

  /// `document` says where the source came from. It names the file in every
  /// diagnostic, and it is the base each relative reference inside the sheet
  /// resolves against -- which is what keeps a sheet loaded from `res://` from
  /// reaching the filesystem, and a sheet loaded from disk resolving beside
  /// itself.
  static Result parse(std::string_view source, ResourceUri document,
                      StyleOrigin origin = StyleOrigin::User);

  /// Takes the document as text: a bare path is a file, `res://theme/x.vss` a
  /// resource. An unparseable one leaves the sheet without a base, so its
  /// relative references fail rather than resolving somewhere arbitrary.
  static Result parse(std::string_view source, std::string_view document = {},
                      StyleOrigin origin = StyleOrigin::User);

  /// Reads through the resource layer, then parses.
  static Result parse_document(const ResourceUri &document,
                               StyleOrigin origin = StyleOrigin::User);

  static Result parse_file(const std::string &path,
                           StyleOrigin origin = StyleOrigin::User);

  /// Theme files: a flat list of `$token: value;` bindings, optionally with
  /// `@base "other.vtheme";` at the top and a `@name "Dark";` label.
  struct ThemeResult {
    std::shared_ptr<Theme> theme;
    std::vector<StyleDiagnostic> diagnostics;
  };

  static ThemeResult parse_theme(std::string_view source,
                                 ResourceUri document);
  static ThemeResult parse_theme(std::string_view source,
                                 std::string_view document = {});

  /// `@base` resolves against the document, so a theme at `res://theme/dark`
  /// can layer over `res://theme/palette` by naming it relatively. A chain that
  /// loops back on itself is cut off with a diagnostic rather than recursing.
  static ThemeResult parse_theme_document(const ResourceUri &document);
  static ThemeResult parse_theme_file(const std::string &path);
};

} // namespace voidui
