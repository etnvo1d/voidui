#include "voidui/core/style/resolver.h"

#include "core/style/animator.h"

#include <algorithm>
#include <chrono>
#include <string>

namespace voidui {
namespace {

constexpr std::size_t kMaxDiagnostics = 64;

struct Applicable {
  StyleOrigin origin;
  Specificity specificity;
  std::uint32_t order;
};

bool less_in_cascade(const Applicable &a, const Applicable &b) {
  if (a.origin != b.origin)
    return a.origin < b.origin;
  if (a.specificity != b.specificity)
    return a.specificity < b.specificity;
  return a.order < b.order;
}

double monotonic_seconds() {
  return std::chrono::duration<double>(
             std::chrono::steady_clock::now().time_since_epoch())
      .count();
}

} // namespace

StyleResolver::StyleResolver()
    : animator_(std::make_unique<StyleAnimator>()),
      animation_time_(monotonic_seconds()) {}

StyleResolver::~StyleResolver() = default;

void StyleResolver::add_default_stylesheet(
    std::shared_ptr<const StyleSheet> sheet) {
  if (!sheet)
    return;
  for (const auto &registered : default_sheets_)
    if (registered.get() == sheet.get())
      return;
  default_sheets_.push_back(std::move(sheet));
  rebuild_stylesheet_();
}

void StyleResolver::set_stylesheet(std::shared_ptr<const StyleSheet> sheet) {
  user_sheet_ = std::move(sheet);
  rebuild_stylesheet_();
}

void StyleResolver::rebuild_stylesheet_() {
  ++animation_revision_;
  if (default_sheets_.empty()) {
    effective_sheet_ = user_sheet_;
    return;
  }

  auto combined = std::make_shared<StyleSheet>();
  for (const auto &defaults : default_sheets_)
    combined->append(*defaults, StyleOrigin::WidgetDefault);
  if (user_sheet_)
    combined->append(*user_sheet_);
  effective_sheet_ = std::move(combined);
}

void StyleResolver::set_theme(std::shared_ptr<const Theme> theme) {
  theme_ = std::move(theme);
  ++animation_revision_;
}

bool StyleResolver::resolve_declared(PropertyIndex property,
                                     const DeclaredValue &declared,
                                     PropertyValue &out) const {
  if (const auto *literal = std::get_if<PropertyValue>(&declared)) {
    out = *literal;
    return true;
  }
  if (!theme_)
    return false;
  const PropertyValue *resolved =
      theme_->resolve(std::get<TokenRef>(declared).token, property);
  if (!resolved)
    return false;
  out = *resolved;
  return true;
}

Invalidation StyleResolver::resolve_tree(StyleNode &root) {
  diagnostics_.clear();
  statistics_ = {};
  const Invalidation invalidation =
      resolve_recursive_(root, /*force=*/true, /*intern=*/true);
  cache_.collect();
  return invalidation;
}

Invalidation StyleResolver::resolve_subtree(StyleNode &node,
                                            bool force_subtree) {
  return resolve_recursive_(node, force_subtree, /*intern=*/true);
}

Invalidation StyleResolver::advance_animations(double now_seconds) {
  // A monotonic clamp protects transition progress if a host supplies a clock
  // with coarse rounding or briefly calls from two nested redraw paths.
  animation_time_ = std::max(animation_time_, now_seconds);
  return animator_->advance(*this, animation_time_);
}

bool StyleResolver::has_active_animations() const {
  return animator_->has_active();
}

void StyleResolver::forget_animations(StyleNode &subtree) {
  animator_->forget_subtree(subtree);
}

Invalidation StyleResolver::resolve_recursive_(StyleNode &node, bool force,
                                               bool intern) {
  ++statistics_.nodes_visited;
  const NodeResolution resolution = resolve_node_(node, intern);
  Invalidation invalidation = resolution.invalidation;

  // When the trigger was an inherited value rather than a structural or status
  // change, a node whose result did not move cannot change anything below it,
  // so the subtree is skipped whole.
  if (!force && !resolution.changed)
    return Invalidation::None;

  for (StyleNode *child : node.children)
    if (child)
      invalidation =
          max_invalidation(invalidation,
                           resolve_recursive_(*child, force, intern));
  return invalidation;
}

StyleResolver::NodeResolution StyleResolver::resolve_node_(StyleNode &node,
                                                           bool intern) {
  const PropertyRegistry &registry = PropertyRegistry::instance();

  ComputedStyle draft;

  // 1. Inheritance. Only the properties whose descriptor says so, which is a
  //    handful even in a large application.
  if (node.parent && node.parent->computed) {
    for (const auto &[property, value] : node.parent->computed->values())
      if (registry.describe(property).inherited)
        draft.set(property, value);
  }

  auto apply = [&](const StyleDeclaration &declaration) {
    for (const StyleDeclaration::Entry &entry : declaration.entries()) {
      if (const auto *literal = std::get_if<PropertyValue>(&entry.value)) {
        draft.set(entry.property, *literal);
        continue;
      }

      const TokenRef &reference = std::get<TokenRef>(entry.value);
      const PropertyValue *resolved =
          theme_ ? theme_->resolve(reference.token, entry.property) : nullptr;
      if (resolved) {
        draft.set(entry.property, *resolved);
        continue;
      }
      // An undefined or mistyped token leaves the property unset, so the
      // declared default shows through. A missing token must never be able to
      // blank out a window.
      if (diagnostics_.size() < kMaxDiagnostics) {
        diagnostics_.push_back(
            "unresolved theme token $" +
            std::string(TokenTable::instance().name(reference.token)) +
            " for property " + registry.describe(entry.property).name);
      }
    }
  };

  // 2. Component defaults and application rules, in cascade order. Component
  //    sheets are merged at WidgetDefault origin, so descendant and state
  //    selectors work without allowing defaults to outrank application VSS.
  if (!node.is_transparent && effective_sheet_) {
    effective_sheet_->collect_candidates(node, candidates_);
    statistics_.rules_considered += candidates_.size();

    matched_.clear();
    for (std::uint32_t index : candidates_) {
      const StyleRule &rule = effective_sheet_->rule(index);
      if (rule.selector.matches(node))
        matched_.push_back(index);
      else if (rule.selector.parts().size() > 1)
        ++statistics_.rules_rejected;
    }
    statistics_.rules_matched += matched_.size();

    const StyleSheet &sheet = *effective_sheet_;
    std::sort(matched_.begin(), matched_.end(),
              [&sheet](std::uint32_t a, std::uint32_t b) {
                const StyleRule &left = sheet.rule(a);
                const StyleRule &right = sheet.rule(b);
                return less_in_cascade(
                    {left.origin, left.selector.specificity(), left.order},
                    {right.origin, right.selector.specificity(), right.order});
              });

    for (std::uint32_t index : matched_)
      apply(effective_sheet_->rule(index).declaration);
  }

  // 3. Whatever was set on this instance alone.
  if (!node.is_transparent && node.inline_declaration)
    apply(*node.inline_declaration);

  draft.finalize();

  // Per-frame inheritance follows a sampled ancestor rather than a stable
  // cascaded value. Those intermediate styles are transient: interning them
  // would leave one expired cache bucket per frame of an infinite animation.
  std::shared_ptr<const ComputedStyle> target =
      intern ? cache_.intern(std::move(draft))
             : std::make_shared<const ComputedStyle>(std::move(draft));
  StyleAnimator::RetargetResult animated =
      animator_->retarget(*this, node, std::move(target), animation_time_);
  node.computed = std::move(animated.style);
  if (!animated.changed) {
    ++statistics_.styles_shared;
  } else {
    ++statistics_.nodes_recomputed;
  }
  return {animated.invalidation, animated.changed};
}

} // namespace voidui
