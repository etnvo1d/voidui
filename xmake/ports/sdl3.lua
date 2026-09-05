set_xmakever("3.1.1")
add_rules("mode.debug", "mode.release")
set_languages("c11", "c++17")
set_allowedplats("windows", "linux", "macosx")

target("SDL3-static")
    set_kind("static")
    add_defines("SDL_STATIC_LIB")
    add_includedirs("include", "src", "include/build_config", "src/video/khronos")
    add_headerfiles("include/(SDL3/*.h)")
    add_files("src/*.c")
    for _, dir in ipairs({"atomic", "audio", "camera", "core", "cpuinfo", "dynapi", "events",
                         "io", "io/generic", "filesystem", "gpu", "joystick", "haptic", "hidapi",
                         "locale", "main", "main/generic", "misc", "power", "process", "dialog", "tray", "render", "sensor",
                         "stdlib", "storage", "storage/generic", "thread", "time", "timer",
                         "video", "video/yuv2rgb", "libm", "video/dummy", "video/offscreen",
                         "audio/dummy", "joystick/dummy", "haptic/dummy", "sensor/dummy", "camera/dummy",
                         "dialog/dummy", "tray/dummy"}) do
        add_files("src/" .. dir .. "/*.c")
    end
    if is_plat("windows") then
        add_defines("_CRT_SECURE_NO_WARNINGS")
        for _, dir in ipairs({"core/windows", "main/windows", "io/windows", "filesystem/windows",
                             "locale/windows", "misc/windows", "loadso/windows", "process/windows",
                             "thread/windows", "time/windows", "timer/windows", "video/windows"}) do
            add_files("src/" .. dir .. "/*.c")
        end
        add_files("src/thread/generic/SDL_syscond.c", "src/thread/generic/SDL_sysrwlock.c")
        add_files("src/core/windows/SDL_gameinput.cpp", "src/video/windows/*.cpp")
        add_syslinks("user32", "gdi32", "winmm", "imm32", "ole32", "oleaut32", "version", "uuid", "advapi32", "setupapi", "shell32")
    else
        for _, dir in ipairs({"thread/pthread", "time/unix", "timer/unix", "loadso/dlopen", "process/posix"}) do
            add_files("src/" .. dir .. "/*.c")
        end
        add_files("src/filesystem/posix/SDL_sysfsops.c")
        if is_plat("macosx") then
            for _, dir in ipairs({"filesystem/cocoa", "locale/macos", "misc/macos", "video/cocoa"}) do
                add_files("src/" .. dir .. "/*.m")
            end
            add_mflags("-fobjc-arc")
            add_frameworks("Cocoa", "Carbon", "IOKit", "CoreVideo", "CoreFoundation", "Foundation", "Metal", "QuartzCore", "UniformTypeIdentifiers")
            add_syslinks("iconv")
        else
            add_files("src/core/unix/*.c", "src/core/linux/*.c", "src/filesystem/unix/SDL_sysfilesystem.c",
                      "src/locale/unix/*.c", "src/misc/unix/*.c", "src/video/x11/*.c")
            add_syslinks("X11", "Xext", "pthread", "dl", "m")
        end
    end
    on_load(function (target)
        local generated = path.join(target:autogendir(), "config")
        os.mkdir(generated)
        target:set("includedirs", table.join({generated}, target:get("includedirs")))
        local platform = target:is_plat("windows") and "windows" or (target:is_plat("macosx") and "macos" or "linux")
        local configfile = platform == "linux" and "SDL_build_config_linux.h" or ("include/build_config/SDL_build_config_" .. platform .. ".h")
        local upstream = io.readfile(configfile)
        local config = {"// Generated VoidUI SDL configuration.\n#pragma once\n", '#include "SDL_build_config_' .. platform .. '.h"\n'}
        if platform == "linux" then
            os.cp(configfile, path.join(generated, "SDL_build_config_linux.h"))
        end
        -- Keep upstream platform/CRT detection; disable unused subsystems and
        -- their backends so a driver can never be enabled without its sources.
        for macro in upstream:gmatch("#%s*define%s+(SDL_[%w_]+)") do
            if macro:match("^SDL_AUDIO_DRIVER_") or macro:match("^SDL_JOYSTICK_") or
               macro:match("^SDL_HAPTIC_") or macro:match("^SDL_SENSOR_") or
               macro:match("^SDL_POWER_") or macro:match("^SDL_CAMERA_DRIVER_") or
               macro:match("^SDL_GPU_") or macro:match("^SDL_VIDEO_RENDER_") or
               macro:match("^SDL_VIDEO_OPENGL") or (platform == "macos" and macro:match("^SDL_VIDEO_DRIVER_X11")) then
                table.insert(config, "#undef " .. macro .. "\n")
            end
        end
        for _, subsystem in ipairs({"AUDIO", "JOYSTICK", "HAPTIC", "HIDAPI", "SENSOR", "POWER", "CAMERA", "GPU", "RENDER"}) do
            table.insert(config, "#define SDL_" .. subsystem .. "_DISABLED 1\n")
        end
        for _, subsystem in ipairs({"JOYSTICK", "HAPTIC", "SENSOR", "DIALOG", "TRAY"}) do
            table.insert(config, "#define SDL_" .. subsystem .. "_DUMMY 1\n")
        end
        table.insert(config, "#define SDL_STORAGE_GENERIC 1\n#define SDL_AUDIO_DRIVER_DUMMY 1\n#define SDL_CAMERA_DRIVER_DUMMY 1\n")
        io.writefile(path.join(generated, "SDL_build_config.h"), table.concat(config))
        io.writefile(path.join(generated, "SDL_revision.h"), '#define SDL_REVISION "release-3.4.14"\n')
    end)
