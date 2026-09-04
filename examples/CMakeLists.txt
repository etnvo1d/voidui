file(
    GLOB VOIDUI_EXAMPLE_SOURCES
    CONFIGURE_DEPENDS
    "${CMAKE_CURRENT_SOURCE_DIR}/*.cpp"
)

foreach(example_source IN LISTS VOIDUI_EXAMPLE_SOURCES)
    get_filename_component(
        example_name
        "${example_source}"
        NAME_WE
    )

    set(example_target "voidui_example_${example_name}")

    add_executable(
        "${example_target}"
        "${example_source}"
    )

    target_link_libraries(
        "${example_target}"
        PRIVATE voidui::voidui
    )

    if(MSVC)
        # Sources carry UTF-8 string literals; without this MSVC reads them in
        # the system codepage and the execution strings come out mangled.
        target_compile_options("${example_target}" PRIVATE /utf-8)
    endif()

    set_target_properties(
        "${example_target}"
        PROPERTIES
            OUTPUT_NAME "${example_name}"
            RUNTIME_OUTPUT_DIRECTORY
                "${CMAKE_BINARY_DIR}/bin/$<CONFIG>/examples"
    )
endforeach()