set_xmakever("3.1.1")
add_rules("mode.debug", "mode.release")
set_languages("c99")

target("freetype")
    set_kind("static")
    add_defines("FT2_BUILD_LIBRARY")
    add_includedirs("include")
    add_headerfiles("include/(ft2build.h)", "include/(freetype/**.h)", {prefixdir = "freetype2"})
    add_files("src/base/ftbase.c", "src/base/ftbbox.c", "src/base/ftbdf.c", "src/base/ftbitmap.c",
              "src/base/ftcid.c", "src/base/ftfstype.c", "src/base/ftgasp.c", "src/base/ftglyph.c",
              "src/base/ftgxval.c", "src/base/ftinit.c", "src/base/ftmm.c", "src/base/ftotval.c",
              "src/base/ftpatent.c", "src/base/ftpfr.c", "src/base/ftstroke.c", "src/base/ftsynth.c",
              "src/base/fttype1.c", "src/base/ftwinfnt.c")
    for _, module in ipairs({"autofit/autofit", "bdf/bdf", "bzip2/ftbzip2", "cache/ftcache", "cff/cff",
                            "cid/type1cid", "gzip/ftgzip", "lzw/ftlzw", "pcf/pcf", "pfr/pfr",
                            "psaux/psaux", "pshinter/pshinter", "psnames/psnames", "raster/raster",
                            "sdf/sdf", "sfnt/sfnt", "smooth/smooth", "svg/svg", "truetype/truetype",
                            "type1/type1", "type42/type42", "winfonts/winfnt"}) do
        add_files("src/" .. module .. ".c")
    end
    -- Upstream's portable configuration uses bundled zlib and no optional
    -- PNG/Brotli/BZip2/HarfBuzz dependencies, matching the library's needs.
    if is_plat("windows") then
        add_defines("_CRT_SECURE_NO_WARNINGS", "_CRT_NONSTDC_NO_WARNINGS")
        add_files("builds/windows/ftsystem.c", "builds/windows/ftdebug.c")
    else
        add_files("src/base/ftsystem.c", "src/base/ftdebug.c")
    end
