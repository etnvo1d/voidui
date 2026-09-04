#pragma once

#include <cstddef>
#include <memory>
#include <unordered_map>
#include <vector>

#include "voidui/core/invalidation.h"
#include "voidui/core/style/animation.h"
#include "voidui/core/style/selector.h"

namespace voidui {

class ComputedStyle;
class StyleResolver;

/// Sparse animation storage owned by StyleResolver.
///
/// Nodes without `animation` or a live `transition` never enter the map. A
/// separate dense list contains only nodes that need the next frame, so frame
/// cost is proportional to visible motion rather than tree size.
///
/// Everything the per-frame path needs -- the interpolator, the invalidation
/// kind, the easing -- is copied into the runtime records when a run is built.
/// Sampling therefore never reaches PropertyRegistry, whose lookups take a
/// mutex, and never touches a shared_ptr control block.
class StyleAnimator {
public:
  struct RetargetResult {
    std::shared_ptr<const ComputedStyle> style;
    Invalidation invalidation = Invalidation::None;
    bool changed = false;
  };

  RetargetResult retarget(StyleResolver &resolver, StyleNode &node,
                          std::shared_ptr<const ComputedStyle> target,
                          double now);

  Invalidation advance(StyleResolver &resolver, double now);
  bool has_active() const { return !active_nodes_.empty(); }
  void forget_subtree(StyleNode &root);

  /// Signature of PropertyDescriptor::interpolate, cached per run.
  using Interpolator = bool (*)(const PropertyValue &from,
                                const PropertyValue &to, float progress,
                                PropertyValue &out);

  // Runtime records are exposed only inside the private source directory so
  // small sampling helpers can stay free functions without becoming friends.
  struct Sample {
    float offset = 0.0f;
    PropertyValue value;
  };

  struct Track {
    PropertyIndex property = kInvalidPropertyIndex;
    Invalidation invalidation = Invalidation::Paint;
    Interpolator interpolate = nullptr;
    bool inherited = false;
    std::vector<Sample> samples;
  };

  struct AnimationRun {
    AnimationSpec spec;
    std::vector<Track> tracks;
    double start = 0.0;
    bool finished = false;
  };

  /// One property in flight, holding everything css-transitions-1 needs to
  /// decide what happens when the style changes again mid-flight.
  struct TransitionRun {
    PropertyIndex property = kInvalidPropertyIndex;
    Invalidation invalidation = Invalidation::Paint;
    Interpolator interpolate = nullptr;

    PropertyValue from;
    PropertyValue to;
    bool inherited = false;

    /// Set only after a reversal, where css-transitions-1 wants the
    /// interrupted run's end value rather than this one's start value. Left
    /// empty otherwise so an ordinary run carries two values, not three --
    /// a PropertyValue larger than 32 bytes owns a heap block.
    PropertyValue reversed_from;

    /// The value this run would return to if it were reversed. Comparing it
    /// against a new target is how a hover-out is recognised as the reverse of
    /// the hover-in rather than as an unrelated change.
    const PropertyValue &reversing_start() const {
      return reversed_from.has_value() ? reversed_from : from;
    }

    /// How much of the declared duration this run was given, in (0, 1].
    /// Reversing at a third of the way through takes a third of the time, so
    /// that the value moves at the speed the author asked for.
    float reversing_factor = 1.0f;

    double start = 0.0;
    float duration = 0.0f;
    float delay = 0.0f;
    Easing easing{};

    /// Set when the property has no interpolator and the author wrote
    /// `transition-behavior: allow-discrete`. The value flips at half the
    /// eased progress instead of being interpolated.
    bool discrete = false;

    /// Eased progress at `now`, which is also the reversing input CSS wants.
    float progress(double now) const;
  };

  struct NodeState {
    std::shared_ptr<const ComputedStyle> target;
    std::shared_ptr<ComputedStyle> sampled;
    AnimationList animation_list;
    std::vector<AnimationRun> animations;
    std::vector<TransitionRun> transitions;
    std::uint64_t animation_revision = 0;
    std::size_t active_slot = static_cast<std::size_t>(-1);
  };

private:
  static constexpr std::size_t kInactive = static_cast<std::size_t>(-1);

  void rebuild_animations_(StyleResolver &resolver, NodeState &state,
                           const AnimationList &list, double now,
                           bool preserve_start);
  void rebuild_transitions_(NodeState &state, const ComputedStyle &current,
                            const TransitionSettings &settings, double now);
  bool sample_(NodeState &state, double now);
  void activate_(StyleNode &node, NodeState &state);
  void deactivate_(StyleNode &node, NodeState &state);
  void forget_node_(StyleNode &node);

  /// A node whose sampled frame moved an inherited value, so its descendants
  /// have to be brought along before the frame is painted.
  struct InheritedChange {
    StyleNode *node = nullptr;

    /// Set on the frame a node's last run ends. Its style has gone back to the
    /// interned cascaded one, so the subtree may go back to interned styles
    /// too instead of keeping the private copies the moving frames needed.
    bool settled = false;
  };

  std::unordered_map<StyleNode *, NodeState> states_;
  std::vector<StyleNode *> active_nodes_;

  // Reused across nodes so rebuilding a node's transitions allocates nothing.
  std::vector<TransitionRun> previous_runs_;
  std::vector<PropertyIndex> candidates_;
  std::vector<InheritedChange> inherited_changes_;
};

/// The five transition longhands of one node, read together.
TransitionSettings style_transition_settings(const ComputedStyle &style);

Invalidation style_difference_invalidation(const ComputedStyle &before,
                                           const ComputedStyle &after);

} // namespace voidui
