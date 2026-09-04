#include "core/style/animator.h"

#include "voidui/core/style.h"

#include <algorithm>
#include <cmath>
#include <limits>

namespace voidui {
namespace {

PropertyValue effective_value(const ComputedStyle &style,
                              PropertyIndex property) {
  if (const PropertyValue *value = style.find(property))
    return *value;
  return PropertyRegistry::instance().describe(property).make_default();
}

/// Walks two finalized styles in lockstep, reporting every property whose
/// entry differs. Both value arrays are sorted by property index, so this is
/// one linear merge -- the same shape the cascade itself uses.
template <class Visitor>
void visit_changed_properties(const ComputedStyle &before,
                              const ComputedStyle &after, Visitor &&visit) {
  const auto &left = before.values();
  const auto &right = after.values();
  std::size_t i = 0;
  std::size_t j = 0;

  while (i < left.size() || j < right.size()) {
    if (j == right.size() ||
        (i < left.size() && left[i].first < right[j].first)) {
      visit(left[i++].first);
    } else if (i == left.size() || right[j].first < left[i].first) {
      visit(right[j++].first);
    } else {
      if (left[i].second != right[j].second)
        visit(left[i].first);
      ++i;
      ++j;
    }
  }
}

float animation_progress(const StyleAnimator::AnimationRun &run, double now,
                         bool &active) {
  const double elapsed = now - run.start - run.spec.delay;
  if (elapsed <= 0.0) {
    active = true;
    return run.spec.easing(0.0f);
  }

  const double duration = std::max<double>(run.spec.duration, 1e-6);
  const bool infinite = !std::isfinite(run.spec.iterations);
  const double iterations = std::max<double>(run.spec.iterations, 0.0);
  const double total = duration * iterations;

  double cycle = 0.0;
  double local = 0.0;
  if (!infinite && elapsed >= total) {
    active = false;
    // At an integer iteration count the final sample is the end of the last
    // cycle, not the beginning of a nonexistent next one.
    const double whole = std::floor(iterations);
    const double fraction = iterations - whole;
    if (fraction > 1e-9) {
      cycle = whole;
      local = fraction;
    } else {
      cycle = std::max(whole - 1.0, 0.0);
      local = 1.0;
    }
  } else {
    active = true;
    const double position = elapsed / duration;
    cycle = std::floor(position);
    local = position - cycle;
  }

  if (run.spec.alternate &&
      (static_cast<std::uint64_t>(cycle) & std::uint64_t{1}) != 0)
    local = 1.0 - local;
  return run.spec.easing(static_cast<float>(local));
}

void sample_track(const StyleAnimator::Track &track, float progress,
                  ComputedStyle &out) {
  if (track.samples.empty())
    return;
  if (progress <= track.samples.front().offset) {
    out.set(track.property, track.samples.front().value);
    return;
  }
  if (progress >= track.samples.back().offset) {
    out.set(track.property, track.samples.back().value);
    return;
  }

  auto upper =
      std::upper_bound(track.samples.begin(), track.samples.end(), progress,
                       [](float value, const StyleAnimator::Sample &sample) {
                         return value < sample.offset;
                       });
  const StyleAnimator::Sample &right = *upper;
  const StyleAnimator::Sample &left = *(upper - 1);
  const float span = right.offset - left.offset;
  const float local = span > 0.0f ? (progress - left.offset) / span : 1.0f;

  PropertyValue value;
  if (track.interpolate &&
      track.interpolate(left.value, right.value, local, value))
    out.set(track.property, std::move(value));
  else
    out.set(track.property, local < 1.0f ? left.value : right.value);
}

/// Whether `property` may take part in a transition governed by `spec`, and
/// whether it does so discretely.
bool property_transitions(PropertyIndex property, const TransitionSpec &spec,
                          const PropertyDescriptor &descriptor,
                          bool &discrete) {
  if (style_is_timing_property(property))
    return false;
  discrete = descriptor.interpolate == nullptr;
  return !discrete || spec.behavior == TransitionBehavior::AllowDiscrete;
}

} // namespace

float StyleAnimator::TransitionRun::progress(double now) const {
  const double elapsed = now - start - delay;
  if (elapsed <= 0.0)
    return easing(0.0f);
  if (duration <= 0.0f)
    return easing(1.0f);
  return easing(static_cast<float>(std::min(elapsed / duration, 1.0)));
}

TransitionSettings style_transition_settings(const ComputedStyle &style) {
  TransitionSettings settings;
  settings.properties = style.get<styles::TransitionProperty>();
  settings.durations = style.get<styles::TransitionDuration>();
  settings.delays = style.get<styles::TransitionDelay>();
  settings.easings = style.get<styles::TransitionTimingFunction>();
  settings.behaviors = style.get<styles::TransitionBehavior>();
  return settings;
}

Invalidation style_difference_invalidation(const ComputedStyle &before,
                                           const ComputedStyle &after) {
  const PropertyRegistry &registry = PropertyRegistry::instance();
  Invalidation result = Invalidation::None;
  visit_changed_properties(before, after, [&](PropertyIndex property) {
    if (result == Invalidation::Layout)
      return;
    result = max_invalidation(result, registry.describe(property).invalidation);
  });
  return result;
}

void StyleAnimator::rebuild_animations_(StyleResolver &resolver,
                                        NodeState &state,
                                        const AnimationList &list, double now,
                                        bool preserve_start) {
  std::vector<AnimationRun> previous = std::move(state.animations);
  state.animations.clear();
  state.animation_list = list;
  state.animation_revision = resolver.animation_revision();
  if (list.empty())
    return;

  state.animations.reserve(list.size());
  for (std::size_t animation_index = 0; animation_index < list.size();
       ++animation_index) {
    const AnimationSpec &spec = list[animation_index];
    const Keyframes *source = resolver.find_keyframes(spec.name);
    if (!source)
      continue;

    AnimationRun run;
    run.spec = spec;
    run.start = now;
    if (preserve_start && animation_index < previous.size() &&
        previous[animation_index].spec.name == spec.name)
      run.start = previous[animation_index].start;

    run.tracks.reserve(source->tracks.size());
    for (const KeyframeTrack &source_track : source->tracks) {
      const PropertyDescriptor &descriptor =
          PropertyRegistry::instance().describe(source_track.property);
      if (!descriptor.interpolate ||
          style_is_timing_property(source_track.property))
        continue;

      Track track;
      track.property = source_track.property;
      track.invalidation = descriptor.invalidation;
      track.interpolate = descriptor.interpolate;
      track.inherited = descriptor.inherited;
      track.samples.reserve(source_track.values.size() + 2);
      for (const KeyframeValue &source_value : source_track.values) {
        PropertyValue value;
        if (resolver.resolve_declared(source_track.property, source_value.value,
                                      value))
          track.samples.push_back({source_value.offset, std::move(value)});
      }
      if (track.samples.empty())
        continue;

      // A missing endpoint is the property's cascaded value. This makes a
      // concise `to { ... }` animation begin from the node's normal style.
      const PropertyValue base = effective_value(*state.target, track.property);
      if (track.samples.front().offset > 0.0f)
        track.samples.insert(track.samples.begin(), {0.0f, base});
      if (track.samples.back().offset < 1.0f)
        track.samples.push_back({1.0f, base});

      const bool constant =
          std::all_of(std::next(track.samples.begin()), track.samples.end(),
                      [&](const Sample &sample) {
                        return sample.value == track.samples.front().value;
                      });
      if (constant)
        continue;
      run.tracks.push_back(std::move(track));
    }

    if (!run.tracks.empty())
      state.animations.push_back(std::move(run));
  }
}

void StyleAnimator::rebuild_transitions_(NodeState &state,
                                         const ComputedStyle &current,
                                         const TransitionSettings &settings,
                                         double now) {
  previous_runs_.clear();
  previous_runs_.swap(state.transitions);
  if (!state.target) {
    previous_runs_.clear();
    return;
  }

  const ComputedStyle &before = *state.target;
  const ComputedStyle &displayed = state.sampled ? *state.sampled : before;
  const PropertyRegistry &registry = PropertyRegistry::instance();

  // Candidates are every property whose cascaded value moved, plus every
  // property already in flight. A run has to be reconsidered even when its own
  // value did not move this time: the declaration governing it may have gone
  // away, which cancels it.
  candidates_.clear();
  visit_changed_properties(before, current, [&](PropertyIndex property) {
    candidates_.push_back(property);
  });
  for (const TransitionRun &run : previous_runs_)
    if (std::find(candidates_.begin(), candidates_.end(), run.property) ==
        candidates_.end())
      candidates_.push_back(run.property);

  state.transitions.reserve(candidates_.size());
  for (const PropertyIndex property : candidates_) {
    TransitionRun *running = nullptr;
    for (TransitionRun &run : previous_runs_)
      if (run.property == property) {
        running = &run;
        break;
      }

    TransitionSpec spec;
    if (!settings.resolve(property, spec))
      continue; // Not listed any more: a run in flight is cancelled.

    // Looked up once. describe() takes the registry's lock, and a candidate
    // list can be long when the declaration is `all`.
    const PropertyDescriptor &descriptor = registry.describe(property);
    bool discrete = false;
    if (!property_transitions(property, spec, descriptor, discrete))
      continue;

    const PropertyValue after = effective_value(current, property);
    const float combined = spec.combined_duration();

    if (!running) {
      if (combined <= 0.0f ||
          effective_value(before, property) == after)
        continue;
      PropertyValue from = effective_value(displayed, property);
      if (from == after)
        continue;

      TransitionRun run;
      run.property = property;
      run.invalidation = descriptor.invalidation;
      run.interpolate = descriptor.interpolate;
      run.inherited = descriptor.inherited;
      run.discrete = discrete;
      run.to = after;
      run.from = std::move(from);
      run.start = now;
      run.duration = spec.duration;
      run.delay = spec.delay;
      run.easing = spec.easing;
      state.transitions.push_back(std::move(run));
      continue;
    }

    // The destination has not moved, so the run keeps its start time and
    // finishes as originally scheduled. This is what makes an unrelated style
    // change -- a sibling class toggling, a theme token rebinding -- not
    // restart everything that happens to be in flight.
    if (running->to == after) {
      state.transitions.push_back(std::move(*running));
      continue;
    }

    // The value the run is showing right now, sampled from the run itself
    // rather than from the previous frame's output, so that an interruption
    // between frames does not visibly snap.
    PropertyValue shown;
    const float running_progress = running->progress(now);
    if (running->discrete)
      shown = running_progress < 0.5f ? running->from : running->to;
    else if (!running->interpolate ||
             !running->interpolate(running->from, running->to,
                                   running_progress, shown))
      shown = effective_value(displayed, property);

    if (shown == after || combined <= 0.0f)
      continue; // Already there, or no time to get there: cancel.

    TransitionRun run;
    run.property = property;
    run.invalidation = descriptor.invalidation;
    run.interpolate = descriptor.interpolate;
    run.inherited = descriptor.inherited;
    run.discrete = discrete;
    run.to = after;
    run.start = now;
    run.easing = spec.easing;

    if (running->reversing_start() == after) {
      // A reversal. The new run is given only the fraction of the duration
      // the old one had used up, so that flicking the pointer on and off a
      // button does not queue up two full-length animations.
      const float factor = std::clamp(
          std::abs(running_progress * running->reversing_factor +
                   (1.0f - running->reversing_factor)),
          0.0f, 1.0f);
      run.reversed_from = running->to;
      run.reversing_factor = factor;
      run.duration = spec.duration * factor;
      run.delay = spec.delay >= 0.0f ? spec.delay : spec.delay * factor;
    } else {
      run.duration = spec.duration;
      run.delay = spec.delay;
    }
    run.from = std::move(shown);
    state.transitions.push_back(std::move(run));
  }

  previous_runs_.clear();
}

bool StyleAnimator::sample_(NodeState &state, double now) {
  if (!state.sampled)
    state.sampled = std::make_shared<ComputedStyle>(*state.target);

  bool active = false;
  for (const TransitionRun &run : state.transitions) {
    const float progress = run.progress(now);
    if (run.discrete) {
      // css-transitions-1: a discrete property flips at the halfway point of
      // its eased progress rather than interpolating.
      state.sampled->set(run.property,
                         progress < 0.5f ? run.from : run.to);
    } else {
      PropertyValue value;
      if (run.interpolate && run.interpolate(run.from, run.to, progress, value))
        state.sampled->set(run.property, std::move(value));
    }
    if (now - run.start - run.delay < run.duration)
      active = true;
  }

  state.transitions.erase(
      std::remove_if(state.transitions.begin(), state.transitions.end(),
                     [now](const TransitionRun &run) {
                       return now - run.start - run.delay >= run.duration;
                     }),
      state.transitions.end());

  // CSS composition order: keyframe animations override transitions. Later
  // comma-separated animations win when two tracks write the same property.
  for (AnimationRun &run : state.animations) {
    if (run.finished)
      continue;
    bool run_active = false;
    const float progress = animation_progress(run, now, run_active);
    for (const Track &track : run.tracks)
      sample_track(track, progress, *state.sampled);
    run.finished = !run_active;
    active |= run_active;
  }

  state.sampled->finalize();
  return active;
}

void StyleAnimator::activate_(StyleNode &node, NodeState &state) {
  if (state.active_slot != kInactive)
    return;
  state.active_slot = active_nodes_.size();
  active_nodes_.push_back(&node);
}

void StyleAnimator::deactivate_(StyleNode &node, NodeState &state) {
  if (state.active_slot == kInactive)
    return;
  const std::size_t slot = state.active_slot;
  StyleNode *moved = active_nodes_.back();
  active_nodes_[slot] = moved;
  active_nodes_.pop_back();
  state.active_slot = kInactive;
  if (moved != &node)
    states_.find(moved)->second.active_slot = slot;
}

StyleAnimator::RetargetResult
StyleAnimator::retarget(StyleResolver &resolver, StyleNode &node,
                        std::shared_ptr<const ComputedStyle> target,
                        double now) {
  RetargetResult result;
  result.changed = !node.computed || *node.computed != *target;
  result.invalidation =
      node.computed ? style_difference_invalidation(*node.computed, *target)
                    : Invalidation::Layout;

  const AnimationList animation_list = target->get<styles::Animation>();
  const bool has_animation = !animation_list.empty();

  // Two array probes decide whether transitions are even possible here. A
  // node that names no duration and no delay cannot transition, whatever
  // transition-property says, because CSS needs a positive combined duration.
  const bool may_transition =
      !target->get<styles::TransitionDuration>().empty() ||
      !target->get<styles::TransitionDelay>().empty();

  auto found = states_.find(&node);
  if (found == states_.end() && !has_animation) {
    // Merely declaring a transition costs no per-node runtime state. A state
    // is created only on the edge where one of its target values changes.
    bool starts_transition = false;
    if (may_transition && node.computed) {
      const TransitionSettings settings = style_transition_settings(*target);
      if (!settings.idle()) {
        const PropertyRegistry &registry = PropertyRegistry::instance();
        visit_changed_properties(
            *node.computed, *target, [&](PropertyIndex property) {
              if (starts_transition)
                return;
              TransitionSpec spec;
              bool discrete = false;
              if (!settings.resolve(property, spec) ||
                  spec.combined_duration() <= 0.0f ||
                  !property_transitions(property, spec,
                                        registry.describe(property), discrete))
                return;
              starts_transition =
                  effective_value(*node.computed, property) !=
                  effective_value(*target, property);
            });
      }
    }
    if (!starts_transition) {
      result.style = std::move(target);
      return result;
    }
  }

  if (found == states_.end()) {
    NodeState state;
    state.target = node.computed ? node.computed : target;
    found = states_.emplace(&node, std::move(state)).first;
  }
  NodeState &state = found->second;
  const bool target_changed = !state.target || *state.target != *target;

  // Transition comparisons need the old cascaded target. The new target is
  // passed as `current` here and installed only after the runs are prepared.
  if (state.target)
    rebuild_transitions_(state, *target, style_transition_settings(*target),
                         now);
  state.target = target;

  const bool animation_changed =
      !style_value_equals(state.animation_list, animation_list);
  const bool animation_source_changed =
      state.animation_revision != resolver.animation_revision();
  if (animation_changed || animation_source_changed || target_changed)
    rebuild_animations_(resolver, state, animation_list, now,
                        /*preserve_start=*/!animation_changed);

  const bool has_runtime_values =
      !state.transitions.empty() || !state.animations.empty();
  if (has_runtime_values && state.sampled)
    // A cascade change is rare and may alter unrelated properties. Copy the
    // new base once here; ordinary animation frames overwrite only their own
    // tracks and avoid copying every PropertyValue (and its shared pointers).
    *state.sampled = *state.target;
  const bool active = has_runtime_values ? sample_(state, now) : false;
  result.style = has_runtime_values ? state.sampled : target;
  if (active)
    activate_(node, state);
  else
    deactivate_(node, state);

  if (state.transitions.empty() && state.animations.empty()) {
    result.style = target;
    deactivate_(node, state);
    states_.erase(found);
  }
  return result;
}

Invalidation StyleAnimator::advance(StyleResolver &resolver, double now) {
  Invalidation invalidation = Invalidation::None;
  inherited_changes_.clear();
  std::size_t slot = 0;
  while (slot < active_nodes_.size()) {
    StyleNode *node = active_nodes_[slot];
    NodeState &state = states_.find(node)->second;
    bool changes_inherited_value = false;

    // Every run cached its invalidation kind when it was built, so a frame
    // never takes the property registry's lock.
    for (const TransitionRun &run : state.transitions) {
      invalidation = max_invalidation(invalidation, run.invalidation);
      changes_inherited_value |= run.inherited;
    }
    for (const AnimationRun &run : state.animations)
      if (!run.finished) {
        for (const Track &track : run.tracks) {
          invalidation = max_invalidation(invalidation, track.invalidation);
          changes_inherited_value |= track.inherited;
        }
      }

    const bool active = sample_(state, now);
    if (changes_inherited_value && !node->children.empty())
      inherited_changes_.push_back({node, !active});

    if (active) {
      ++slot;
    } else {
      // A completed transition returns to the interned target. Finished
      // keyframes retain their final sample until their declaration changes.
      const bool holds_keyframe =
          std::any_of(state.animations.begin(), state.animations.end(),
                      [](const AnimationRun &run) { return run.finished; });
      if (!holds_keyframe)
        node->computed = state.target;
      deactivate_(*node, state);
      if (state.animations.empty() && state.transitions.empty())
        states_.erase(node);
    }
  }

  // A sampled inherited property is the computed value descendants inherit.
  // Retargeting the animated node alone leaves children frozen at the value
  // from the cascade pass until an unrelated status change happens to restyle
  // the subtree. Propagate after sampling all active nodes so descendants see
  // one coherent frame, and let the normal unchanged-node pruning stop at an
  // explicit override.
  //
  // A node under another node on the list is skipped: that ancestor's own pass
  // reaches it anyway. Sorting first turns the pairwise search that would need
  // into one binary search per ancestor link, which is what keeps a list whose
  // rows all fade at once from costing time quadratic in its length.
  std::sort(inherited_changes_.begin(), inherited_changes_.end(),
            [](const InheritedChange &a, const InheritedChange &b) {
              return a.node < b.node;
            });
  const auto on_list = [this](const StyleNode *candidate) {
    const auto found = std::lower_bound(
        inherited_changes_.begin(), inherited_changes_.end(), candidate,
        [](const InheritedChange &change, const StyleNode *key) {
          return change.node < key;
        });
    return found != inherited_changes_.end() && found->node == candidate;
  };

  for (const InheritedChange &change : inherited_changes_) {
    bool covered_by_ancestor = false;
    for (const StyleNode *parent = change.node->parent;
         parent && !covered_by_ancestor; parent = parent->parent)
      covered_by_ancestor = on_list(parent);
    if (covered_by_ancestor)
      continue;
    for (StyleNode *child : change.node->children)
      if (child)
        // Moving frames prune wherever a descendant's own result did not
        // move, which is how an explicit override stops the walk. The settling
        // frame descends the whole subtree instead, so that every node it
        // handed a private style gets an interned one back.
        invalidation = max_invalidation(
            invalidation, resolver.resolve_recursive_(*child, change.settled,
                                                      change.settled));
  }
  return invalidation;
}

void StyleAnimator::forget_node_(StyleNode &node) {
  auto found = states_.find(&node);
  if (found == states_.end())
    return;
  deactivate_(node, found->second);
  states_.erase(found);
}

void StyleAnimator::forget_subtree(StyleNode &root) {
  for (StyleNode *child : root.children)
    if (child)
      forget_subtree(*child);
  forget_node_(root);
}

} // namespace voidui
