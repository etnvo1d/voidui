#include "voidui/paint/svg.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <cstdlib>
#include <unordered_map>

namespace voidui {

namespace {

constexpr float kPi = 3.14159265358979323846f;

/// How deep a `<g>` nest or an `href` chain may go before the document is
/// treated as malformed. Deep enough for anything a design tool emits, shallow
/// enough that a file which references itself in a cycle is a skipped subtree
/// rather than a blown stack.
constexpr int kMaxDepth = 64;

bool is_space(char c) {
  return c == ' ' || c == '\t' || c == '\n' || c == '\r' || c == '\f' ||
         c == '\v';
}

char lower(char c) {
  return (c >= 'A' && c <= 'Z') ? static_cast<char>(c - 'A' + 'a') : c;
}

bool keyword_is(std::string_view text, std::string_view keyword) {
  if (text.size() != keyword.size())
    return false;
  for (std::size_t i = 0; i < text.size(); ++i)
    if (lower(text[i]) != keyword[i])
      return false;
  return true;
}

std::string_view trim(std::string_view text) {
  std::size_t begin = 0;
  std::size_t end = text.size();
  while (begin < end && is_space(text[begin]))
    ++begin;
  while (end > begin && is_space(text[end - 1]))
    --end;
  return text.substr(begin, end - begin);
}

// -- Numbers -----------------------------------------------------------------

/// A scanner over one of SVG's numeric lists.
///
/// The grammar is looser than a whitespace split: `10,20`, `10 20`, `10-20` and
/// `.5.5` are all two numbers, because a sign or a second decimal point starts
/// the next one. Every list in the format -- path data, `viewBox`, `points`, a
/// transform's arguments -- is read through this, so they all agree about it.
class NumberScanner {
public:
  explicit NumberScanner(std::string_view text) : text_(text) {}

  void skip_separators() {
    while (at_ < text_.size() && (is_space(text_[at_]) || text_[at_] == ','))
      ++at_;
  }

  bool at_end() {
    skip_separators();
    return at_ >= text_.size();
  }

  /// Reads one number. Leaves the cursor where the number ended, so a caller
  /// that has run out of arguments can tell.
  bool next(float &out) {
    skip_separators();
    const std::size_t start = at_;

    if (at_ < text_.size() && (text_[at_] == '+' || text_[at_] == '-'))
      ++at_;

    bool digits = false;
    while (at_ < text_.size() && text_[at_] >= '0' && text_[at_] <= '9') {
      ++at_;
      digits = true;
    }
    if (at_ < text_.size() && text_[at_] == '.') {
      ++at_;
      while (at_ < text_.size() && text_[at_] >= '0' && text_[at_] <= '9') {
        ++at_;
        digits = true;
      }
    }
    if (!digits) {
      at_ = start;
      return false;
    }

    if (at_ < text_.size() && (text_[at_] == 'e' || text_[at_] == 'E')) {
      const std::size_t exponent = at_;
      ++at_;
      if (at_ < text_.size() && (text_[at_] == '+' || text_[at_] == '-'))
        ++at_;
      bool exponent_digits = false;
      while (at_ < text_.size() && text_[at_] >= '0' && text_[at_] <= '9') {
        ++at_;
        exponent_digits = true;
      }
      if (!exponent_digits)
        at_ = exponent;
    }

    const std::string buffer(text_.substr(start, at_ - start));
    out = std::strtof(buffer.c_str(), nullptr);
    return std::isfinite(out);
  }

  /// A flag argument in an elliptical arc, which the grammar allows to be
  /// written without a separator: `a1 1 0 011 1` is legal.
  bool next_flag(bool &out) {
    skip_separators();
    if (at_ >= text_.size())
      return false;
    if (text_[at_] == '0' || text_[at_] == '1') {
      out = text_[at_] == '1';
      ++at_;
      return true;
    }
    return false;
  }

  char peek_command() {
    skip_separators();
    if (at_ >= text_.size())
      return '\0';
    const char c = text_[at_];
    const bool letter = (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z');
    return letter ? c : '\0';
  }

  void take_command() { ++at_; }

private:
  std::string_view text_;
  std::size_t at_ = 0;
};

bool parse_number(std::string_view text, float &out) {
  NumberScanner scanner(trim(text));
  if (!scanner.next(out))
    return false;
  return scanner.at_end();
}

/// A `width`/`height` attribute. Percentages resolve against a viewport this
/// layer does not have, so they report "no intrinsic size on that axis" -- the
/// same answer as an absent attribute, and the answer that makes the view box
/// decide the shape.
bool parse_length(std::string_view text, float &out) {
  text = trim(text);
  if (text.empty() || text.back() == '%')
    return false;

  float scale = 1.0f;
  if (text.size() > 2) {
    const std::string_view unit = text.substr(text.size() - 2);
    if (keyword_is(unit, "px")) {
      text = text.substr(0, text.size() - 2);
    } else if (keyword_is(unit, "pt")) {
      scale = 96.0f / 72.0f;
      text = text.substr(0, text.size() - 2);
    } else if (keyword_is(unit, "pc")) {
      scale = 16.0f;
      text = text.substr(0, text.size() - 2);
    } else if (keyword_is(unit, "in")) {
      scale = 96.0f;
      text = text.substr(0, text.size() - 2);
    } else if (keyword_is(unit, "mm")) {
      scale = 96.0f / 25.4f;
      text = text.substr(0, text.size() - 2);
    } else if (keyword_is(unit, "cm")) {
      scale = 96.0f / 2.54f;
      text = text.substr(0, text.size() - 2);
    }
  }

  if (!parse_number(text, out))
    return false;
  out *= scale;
  return true;
}

// -- A minimal XML tree ------------------------------------------------------

/// One element. Names and attribute values are views into the source, which
/// outlives the tree: an SVG has no escapes outside text content, and this
/// parser draws no text, so nothing here has to be decoded or copied.
struct XmlElement {
  std::string_view name;
  std::vector<std::pair<std::string_view, std::string_view>> attributes;
  std::vector<std::uint32_t> children;
};

/// Namespace prefixes are dropped rather than tracked. A document that binds
/// `svg:` to something other than the SVG namespace is not a document anyone
/// writes, and paying for a prefix table on every element to catch it would be
/// the wrong trade.
std::string_view strip_prefix(std::string_view name) {
  const std::size_t colon = name.find(':');
  return colon == std::string_view::npos ? name : name.substr(colon + 1);
}

class XmlParser {
public:
  explicit XmlParser(std::string_view source) : source_(source) {}

  /// Returns the index of the root element, or `kNoSvgIndex`.
  std::uint32_t parse() {
    std::vector<std::uint32_t> stack;
    std::uint32_t root = kNoSvgIndex;

    while (at_ < source_.size()) {
      if (source_[at_] != '<') {
        ++at_;
        continue;
      }

      if (skip_non_element())
        continue;

      ++at_; // '<'
      if (at_ < source_.size() && source_[at_] == '/') {
        ++at_;
        while (at_ < source_.size() && source_[at_] != '>')
          ++at_;
        if (at_ < source_.size())
          ++at_;
        if (!stack.empty())
          stack.pop_back();
        continue;
      }

      const std::size_t name_start = at_;
      while (at_ < source_.size() && !is_space(source_[at_]) &&
             source_[at_] != '>' && source_[at_] != '/')
        ++at_;

      XmlElement element;
      element.name =
          strip_prefix(source_.substr(name_start, at_ - name_start));

      bool self_closing = false;
      read_attributes(element, self_closing);

      const std::uint32_t index = static_cast<std::uint32_t>(elements_.size());
      elements_.push_back(std::move(element));

      if (stack.empty()) {
        if (root == kNoSvgIndex)
          root = index;
      } else {
        elements_[stack.back()].children.push_back(index);
      }

      // A stack deeper than this is either generated garbage or an attack; the
      // subtree is dropped and its siblings still draw.
      if (!self_closing && stack.size() < kMaxDepth)
        stack.push_back(index);
      else if (!self_closing)
        skip_to_close(elements_[index].name);
    }

    return root;
  }

  std::vector<XmlElement> take() { return std::move(elements_); }

private:
  /// Comments, declarations, doctypes and CDATA. True when one was consumed.
  bool skip_non_element() {
    if (source_.compare(at_, 4, "<!--") == 0) {
      const std::size_t end = source_.find("-->", at_ + 4);
      at_ = end == std::string_view::npos ? source_.size() : end + 3;
      return true;
    }
    if (source_.compare(at_, 9, "<![CDATA[") == 0) {
      const std::size_t end = source_.find("]]>", at_ + 9);
      at_ = end == std::string_view::npos ? source_.size() : end + 3;
      return true;
    }
    if (source_.compare(at_, 2, "<?") == 0) {
      const std::size_t end = source_.find("?>", at_ + 2);
      at_ = end == std::string_view::npos ? source_.size() : end + 2;
      return true;
    }
    if (source_.compare(at_, 2, "<!") == 0) {
      // A doctype may carry an internal subset in brackets, which may itself
      // contain '>'.
      std::size_t cursor = at_ + 2;
      int brackets = 0;
      while (cursor < source_.size()) {
        const char c = source_[cursor];
        if (c == '[')
          ++brackets;
        else if (c == ']')
          brackets = brackets > 0 ? brackets - 1 : 0;
        else if (c == '>' && brackets == 0)
          break;
        ++cursor;
      }
      at_ = cursor < source_.size() ? cursor + 1 : source_.size();
      return true;
    }
    return false;
  }

  void read_attributes(XmlElement &element, bool &self_closing) {
    for (;;) {
      while (at_ < source_.size() && is_space(source_[at_]))
        ++at_;
      if (at_ >= source_.size())
        return;

      if (source_[at_] == '/') {
        self_closing = true;
        ++at_;
        continue;
      }
      if (source_[at_] == '>') {
        ++at_;
        return;
      }

      const std::size_t name_start = at_;
      while (at_ < source_.size() && !is_space(source_[at_]) &&
             source_[at_] != '=' && source_[at_] != '>' && source_[at_] != '/')
        ++at_;
      const std::string_view name =
          source_.substr(name_start, at_ - name_start);

      while (at_ < source_.size() && is_space(source_[at_]))
        ++at_;
      if (at_ >= source_.size() || source_[at_] != '=') {
        if (!name.empty())
          element.attributes.emplace_back(name, std::string_view());
        continue;
      }
      ++at_;
      while (at_ < source_.size() && is_space(source_[at_]))
        ++at_;
      if (at_ >= source_.size())
        return;

      const char quote = source_[at_];
      if (quote != '"' && quote != '\'')
        continue;
      ++at_;
      const std::size_t value_start = at_;
      while (at_ < source_.size() && source_[at_] != quote)
        ++at_;
      const std::string_view value =
          source_.substr(value_start, at_ - value_start);
      if (at_ < source_.size())
        ++at_;

      if (!name.empty())
        element.attributes.emplace_back(name, value);
    }
  }

  void skip_to_close(std::string_view name) {
    // Best effort for a subtree that was too deep to keep: run to the matching
    // end tag at this nesting level.
    int depth = 1;
    while (at_ < source_.size() && depth > 0) {
      const std::size_t next = source_.find('<', at_);
      if (next == std::string_view::npos)
        break;
      at_ = next;
      if (source_.compare(at_, 2, "</") == 0) {
        at_ += 2;
        if (source_.compare(at_, name.size(), name) == 0)
          --depth;
      } else {
        ++at_;
        if (source_.compare(at_, name.size(), name) == 0)
          ++depth;
      }
    }
  }

  std::string_view source_;
  std::size_t at_ = 0;
  std::vector<XmlElement> elements_;
};

std::string_view attribute(const XmlElement &element, std::string_view name) {
  for (const auto &entry : element.attributes)
    if (entry.first == name)
      return entry.second;
  return {};
}

bool has_attribute(const XmlElement &element, std::string_view name) {
  for (const auto &entry : element.attributes)
    if (entry.first == name)
      return true;
  return false;
}

/// The fragment a `url(#id)` names, and nothing else.
///
/// Deliberately not tolerant of a bare `#id`: a paint value's grammar has no
/// such form, and `fill="#ff0000"` is a colour. Reading one as the other paints
/// every hex fill in the file black, which is exactly what a looser version of
/// this function did.
std::string_view url_fragment(std::string_view text) {
  text = trim(text);
  if (text.size() < 5 || !keyword_is(text.substr(0, 4), "url("))
    return {};
  if (text.back() != ')')
    return {};

  text = trim(text.substr(4, text.size() - 5));
  if (text.size() >= 2 && (text.front() == '"' || text.front() == '\'') &&
      text.back() == text.front())
    text = text.substr(1, text.size() - 2);

  if (!text.empty() && text.front() == '#')
    return text.substr(1);
  return {};
}

/// The fragment an `href` or `xlink:href` names, which is written bare.
std::string_view href_fragment(std::string_view text) {
  text = trim(text);
  if (!text.empty() && text.front() == '#')
    return text.substr(1);
  return {};
}

// -- Transform lists ---------------------------------------------------------

Transform parse_transform_list(std::string_view text) {
  Transform result;
  std::size_t at = 0;

  while (at < text.size()) {
    while (at < text.size() && (is_space(text[at]) || text[at] == ','))
      ++at;
    const std::size_t name_start = at;
    while (at < text.size() && text[at] != '(' && !is_space(text[at]))
      ++at;
    const std::string_view name = text.substr(name_start, at - name_start);
    while (at < text.size() && is_space(text[at]))
      ++at;
    if (at >= text.size() || text[at] != '(')
      break;
    const std::size_t body_start = ++at;
    while (at < text.size() && text[at] != ')')
      ++at;
    const std::string_view body = text.substr(body_start, at - body_start);
    if (at < text.size())
      ++at;

    std::array<float, 6> values{};
    std::size_t count = 0;
    NumberScanner scanner(body);
    while (count < values.size() && scanner.next(values[count]))
      ++count;

    Transform item;
    if (keyword_is(name, "matrix") && count == 6) {
      item = Transform(values[0], values[1], values[2], values[3], values[4],
                       values[5]);
    } else if (keyword_is(name, "translate") && count >= 1) {
      item = Transform::translate(values[0], count > 1 ? values[1] : 0.0f);
    } else if (keyword_is(name, "scale") && count >= 1) {
      item = Transform::scale(values[0], count > 1 ? values[1] : values[0]);
    } else if (keyword_is(name, "rotate") && count >= 1) {
      const Transform rotation = Transform::rotate(values[0] * kPi / 180.0f);
      if (count >= 3) {
        // Around a point: move it to the origin, turn, and put it back.
        item = Transform::translate(values[1], values[2])
                   .concat(rotation)
                   .concat(Transform::translate(-values[1], -values[2]));
      } else {
        item = rotation;
      }
    } else if (keyword_is(name, "skewx") && count >= 1) {
      item = Transform(1.0f, 0.0f, std::tan(values[0] * kPi / 180.0f), 1.0f,
                       0.0f, 0.0f);
    } else if (keyword_is(name, "skewy") && count >= 1) {
      item = Transform(1.0f, std::tan(values[0] * kPi / 180.0f), 0.0f, 1.0f,
                       0.0f, 0.0f);
    } else {
      continue;
    }

    result = result.concat(item);
  }

  return result;
}

// -- Path data ---------------------------------------------------------------

/// One elliptical arc, as up to four cubic segments.
///
/// The endpoint parameterisation SVG writes has to become the centre one before
/// anything can be drawn from it; the conversion is the one in the
/// specification's implementation notes, including its out-of-range radius
/// correction, which real files do rely on.
void arc_to_cubics(Path &path, Point<float> from, float rx, float ry,
                   float rotation_degrees, bool large_arc, bool sweep,
                   Point<float> to) {
  if (rx == 0.0f || ry == 0.0f) {
    path.line_to(to);
    return;
  }

  rx = std::abs(rx);
  ry = std::abs(ry);

  const float angle = rotation_degrees * kPi / 180.0f;
  const float cos_a = std::cos(angle);
  const float sin_a = std::sin(angle);

  const float dx2 = (from.x - to.x) * 0.5f;
  const float dy2 = (from.y - to.y) * 0.5f;
  const float x1 = cos_a * dx2 + sin_a * dy2;
  const float y1 = -sin_a * dx2 + cos_a * dy2;

  // Radii too small to span the chord are scaled up until they exactly do.
  const float lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
  if (lambda > 1.0f) {
    const float scale = std::sqrt(lambda);
    rx *= scale;
    ry *= scale;
  }

  const float rx2 = rx * rx;
  const float ry2 = ry * ry;
  const float denominator = rx2 * y1 * y1 + ry2 * x1 * x1;
  float factor = 0.0f;
  if (denominator > 0.0f) {
    const float numerator = rx2 * ry2 - denominator;
    factor = std::sqrt(std::max(numerator / denominator, 0.0f));
  }
  if (large_arc == sweep)
    factor = -factor;

  const float cx1 = factor * rx * y1 / ry;
  const float cy1 = -factor * ry * x1 / rx;

  const float cx = cos_a * cx1 - sin_a * cy1 + (from.x + to.x) * 0.5f;
  const float cy = sin_a * cx1 + cos_a * cy1 + (from.y + to.y) * 0.5f;

  const auto angle_of = [](float ux, float uy, float vx, float vy) {
    const float dot = ux * vx + uy * vy;
    const float length =
        std::sqrt((ux * ux + uy * uy) * (vx * vx + vy * vy));
    float result =
        length > 0.0f ? std::acos(std::clamp(dot / length, -1.0f, 1.0f)) : 0.0f;
    if (ux * vy - uy * vx < 0.0f)
      result = -result;
    return result;
  };

  const float start_x = (x1 - cx1) / rx;
  const float start_y = (y1 - cy1) / ry;
  const float end_x = (-x1 - cx1) / rx;
  const float end_y = (-y1 - cy1) / ry;

  const float theta = angle_of(1.0f, 0.0f, start_x, start_y);
  float sweep_angle = angle_of(start_x, start_y, end_x, end_y);
  if (!sweep && sweep_angle > 0.0f)
    sweep_angle -= 2.0f * kPi;
  else if (sweep && sweep_angle < 0.0f)
    sweep_angle += 2.0f * kPi;

  // A cubic approximates a circular arc well below a quarter turn and poorly
  // above it, so the sweep is split until every piece is under one.
  const int segments =
      std::max(1, static_cast<int>(std::ceil(std::abs(sweep_angle) /
                                             (kPi * 0.5f) - 1e-4f)));
  const float delta = sweep_angle / static_cast<float>(segments);
  const float alpha =
      4.0f / 3.0f * std::tan(delta * 0.25f);

  float current = theta;
  for (int i = 0; i < segments; ++i) {
    const float next = current + delta;

    const float cos0 = std::cos(current);
    const float sin0 = std::sin(current);
    const float cos1 = std::cos(next);
    const float sin1 = std::sin(next);

    const auto to_user = [&](float ex, float ey) {
      return Point<float>(cx + cos_a * (rx * ex) - sin_a * (ry * ey),
                          cy + sin_a * (rx * ex) + cos_a * (ry * ey));
    };

    const Point<float> control1 = to_user(cos0 - alpha * sin0,
                                          sin0 + alpha * cos0);
    const Point<float> control2 = to_user(cos1 + alpha * sin1,
                                          sin1 - alpha * cos1);
    const Point<float> end =
        (i == segments - 1) ? to : to_user(cos1, sin1);

    path.cubic_to(control1, control2, end);
    current = next;
  }
}

bool parse_path_data(std::string_view data, Path &out) {
  NumberScanner scanner(data);

  // Two floats per point, and roughly one verb per three numbers, is close
  // enough to size both arenas in one go.
  out.reserve(data.size() / 6 + 4, data.size() / 4 + 4);

  Point<float> current(0.0f, 0.0f);
  Point<float> start(0.0f, 0.0f);
  Point<float> last_cubic_control(0.0f, 0.0f);
  Point<float> last_quad_control(0.0f, 0.0f);
  char previous = '\0';
  char command = '\0';
  bool any = false;

  // Set by a closepath. A drawing command that follows one without an
  // intervening moveto starts a new subpath at the closed one's first
  // point, which is what the grammar says and what a trailing `Z l 4 0`
  // in a real file means; without it the line would be appended to the
  // contour that was just closed.
  bool pending_move = false;

  const auto relative = [&](float x, float y, bool is_relative) {
    return is_relative ? Point<float>(current.x + x, current.y + y)
                       : Point<float>(x, y);
  };

  for (;;) {
    const char next = scanner.peek_command();
    if (next != '\0') {
      scanner.take_command();
      command = next;
    } else if (scanner.at_end()) {
      break;
    } else if (command == '\0') {
      return any;
    } else if (command == 'M') {
      // A repeated moveto argument is an implicit lineto, and the same for a
      // relative one. Anything else simply repeats.
      command = 'L';
    } else if (command == 'm') {
      command = 'l';
    }

    const bool is_relative = command >= 'a' && command <= 'z';
    const char kind = static_cast<char>(lower(command));
    if (pending_move && kind != 'm' && kind != 'z') {
      out.move_to(current);
      start = current;
      pending_move = false;
    }
    float a = 0.0f;
    float b = 0.0f;
    float c = 0.0f;
    float d = 0.0f;
    float e = 0.0f;
    float f = 0.0f;

    switch (kind) {
    case 'm':
      if (!scanner.next(a) || !scanner.next(b))
        return any;
      current = relative(a, b, is_relative);
      start = current;
      out.move_to(current);
      pending_move = false;
      any = true;
      break;

    case 'l':
      if (!scanner.next(a) || !scanner.next(b))
        return any;
      current = relative(a, b, is_relative);
      out.line_to(current);
      break;

    case 'h':
      if (!scanner.next(a))
        return any;
      current = Point<float>(is_relative ? current.x + a : a, current.y);
      out.line_to(current);
      break;

    case 'v':
      if (!scanner.next(a))
        return any;
      current = Point<float>(current.x, is_relative ? current.y + a : a);
      out.line_to(current);
      break;

    case 'c': {
      if (!scanner.next(a) || !scanner.next(b) || !scanner.next(c) ||
          !scanner.next(d) || !scanner.next(e) || !scanner.next(f))
        return any;
      const Point<float> control1 = relative(a, b, is_relative);
      const Point<float> control2 = relative(c, d, is_relative);
      current = relative(e, f, is_relative);
      out.cubic_to(control1, control2, current);
      last_cubic_control = control2;
      break;
    }

    case 's': {
      if (!scanner.next(a) || !scanner.next(b) || !scanner.next(c) ||
          !scanner.next(d))
        return any;
      const char before = static_cast<char>(lower(previous));
      const Point<float> control1 =
          (before == 'c' || before == 's')
              ? Point<float>(2.0f * current.x - last_cubic_control.x,
                             2.0f * current.y - last_cubic_control.y)
              : current;
      const Point<float> control2 = relative(a, b, is_relative);
      current = relative(c, d, is_relative);
      out.cubic_to(control1, control2, current);
      last_cubic_control = control2;
      break;
    }

    case 'q': {
      if (!scanner.next(a) || !scanner.next(b) || !scanner.next(c) ||
          !scanner.next(d))
        return any;
      const Point<float> control = relative(a, b, is_relative);
      current = relative(c, d, is_relative);
      out.quad_to(control, current);
      last_quad_control = control;
      break;
    }

    case 't': {
      if (!scanner.next(a) || !scanner.next(b))
        return any;
      const char before = static_cast<char>(lower(previous));
      const Point<float> control =
          (before == 'q' || before == 't')
              ? Point<float>(2.0f * current.x - last_quad_control.x,
                             2.0f * current.y - last_quad_control.y)
              : current;
      current = relative(a, b, is_relative);
      out.quad_to(control, current);
      last_quad_control = control;
      break;
    }

    case 'a': {
      bool large_arc = false;
      bool sweep = false;
      if (!scanner.next(a) || !scanner.next(b) || !scanner.next(c) ||
          !scanner.next_flag(large_arc) || !scanner.next_flag(sweep) ||
          !scanner.next(e) || !scanner.next(f))
        return any;
      const Point<float> end = relative(e, f, is_relative);
      arc_to_cubics(out, current, a, b, c, large_arc, sweep, end);
      current = end;
      break;
    }

    case 'z':
      out.close();
      current = start;
      pending_move = true;
      break;

    default:
      return any;
    }

    previous = command;
  }

  return any;
}

// -- Shape geometry ----------------------------------------------------------

float attribute_number(const XmlElement &element, std::string_view name,
                       float fallback) {
  float value = fallback;
  const std::string_view text = attribute(element, name);
  if (text.empty() || !parse_number(text, value))
    return fallback;
  return value;
}

bool build_rect(const XmlElement &element, Path &out) {
  const float x = attribute_number(element, "x", 0.0f);
  const float y = attribute_number(element, "y", 0.0f);
  const float width = attribute_number(element, "width", 0.0f);
  const float height = attribute_number(element, "height", 0.0f);
  if (!(width > 0.0f) || !(height > 0.0f))
    return false;

  // One radius given implies the other, and both are clamped to half the side.
  float rx = attribute_number(element, "rx", -1.0f);
  float ry = attribute_number(element, "ry", -1.0f);
  if (rx < 0.0f)
    rx = ry;
  if (ry < 0.0f)
    ry = rx;
  rx = std::clamp(rx < 0.0f ? 0.0f : rx, 0.0f, width * 0.5f);
  ry = std::clamp(ry < 0.0f ? 0.0f : ry, 0.0f, height * 0.5f);

  const Rect<float> bounds(x, y, width, height);
  if (rx <= 0.0f || ry <= 0.0f) {
    out = Path::rect(bounds);
    return true;
  }

  // Radius carries one scalar per corner, so an elliptical corner cannot take
  // the analytic route and is written out as arcs.
  if (std::abs(rx - ry) < 1e-4f) {
    out = Path::rounded_rect(bounds, Radius(rx));
    return true;
  }

  constexpr float k = 0.5522847498307933f;
  const float x1 = x + width;
  const float y1 = y + height;
  out.move_to(Point<float>(x + rx, y));
  out.line_to(Point<float>(x1 - rx, y));
  out.cubic_to(Point<float>(x1 - rx + rx * k, y),
               Point<float>(x1, y + ry - ry * k), Point<float>(x1, y + ry));
  out.line_to(Point<float>(x1, y1 - ry));
  out.cubic_to(Point<float>(x1, y1 - ry + ry * k),
               Point<float>(x1 - rx + rx * k, y1), Point<float>(x1 - rx, y1));
  out.line_to(Point<float>(x + rx, y1));
  out.cubic_to(Point<float>(x + rx - rx * k, y1),
               Point<float>(x, y1 - ry + ry * k), Point<float>(x, y1 - ry));
  out.line_to(Point<float>(x, y + ry));
  out.cubic_to(Point<float>(x, y + ry - ry * k),
               Point<float>(x + rx - rx * k, y), Point<float>(x + rx, y));
  out.close();
  return true;
}

bool build_ellipse(const XmlElement &element, Path &out, bool circle) {
  const float cx = attribute_number(element, "cx", 0.0f);
  const float cy = attribute_number(element, "cy", 0.0f);

  float rx = 0.0f;
  float ry = 0.0f;
  if (circle) {
    rx = ry = attribute_number(element, "r", 0.0f);
  } else {
    rx = attribute_number(element, "rx", 0.0f);
    ry = attribute_number(element, "ry", 0.0f);
  }
  if (!(rx > 0.0f) || !(ry > 0.0f))
    return false;

  out = Path::oval(Rect<float>(cx - rx, cy - ry, rx * 2.0f, ry * 2.0f));
  return true;
}

bool build_line(const XmlElement &element, Path &out) {
  const Point<float> from(attribute_number(element, "x1", 0.0f),
                          attribute_number(element, "y1", 0.0f));
  const Point<float> to(attribute_number(element, "x2", 0.0f),
                        attribute_number(element, "y2", 0.0f));
  out.move_to(from);
  out.line_to(to);
  return true;
}

bool build_poly(const XmlElement &element, Path &out, bool close) {
  NumberScanner scanner(attribute(element, "points"));
  bool first = true;
  float x = 0.0f;
  float y = 0.0f;
  while (scanner.next(x) && scanner.next(y)) {
    if (first) {
      out.move_to(Point<float>(x, y));
      first = false;
    } else {
      out.line_to(Point<float>(x, y));
    }
  }
  if (first)
    return false;
  if (close)
    out.close();
  return true;
}

// -- Gradients ---------------------------------------------------------------

struct GradientDefinition {
  bool radial = false;
  bool user_space = false;
  bool has_transform = false;
  Transform transform;

  float x1 = 0.0f, y1 = 0.0f, x2 = 1.0f, y2 = 0.0f;
  float cx = 0.5f, cy = 0.5f, radius = 0.5f;

  std::vector<GradientStop> stops;
  std::string_view href;

  /// Which of the geometry attributes this element stated itself. An attribute
  /// it did not state is taken from whatever it references, which is how the
  /// common "one gradient, several tints" idiom in exported files works.
  bool has_geometry = false;
  bool has_stops = false;
};

/// `stop-color` and `stop-opacity`, which are the only two properties inside a
/// gradient that matter here.
///
/// `currentColor` in a stop cannot be honoured: the widget's colour is not
/// known when the file is parsed, and a gradient is shared between shapes that
/// may not agree on one. Black is what an unresolvable paint falls back to
/// everywhere else in SVG, so it is what it falls back to here.
Color parse_stop_color(const XmlElement &element) {
  Color color(0, 0, 0);
  std::string_view text = attribute(element, "stop-color");
  std::string_view opacity_text = attribute(element, "stop-opacity");

  const std::string_view inline_style = attribute(element, "style");
  if (!inline_style.empty()) {
    std::size_t at = 0;
    while (at < inline_style.size()) {
      const std::size_t end = inline_style.find(';', at);
      const std::string_view entry = inline_style.substr(
          at, end == std::string_view::npos ? std::string_view::npos : end - at);
      const std::size_t colon = entry.find(':');
      if (colon != std::string_view::npos) {
        const std::string_view name = trim(entry.substr(0, colon));
        const std::string_view value = trim(entry.substr(colon + 1));
        if (name == "stop-color")
          text = value;
        else if (name == "stop-opacity")
          opacity_text = value;
      }
      if (end == std::string_view::npos)
        break;
      at = end + 1;
    }
  }

  if (!text.empty() && !keyword_is(text, "currentcolor"))
    (void)parse_style_value(text, color);

  float opacity = 1.0f;
  if (!opacity_text.empty() && parse_number(opacity_text, opacity))
    color = color.with_alpha(color.a * std::clamp(opacity, 0.0f, 1.0f));

  return color;
}

} // namespace

// -- The builder -------------------------------------------------------------

/// Walks the element tree once and writes the flat document out of it.
///
/// A friend of SvgDocument rather than a member because it is scaffolding: it
/// holds the id table, the gradient definitions and the inheritance stack,
/// none of which survive the parse.
class SvgBuilder {
public:
  SvgBuilder(std::vector<XmlElement> elements, SvgDocument &document)
      : elements_(std::move(elements)), document_(document) {}

  void run(std::uint32_t root) {
    index_ids_();
    read_viewport_(elements_[root]);

    Frame frame;
    frame.style.fill = SvgPaint::solid(Color(0, 0, 0));
    frame.ctm = Transform();
    walk_children_(root, frame, 0);

    document_.shapes_.shrink_to_fit();
    document_.paths_.shrink_to_fit();
    document_.clips_.shrink_to_fit();
    document_.dash_patterns_.shrink_to_fit();
    document_.brushes_.shrink_to_fit();
  }

private:
  /// The inherited state at one point in the tree.
  struct Frame {
    SvgShapeStyle style;
    Transform ctm;
    std::uint32_t clip = kNoSvgIndex;

    /// The fragment a `url(#...)` paint named, kept unresolved until a shape
    /// with a bounding box comes along to resolve it against.
    std::string_view fill_server;
    std::string_view stroke_server;
  };

  void index_ids_() {
    for (std::uint32_t i = 0; i < elements_.size(); ++i) {
      const std::string_view id = attribute(elements_[i], "id");
      if (!id.empty())
        ids_.emplace(id, i);
    }
  }

  void read_viewport_(const XmlElement &root) {
    float width = 0.0f;
    float height = 0.0f;
    if (parse_length(attribute(root, "width"), width) && width > 0.0f)
      document_.intrinsic_.width = width;
    if (parse_length(attribute(root, "height"), height) && height > 0.0f)
      document_.intrinsic_.height = height;

    NumberScanner scanner(attribute(root, "viewBox"));
    float x = 0.0f, y = 0.0f, w = 0.0f, h = 0.0f;
    if (scanner.next(x) && scanner.next(y) && scanner.next(w) &&
        scanner.next(h) && w > 0.0f && h > 0.0f) {
      document_.view_box_ = Rect<float>(x, y, w, h);
    } else if (document_.intrinsic_.width > 0.0f &&
               document_.intrinsic_.height > 0.0f) {
      // No view box: the intrinsic size is the user-unit extent, which is what
      // makes an icon exported without one still land in the right place.
      document_.view_box_ = Rect<float>(0.0f, 0.0f, document_.intrinsic_.width,
                                        document_.intrinsic_.height);
    }

    read_preserve_aspect_ratio_(attribute(root, "preserveAspectRatio"));
  }

  void read_preserve_aspect_ratio_(std::string_view text) {
    text = trim(text);
    if (text.empty())
      return;

    SvgPreserveAspectRatio value;
    std::size_t at = 0;
    while (at < text.size()) {
      while (at < text.size() && is_space(text[at]))
        ++at;
      const std::size_t start = at;
      while (at < text.size() && !is_space(text[at]))
        ++at;
      const std::string_view word = text.substr(start, at - start);
      if (word.empty())
        continue;

      if (keyword_is(word, "none")) {
        value.uniform = false;
      } else if (keyword_is(word, "slice")) {
        value.slice = true;
      } else if (keyword_is(word, "meet")) {
        value.slice = false;
      } else if (word.size() == 8 && word.compare(0, 4, "xMin") == 0) {
        value.align_x = 0.0f;
        value.align_y = align_of_(word.substr(4));
      } else if (word.size() == 8 && word.compare(0, 4, "xMid") == 0) {
        value.align_x = 0.5f;
        value.align_y = align_of_(word.substr(4));
      } else if (word.size() == 8 && word.compare(0, 4, "xMax") == 0) {
        value.align_x = 1.0f;
        value.align_y = align_of_(word.substr(4));
      }
      // `defer` and anything unrecognised are ignored, as the specification
      // says to do with a reference this renderer has no use for.
    }

    document_.preserve_ = value;
  }

  static float align_of_(std::string_view suffix) {
    if (suffix == "YMin")
      return 0.0f;
    if (suffix == "YMax")
      return 1.0f;
    return 0.5f;
  }

  void walk_children_(std::uint32_t element, const Frame &frame, int depth) {
    if (depth >= kMaxDepth)
      return;
    for (const std::uint32_t child : elements_[element].children)
      walk_(child, frame, depth + 1);
  }

  void walk_(std::uint32_t index, const Frame &parent, int depth) {
    const XmlElement &element = elements_[index];
    const std::string_view name = element.name;

    // Definitions are visited only through a reference, never drawn where they
    // stand.
    if (name == "defs" || name == "clipPath" || name == "linearGradient" ||
        name == "radialGradient" || name == "symbol" || name == "mask" ||
        name == "pattern" || name == "filter" || name == "marker" ||
        name == "style" || name == "title" || name == "desc" ||
        name == "metadata")
      return;

    if (keyword_is(attribute(element, "display"), "none"))
      return;

    Frame frame = parent;
    apply_presentation_(element, frame);

    const std::string_view transform_text = attribute(element, "transform");
    if (!transform_text.empty())
      frame.ctm = frame.ctm.concat(parse_transform_list(transform_text));

    const std::string_view clip_text = attribute(element, "clip-path");
    if (!clip_text.empty())
      resolve_clip_(url_fragment(clip_text), frame);

    if (name == "g" || name == "svg" || name == "a" || name == "switch") {
      walk_children_(index, frame, depth);
      return;
    }

    if (keyword_is(attribute(element, "visibility"), "hidden"))
      return;

    Path path;
    bool built = false;
    if (name == "path") {
      built = parse_path_data(attribute(element, "d"), path);
    } else if (name == "rect") {
      built = build_rect(element, path);
    } else if (name == "circle") {
      built = build_ellipse(element, path, true);
    } else if (name == "ellipse") {
      built = build_ellipse(element, path, false);
    } else if (name == "line") {
      built = build_line(element, path);
    } else if (name == "polyline") {
      built = build_poly(element, path, false);
    } else if (name == "polygon") {
      built = build_poly(element, path, true);
    }

    if (built && !path.empty())
      emit_(std::move(path), frame);
  }

  /// Reads the presentation attributes and the inline `style`, in that order --
  /// a declaration in `style="..."` wins over the matching attribute, which is
  /// what CSS says and what every exporter relies on.
  void apply_presentation_(const XmlElement &element, Frame &frame) {
    for (const auto &entry : element.attributes)
      apply_property_(entry.first, entry.second, frame);

    const std::string_view inline_style = attribute(element, "style");
    if (inline_style.empty())
      return;

    std::size_t at = 0;
    while (at < inline_style.size()) {
      const std::size_t end = inline_style.find(';', at);
      const std::string_view entry = inline_style.substr(
          at, end == std::string_view::npos ? std::string_view::npos : end - at);
      const std::size_t colon = entry.find(':');
      if (colon != std::string_view::npos)
        apply_property_(trim(entry.substr(0, colon)),
                        trim(entry.substr(colon + 1)), frame);
      if (end == std::string_view::npos)
        break;
      at = end + 1;
    }
  }

  void apply_property_(std::string_view name, std::string_view value,
                       Frame &frame) {
    if (value.empty())
      return;

    // `inherit` means "take the parent's", which is already what the frame
    // holds -- but it also means the document did not decide, so the specified
    // bit is deliberately left alone.
    if (keyword_is(value, "inherit"))
      return;

    SvgShapeStyle &style = frame.style;

    if (name == "fill") {
      const std::string_view fragment = url_fragment(value);
      if (!fragment.empty()) {
        frame.fill_server = fragment;
        style.specified |= kSvgFillSet;
        return;
      }
      SvgPaint paint;
      if (parse_style_value(value, paint)) {
        frame.fill_server = {};
        style.fill = paint;
        style.specified |= kSvgFillSet;
      }
    } else if (name == "stroke") {
      const std::string_view fragment = url_fragment(value);
      if (!fragment.empty()) {
        frame.stroke_server = fragment;
        style.specified |= kSvgStrokeSet;
        return;
      }
      SvgPaint paint;
      if (parse_style_value(value, paint)) {
        frame.stroke_server = {};
        style.stroke = paint;
        style.specified |= kSvgStrokeSet;
      }
    } else if (name == "fill-opacity") {
      float opacity = 1.0f;
      if (parse_number(value, opacity)) {
        style.fill_opacity = std::clamp(opacity, 0.0f, 1.0f);
        style.specified |= kSvgFillOpacitySet;
      }
    } else if (name == "stroke-opacity") {
      float opacity = 1.0f;
      if (parse_number(value, opacity)) {
        style.stroke_opacity = std::clamp(opacity, 0.0f, 1.0f);
        style.specified |= kSvgStrokeOpacitySet;
      }
    } else if (name == "opacity") {
      // Not a specified-bit property: group opacities multiply down the tree,
      // so there is nothing for a stylesheet to override on a single shape.
      float opacity = 1.0f;
      if (parse_number(value, opacity))
        style.opacity *= std::clamp(opacity, 0.0f, 1.0f);
    } else if (name == "fill-rule") {
      FillRule rule = FillRule::NonZero;
      if (parse_style_value(value, rule)) {
        style.fill_rule = rule;
        style.specified |= kSvgFillRuleSet;
      }
    } else if (name == "stroke-width") {
      float width = 1.0f;
      if (parse_number(value, width) && width >= 0.0f) {
        style.stroke_width = width;
        style.specified |= kSvgStrokeWidthSet;
      }
    } else if (name == "stroke-linecap") {
      LineCap cap = LineCap::Butt;
      if (parse_style_value(value, cap)) {
        style.cap = cap;
        style.specified |= kSvgStrokeLinecapSet;
      }
    } else if (name == "stroke-linejoin") {
      LineJoin join = LineJoin::Miter;
      if (parse_style_value(value, join)) {
        style.join = join;
        style.specified |= kSvgStrokeLinejoinSet;
      }
    } else if (name == "stroke-miterlimit") {
      float limit = 4.0f;
      if (parse_number(value, limit) && limit >= 1.0f) {
        style.miter_limit = limit;
        style.specified |= kSvgStrokeMiterlimitSet;
      }
    } else if (name == "stroke-dasharray") {
      SvgDashArray dashes;
      if (parse_style_value(value, dashes)) {
        style.dashes = intern_dashes_(dashes, dash_offset_of_(style));
        style.specified |= kSvgStrokeDasharraySet;
      }
    } else if (name == "stroke-dashoffset") {
      float offset = 0.0f;
      if (parse_number(value, offset)) {
        SvgDashArray dashes;
        if (style.dashes != kNoSvgDashes)
          dashes = document_.dash_patterns_[style.dashes].array;
        style.dashes = intern_dashes_(dashes, offset);
        style.specified |= kSvgStrokeDasharraySet;
      }
    } else if (name == "paint-order") {
      SvgPaintOrder order = SvgPaintOrder::FillStroke;
      if (parse_style_value(value, order)) {
        style.order = order;
        style.specified |= kSvgPaintOrderSet;
      }
    }
  }

  float dash_offset_of_(const SvgShapeStyle &style) const {
    return style.dashes == kNoSvgDashes
               ? 0.0f
               : document_.dash_patterns_[style.dashes].offset;
  }

  std::uint16_t intern_dashes_(const SvgDashArray &array, float offset) {
    if (array.empty() && offset == 0.0f)
      return kNoSvgDashes;

    for (std::size_t i = 0; i < document_.dash_patterns_.size(); ++i) {
      const SvgDashPattern &entry = document_.dash_patterns_[i];
      if (entry.offset == offset && style_value_equals(entry.array, array))
        return static_cast<std::uint16_t>(i);
    }
    if (document_.dash_patterns_.size() >= kNoSvgDashes)
      return kNoSvgDashes;

    document_.dash_patterns_.push_back(SvgDashPattern{array, offset});
    return static_cast<std::uint16_t>(document_.dash_patterns_.size() - 1);
  }

  /// A `clip-path` this renderer can express.
  ///
  /// The scissor is a rectangle, so a clip becomes one when the referenced
  /// element is a single rectangle under an axis-aligned transform -- which is
  /// exactly the "clip artwork to the frame" shape every design tool emits, and
  /// the only clip most icons carry. Anything else is dropped: drawing the
  /// shape unclipped shows more than was asked for, but not drawing it at all
  /// shows nothing, and the first is the better failure.
  void resolve_clip_(std::string_view fragment, Frame &frame) {
    if (fragment.empty())
      return;
    const auto found = ids_.find(fragment);
    if (found == ids_.end())
      return;

    const XmlElement &clip = elements_[found->second];
    if (clip.name != "clipPath" || clip.children.size() != 1)
      return;

    const XmlElement &shape = elements_[clip.children.front()];
    if (shape.name != "rect")
      return;

    Transform ctm = frame.ctm;
    const std::string_view clip_transform = attribute(clip, "transform");
    if (!clip_transform.empty())
      ctm = ctm.concat(parse_transform_list(clip_transform));
    const std::string_view shape_transform = attribute(shape, "transform");
    if (!shape_transform.empty())
      ctm = ctm.concat(parse_transform_list(shape_transform));

    if (!ctm.is_axis_aligned())
      return;

    Path path;
    if (!build_rect(shape, path))
      return;

    // The clip is stored in the document's root user units, so a shape whose
    // own transform has already been baked can still be tested against it.
    Rect<float> rect = ctm.map_bounds(path.bounds());
    if (frame.clip != kNoSvgIndex)
      rect = intersect_(document_.clips_[frame.clip], rect);

    document_.clips_.push_back(rect);
    frame.clip = static_cast<std::uint32_t>(document_.clips_.size() - 1);
  }

  static Rect<float> intersect_(Rect<float> a, Rect<float> b) {
    const float x0 = std::max(a.origin.x, b.origin.x);
    const float y0 = std::max(a.origin.y, b.origin.y);
    const float x1 = std::min(a.origin.x + a.size.width,
                              b.origin.x + b.size.width);
    const float y1 = std::min(a.origin.y + a.size.height,
                              b.origin.y + b.size.height);
    return Rect<float>(x0, y0, std::max(x1 - x0, 0.0f),
                       std::max(y1 - y0, 0.0f));
  }

  void emit_(Path path, const Frame &frame) {
    SvgShape shape;
    shape.style = frame.style;
    shape.clip = frame.clip;
    shape.stroke_scale = frame.ctm.approximate_scale();

    // A gradient in bounding-box units needs the box before the transform, so
    // the servers are resolved first and the geometry is baked after.
    const Rect<float> local_bounds = path.bounds();
    if (!frame.fill_server.empty())
      shape.style.fill = resolve_server_(frame.fill_server, local_bounds);
    if (!frame.stroke_server.empty())
      shape.style.stroke = resolve_server_(frame.stroke_server, local_bounds);

    // A shape the document itself declared invisible is dropped here rather
    // than walked past on every frame. Only a *stated* `none` counts: a
    // property the file left open belongs to the cascade, and dropping a shape
    // because its default happens not to paint would make `.icon { stroke: ... }`
    // silently do nothing.
    const auto dead = [&](std::uint16_t bit, const SvgPaint &paint) {
      return (shape.style.specified & bit) != 0 && !paint.paints();
    };
    if (dead(kSvgFillSet, shape.style.fill) &&
        dead(kSvgStrokeSet, shape.style.stroke))
      return;

    if (!frame.ctm.is_identity())
      path.apply_transform(frame.ctm);

    document_.paths_.push_back(std::make_shared<const Path>(std::move(path)));
    shape.path = static_cast<std::uint32_t>(document_.paths_.size() - 1);
    document_.shapes_.push_back(shape);
  }

  SvgPaint resolve_server_(std::string_view fragment, Rect<float> bounds) {
    GradientDefinition definition;
    if (!collect_gradient_(fragment, definition, 0) || definition.stops.empty())
      return SvgPaint::solid(Color(0, 0, 0));

    if (document_.brushes_.size() >= kNoSvgDashes)
      return SvgPaint::solid(definition.stops.front().color);

    document_.brushes_.push_back(make_brush_(definition, bounds));
    return SvgPaint::server(
        static_cast<std::uint16_t>(document_.brushes_.size() - 1));
  }

  /// Follows an `href` chain, nearest definition winning.
  ///
  /// The idiom this exists for is one gradient defining the stops and several
  /// others referencing it with only their own coordinates -- which is what
  /// every exporter emits for a repeated fill, and what makes a file half the
  /// size it would otherwise be.
  bool collect_gradient_(std::string_view fragment, GradientDefinition &out,
                         int depth) {
    if (depth >= 8 || fragment.empty())
      return false;
    const auto found = ids_.find(fragment);
    if (found == ids_.end())
      return false;

    const XmlElement &element = elements_[found->second];
    const bool linear = element.name == "linearGradient";
    const bool radial = element.name == "radialGradient";
    if (!linear && !radial)
      return false;

    GradientDefinition local;
    local.radial = radial;
    local.user_space =
        keyword_is(attribute(element, "gradientUnits"), "userSpaceOnUse");

    const std::string_view transform_text =
        attribute(element, "gradientTransform");
    if (!transform_text.empty()) {
      local.transform = parse_transform_list(transform_text);
      local.has_transform = true;
    }

    if (linear) {
      local.has_geometry = has_attribute(element, "x1") ||
                           has_attribute(element, "y1") ||
                           has_attribute(element, "x2") ||
                           has_attribute(element, "y2");
      local.x1 = fraction_(element, "x1", 0.0f);
      local.y1 = fraction_(element, "y1", 0.0f);
      local.x2 = fraction_(element, "x2", 1.0f);
      local.y2 = fraction_(element, "y2", 0.0f);
    } else {
      local.has_geometry = has_attribute(element, "cx") ||
                           has_attribute(element, "cy") ||
                           has_attribute(element, "r");
      local.cx = fraction_(element, "cx", 0.5f);
      local.cy = fraction_(element, "cy", 0.5f);
      local.radius = fraction_(element, "r", 0.5f);
    }

    for (const std::uint32_t child : element.children) {
      const XmlElement &stop = elements_[child];
      if (stop.name != "stop")
        continue;
      if (local.stops.size() >= kMaxGradientStops)
        break;

      float offset = 0.0f;
      const std::string_view offset_text = attribute(stop, "offset");
      if (!offset_text.empty()) {
        if (!offset_text.empty() && offset_text.back() == '%') {
          if (parse_number(offset_text.substr(0, offset_text.size() - 1),
                           offset))
            offset *= 0.01f;
        } else {
          (void)parse_number(offset_text, offset);
        }
      }
      local.stops.push_back(
          GradientStop(parse_stop_color(stop), std::clamp(offset, 0.0f, 1.0f)));
    }
    local.has_stops = !local.stops.empty();

    GradientDefinition inherited;
    const std::string_view href = attribute(element, "href").empty()
                                      ? attribute(element, "xlink:href")
                                      : attribute(element, "href");
    const bool has_base =
        collect_gradient_(href_fragment(href), inherited, depth + 1);

    out = local;
    if (has_base) {
      if (!local.has_stops)
        out.stops = inherited.stops;
      if (!local.has_geometry) {
        out.x1 = inherited.x1;
        out.y1 = inherited.y1;
        out.x2 = inherited.x2;
        out.y2 = inherited.y2;
        out.cx = inherited.cx;
        out.cy = inherited.cy;
        out.radius = inherited.radius;
      }
      if (!local.has_transform && inherited.has_transform) {
        out.transform = inherited.transform;
        out.has_transform = true;
      }
    }
    return true;
  }

  /// A gradient coordinate. Percentages and plain numbers mean the same thing
  /// in bounding-box units and differ by a hundred in user space, which the
  /// caller sorts out once it knows which it is looking at.
  static float fraction_(const XmlElement &element, std::string_view name,
                         float fallback) {
    std::string_view text = attribute(element, name);
    if (text.empty())
      return fallback;
    bool percent = false;
    if (text.back() == '%') {
      percent = true;
      text = text.substr(0, text.size() - 1);
    }
    float value = fallback;
    if (!parse_number(text, value))
      return fallback;
    return percent ? value * 0.01f : value;
  }

  Brush make_brush_(const GradientDefinition &definition,
                    Rect<float> bounds) const {
    Point<float> start(definition.x1, definition.y1);
    Point<float> end(definition.x2, definition.y2);

    if (definition.radial) {
      // No radial brush exists downstream, so the gradient is laid along a
      // radius. The colours and their order survive, which is what a shape
      // filled with one actually shows today; the shape of the falloff does
      // not.
      start = Point<float>(definition.cx, definition.cy);
      end = Point<float>(definition.cx + definition.radius, definition.cy);
    }

    if (definition.has_transform) {
      start = definition.transform.apply(start);
      end = definition.transform.apply(end);
    }

    // The brush wants its axis in the shape's own 0..1 box, which is what
    // objectBoundingBox already is and what user space has to be converted to.
    if (definition.user_space) {
      const float width = bounds.size.width > 0.0f ? bounds.size.width : 1.0f;
      const float height =
          bounds.size.height > 0.0f ? bounds.size.height : 1.0f;
      start = Point<float>((start.x - bounds.origin.x) / width,
                           (start.y - bounds.origin.y) / height);
      end = Point<float>((end.x - bounds.origin.x) / width,
                         (end.y - bounds.origin.y) / height);
    }

    return LinearGradient(start, end,
                          std::span<const GradientStop>(definition.stops));
  }

  std::vector<XmlElement> elements_;
  std::unordered_map<std::string_view, std::uint32_t> ids_;
  SvgDocument &document_;
};

// -- Entry points -------------------------------------------------------------

std::size_t SvgDocument::byte_size() const {
  std::size_t bytes = sizeof(SvgDocument);
  bytes += shapes_.capacity() * sizeof(SvgShape);
  bytes += clips_.capacity() * sizeof(Rect<float>);
  bytes += dash_patterns_.capacity() * sizeof(SvgDashPattern);
  bytes += brushes_.capacity() * sizeof(Brush);
  bytes += paths_.capacity() * sizeof(std::shared_ptr<const Path>);
  for (const std::shared_ptr<const Path> &path : paths_) {
    if (!path)
      continue;
    bytes += sizeof(Path);
    bytes += path->verbs().capacity() * sizeof(PathVerb);
    bytes += path->points().capacity() * sizeof(Point<float>);
  }
  return bytes;
}

SvgDocument::Result SvgDocument::parse(std::string_view source) {
  Result result;

  XmlParser xml(source);
  const std::uint32_t root = xml.parse();
  std::vector<XmlElement> elements = xml.take();

  if (root == kNoSvgIndex || elements.empty() || elements[root].name != "svg") {
    result.error = "no <svg> element";
    return result;
  }

  auto document = std::make_shared<SvgDocument>();
  SvgBuilder builder(std::move(elements), *document);
  builder.run(root);

  result.document = std::move(document);
  return result;
}

} // namespace voidui
