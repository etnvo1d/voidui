// Exercise of the resource layer: URI normalisation, the traversal refusals,
// mount ordering and overlay fall-through, the native-path and revision escape
// hatches, and the same-origin policy that keeps an embedded stylesheet off the
// filesystem.
#include "voidui/core/resource.h"
#include "voidui/core/style.h"
#include "voidui/core/style/parser.h"
#include "voidui/paint/font.h"
#include "voidui/paint/font_registry.h"

#include <array>
#include <chrono>
#include <cstdio>
#include <cstring>
#include <filesystem>
#include <fstream>
#include <memory>
#include <optional>
#include <string>
#include <utility>
#include <vector>

using namespace voidui;

namespace demo {

/// A selector has to name a registered widget or the rule is skipped, so the
/// sheets below are written against one declared here.
class Button {
public:
  VOIDUI_STYLE_SCOPE(Button, "button")
};

} // namespace demo

namespace {

int failures = 0;

void check(bool condition, const char *what) {
  if (!condition) {
    ++failures;
    std::printf("FAIL  %s\n", what);
  } else {
    std::printf("ok    %s\n", what);
  }
}

/// A URI's canonical text, or the name of the error it produced. Comparing
/// strings keeps the assertions readable when one of them fails.
std::string uri_text(std::string_view text) {
  const ResourceResult<ResourceUri> uri = ResourceUri::parse(text);
  if (!uri)
    return std::string(to_string(uri.error()));
  return uri->to_string();
}

std::string resolved_text(std::string_view base, std::string_view reference) {
  const ResourceResult<ResourceUri> parsed = ResourceUri::parse(base);
  if (!parsed)
    return std::string(to_string(parsed.error()));
  const ResourceResult<ResourceUri> uri = parsed->resolve(reference);
  if (!uri)
    return std::string(to_string(uri.error()));
  return uri->to_string();
}

Blob text_blob(std::string_view text) {
  std::vector<std::byte> data(text.size());
  std::memcpy(data.data(), text.data(), text.size());
  return Blob::own(std::move(data));
}

std::shared_ptr<MemoryProvider> provider_with(
    std::initializer_list<std::pair<const char *, const char *>> entries) {
  auto provider = std::make_shared<MemoryProvider>();
  for (const auto &entry : entries)
    provider->add(entry.first, text_blob(entry.second));
  return provider;
}

std::string opened(const Resources &resources, std::string_view reference,
                   const ResourceUri &base = {}) {
  const ResourceResult<Blob> blob = resources.open(reference, base);
  if (!blob)
    return std::string(to_string(blob.error()));
  return std::string(blob->text());
}

void write_file(const std::filesystem::path &path, std::string_view text) {
  std::filesystem::create_directories(path.parent_path());
  std::ofstream stream(path, std::ios::binary | std::ios::trunc);
  stream.write(text.data(), static_cast<std::streamsize>(text.size()));
}

} // namespace

int main() {
  // -- URI normalisation ----------------------------------------------------

  {
    check(uri_text("res://fonts/Inter.ttf") == "res://fonts/Inter.ttf",
          "a res URI round trips");
    check(uri_text("res:fonts/Inter.ttf") == "res://fonts/Inter.ttf",
          "the authority-less spelling normalises to the canonical one");
    check(uri_text("res:///fonts//Inter.ttf") == "res://fonts/Inter.ttf",
          "leading and doubled separators collapse");
    check(uri_text("res://a/./b/../Inter.ttf") == "res://a/Inter.ttf",
          "'.' drops out and '..' pops a segment");
    check(uri_text("res://fonts\\Inter.ttf") == "res://fonts/Inter.ttf",
          "backslashes are separators too");

    check(uri_text("res://../secrets") == to_string(ResourceError::BadUri),
          "a traversal above the root is refused, not clamped");
    check(uri_text("res://a/../../secrets") == to_string(ResourceError::BadUri),
          "a traversal that only escapes after a descent is refused");
    check(uri_text("ftp://example.com/x") == to_string(ResourceError::BadUri),
          "an unknown scheme is refused");
    check(uri_text("") == to_string(ResourceError::BadUri),
          "an empty URI is refused");
  }

  {
    check(uri_text("C:/fonts/Inter.ttf") == "file:///C:/fonts/Inter.ttf",
          "a drive letter is a path, not a one-character scheme");
    check(uri_text("file:///C:/fonts/Inter.ttf") == "file:///C:/fonts/Inter.ttf",
          "an explicit file URI keeps its drive");
    check(uri_text("file:///home/user/x.vss") == "file:///home/user/x.vss",
          "a POSIX-rooted file URI keeps its leading slash");
    check(uri_text("assets/theme.vss") == "file:assets/theme.vss",
          "a bare relative path is a file URI with no authority");
    check(uri_text("C:/app/../fonts/x.ttf") == "file:///C:/fonts/x.ttf",
          "file paths normalise like resource paths");
    check(uri_text("C:/../secrets") == to_string(ResourceError::BadUri),
          "a rooted file path cannot climb above its drive");
    check(uri_text("../assets/x.ttf") == "file:../assets/x.ttf",
          "a relative file path keeps a leading '..', having no root to leave");
  }

  // -- Reference resolution -------------------------------------------------

  {
    check(resolved_text("res://theme/dark.vss", "../fonts/Inter.ttf") ==
              "res://fonts/Inter.ttf",
          "a relative reference resolves against the document's directory");
    check(resolved_text("res://theme/dark.vss", "sub/light.vss") ==
              "res://theme/sub/light.vss",
          "a sibling reference stays in the document's directory");
    check(resolved_text("res://dark.vss", "fonts/Inter.ttf") ==
              "res://fonts/Inter.ttf",
          "a document at the root resolves against the root");
    check(resolved_text("res://theme/dark.vss", "/fonts/Inter.ttf") ==
              "res://fonts/Inter.ttf",
          "a root-relative reference ignores the document's directory");
    check(resolved_text("res://theme/dark.vss", "../../fonts/x.ttf") ==
              to_string(ResourceError::BadUri),
          "a reference cannot escape the resource root either");

    check(resolved_text("res://theme/dark.vss", "res://other/x.ttf") ==
              "res://other/x.ttf",
          "an absolute reference is taken as it stands");
    check(resolved_text("res://theme/dark.vss", "file:///C:/x.ttf") ==
              "file:///C:/x.ttf",
          "crossing schemes requires writing the scheme out");

    check(resolved_text("C:/app/theme/dark.vss", "../fonts/Inter.ttf") ==
              "file:///C:/app/fonts/Inter.ttf",
          "a loose document resolves beside itself, staying on file:");
    check(resolved_text("C:/app/theme/dark.vss", "/assets/x.ttf") ==
              "file:///C:/assets/x.ttf",
          "a root-relative reference keeps the document's drive");
  }

  // -- Mount ordering and overlay -------------------------------------------

  {
    Resources resources;
    check(opened(resources, "res://fonts/a.ttf") ==
              to_string(ResourceError::NotMounted),
          "an empty table reports nothing mounted, not nothing found");

    const auto pack = provider_with({{"fonts/a.ttf", "packed-a"},
                                     {"fonts/b.ttf", "packed-b"}});
    resources.mount("", pack, 0);

    check(opened(resources, "res://fonts/a.ttf") == "packed-a",
          "a mounted provider answers");
    check(opened(resources, "res://fonts/missing.ttf") ==
              to_string(ResourceError::NotFound),
          "a claimed prefix with no such entry reports not found");

    const auto overlay = provider_with({{"fonts/a.ttf", "loose-a"}});
    const Resources::MountId id = resources.mount("", overlay, 10);

    check(opened(resources, "res://fonts/a.ttf") == "loose-a",
          "the higher-priority mount shadows the lower one");
    check(opened(resources, "res://fonts/b.ttf") == "packed-b",
          "a file the overlay lacks falls through to the mount underneath");

    check(resources.unmount(id), "unmount finds the mount it was given");
    check(opened(resources, "res://fonts/a.ttf") == "packed-a",
          "removing the overlay uncovers the original");
    check(!resources.unmount(id), "unmounting twice reports nothing removed");
  }

  {
    Resources resources;
    resources.mount("", provider_with({{"fonts/a.ttf", "root-mount"}}), 100);
    resources.mount("fonts", provider_with({{"a.ttf", "prefix-mount"}}), 0);

    check(opened(resources, "res://fonts/a.ttf") == "prefix-mount",
          "the longer prefix wins regardless of priority");
    check(opened(resources, "res://other/a.ttf") ==
              to_string(ResourceError::NotFound),
          "a path outside the prefixed mount still reaches the root mount");
  }

  // -- Loose files: native paths and revisions ------------------------------

  const std::filesystem::path root =
      std::filesystem::temp_directory_path() / "voidui_resource_selftest";
  std::error_code cleanup;
  std::filesystem::remove_all(root, cleanup);

  {
    write_file(root / "fonts" / "a.ttf", "on-disk-a");

    Resources resources;
    resources.mount("", directory_provider(root.string()), 0);

    check(opened(resources, "res://fonts/a.ttf") == "on-disk-a",
          "a directory provider reads the file behind the URI");

    const ResourceUri uri = *ResourceUri::parse("res://fonts/a.ttf");
    const std::optional<std::string> native = resources.native_path(uri);
    check(native.has_value() &&
              std::filesystem::equivalent(*native, root / "fonts" / "a.ttf"),
          "a loose resource reports the real file behind it");
    check(!resources.native_path(*ResourceUri::parse("res://fonts/gone.ttf")),
          "a missing resource reports no native path");

    const std::optional<std::uint64_t> before = resources.revision(uri);
    check(before.has_value(), "a loose resource reports a revision");

    // Set the timestamp rather than racing the filesystem's resolution.
    std::filesystem::last_write_time(root / "fonts" / "a.ttf",
                                     std::filesystem::last_write_time(
                                         root / "fonts" / "a.ttf") +
                                         std::chrono::seconds(5));
    check(resources.revision(uri) != before,
          "rewriting the file moves the revision");

    Resources embedded;
    embedded.mount("", provider_with({{"fonts/a.ttf", "packed"}}), 0);
    check(!embedded.revision(uri),
          "a resource that cannot change reports no revision at all");
  }

  // -- Native access policy -------------------------------------------------

  {
    write_file(root / "loose.txt", "loose-bytes");
    const std::string native = (root / "loose.txt").string();
    const std::string native_uri = "file:///" + native;

    Resources resources;
    resources.mount("", provider_with({{"theme/dark.vss", "sheet"}}), 0);

    const ResourceUri embedded_base = *ResourceUri::parse("res://theme/dark.vss");
    const ResourceUri loose_base =
        *ResourceUri::parse((root / "theme" / "dark.vss").string());

    check(opened(resources, native_uri, embedded_base) ==
              to_string(ResourceError::Denied),
          "an embedded document cannot reach the filesystem");
    check(opened(resources, native, embedded_base) ==
              to_string(ResourceError::BadUri),
          "a bare native path is not a reference an embedded document can make");
    check(opened(resources, "../loose.txt", loose_base) == "loose-bytes",
          "a loose document reaches the files beside it");
    check(opened(resources, native, loose_base) == "loose-bytes",
          "under file: a drive-rooted reference is absolute, not relative");
    check(opened(resources, native) == "loose-bytes",
          "with no document in play, C++ reads what it asks for");

    resources.set_native_access(Resources::NativeAccess::Always);
    check(opened(resources, native_uri, embedded_base) == "loose-bytes",
          "the policy can be opened up deliberately");

    resources.set_native_access(Resources::NativeAccess::Never);
    check(opened(resources, "../loose.txt", loose_base) ==
              to_string(ResourceError::Denied),
          "Never shuts out even a document that came from disk");
    check(opened(resources, "res://theme/dark.vss", embedded_base) == "sheet",
          "the policy governs file: only, never res:");
  }

  // -- Stylesheets and themes read through the layer ------------------------

  {
    // The parser reads through the process-wide table, so this section mounts
    // there and takes the mount back down at the end.
    auto pack = std::make_shared<MemoryProvider>();
    pack->add("ui/app.vss", text_blob("button { padding: 6 12; }"));
    pack->add("ui/broken.vss", text_blob("button { padding: not-a-number; }"));
    pack->add("theme/palette.vtheme", text_blob("$surface: #101010;"));
    pack->add("theme/dark.vtheme",
              text_blob("@base \"palette.vtheme\";\n"
                        "@name \"Dark\";\n"
                        "$text: #f0f0f0;\n"));
    pack->add("theme/loop.vtheme", text_blob("@base \"loop.vtheme\";\n"));

    const Resources::MountId id = Resources::global().mount("", pack, 0);

    const ResourceUri sheet_uri = *ResourceUri::parse("res://ui/app.vss");
    StyleParser::Result sheet = StyleParser::parse_document(sheet_uri);
    check(sheet.diagnostics.empty() && sheet.sheet && sheet.sheet->size() == 1,
          "a stylesheet parses straight out of a mounted resource");

    StyleParser::Result missing =
        StyleParser::parse_document(*ResourceUri::parse("res://ui/gone.vss"));
    check(missing.diagnostics.size() == 1 &&
              missing.diagnostics.front().file == "res://ui/gone.vss",
          "a missing document reports itself by URI, not by guessed path");

    StyleParser::Result broken =
        StyleParser::parse_document(*ResourceUri::parse("res://ui/broken.vss"));
    check(broken.diagnostics.size() == 1 &&
              broken.diagnostics.front().file == "res://ui/broken.vss",
          "a diagnostic inside a resource names the resource");

    StyleParser::ThemeResult theme = StyleParser::parse_theme_document(
        *ResourceUri::parse("res://theme/dark.vtheme"));
    check(theme.diagnostics.empty() && theme.theme &&
              theme.theme->name() == "Dark",
          "a theme parses out of a resource");
    check(theme.theme && theme.theme->contains("text") &&
              theme.theme->contains("surface"),
          "@base resolves relatively, inside the resource namespace");

    StyleParser::ThemeResult cycle = StyleParser::parse_theme_document(
        *ResourceUri::parse("res://theme/loop.vtheme"));
    check(!cycle.diagnostics.empty(),
          "a theme that bases itself is cut off with a diagnostic");

    check(Resources::global().unmount(id), "the test's mount comes back down");
  }

  // -- font-family values ---------------------------------------------------

  {
    const auto parsed = [](std::string_view text) {
      FontFamilyList list;
      return parse_style_value(text, list) ? list.to_string()
                                           : std::string("<invalid>");
    };

    check(parsed("\"Inter\", Helvetica Neue, sans-serif") ==
              "Inter, Helvetica Neue, sans-serif",
          "a family list round trips through parse and print");
    check(parsed("  Helvetica   Neue  ") == "Helvetica Neue",
          "runs of whitespace inside an unquoted name fold to one space");
    check(parsed("微软雅黑") == "微软雅黑",
          "an unquoted name may be written in its own script");
    check(parsed("\"Some, Font\"") == "\"Some, Font\"",
          "a comma inside a quoted name does not split the list");
    check(parsed("'Inter'") == "Inter",
          "single quotes work the way double quotes do");

    check(parsed("Inter, , sans-serif") == "<invalid>",
          "an empty slot invalidates the whole declaration");
    check(parsed("Inter,") == "<invalid>", "so does a trailing comma");
    check(parsed("\"Inter") == "<invalid>", "so does an unterminated string");
    check(parsed("2Fast") == "<invalid>",
          "an unquoted name may not start with a digit");

    FontFamilyList a;
    FontFamilyList b;
    parse_style_value("Inter, sans-serif", a);
    parse_style_value("Inter , sans-serif", b);
    check(a == b && a.hash() == b.hash(),
          "two lists that name the same families are one value");
    check(a.primary() == "Inter", "the first family is the preferred one");
    check(FontFamilyList{}.empty() && FontFamilyList{}.primary().empty(),
          "an unset font-family is the empty list");
  }

  // -- @font-face -----------------------------------------------------------

  {
    auto pack = std::make_shared<MemoryProvider>();
    pack->add("ui/app.vss",
              text_blob("@font-face {\n"
                        "  font-family: \"Inter\";\n"
                        "  src: local(\"Nothing Installed Here\"),\n"
                        "       url(\"../fonts/Inter.ttf\") format(\"truetype\");\n"
                        "  font-weight: 600;\n"
                        "}\n"
                        "button { font-family: \"Inter\", sans-serif; }\n"));
    pack->add("ui/bad.vss", text_blob("@font-face { font-family: \"X\"; }\n"
                                      "@font-face { src: url(\"a.ttf\"); }\n"
                                      "@font-face {\n"
                                      "  font-family: \"Y\";\n"
                                      "  src: url(\"../../outside.ttf\");\n"
                                      "}\n"));

    const Resources::MountId id = Resources::global().mount("", pack, 0);

    StyleParser::Result sheet =
        StyleParser::parse_document(*ResourceUri::parse("res://ui/app.vss"));
    check(sheet.diagnostics.empty(), "a @font-face rule parses cleanly");
    check(sheet.sheet && sheet.sheet->font_faces().size() == 1,
          "the rule lands on the sheet rather than in the registry");

    if (sheet.sheet && sheet.sheet->font_faces().size() == 1) {
      const FontFaceRule &rule = sheet.sheet->font_faces().front();
      check(rule.family == "Inter" && rule.weight == FontWeight::SemiBold,
            "the family and weight descriptors are read");
      check(rule.sources.size() == 2, "both sources are kept, in order");
      check(rule.sources[0].local &&
                rule.sources[0].family == "Nothing Installed Here",
            "local() keeps the family it names");
      check(!rule.sources[1].local &&
                rule.sources[1].uri.to_string() == "res://fonts/Inter.ttf" &&
                rule.sources[1].format == "truetype",
            "url() is resolved against the sheet, staying inside res:");
    }

    StyleParser::Result bad =
        StyleParser::parse_document(*ResourceUri::parse("res://ui/bad.vss"));
    check(bad.diagnostics.size() == 3,
          "a rule with no src, no family, or an escaping url is reported");
    check(bad.sheet && bad.sheet->font_faces().empty(),
          "and none of them reaches the sheet");

    check(Resources::global().unmount(id), "the test's mount comes back down");
  }

  // -- The font registry ----------------------------------------------------

  {
    FontRegistry registry;
    check(!registry.find("Inter", FontWeight::Normal),
          "an unregistered family is not found");

    // Not real font bytes: `find` matches names and weights, and loading is
    // somebody else's problem.
    registry.add("Inter", text_blob("regular"), FontWeight::Normal);
    registry.add("Inter", text_blob("bold"), FontWeight::Bold);

    const auto chosen = [&](FontWeight want) {
      const std::optional<RegisteredFace> face = registry.find("Inter", want);
      return face ? std::string(face->bytes.text()) : std::string("<none>");
    };

    check(chosen(FontWeight::Normal) == "regular", "an exact weight matches");
    check(chosen(FontWeight::Bold) == "bold", "so does the other one");
    check(chosen(FontWeight::Light) == "regular",
          "below 400 a lighter face is preferred -- 300 takes 400, not 700");
    check(chosen(FontWeight::Black) == "bold",
          "above 500 a heavier face is preferred");
    check(chosen(FontWeight::Medium) == "regular",
          "500 looks up only as far as 500, so 400 beats 700");

    check(registry.find("INTER", FontWeight::Normal).has_value() &&
              registry.find("  inter  ", FontWeight::Normal).has_value(),
          "family names match case-insensitively, ignoring space");
    check(registry.families().size() == 1,
          "two weights of one family are one family");

    registry.add("Inter", text_blob("replaced"), FontWeight::Normal);
    check(chosen(FontWeight::Normal) == "replaced",
          "re-registering a weight replaces it rather than piling up");

    check(registry.remove("Inter") && !registry.contains("Inter"),
          "a family can be taken back out");
  }

  // -- End to end: a packed face reaches the shaper --------------------------
  //
  // Everything above stops short of FreeType. This part registers a real face
  // through a real @font-face and asks the font layer to shape with it, which
  // is the only check that the whole path -- resource, blob, registry, family
  // list, stack -- actually lines up. It needs a font file to copy, and skips
  // itself when the machine has none rather than failing on a platform whose
  // fonts live somewhere else.

  {
    std::filesystem::path source;
    for (const char *candidate :
         {"C:/Windows/Fonts/segoeui.ttf", "C:/Windows/Fonts/arial.ttf",
          "/System/Library/Fonts/SFNS.ttf",
          "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"}) {
      std::error_code found;
      if (std::filesystem::is_regular_file(candidate, found) && !found) {
        source = candidate;
        break;
      }
    }

    if (source.empty()) {
      std::printf("skip  no font file to pack; end-to-end check not run\n");
    } else {
      std::filesystem::create_directories(root / "fonts");
      std::error_code copied;
      std::filesystem::copy_file(
          source, root / "fonts" / "packed.ttf",
          std::filesystem::copy_options::overwrite_existing, copied);
      write_file(root / "ui" / "app.vss",
                 "@font-face {\n"
                 "  font-family: \"VoidUI Packed Test\";\n"
                 "  src: url(\"../fonts/packed.ttf\");\n"
                 "}\n");

      const Resources::MountId id = Resources::global().mount(
          "", directory_provider(root.string()), 0);

      StyleParser::Result sheet =
          StyleParser::parse_document(*ResourceUri::parse("res://ui/app.vss"));
      check(sheet.diagnostics.empty() && sheet.sheet &&
                sheet.sheet->font_faces().size() == 1,
            "a @font-face over a loose directory parses");

      std::vector<std::string> problems;
      register_font_faces(*sheet.sheet, &problems);
      check(problems.empty(), "the packed face loads and registers");
      check(FontRegistry::global().contains("VoidUI Packed Test"),
            "the family is now one the registry knows");

      const FontFamilyList families =
          FontFamilyList::of({"VoidUI Packed Test", "sans-serif"});
      const std::shared_ptr<FontStack> stack =
          FontStack::cached(families, 16.0f);
      check(stack && stack->base() != nullptr,
            "a stack resolves the packed family to a real face");
      check(stack && stack->family() == "VoidUI Packed Test",
            "and reports the family it actually took");
      check(stack && stack->line_height() > 0.0f && stack->measure("Hi") > 0.0f,
            "the face shapes and measures text");

      const std::shared_ptr<FontStack> again =
          FontStack::cached(families, 16.0f);
      check(stack == again, "asking twice returns one shared stack");

      const std::shared_ptr<FontStack> larger =
          FontStack::cached(families, 32.0f);
      check(larger && larger->base() &&
                larger->line_height() > stack->line_height(),
            "the same packed bytes serve another size");

      FontRegistry::global().remove("VoidUI Packed Test");
      check(Resources::global().unmount(id), "the test's mount comes back down");
    }
  }

  // -- the network scheme ----------------------------------------------------
  //
  // Driven through a provider of our own. Nothing here touches a socket: what
  // is being checked is that a URL survives parsing with its authority intact,
  // that it reaches a provider rather than the mount table, and that the access
  // policy is applied to documents and not to C++.

  {
    const ResourceResult<ResourceUri> url =
        ResourceUri::parse("https://example.com/icons/user.png");
    check(url && url->is_network() && !url->is_resource() && !url->is_file(),
          "an https reference parses as a network uri");
    check(url && url->scheme() == "https" &&
              url->path() == "example.com/icons/user.png",
          "with the authority kept as the head of the path");
    check(url && url->to_string() == "https://example.com/icons/user.png",
          "and prints back exactly as it arrived");

    const ResourceResult<ResourceUri> plain =
        ResourceUri::parse("http://example.com/a.png");
    check(plain && plain->is_network() && plain->scheme() == "http",
          "http parses too, and keeps its own scheme");

    check(!ResourceUri::parse("https://example.com/../../etc/passwd"),
          "a traversal that would climb past the authority is refused");

    const ResourceResult<ResourceUri> sibling = url->resolve("avatar.png");
    check(sibling && sibling->to_string() == "https://example.com/icons/avatar.png",
          "a relative reference resolves beside it and inherits the scheme");

    const ResourceResult<ResourceUri> query =
        ResourceUri::parse("https://example.com/a.png?w=64");
    check(query && query->to_string() == "https://example.com/a.png?w=64",
          "a query string rides along rather than being parsed or dropped");
  }

  {
    Resources resources;

    const ResourceUri url = *ResourceUri::parse("https://example.com/a.png");
    check(!resources.open(url) &&
              resources.open(url).error() == ResourceError::NotMounted,
          "with no network provider installed, a url is unreachable");

    auto network = std::make_shared<MemoryProvider>();
    // The provider is handed the whole uri, scheme and authority included --
    // unlike a mount there is nothing to strip, because the name is the address.
    network->add("https://example.com/a.png", text_blob("remote-bytes"));
    resources.set_network_provider(network);

    const ResourceResult<Blob> blob = resources.open(url);
    check(blob && blob->text() == "remote-bytes",
          "and with one, the uri reaches it whole");

    check(!resources.native_path(url),
          "a remote resource reports no native path, cached copy or not");

    // The policy governs documents, not C++ -- exactly as NativeAccess does.
    check(resources.network_access() == Resources::NetworkAccess::Never,
          "a document may not reach the network unless the application says so");

    const ResourceUri embedded = *ResourceUri::parse("res://theme/dark.vss");
    check(!resources.open("https://example.com/a.png", embedded) &&
              resources.open("https://example.com/a.png", embedded).error() ==
                  ResourceError::Denied,
          "so a stylesheet's url() is refused by default");

    check(resources.open(url) && resources.open(url)->text() == "remote-bytes",
          "while code naming the uri directly is not, because it could open a "
          "socket anyway");

    resources.set_network_access(Resources::NetworkAccess::SameOrigin);
    check(!resources.open("https://example.com/a.png", embedded),
          "same-origin still refuses a document that came from elsewhere");

    const ResourceUri remote_document =
        *ResourceUri::parse("https://example.com/theme.vss");
    check(resources.open("a.png", remote_document).has_value(),
          "and admits one that came from the network itself");

    resources.set_network_access(Resources::NetworkAccess::Always);
    check(resources.open("https://example.com/a.png", embedded).has_value(),
          "always admits every document, which is an application's to choose");
  }

  // -- Blob ownership -------------------------------------------------------

  {
    static constexpr std::array<std::byte, 4> stat1{
        std::byte{'v'}, std::byte{'o'}, std::byte{'i'}, std::byte{'d'}};

    Blob borrowed = Blob::borrow(stat1);
    check(borrowed.data() == stat1.data(),
          "a borrowed blob points at the original bytes, copying nothing");
    check(borrowed.text() == "void", "a borrowed blob reads back as text");

    Blob copy = borrowed;
    check(copy.data() == stat1.data(), "copying a blob does not copy bytes");

    Blob owned = text_blob("owned");
    const std::byte *address = owned.data();
    Blob moved_on = owned;
    owned = Blob();
    check(moved_on.data() == address && moved_on.text() == "owned",
          "an owned blob survives its last named holder going away");
    check(Blob().empty(), "a default blob is empty");
  }

  std::filesystem::remove_all(root, cleanup);

  std::printf("\n%s (%d failure%s)\n", failures ? "FAILED" : "PASSED", failures,
              failures == 1 ? "" : "s");
  return failures == 0 ? 0 : 1;
}
