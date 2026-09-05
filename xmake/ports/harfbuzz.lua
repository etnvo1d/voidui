set_xmakever("3.1.1")
add_rules("mode.debug", "mode.release")
set_languages("c++17")

target("harfbuzz")
    set_kind("static")
    -- Upstream's supported amalgamation includes the built-in OpenType shaper
    -- and Unicode tables, without requiring configure or generated code.
    add_files("src/harfbuzz.cc")
    add_includedirs("src")
    add_headerfiles("src/(hb*.h)", {prefixdir = "harfbuzz"})
    if is_plat("windows") then
        add_defines("_CRT_SECURE_NO_WARNINGS")
        add_cxxflags("/bigobj", {tools = {"cl", "clang_cl"}})
    end
