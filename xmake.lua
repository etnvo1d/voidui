set_xmakever("3.1.1")
set_project("voidui")
set_languages("c++23")
set_allowedplats("windows", "linux", "macosx")
add_rules("mode.debug", "mode.release", "mode.releasedbg")
add_rules("plugin.compile_commands.autoupdate", {outputdir = ".", lsp = "clangd"})

option("examples", {default = true, description = "Build the examples"})
option("tools", {default = true, description = "Build developer tools and self-tests"})
option("slangc", {description = "Use an existing Slang compiler instead of downloading it", type = "string"})

includes("xmake/packages.lua")
includes("xmake/shaders.lua")
add_requires("voidui-sdl3 3.4.14", "voidui-freetype 2.14.3", "voidui-harfbuzz 14.4.0",
             {system = false, configs = {shared = false}})
if not has_config("slangc") then
    add_requires("voidui-slang 2026.16.1", {host = true, system = false})
end
if is_plat("linux") then
    add_requires("vulkansdk")
end
if is_plat("windows") then
    set_runtimes(is_mode("debug") and "MDd" or "MD")
    add_cxflags("/utf-8", "/permissive-", {tools = {"cl", "clang_cl"}})
end

target("voidui")
    set_kind("static")
    set_warnings("allextra")
    add_includedirs("include", {public = true})
    add_includedirs("src")
    add_headerfiles("include/(voidui/**.h)")
    add_files("src/core/**.cpp", "src/paint/**.cpp", "src/render/**.cpp", "src/sdl/**.cpp")
    add_packages("voidui-sdl3", "voidui-freetype", "voidui-harfbuzz")
    add_rules("voidui.shaders")
    if not has_config("slangc") then
        add_packages("voidui-slang")
    end
    if is_plat("windows") then
        remove_files("src/core/http_null.cpp", "src/paint/font_provider_null.cpp", "src/paint/image_codec_null.cpp")
        add_files("src/rhi/device_d3d11.cpp")
        add_syslinks("dwrite", "windowscodecs", "winhttp", "d3d11", "dxgi")
    else
        remove_files("src/core/http_win.cpp", "src/paint/font_provider_win.cpp", "src/paint/image_codec_win.cpp")
        if is_plat("macosx") then
            add_files("src/rhi/device_metal.mm", {mxxflags = "-fobjc-arc"})
            add_frameworks("Foundation", "Metal", "QuartzCore")
        else
            add_files("src/rhi/device_vulkan.cpp")
            add_packages("vulkansdk")
            add_syslinks("pthread")
        end
    end
target_end()

if has_config("examples") then
    for _, source in ipairs(os.files("examples/*.cpp")) do
        local name = path.basename(source)
        target("voidui_example_" .. name)
            set_kind("binary")
            set_basename(name)
            set_targetdir("$(builddir)/$(plat)/$(arch)/$(mode)/examples")
            set_rundir(".")
            add_files(source)
            add_deps("voidui")
        target_end()
    end
end

if has_config("tools") then
    for _, source in ipairs(os.files("tools/*.cpp")) do
        local name = path.basename(source)
        if name ~= "font_probe_win" or is_plat("windows") then
            target("voidui_" .. name)
                set_kind("binary")
                set_group("tools")
                set_rundir(".")
                add_files(source)
                add_deps("voidui")
                if name:endswith("_selftest") then
                    -- These tests use assert(), including in Release builds.
                    add_undefines("NDEBUG")
                    add_tests("default")
                elseif name == "font_probe_win" then
                    add_syslinks("dwrite", "user32")
                end
            target_end()
        end
    end
end
