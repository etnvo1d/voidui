#include "voidui/core/style/theme.h"

#include <fstream>
#include <sstream>

#include "voidui/core/style.h"
#include "voidui/core/style/parser.h"

namespace voidui {
namespace {

std::uint64_t cache_key(TokenId token, PropertyIndex property) {
  return (static_cast<std::uint64_t>(token) << 32) |
         static_cast<std::uint64_t>(property);
}

} // namespace

void Theme::set(std::string_view token, std::string text) {
  Binding binding;
  binding.text = std::move(text);
  binding.typed = false;
  bindings_.insert_or_assign(TokenTable::instance().intern(token),
                             std::move(binding));
  invalidate_();
}

void Theme::set_property_value(std::string_view token, PropertyValue value) {
  Binding binding;
  binding.value = std::move(value);
  binding.typed = true;
  bindings_.insert_or_assign(TokenTable::instance().intern(token),
                             std::move(binding));
  invalidate_();
}

bool Theme::contains(std::string_view token) const {
  return lookup_(TokenTable::instance().find(token)) != nullptr;
}

void Theme::set_base(std::shared_ptr<const Theme> base) {
  base_ = std::move(base);
  invalidate_();
}

void Theme::invalidate_() {
  cache_.clear();
  failed_.clear();
  ++revision_;
}

const Theme::Binding *Theme::lookup_(TokenId token) const {
  if (token == kNoToken)
    return nullptr;
  for (const Theme *theme = this; theme; theme = theme->base_.get()) {
    auto it = theme->bindings_.find(token);
    if (it != theme->bindings_.end())
      return &it->second;
  }
  return nullptr;
}

const PropertyValue *Theme::resolve(TokenId token,
                                    PropertyIndex property) const {
  if (token == kNoToken || property == kInvalidPropertyIndex)
    return nullptr;

  const std::uint64_t key = cache_key(token, property);
  auto cached = cache_.find(key);
  if (cached != cache_.end())
    return &cached->second;
  if (failed_.find(key) != failed_.end())
    return nullptr;

  const Binding *binding = lookup_(token);
  if (!binding) {
    failed_.emplace(key, true);
    return nullptr;
  }

  const PropertyDescriptor &descriptor =
      PropertyRegistry::instance().describe(property);

  if (binding->typed) {
    // A typed binding only serves properties of the matching value type;
    // anything else falls back to the property default rather than reinterpret
    // the bytes.
    if (binding->value.type() != descriptor.value_type) {
      failed_.emplace(key, true);
      return nullptr;
    }
    auto inserted = cache_.emplace(key, binding->value);
    return &inserted.first->second;
  }

  // Text bindings are parsed against whichever property first asks for them,
  // which is what lets one token feed a Color property and a Brush property
  // without the theme author having to declare a type.
  if (!descriptor.parse) {
    failed_.emplace(key, true);
    return nullptr;
  }
  PropertyValue value;
  if (!descriptor.parse(binding->text, value)) {
    failed_.emplace(key, true);
    return nullptr;
  }
  auto inserted = cache_.emplace(key, std::move(value));
  return &inserted.first->second;
}

Color Theme::background(Color fallback) const {
  static const TokenId token = TokenTable::instance().intern("app.background");
  const PropertyValue *value = resolve(token, styles::Background::index());
  if (!value)
    return fallback;
  if (const Brush *brush = value->as<Brush>())
    if (const Color *color = std::get_if<Color>(brush))
      return *color;
  return fallback;
}

std::vector<std::string> Theme::token_names() const {
  std::vector<std::string> names;
  for (const Theme *theme = this; theme; theme = theme->base_.get())
    for (const auto &[token, binding] : theme->bindings_)
      names.emplace_back(TokenTable::instance().name(token));
  return names;
}

std::shared_ptr<Theme> Theme::from_source(std::string_view source,
                                          std::string *error) {
  StyleParser::ThemeResult result = StyleParser::parse_theme(source);
  if (error && !result.diagnostics.empty()) {
    std::ostringstream stream;
    for (const StyleDiagnostic &diagnostic : result.diagnostics)
      stream << diagnostic.to_string() << '\n';
    *error = stream.str();
  }
  return result.theme;
}

std::shared_ptr<Theme> Theme::from_file(const std::string &path,
                                        std::string *error) {
  StyleParser::ThemeResult result = StyleParser::parse_theme_file(path);
  if (error && !result.diagnostics.empty()) {
    std::ostringstream stream;
    for (const StyleDiagnostic &diagnostic : result.diagnostics)
      stream << diagnostic.to_string() << '\n';
    *error = stream.str();
  }
  return result.theme;
}

} // namespace voidui
