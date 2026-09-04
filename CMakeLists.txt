cmake_minimum_required(VERSION 3.24)
project(voidui LANGUAGES C CXX)

include(FetchContent)

option(VOIDUI_BUILD_EXAMPLES "Build VoidUI examples" ON)
option(VOIDUI_BUILD_TOOLS "Build VoidUI developer tools and self-tests" ON)

set(CMAKE_EXPORT_COMPILE_COMMANDS ON)

# SDL3

set(SDL_SHARED OFF CACHE BOOL "" FORCE)
set(SDL_STATIC ON CACHE BOOL "" FORCE)
set(SDL_TESTS OFF CACHE BOOL "" FORCE)
set(SDL_TEST_LIBRARY OFF CACHE BOOL "" FORCE)
set(SDL_EXAMPLES OFF CACHE BOOL "" FORCE)
set(SDL_AUDIO OFF CACHE BOOL "" FORCE)
set(SDL_CAMERA OFF CACHE BOOL "" FORCE)
set(SDL_JOYSTICK OFF CACHE BOOL "" FORCE)
set(SDL_HAPTIC OFF CACHE BOOL "" FORCE)
set(SDL_HIDAPI OFF CACHE BOOL "" FORCE)
set(SDL_POWER OFF CACHE BOOL "" FORCE)
set(SDL_SENSOR OFF CACHE BOOL "" FORCE)
set(SDL_DIALOG OFF CACHE BOOL "" FORCE)
set(SDL_TRAY OFF CACHE BOOL "" FORCE)
set(SDL_GPU OFF CACHE BOOL "" FORCE)
set(SDL_RENDER OFF CACHE BOOL "" FORCE)
set(SDL_OPENGL OFF CACHE BOOL "" FORCE)
set(SDL_OPENGLES OFF CACHE BOOL "" FORCE)
set(SDL_DISABLE_INSTALL ON CACHE BOOL "" FORCE)
set(SDL_DISABLE_INSTALL_DOCS ON CACHE BOOL "" FORCE)

FetchContent_Declare(
    SDL3
    GIT_REPOSITORY https://github.com/libsdl-org/SDL.git
    GIT_TAG release-3.4.14
    GIT_SHALLOW TRUE
)

FetchContent_MakeAvailable(SDL3)

# Text
#
# FreeType rasterises glyph outlines; HarfBuzz shapes runs. They are kept
# decoupled -- HarfBuzz reads the font tables itself rather than going through
# an FT_Face, so shaping stays independent of whatever pixel size the
# rasteriser happens to be set to.

set(FT_DISABLE_ZLIB      ON CACHE BOOL "" FORCE)
set(FT_DISABLE_BZIP2     ON CACHE BOOL "" FORCE)
set(FT_DISABLE_PNG       ON CACHE BOOL "" FORCE)
set(FT_DISABLE_BROTLI    ON CACHE BOOL "" FORCE)
set(FT_DISABLE_HARFBUZZ  ON CACHE BOOL "" FORCE)

FetchContent_Declare(
    freetype
    GIT_REPOSITORY https://github.com/freetype/freetype.git
    GIT_TAG VER-2-14-3
    GIT_SHALLOW TRUE
)
FetchContent_MakeAvailable(freetype)

set(HB_BUILD_SUBSET  OFF CACHE BOOL "" FORCE)
set(HB_BUILD_TESTS   OFF CACHE BOOL "" FORCE)
set(HB_BUILD_UTILS   OFF CACHE BOOL "" FORCE)
set(HB_HAVE_FREETYPE OFF CACHE BOOL "" FORCE)

FetchContent_Declare(
    harfbuzz
    GIT_REPOSITORY https://github.com/harfbuzz/harfbuzz.git
    GIT_TAG 14.4.0
    GIT_SHALLOW TRUE
)
FetchContent_MakeAvailable(harfbuzz)

# Shaders
#
# SDL supplies windows and input. Rendering uses the native D3D11, Metal or
# Vulkan backend selected by the target platform. Slang compiles one shader
# source into that backend's format and bakes it into a header.
list(APPEND CMAKE_MODULE_PATH "${CMAKE_CURRENT_SOURCE_DIR}/cmake")
include(VoidUIShaders)

set(VOIDUI_SHADER_COMMON "${CMAKE_CURRENT_SOURCE_DIR}/shaders/common.slang")

voidui_add_shader_library(
    voidui_quad_shader
    SOURCE   "${CMAKE_CURRENT_SOURCE_DIR}/shaders/quad.slang"
    NAME     quad
    VERTEX   vs_main
    FRAGMENT fs_main
    DEPENDS  "${VOIDUI_SHADER_COMMON}"
)

voidui_add_shader_library(
    voidui_mask_shader
    SOURCE   "${CMAKE_CURRENT_SOURCE_DIR}/shaders/mask.slang"
    NAME     mask
    VERTEX   vs_main
    FRAGMENT fs_main
    DEPENDS  "${VOIDUI_SHADER_COMMON}"
)

add_library(voidui_shaders INTERFACE)
target_link_libraries(voidui_shaders INTERFACE voidui_quad_shader voidui_mask_shader)

add_subdirectory(src)

if(VOIDUI_BUILD_EXAMPLES)
    add_subdirectory(examples)
endif()

if(VOIDUI_BUILD_TOOLS)
    enable_testing()
    add_subdirectory(tools)
endif()
