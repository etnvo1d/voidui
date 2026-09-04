#pragma once

#include <cstdint>
#include <functional>
#include <memory>
#include <string>
#include <vector>

#include "voidui/core/style/computed.h"
#include "voidui/core/style/stylesheet.h"
#include "voidui/core/style/theme.h"

namespace voidui {

class StyleAnimator;

/// Turns a style tree plus a stylesheet plus a theme into a ComputedStyle on
/// every node.
///
/// Resolution is not a per-frame cost: it runs when the tree is built, when a
/// status such as :hover changes, when the theme is swapped, and when a
/// stylesheet is hot-reloaded. Everything expensive is arranged around that --
/// candidate lookup goes through the sheet's index, ancestor walks are gated on
/// each node's Bloom filter, and results are interned so that the common case
/// of many identical siblings collapses to one allocation.
///
/// The one exception is an animating *inherited* property, whose sampled value
/// is what its descendants inherit. Those descendants are re-resolved every
/// frame the value moves, and their intermediate styles are kept out of the
/// interning cache; the subtree goes back to shared styles on the frame the
/// run settles. An animation of a non-inherited property costs nothing beyond
/// the node itself.
class StyleResolver {
public:
  StyleResolver();
  ~StyleResolver();

  StyleResolver(const StyleResolver &) = delete;
  StyleResolver &operator=(const StyleResolver &) = delete;

  /// Registers one component-owned stylesheet. Re-registering the same shared
  /// sheet is a no-op, so a tree with many instances keeps one set of rules.
  void add_default_stylesheet(std::shared_ptr<const StyleSheet> sheet);

  void set_stylesheet(std::shared_ptr<const StyleSheet> sheet);
  const std::shared_ptr<const StyleSheet> &stylesheet() const {
    return user_sheet_;
  }

  void set_theme(std::shared_ptr<const Theme> theme);
  const std::shared_ptr<const Theme> &theme() const { return theme_; }

  const Keyframes *find_keyframes(std::string_view name) const {
    return effective_sheet_ ? effective_sheet_->find_keyframes(name) : nullptr;
  }

  bool resolve_declared(PropertyIndex property, const DeclaredValue &declared,
                        PropertyValue &out) const;

  /// Full pass over the tree. Used on build, theme switch and hot reload.
  Invalidation resolve_tree(StyleNode &root);

  /// Re-resolves one node and, where it matters, what is below it.
  ///
  /// `force_subtree` must be set when the reason for re-resolving is a change
  /// to this node's own status or classes: a descendant selector such as
  /// `button:hover .label` can start matching further down even though this
  /// node's own computed style did not move. When the trigger is only a changed
  /// inherited value, the pass prunes wherever a node's result is unchanged.
  Invalidation resolve_subtree(StyleNode &node, bool force_subtree);

  /// Samples only currently active animations at an absolute monotonic time.
  /// The explicit clock makes the runtime deterministic in tests and lets the
  /// window use its existing frame scheduler without a timer object per node.
  Invalidation advance_animations(double now_seconds);
  bool has_active_animations() const;
  void forget_animations(StyleNode &subtree);

  double animation_time() const { return animation_time_; }
  std::uint64_t animation_revision() const { return animation_revision_; }

  /// Diagnostics accumulated during the last pass (unknown tokens, type
  /// mismatches). Cleared at the start of each full pass.
  const std::vector<std::string> &diagnostics() const { return diagnostics_; }

  struct Statistics {
    std::uint64_t nodes_visited = 0;
    std::uint64_t nodes_recomputed = 0;
    std::uint64_t rules_considered = 0;
    std::uint64_t rules_matched = 0;
    std::uint64_t rules_rejected = 0;
    std::uint64_t styles_shared = 0;
  };

  const Statistics &statistics() const { return statistics_; }
  void reset_statistics() { statistics_ = {}; }

  ComputedStyleCache &cache() { return cache_; }

private:
  friend class StyleAnimator;

  struct NodeResolution {
    Invalidation invalidation = Invalidation::None;
    bool changed = false;
  };

  NodeResolution resolve_node_(StyleNode &node, bool intern);

  Invalidation resolve_recursive_(StyleNode &node, bool force, bool intern);
  void rebuild_stylesheet_();

  std::vector<std::shared_ptr<const StyleSheet>> default_sheets_;
  std::shared_ptr<const StyleSheet> user_sheet_;
  std::shared_ptr<const StyleSheet> effective_sheet_;
  std::shared_ptr<const Theme> theme_;
  ComputedStyleCache cache_;
  std::unique_ptr<StyleAnimator> animator_;
  double animation_time_ = 0.0;
  std::uint64_t animation_revision_ = 1;

  // Reused across nodes so a full pass allocates nothing per node.
  std::vector<std::uint32_t> candidates_;
  std::vector<std::uint32_t> matched_;

  std::vector<std::string> diagnostics_;
  Statistics statistics_;
};

} // namespace voidui
