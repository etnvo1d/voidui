# VoidUI

A C++23 UI library. The project and all third-party libraries are built natively with xmake; CMake is not required. It uses D3D11 on Windows, Metal on macOS, and Vulkan on Linux.

## Prerequisites

- [xmake](https://xmake.io/guide/quick-start) 3.1.1 or later, available on `PATH`.
- Git. The initial configuration requires an internet connection to download dependency sources and the Slang compiler.
- Windows: the Desktop development with C++ workload from Visual Studio or Build Tools, plus the Windows SDK. xmake detects MSVC automatically, so it does not need to be launched from a Developer Command Prompt.
- macOS: Xcode Command Line Tools. Linux: a C++23-capable compiler, the Vulkan SDK, and the X11/Xext development libraries. Linux windows use X11. Wayland sessions require XWayland; the current native build configuration does not include SDL's native Wayland backend.
- [clangd](https://clangd.llvm.org/installation), preferably a recent version that matches your compiler, available on `PATH`.

Dependencies are pinned to SDL 3.4.14, FreeType 2.14.3, HarfBuzz 14.4.0, and Slang 2026.16.1. The first three are built from source as static libraries using scripts under `xmake/ports/`, without invoking their upstream build systems. Like the C/C++ compiler, Slang is a build tool. A precompiled version for the host is downloaded automatically, or an existing `slangc` executable can be specified.

## Build and Run

```sh
xmake f -m debug -y
xmake
xmake run voidui_example_counter
xmake test -v
```

Files matching `examples/*.cpp` are automatically registered as `voidui_example_<filename>`. Files matching `tools/*_selftest.cpp` are automatically registered as `voidui_<filename>` and added to `xmake test`. Assertions remain enabled in release self-tests. `image_probe` and the Windows-only `font_probe_win` are diagnostic tools and are not run as tests.

```sh
xmake f -m release
xmake
# Build only the library
xmake f --examples=n --tools=n
xmake
# Re-enable examples and tools
xmake f --examples=y --tools=y
# Use an installed Slang compiler (pass the executable path)
xmake f --slangc=/absolute/path/to/slangc
```
