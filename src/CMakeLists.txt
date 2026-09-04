add_library(voidui STATIC)
add_library(voidui::voidui ALIAS voidui)

find_package(Threads REQUIRED)

if(WIN32)
    set(VOIDUI_FONT_PROVIDER "${CMAKE_CURRENT_SOURCE_DIR}/paint/font_provider_win.cpp")
    set(VOIDUI_IMAGE_CODEC "${CMAKE_CURRENT_SOURCE_DIR}/paint/image_codec_win.cpp")
    set(VOIDUI_HTTP "${CMAKE_CURRENT_SOURCE_DIR}/core/http_win.cpp")
else()
    set(VOIDUI_FONT_PROVIDER "${CMAKE_CURRENT_SOURCE_DIR}/paint/font_provider_null.cpp")
    set(VOIDUI_IMAGE_CODEC "${CMAKE_CURRENT_SOURCE_DIR}/paint/image_codec_null.cpp")
    set(VOIDUI_HTTP "${CMAKE_CURRENT_SOURCE_DIR}/core/http_null.cpp")
endif()

target_sources(
    voidui

    PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}/core/widget_tree.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/async/executor.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/async/thread_pool.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/component.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/color.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/cursor.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/gradient.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/input.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/selection.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/resource.cpp"
        "${VOIDUI_HTTP}"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/value.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/easing.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/animation.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/animator.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/property.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/declaration.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/selector.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/stylesheet.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/svg.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/computed.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/theme.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/resolver.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/parser.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/core/style/hot_reload.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/font.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/font_registry.cpp"
        "${VOIDUI_FONT_PROVIDER}"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/image.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/image_codec.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/image_cache.cpp"
        "${VOIDUI_IMAGE_CODEC}"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/path.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/svg.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/svg_cache.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/text_layout.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/paint/painter.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/render/atlas.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/render/gradient_table.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/render/glyph_cache.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/render/rasterizer.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/render/gpu/renderer.cpp"
        "${CMAKE_CURRENT_SOURCE_DIR}/sdl/window.cpp"

    PUBLIC
        FILE_SET public_headers
        TYPE HEADERS
        BASE_DIRS "${PROJECT_SOURCE_DIR}/include"
        FILES
            "${PROJECT_SOURCE_DIR}/include/voidui/core/color.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/async.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/async/executor.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/async/thread_pool.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/invalidation.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/geometry.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/widget.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/component.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/state.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/event.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/keycode.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/border.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/layout.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/window.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/gradient.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/element.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/value.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/easing.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/animation.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/property.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/declaration.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/selector.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/stylesheet.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/svg.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/computed.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/theme.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/resolver.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/parser.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/style/hot_reload.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/widget_tree.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/context.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/application.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/cursor.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/selection.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/transform.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/typography.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/typemap.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/resource.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/core/http.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/font.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/font_provider.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/font_registry.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/image.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/image_codec.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/image_cache.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/paint.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/path.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/svg.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/svg_cache.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/text_layout.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/display_list.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/paint/painter.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/render/renderer.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/render/shape.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/render/brush.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/button.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/canvas.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/column.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/row.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/scrollable.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/text.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/input.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/image.h"
            "${PROJECT_SOURCE_DIR}/include/voidui/widgets/svg.h"
)

target_compile_features(
    voidui
    PUBLIC cxx_std_23
)

target_include_directories(
    voidui

    PUBLIC
        $<BUILD_INTERFACE:${PROJECT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>

    PRIVATE
        "${CMAKE_CURRENT_SOURCE_DIR}"
)

target_link_libraries(
    voidui

    PRIVATE
        SDL3::SDL3
        Threads::Threads
        voidui_shaders
        freetype
        harfbuzz
        $<$<BOOL:${WIN32}>:dwrite>
        $<$<BOOL:${WIN32}>:windowscodecs>
        $<$<BOOL:${WIN32}>:winhttp>
)

if(WIN32)
    target_sources(voidui PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/rhi/device_d3d11.cpp")
    target_link_libraries(voidui PRIVATE d3d11 dxgi)
elseif(APPLE)
    enable_language(OBJCXX)
    target_sources(voidui PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/rhi/device_metal.mm")
    set_source_files_properties(
        "${CMAKE_CURRENT_SOURCE_DIR}/rhi/device_metal.mm"
        PROPERTIES COMPILE_OPTIONS "-fobjc-arc"
    )
    target_link_libraries(voidui PRIVATE
        "-framework Foundation"
        "-framework Metal"
        "-framework QuartzCore"
    )
else()
    find_package(Vulkan REQUIRED)
    target_sources(voidui PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}/rhi/device_vulkan.cpp")
    target_link_libraries(voidui PRIVATE Vulkan::Vulkan)
endif()

if(MSVC)
    target_compile_options(
        voidui
        PRIVATE
            /W4
            /permissive-
            /utf-8
    )
else()
    target_compile_options(
        voidui
        PRIVATE
            -Wall
            -Wextra
            -Wpedantic
    )
endif()
