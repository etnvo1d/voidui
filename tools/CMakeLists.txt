# A standalone exercise of the style system: parsing, combinators, the shadow
# boundary, themes, incremental re-resolution and computed-style sharing.
# Links only voidui, and asserts its way through every behaviour the design
# promises, so a regression shows up as a named failing line.
add_executable(voidui_style_selftest "${CMAKE_CURRENT_SOURCE_DIR}/style_selftest.cpp")
target_link_libraries(voidui_style_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_style_selftest PRIVATE cxx_std_20)

add_test(NAME style_selftest COMMAND voidui_style_selftest)

add_executable(voidui_layout_selftest "${CMAKE_CURRENT_SOURCE_DIR}/layout_selftest.cpp")
target_link_libraries(voidui_layout_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_layout_selftest PRIVATE cxx_std_20)

add_test(NAME layout_selftest COMMAND voidui_layout_selftest)

add_executable(voidui_scrollable_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/scrollable_selftest.cpp")
target_link_libraries(voidui_scrollable_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_scrollable_selftest PRIVATE cxx_std_20)

add_test(NAME scrollable_selftest COMMAND voidui_scrollable_selftest)

add_executable(voidui_component_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/component_selftest.cpp")
target_link_libraries(voidui_component_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_component_selftest PRIVATE cxx_std_20)

add_test(NAME component_selftest COMMAND voidui_component_selftest)

add_executable(voidui_animation_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/animation_selftest.cpp")
target_link_libraries(voidui_animation_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_animation_selftest PRIVATE cxx_std_20)

add_test(NAME animation_selftest COMMAND voidui_animation_selftest)

add_executable(voidui_transition_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/transition_selftest.cpp")
target_link_libraries(voidui_transition_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_transition_selftest PRIVATE cxx_std_20)

add_test(NAME transition_selftest COMMAND voidui_transition_selftest)

add_executable(voidui_selection_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/selection_selftest.cpp")
target_link_libraries(voidui_selection_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_selection_selftest PRIVATE cxx_std_20)

add_test(NAME selection_selftest COMMAND voidui_selection_selftest)

add_executable(voidui_transform_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/transform_selftest.cpp")
target_link_libraries(voidui_transform_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_transform_selftest PRIVATE cxx_std_20)

add_test(NAME transform_selftest COMMAND voidui_transform_selftest)

add_executable(voidui_input_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/input_selftest.cpp")
target_link_libraries(voidui_input_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_input_selftest PRIVATE cxx_std_20)

add_test(NAME input_selftest COMMAND voidui_input_selftest)

add_executable(voidui_color_selftest "${CMAKE_CURRENT_SOURCE_DIR}/color_selftest.cpp")
target_link_libraries(voidui_color_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_color_selftest PRIVATE cxx_std_20)

add_test(NAME color_selftest COMMAND voidui_color_selftest)

add_executable(voidui_async_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/async_selftest.cpp")
target_link_libraries(voidui_async_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_async_selftest PRIVATE cxx_std_23)

add_test(NAME async_selftest COMMAND voidui_async_selftest)

add_executable(voidui_resource_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/resource_selftest.cpp")
target_link_libraries(voidui_resource_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_resource_selftest PRIVATE cxx_std_23)

add_test(NAME resource_selftest COMMAND voidui_resource_selftest)

add_executable(voidui_image_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/image_selftest.cpp")
target_link_libraries(voidui_image_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_image_selftest PRIVATE cxx_std_23)
target_include_directories(voidui_image_selftest PRIVATE "${CMAKE_CURRENT_SOURCE_DIR}")

add_test(NAME image_selftest COMMAND voidui_image_selftest)

# Not a test: a diagnostic for one URI, reporting which stage of the image path
# it failed at. `voidui_image_probe <uri>`.
add_executable(voidui_image_probe "${CMAKE_CURRENT_SOURCE_DIR}/image_probe.cpp")
target_link_libraries(voidui_image_probe PRIVATE voidui::voidui)
target_compile_features(voidui_image_probe PRIVATE cxx_std_23)

add_executable(voidui_svg_selftest
               "${CMAKE_CURRENT_SOURCE_DIR}/svg_selftest.cpp")
target_link_libraries(voidui_svg_selftest PRIVATE voidui::voidui)
target_compile_features(voidui_svg_selftest PRIVATE cxx_std_23)
if(MSVC)
    target_compile_options(voidui_svg_selftest PRIVATE /utf-8)
endif()

add_test(NAME svg_selftest COMMAND voidui_svg_selftest)
