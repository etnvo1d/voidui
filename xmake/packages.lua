-- All library dependencies are built from source using the native ports below.
package("voidui-sdl3")
    set_description("VoidUI's minimal static SDL3")
    set_license("zlib")
    add_urls("https://github.com/libsdl-org/SDL.git")
    add_versions("3.4.14", "release-3.4.14")
    add_configs("buildsystem", {default = "xmake-v1", type = "string", readonly = true})
    on_load(function (package)
        package:add("links", "SDL3-static")
        if package:is_plat("windows") then
            package:add("syslinks", "user32", "gdi32", "winmm", "imm32", "ole32", "oleaut32", "version", "uuid", "advapi32", "setupapi", "shell32")
        elseif package:is_plat("macosx") then
            package:add("frameworks", "Cocoa", "Carbon", "IOKit", "CoreVideo", "CoreFoundation", "Foundation", "Metal", "QuartzCore", "UniformTypeIdentifiers")
            package:add("syslinks", "iconv")
        else
            package:add("syslinks", "X11", "Xext", "pthread", "dl", "m")
        end
    end)
    on_install(function (package)
        os.cp(path.join(package:scriptdir(), "xmake/ports/sdl3.lua"), "xmake.lua")
        os.cp(path.join(package:scriptdir(), "xmake/ports/sdl3-linux.h"), "SDL_build_config_linux.h")
        import("package.tools.xmake").install(package)
    end)
    on_test(function (package)
        assert(package:check_csnippets({test = [[
            #include <SDL3/SDL.h>
            int main(void) { SDL_Init(0); SDL_Quit(); return 0; }
        ]]}))
    end)
package_end()

package("voidui-freetype")
    set_description("FreeType without optional compression and shaping libraries")
    set_license("FTL")
    add_urls("https://github.com/freetype/freetype.git")
    add_versions("2.14.3", "VER-2-14-3")
    add_configs("buildsystem", {default = "xmake-v1", type = "string", readonly = true})
    add_includedirs("include/freetype2")
    add_links("freetype")
    on_install(function (package)
        os.cp(path.join(package:scriptdir(), "xmake/ports/freetype.lua"), "xmake.lua")
        import("package.tools.xmake").install(package)
    end)
    on_test(function (package)
        assert(package:has_cfuncs("FT_Init_FreeType", {includes = {"ft2build.h", "freetype/freetype.h"}}))
    end)
package_end()

package("voidui-harfbuzz")
    set_description("HarfBuzz with independent font table loading")
    set_license("MIT")
    -- Exclude upstream test fixtures with filenames exceeding Windows limits.
    add_urls("https://github.com/harfbuzz/harfbuzz/archive/refs/tags/$(version).tar.gz", {excludes = {"*/test"}})
    add_versions("14.4.0", "46dc4f3b6aefc4d8256b10017186f5ebe50ea086714ab8948cdac4695a7a80a8")
    add_configs("buildsystem", {default = "xmake-v1", type = "string", readonly = true})
    add_includedirs("include/harfbuzz")
    add_links("harfbuzz")
    on_install(function (package)
        os.cp(path.join(package:scriptdir(), "xmake/ports/harfbuzz.lua"), "xmake.lua")
        import("package.tools.xmake").install(package)
    end)
    on_test(function (package)
        assert(package:has_cxxfuncs("hb_buffer_create", {includes = "hb.h"}))
    end)
package_end()

package("voidui-slang")
    set_kind("binary")
    set_description("Slang shader compiler (prebuilt host tool)")
    set_license("Apache-2.0")
    local host = os.host() == "macosx" and "macos" or os.host()
    local arch = os.arch() == "arm64" and "aarch64" or "x86_64"
    local checksums = {
        ["windows-x86_64"] = "0fd3e6a9a5d05ed4cdd000d467f1ffb5d9701b827e83bfb428902a45c37ef8a5",
        ["windows-aarch64"] = "315a18a2ee56803bf558778d91481b47cefb51df14207342afdc9a4d9166c588",
        ["linux-x86_64"] = "61075e5ac817621233c7f444e5258b5bbdb262920c09479f2054b3add3760abf",
        ["linux-aarch64"] = "b573bff97ffadeb158e4952d5d46aff0b7015c13284b577b0d9ac389c95e9711",
        ["macos-x86_64"] = "d2a0c98c94df0a2d1ca3857fe6fa831ebb090207efef89ce0e48704b944f85d9",
        ["macos-aarch64"] = "f9b958ccf3f35408f5618f7e455d7b4efdea9959f21befc8dd5d9d0963cc5645"
    }
    add_urls("https://github.com/shader-slang/slang/releases/download/v$(version)/slang-$(version)-" .. host .. "-" .. arch .. ".zip")
    add_versions("2026.16.1", checksums[host .. "-" .. arch])
    on_install(function (package)
        os.cp("bin", package:installdir())
        if os.isdir("lib") then os.cp("lib", package:installdir()) end
        if os.isdir("LICENSES") then os.cp("LICENSES", package:installdir()) end
    end)
package_end()
