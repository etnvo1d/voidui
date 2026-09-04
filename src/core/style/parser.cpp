#include "voidui/core/style/parser.h"

#include "voidui/core/style.h"

#include <algorithm>
#include <cstdlib>
#include <sstream>

namespace voidui {
namespace {

bool is_space(char c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' ||
         c == '\v';
}

bool is_ident_start(char c) {
  return (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_';
}

bool is_ident_char(char c) {
  return is_ident_start(c) || (c >= '0' && c <= '9') || c == '-';
}

/// Splits a `src` list on its top-level commas, so a comma inside a quoted
/// name or inside `format(...)` does not start a new source.
std::vector<std::string_view> split_font_face_src(std::string_view text) {
  std::vector<std::string_view> parts;
  int depth = 0;
  char quote = '\0';
  std::size_t start = 0;

  for (std::size_t i = 0; i < text.size(); ++i) {
    const char c = text[i];
    if (quote != '\0') {
      if (c == quote)
        quote = '\0';
    } else if (c == '"' || c == '\'') {
      quote = c;
    } else if (c == '(') {
      ++depth;
    } else if (c == ')') {
      depth = depth > 0 ? depth - 1 : 0;
    } else if (c == ',' && depth == 0) {
      parts.push_back(style_trim(text.substr(start, i - start)));
      start = i + 1;
    }
  }
  parts.push_back(style_trim(text.substr(start)));
  return parts;
}

/// Peels one `name(body)` off the front of `text`, leaving the remainder.
bool take_function(std::string_view &text, std::string_view &name,
                   std::string_view &body) {
  text = style_trim(text);
  const std::size_t open = text.find('(');
  if (open == std::string_view::npos)
    return false;

  int depth = 0;
  char quote = '\0';
  std::size_t at = open;
  for (; at < text.size(); ++at) {
    const char c = text[at];
    if (quote != '\0') {
      if (c == quote)
        quote = '\0';
      continue;
    }
    if (c == '"' || c == '\'') {
      quote = c;
    } else if (c == '(') {
      ++depth;
    } else if (c == ')' && --depth == 0) {
      break;
    }
  }
  if (at >= text.size())
    return false;

  name = style_trim(text.substr(0, open));
  body = text.substr(open + 1, at - open - 1);
  text = text.substr(at + 1);
  return !name.empty();
}

/// Strips a matching pair of quotes, if there is one. VSS has no string
/// escapes, so what is inside is what is meant.
std::string unquote(std::string_view text) {
  if (text.size() >= 2 && (text.front() == '"' || text.front() == '\'') &&
      text.back() == text.front())
    return std::string(text.substr(1, text.size() - 2));
  return std::string(text);
}

/// A document name as written -- a bare path, or a URI.
///
/// An unparseable one yields an empty URI rather than an error. The sheet still
/// parses; what it loses is a base, so its relative references fail instead of
/// resolving against somewhere arbitrary.
ResourceUri document_uri(std::string_view text) {
  if (text.empty())
    return {};
  const ResourceResult<ResourceUri> uri = ResourceUri::parse(text);
  return uri ? *uri : ResourceUri{};
}

/// How deep the `@base` chain currently being followed is.
///
/// A theme that bases itself, directly or around a cycle, would otherwise
/// recurse until the stack gave out -- and it is reachable from a data file the
/// hot reloader picks up, so it has to be a diagnostic rather than a crash.
thread_local int theme_depth = 0;
constexpr int kMaxThemeDepth = 16;

struct ThemeDepthGuard {
  ThemeDepthGuard() { ++theme_depth; }
  ~ThemeDepthGuard() { --theme_depth; }
};

StyleDiagnostic document_error(const ResourceUri &document, std::string what) {
  StyleDiagnostic diagnostic;
  diagnostic.file = document.display();
  diagnostic.message = std::move(what);
  return diagnostic;
}

/// Hand-written recursive descent over the .vss subset.
///
/// Every failure path skips to the next plausible boundary and records a
/// diagnostic rather than aborting, so one bad rule in a hot-reloaded file
/// costs that rule and nothing else.
class Parser {
public:
  Parser(std::string_view source, ResourceUri document)
      : source_(source), document_(std::move(document)) {}

  std::vector<StyleDiagnostic> take_diagnostics() {
    return std::move(diagnostics_);
  }

  void parse_sheet(StyleSheet &sheet, StyleOrigin origin) {
    skip_trivia();
    while (!at_end()) {
      const bool parsed = peek() == '@' ? parse_sheet_directive(sheet)
                                        : parse_rule(sheet, origin);
      if (!parsed)
        recover_past_block();
      skip_trivia();
    }
  }

  void parse_theme(Theme &theme) {
    skip_trivia();
    while (!at_end()) {
      if (peek() == '@') {
        parse_theme_directive(theme);
      } else if (peek() == '$') {
        parse_token_binding(theme);
      } else {
        error("expected a $token binding or an @directive");
        recover_past_semicolon();
      }
      skip_trivia();
    }
  }

private:
  // -- Cursor ---------------------------------------------------------------

  bool at_end() const { return position_ >= source_.size(); }
  char peek() const { return at_end() ? '\0' : source_[position_]; }
  char peek(std::size_t ahead) const {
    return position_ + ahead >= source_.size() ? '\0'
                                               : source_[position_ + ahead];
  }
  char advance() { return at_end() ? '\0' : source_[position_++]; }

  bool consume(char expected) {
    if (peek() != expected)
      return false;
    ++position_;
    return true;
  }

  void skip_trivia() {
    for (;;) {
      while (!at_end() && is_space(peek()))
        ++position_;
      if (peek() == '/' && peek(1) == '*') {
        position_ += 2;
        while (!at_end() && !(peek() == '*' && peek(1) == '/'))
          ++position_;
        if (!at_end())
          position_ += 2;
        continue;
      }
      if (peek() == '/' && peek(1) == '/') {
        while (!at_end() && peek() != '\n')
          ++position_;
        continue;
      }
      return;
    }
  }

  std::string_view read_identifier() {
    const std::size_t start = position_;
    if (!is_ident_start(peek()))
      return {};
    while (!at_end() && is_ident_char(peek()))
      ++position_;
    return source_.substr(start, position_ - start);
  }

  void error(std::string message) {
    if (diagnostics_.size() >= 200)
      return;
    StyleDiagnostic diagnostic;
    diagnostic.file = document_.display();
    diagnostic.message = std::move(message);

    std::uint32_t line = 1;
    std::uint32_t column = 1;
    for (std::size_t i = 0; i < position_ && i < source_.size(); ++i) {
      if (source_[i] == '\n') {
        ++line;
        column = 1;
      } else {
        ++column;
      }
    }
    diagnostic.line = line;
    diagnostic.column = column;
    diagnostics_.push_back(std::move(diagnostic));
  }

  void recover_past_block() {
    int depth = 0;
    while (!at_end()) {
      const char c = advance();
      if (c == '{')
        ++depth;
      else if (c == '}') {
        if (--depth <= 0)
          return;
      }
    }
  }

  void recover_past_semicolon() {
    while (!at_end() && advance() != ';') {
    }
  }

  // -- Selectors ------------------------------------------------------------

  /// Returns false when the compound named something unknown; the caller then
  /// drops the whole rule but keeps reading the file.
  bool parse_compound(CompoundSelector &compound, bool &saw_anything) {
    saw_anything = false;
    for (;;) {
      const char c = peek();
      if (c == '*') {
        ++position_;
        saw_anything = true;
      } else if (c == '.') {
        ++position_;
        const std::string_view name = read_identifier();
        if (name.empty()) {
          error("expected a class name after '.'");
          return false;
        }
        compound.classes.push_back(AtomTable::instance().intern(name));
        saw_anything = true;
      } else if (c == '#') {
        ++position_;
        const std::string_view name = read_identifier();
        if (name.empty()) {
          error("expected an id after '#'");
          return false;
        }
        compound.id = AtomTable::instance().intern(name);
        saw_anything = true;
      } else if (c == ':' && peek(1) == ':') {
        position_ += 2;
        const std::string_view pseudo = read_identifier();
        if (pseudo != "part") {
          error("unknown pseudo-element '::" + std::string(pseudo) + "'");
          return false;
        }
        if (!consume('(')) {
          error("expected '(' after ::part");
          return false;
        }
        skip_trivia();
        const std::string_view name = read_identifier();
        skip_trivia();
        if (name.empty() || !consume(')')) {
          error("expected a part name in ::part(...)");
          return false;
        }
        compound.part = AtomTable::instance().intern(name);
        saw_anything = true;
      } else if (c == ':') {
        ++position_;
        const std::string_view pseudo = read_identifier();
        if (pseudo == "hover")
          compound.required_status |= StatusBits::kHovered;
        else if (pseudo == "active")
          compound.required_status |= StatusBits::kActive;
        else if (pseudo == "focus" || pseudo == "focused")
          compound.required_status |= StatusBits::kFocused;
        else {
          error("unknown pseudo-class ':" + std::string(pseudo) + "'");
          return false;
        }
        saw_anything = true;
      } else if (is_ident_start(c)) {
        if (compound.type.has_value()) {
          error("a compound selector may name only one widget type");
          return false;
        }
        const std::string_view name = read_identifier();
        const std::type_index *type = WidgetTypeRegistry::instance().find(name);
        if (!type) {
          // Usually a plugin that is not loaded. Skipping the rule is the
          // right call; failing the file is not.
          error("unknown widget type '" + std::string(name) + "'");
          return false;
        }
        compound.type = *type;
        saw_anything = true;
      } else {
        break;
      }
    }
    std::sort(compound.classes.begin(), compound.classes.end());
    compound.classes.erase(
        std::unique(compound.classes.begin(), compound.classes.end()),
        compound.classes.end());
    return true;
  }

  bool parse_selector(Selector &out) {
    std::vector<SelectorPart> parts;
    Combinator pending = Combinator::None;

    for (;;) {
      skip_trivia();
      CompoundSelector compound;
      bool saw_anything = false;
      if (!parse_compound(compound, saw_anything))
        return false;
      if (!saw_anything) {
        error("expected a selector");
        return false;
      }
      parts.push_back({std::move(compound), pending});

      // A run of whitespace is a descendant combinator only when another
      // compound follows it, so the lookahead has to happen before committing.
      const std::size_t before = position_;
      bool had_space = false;
      while (!at_end() && is_space(peek())) {
        had_space = true;
        ++position_;
      }
      skip_trivia();

      if (peek() == '>') {
        ++position_;
        pending = Combinator::Child;
        continue;
      }
      if (peek() == '{' || peek() == ',' || at_end()) {
        break;
      }
      if (had_space) {
        pending = Combinator::Descendant;
        continue;
      }
      position_ = before;
      break;
    }

    out = Selector(std::move(parts));
    return true;
  }

  // -- Declarations ---------------------------------------------------------

  /// Reads `value` up to the terminating ';' or '}', honouring nesting and
  /// quotes so a gradient or a quoted family name survives intact.
  std::string_view read_value() {
    const std::size_t start = position_;
    int depth = 0;
    char quote = '\0';
    while (!at_end()) {
      const char c = peek();
      if (quote != '\0') {
        if (c == quote)
          quote = '\0';
        ++position_;
        continue;
      }
      if (c == '"' || c == '\'') {
        quote = c;
        ++position_;
        continue;
      }
      if (c == '(')
        ++depth;
      else if (c == ')')
        depth = depth > 0 ? depth - 1 : 0;
      else if (depth == 0 && (c == ';' || c == '}'))
        break;
      ++position_;
    }
    return style_trim(source_.substr(start, position_ - start));
  }

  PropertyIndex lookup_property(std::string_view name,
                                const CompoundSelector &key) {
    const PropertyRegistry &registry = PropertyRegistry::instance();

    // Inside `input { ... }`, `caret-color` means `input.caret-color`. The
    // scoped form is tried first so a component's own property always wins
    // inside its own rule.
    if (key.type.has_value()) {
      const std::string_view scope =
          WidgetTypeRegistry::instance().name_of(*key.type);
      if (!scope.empty()) {
        std::string scoped(scope);
        scoped += '.';
        scoped += name;
        const PropertyIndex index = registry.find(scoped);
        if (index != kInvalidPropertyIndex)
          return index;
      }
    }
    return registry.find(name);
  }

  /// The rule's key widget name, which is what lets an unprefixed property
  /// name inside `input { ... }` mean `input.caret-color`.
  static std::string_view scope_of(const CompoundSelector &key) {
    if (!key.type.has_value())
      return {};
    return WidgetTypeRegistry::instance().name_of(*key.type);
  }

  bool parse_transition(StyleDeclaration &declaration, std::string_view value,
                        const CompoundSelector &key) {
    // A token binds to one property, and the shorthand writes five of
    // different types, so there is nothing sensible to resolve it against.
    if (!value.empty() && value.front() == '$') {
      error("'transition' is a shorthand and cannot take a theme token; bind "
            "a longhand such as 'transition-duration' instead");
      return false;
    }

    TransitionShorthand shorthand;
    if (!parse_transition_shorthand(value, scope_of(key), shorthand)) {
      error("could not read '" + std::string(value) +
            "' as a value for 'transition'");
      return false;
    }
    declaration.set<styles::TransitionProperty>(
        std::move(shorthand.properties));
    declaration.set<styles::TransitionDuration>(std::move(shorthand.durations));
    declaration.set<styles::TransitionDelay>(std::move(shorthand.delays));
    declaration.set<styles::TransitionTimingFunction>(
        std::move(shorthand.easings));
    declaration.set<styles::TransitionBehavior>(std::move(shorthand.behaviors));
    return true;
  }

  bool parse_transition_property(StyleDeclaration &declaration,
                                 std::string_view value,
                                 const CompoundSelector &key) {
    TransitionPropertyList list;
    if (!parse_transition_property_list(value, scope_of(key), list)) {
      error("could not read '" + std::string(value) +
            "' as a value for 'transition-property'");
      return false;
    }
    declaration.set<styles::TransitionProperty>(std::move(list));
    return true;
  }

  bool parse_margin(StyleDeclaration &declaration, std::string_view value) {
    if (!value.empty() && value.front() == '$') {
      const std::string_view token = style_trim(value.substr(1));
      if (token.empty()) {
        error("expected a token name after '$'");
        return false;
      }
      const DeclaredValue reference(
          TokenRef{TokenTable::instance().intern(token)});
      declaration.assign(styles::MarginTop::index(), reference);
      declaration.assign(styles::MarginRight::index(), reference);
      declaration.assign(styles::MarginBottom::index(), reference);
      declaration.assign(styles::MarginLeft::index(), reference);
      return true;
    }

    Spacing<MarginValue> margin;
    if (!parse_style_value(value, margin)) {
      error("could not read '" + std::string(value) +
            "' as a value for 'margin'");
      return false;
    }
    declaration.set<styles::MarginTop>(margin.top);
    declaration.set<styles::MarginRight>(margin.right);
    declaration.set<styles::MarginBottom>(margin.bottom);
    declaration.set<styles::MarginLeft>(margin.left);
    return true;
  }

  bool parse_declaration_block(StyleDeclaration &declaration,
                               const CompoundSelector &key) {
    if (!consume('{')) {
      error("expected '{'");
      return false;
    }

    for (;;) {
      skip_trivia();
      if (consume('}'))
        return true;
      if (at_end()) {
        error("unterminated rule");
        return false;
      }

      const std::size_t name_start = position_;
      while (!at_end() && (is_ident_char(peek()) || peek() == '.'))
        ++position_;
      const std::string_view name =
          source_.substr(name_start, position_ - name_start);
      if (name.empty()) {
        error("expected a property name");
        return false;
      }

      skip_trivia();
      if (!consume(':')) {
        error("expected ':' after property '" + std::string(name) + "'");
        return false;
      }
      skip_trivia();
      const std::string_view value = read_value();
      consume(';');

      // CSS shorthands are expanded before the cascade. Keeping one property
      // per edge makes `margin` and `margin-left` obey normal declaration
      // order, specificity and origin without special work in the resolver.
      if (name == "margin") {
        parse_margin(declaration, value);
        continue;
      }

      // `transition` expands into its five longhands, and both it and
      // `transition-property` resolve the property names they mention against
      // the rule's key widget, which only this level can see.
      if (name == "transition") {
        parse_transition(declaration, value, key);
        continue;
      }
      if (name == "transition-property" &&
          (value.empty() || value.front() != '$')) {
        parse_transition_property(declaration, value, key);
        continue;
      }

      const PropertyIndex property = lookup_property(name, key);
      if (property == kInvalidPropertyIndex) {
        error("unknown style property '" + std::string(name) + "'");
        continue;
      }

      if (!value.empty() && value.front() == '$') {
        const std::string_view token = style_trim(value.substr(1));
        if (token.empty()) {
          error("expected a token name after '$'");
          continue;
        }
        declaration.assign(
            property,
            DeclaredValue(TokenRef{TokenTable::instance().intern(token)}));
        continue;
      }

      const PropertyDescriptor &descriptor =
          PropertyRegistry::instance().describe(property);
      if (!descriptor.parse) {
        error("property '" + descriptor.name +
              "' cannot be written in a stylesheet");
        continue;
      }
      PropertyValue parsed;
      if (!descriptor.parse(value, parsed)) {
        error("could not read '" + std::string(value) + "' as a value for '" +
              descriptor.name + "'");
        continue;
      }
      declaration.assign(property, DeclaredValue(std::move(parsed)));
    }
  }

  bool parse_rule(StyleSheet &sheet, StyleOrigin origin) {
    std::vector<Selector> selectors;
    for (;;) {
      Selector selector;
      if (!parse_selector(selector))
        return false;
      selectors.push_back(std::move(selector));
      skip_trivia();
      if (consume(','))
        continue;
      break;
    }
    if (selectors.empty())
      return false;

    skip_trivia();
    StyleDeclaration declaration;
    if (!parse_declaration_block(declaration, selectors.front().key()))
      return false;

    // A selector list shares one declaration, exactly as in CSS. Each gets its
    // own rule so specificity stays per-selector.
    for (Selector &selector : selectors)
      sheet.add(std::move(selector), declaration, origin);
    return true;
  }

  bool parse_keyframe_offset(float &offset) {
    if (source_.substr(position_, 4) == "from" && !is_ident_char(peek(4))) {
      position_ += 4;
      offset = 0.0f;
      return true;
    }
    if (source_.substr(position_, 2) == "to" && !is_ident_char(peek(2))) {
      position_ += 2;
      offset = 1.0f;
      return true;
    }

    const std::size_t begin = position_;
    while (!at_end() && ((peek() >= '0' && peek() <= '9') || peek() == '.'))
      ++position_;
    if (begin == position_ || !consume('%'))
      return false;
    const std::string number(source_.substr(begin, position_ - begin - 1));
    char *end = nullptr;
    const float value = std::strtof(number.c_str(), &end);
    if (end == number.c_str() || *end != '\0' || value < 0.0f || value > 100.0f)
      return false;
    offset = value * 0.01f;
    return true;
  }

  bool parse_keyframes(StyleSheet &sheet) {
    skip_trivia();
    const std::string_view name = read_identifier();
    if (name.empty()) {
      error("expected a name after @keyframes");
      return false;
    }
    skip_trivia();
    if (!consume('{')) {
      error("expected '{' after @keyframes name");
      return false;
    }

    Keyframes result;
    result.name = std::string(name);
    CompoundSelector global_key;
    for (;;) {
      skip_trivia();
      if (consume('}'))
        break;
      if (at_end()) {
        error("unterminated @keyframes block");
        return false;
      }

      std::vector<float> offsets;
      for (;;) {
        float offset = 0.0f;
        if (!parse_keyframe_offset(offset)) {
          error("expected from, to, or a percentage in @keyframes");
          return false;
        }
        offsets.push_back(offset);
        skip_trivia();
        if (!consume(','))
          break;
        skip_trivia();
      }

      skip_trivia();
      StyleDeclaration declaration;
      if (!parse_declaration_block(declaration, global_key))
        return false;
      for (float offset : offsets)
        result.frames.push_back({offset, declaration});
    }

    std::stable_sort(result.frames.begin(), result.frames.end(),
                     [](const Keyframe &a, const Keyframe &b) {
                       return a.offset < b.offset;
                     });
    sheet.add_keyframes(std::move(result));
    return true;
  }

  bool parse_sheet_directive(StyleSheet &sheet) {
    consume('@');
    const std::string_view directive = read_identifier();
    if (directive == "keyframes")
      return parse_keyframes(sheet);
    if (directive == "font-face")
      return parse_font_face(sheet);
    error("unknown stylesheet directive '@" + std::string(directive) + "'");
    return false;
  }

  // -- @font-face -----------------------------------------------------------

  /// `local(<name>)` or `url(<reference>) [format(<hint>)]`, comma separated.
  ///
  /// A `url()` is resolved against the stylesheet that wrote it, here rather
  /// than at load time, because this is where the document is known. That is
  /// what keeps a sheet read from `res://` naming resources and a sheet read
  /// from disk naming the files beside it -- and what stops the first from
  /// reaching the second.
  bool parse_font_face_src(std::string_view text,
                           std::vector<FontFaceSource> &out) {
    for (const std::string_view entry : split_font_face_src(text)) {
      std::string_view rest = entry;
      std::string_view name;
      std::string_view body;
      if (!take_function(rest, name, body)) {
        error("expected local() or url() in @font-face src");
        return false;
      }

      FontFaceSource source;
      if (name == "local") {
        source.local = true;
        source.family = unquote(style_trim(body));
        if (source.family.empty()) {
          error("local() needs a family name");
          return false;
        }
      } else if (name == "url") {
        const std::string reference = unquote(style_trim(body));
        if (reference.empty()) {
          error("url() needs a reference");
          return false;
        }
        const ResourceResult<ResourceUri> uri = document_.resolve(reference);
        if (!uri) {
          error("'" + reference + "' is not a reference this stylesheet can "
                                  "make: " +
                std::string(to_string(uri.error())));
          return false;
        }
        source.uri = *uri;
      } else {
        error("unknown @font-face source '" + std::string(name) + "()'");
        return false;
      }

      // A trailing format() hint, which is recorded and otherwise ignored.
      std::string_view hint_name;
      std::string_view hint_body;
      if (take_function(rest, hint_name, hint_body)) {
        if (hint_name != "format") {
          error("expected format() after a @font-face source");
          return false;
        }
        source.format = unquote(style_trim(hint_body));
      }
      if (!style_trim(rest).empty()) {
        error("trailing text after a @font-face source");
        return false;
      }

      out.push_back(std::move(source));
    }
    return !out.empty();
  }

  bool parse_font_face(StyleSheet &sheet) {
    skip_trivia();
    if (!consume('{')) {
      error("expected '{' after @font-face");
      return false;
    }

    FontFaceRule rule;
    bool saw_src = false;
    bool src_readable = false;

    for (;;) {
      skip_trivia();
      if (consume('}'))
        break;
      if (at_end()) {
        error("unterminated @font-face block");
        return false;
      }

      const std::string_view descriptor = read_identifier();
      if (descriptor.empty()) {
        error("expected a descriptor inside @font-face");
        return false;
      }
      skip_trivia();
      if (!consume(':')) {
        error("expected ':' after '" + std::string(descriptor) + "'");
        return false;
      }
      skip_trivia();
      const std::string_view value = read_value();
      consume(';');

      // One bad descriptor costs that descriptor. The rule is only dropped if
      // what is missing afterwards makes it meaningless.
      if (descriptor == "font-family") {
        FontFamilyList families;
        if (!parse_style_value(value, families) || families.size() != 1)
          error("@font-face font-family names exactly one family");
        else
          rule.family = families.primary();
      } else if (descriptor == "src") {
        rule.sources.clear();
        saw_src = true;
        src_readable = parse_font_face_src(value, rule.sources);
      } else if (descriptor == "font-weight") {
        FontWeight weight = FontWeight::Normal;
        if (!parse_style_value(value, weight))
          error("'" + std::string(value) + "' is not a font weight");
        else
          rule.weight = weight;
      } else {
        error("unknown @font-face descriptor '" + std::string(descriptor) +
              "'");
      }
    }

    if (rule.family.empty()) {
      error("@font-face needs a font-family");
      return true;
    }
    if (!saw_src) {
      error("@font-face for '" + rule.family + "' needs a src");
      return true;
    }
    // A src that was written but could not be read has already said why; the
    // rule is still dropped, but saying "needs a src" on top of it would only
    // bury the reason.
    if (!src_readable || rule.sources.empty())
      return true;

    sheet.add_font_face(std::move(rule));
    return true;
  }

  // -- Themes ---------------------------------------------------------------

  void parse_token_binding(Theme &theme) {
    consume('$');
    const std::size_t name_start = position_;
    while (!at_end() && (is_ident_char(peek()) || peek() == '.'))
      ++position_;
    const std::string_view name =
        source_.substr(name_start, position_ - name_start);
    if (name.empty()) {
      error("expected a token name after '$'");
      recover_past_semicolon();
      return;
    }
    skip_trivia();
    if (!consume(':')) {
      error("expected ':' after token '" + std::string(name) + "'");
      recover_past_semicolon();
      return;
    }
    skip_trivia();
    const std::string_view value = read_value();
    consume(';');
    theme.set(name, std::string(value));
  }

  void parse_theme_directive(Theme &theme) {
    consume('@');
    const std::string_view directive = read_identifier();
    skip_trivia();
    const std::string_view argument = read_value();
    consume(';');

    std::string text;
    parse_style_value(argument, text);

    if (directive == "name") {
      theme.set_name(text);
      return;
    }
    if (directive == "base") {
      const ResourceResult<ResourceUri> path = document_.resolve(text);
      if (!path) {
        error("base theme '" + text +
              "' is not a reference this document can make: " +
              std::string(to_string(path.error())));
        return;
      }
      StyleParser::ThemeResult base = StyleParser::parse_theme_document(*path);
      for (StyleDiagnostic &diagnostic : base.diagnostics)
        diagnostics_.push_back(std::move(diagnostic));
      if (base.theme)
        theme.set_base(std::move(base.theme));
      else
        error("could not read base theme '" + text + "'");
      return;
    }
    error("unknown directive '@" + std::string(directive) + "'");
  }

  std::string_view source_;
  ResourceUri document_;
  std::size_t position_ = 0;
  std::vector<StyleDiagnostic> diagnostics_;
};

} // namespace

std::string StyleDiagnostic::to_string() const {
  std::ostringstream stream;
  if (!file.empty())
    stream << file << ':';
  stream << line << ':' << column << ": " << message;
  return stream.str();
}

StyleParser::Result StyleParser::parse(std::string_view source,
                                       ResourceUri document,
                                       StyleOrigin origin) {
  Result result;
  result.sheet = std::make_shared<StyleSheet>();
  Parser parser(source, std::move(document));
  parser.parse_sheet(*result.sheet, origin);
  result.diagnostics = parser.take_diagnostics();
  for (const StyleDiagnostic &diagnostic : result.diagnostics)
    result.sheet->add_diagnostic(diagnostic.to_string());
  return result;
}

StyleParser::Result StyleParser::parse(std::string_view source,
                                       std::string_view document,
                                       StyleOrigin origin) {
  return parse(source, document_uri(document), origin);
}

StyleParser::Result StyleParser::parse_document(const ResourceUri &document,
                                                StyleOrigin origin) {
  const ResourceResult<Blob> blob = Resources::global().open(document);
  if (!blob) {
    Result result;
    result.sheet = std::make_shared<StyleSheet>();
    result.diagnostics.push_back(document_error(
        document,
        "could not open stylesheet: " + std::string(to_string(blob.error()))));
    result.sheet->add_diagnostic(result.diagnostics.front().to_string());
    return result;
  }
  return parse(blob->text(), document, origin);
}

StyleParser::Result StyleParser::parse_file(const std::string &path,
                                            StyleOrigin origin) {
  return parse_document(document_uri(path), origin);
}

StyleParser::ThemeResult StyleParser::parse_theme(std::string_view source,
                                                  ResourceUri document) {
  ThemeResult result;
  result.theme = std::make_shared<Theme>();
  Parser parser(source, std::move(document));
  parser.parse_theme(*result.theme);
  result.diagnostics = parser.take_diagnostics();
  return result;
}

StyleParser::ThemeResult StyleParser::parse_theme(std::string_view source,
                                                  std::string_view document) {
  return parse_theme(source, document_uri(document));
}

StyleParser::ThemeResult
StyleParser::parse_theme_document(const ResourceUri &document) {
  if (theme_depth >= kMaxThemeDepth) {
    ThemeResult result;
    result.diagnostics.push_back(
        document_error(document, "@base chain is too deep to be anything but a "
                                 "cycle; stopping here"));
    return result;
  }
  const ThemeDepthGuard guard;

  const ResourceResult<Blob> blob = Resources::global().open(document);
  if (!blob) {
    ThemeResult result;
    result.diagnostics.push_back(document_error(
        document,
        "could not open theme: " + std::string(to_string(blob.error()))));
    return result;
  }
  return parse_theme(blob->text(), document);
}

StyleParser::ThemeResult
StyleParser::parse_theme_file(const std::string &path) {
  return parse_theme_document(document_uri(path));
}

} // namespace voidui
