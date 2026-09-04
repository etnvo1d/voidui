# Slang toolchain acquisition + .slang -> {DXBC, SPIR-V, MSL} -> C++ header.
#
# Slang is fetched as a prebuilt release archive rather than built from source:
# the compiler is a build-time tool only, and building it would dwarf the rest
# of this project.

include(FetchContent)

set(VOIDUI_SLANG_VERSION "2026.16.1" CACHE STRING "Slang release to use for shader compilation")

function(_voidui_slang_archive out_name)
    set(_ver "${VOIDUI_SLANG_VERSION}")
    string(TOLOWER "${CMAKE_HOST_SYSTEM_PROCESSOR}" _arch)

    if(_arch MATCHES "arm64|aarch64")
        set(_arch "aarch64")
    else()
        set(_arch "x86_64")
    endif()

    if(CMAKE_HOST_WIN32)
        set(_os "windows")
    elseif(CMAKE_HOST_APPLE)
        set(_os "macos")
    else()
        set(_os "linux")
    endif()

    set(${out_name} "slang-${_ver}-${_os}-${_arch}.zip" PARENT_SCOPE)
endfunction()

function(voidui_provision_slang)
    if(VOIDUI_SLANGC)
        return()
    endif()

    _voidui_slang_archive(_archive)
    set(_url "https://github.com/shader-slang/slang/releases/download/v${VOIDUI_SLANG_VERSION}/${_archive}")

    message(STATUS "VoidUI: fetching Slang ${VOIDUI_SLANG_VERSION} (${_archive})")

    FetchContent_Declare(voidui_slang URL "${_url}")
    FetchContent_MakeAvailable(voidui_slang)

    find_program(
        VOIDUI_SLANGC
        NAMES slangc
        PATHS "${voidui_slang_SOURCE_DIR}/bin"
        NO_DEFAULT_PATH
        REQUIRED
    )

endfunction()

# Each platform bakes only the format consumed by its native backend.
function(_voidui_shader_variants out)
    if(WIN32)
        set(${out} "dxbc" PARENT_SCOPE)
    elseif(APPLE)
        set(${out} "msl" PARENT_SCOPE)
    else()
        set(${out} "spirv" PARENT_SCOPE)
    endif()
endfunction()

# voidui_add_shader_library(<target>
#     SOURCE  <file.slang>
#     NAME    <cpp-namespace-leaf>
#     VERTEX  <entry>
#     FRAGMENT <entry>)
#
# Produces an INTERFACE target carrying the generated header's include dir.
function(voidui_add_shader_library target)
    cmake_parse_arguments(ARG "" "SOURCE;NAME;VERTEX;FRAGMENT" "DEPENDS" ${ARGN})

    voidui_provision_slang()
    _voidui_shader_variants(_variants)

    get_filename_component(_source "${ARG_SOURCE}" ABSOLUTE)
    set(_gen_dir "${CMAKE_CURRENT_BINARY_DIR}/generated/voidui/shaders")
    set(_header "${_gen_dir}/${ARG_NAME}.h")
    file(MAKE_DIRECTORY "${_gen_dir}")

    set(_artifacts "")
    set(_outputs "")

    foreach(_variant IN LISTS _variants)
        if(_variant STREQUAL "msl")
            set(_slang_target "metal")
        else()
            set(_slang_target "${_variant}")
        endif()
        foreach(_stage_pair "vertex:${ARG_VERTEX}" "fragment:${ARG_FRAGMENT}")
            string(REPLACE ":" ";" _pair "${_stage_pair}")
            list(GET _pair 0 _stage)
            list(GET _pair 1 _entry)

            set(_out "${CMAKE_CURRENT_BINARY_DIR}/shaders/${ARG_NAME}.${_stage}.${_variant}")

            set(_extra_args "")

            # Texture binding and uniform transport differ by native API.
            string(TOUPPER "${_variant}" _variant_upper)
            list(APPEND _extra_args "-DVOIDUI_TARGET_${_variant_upper}")

            if(_variant STREQUAL "dxbc")
                list(APPEND _extra_args -profile sm_5_0)
            elseif(_variant STREQUAL "spirv")
                # Slang renames entry points to "main" for SPIR-V by convention.
                # Keep the source name so one entrypoint string works for every
                # backend -- SDL_CreateGPUShader accepts a name it never checks,
                # so a mismatch only surfaces later as a pipeline failure.
                list(APPEND _extra_args -fvk-use-entrypoint-name -fvk-invert-y)
            elseif(_variant STREQUAL "msl")
                # D3D register annotations intentionally select Metal resource
                # indices too; Slang warns even though the emitted MSL is valid.
                list(APPEND _extra_args -Wno-39029 -Wno-39001)
            endif()

            add_custom_command(
                OUTPUT "${_out}"
                COMMAND "${VOIDUI_SLANGC}" "${_source}"
                        -target ${_slang_target} ${_extra_args}
                        -entry ${_entry} -stage ${_stage}
                        -o "${_out}"
                DEPENDS "${_source}" ${ARG_DEPENDS}
                COMMENT "slangc ${ARG_NAME}.${_stage} -> ${_variant}"
                VERBATIM
            )

            list(APPEND _outputs "${_out}")
            list(APPEND _artifacts "${_variant}:${_stage}:${_out}")
        endforeach()
    endforeach()

    add_custom_command(
        OUTPUT "${_header}"
        COMMAND ${CMAKE_COMMAND}
                -DNAME=${ARG_NAME}
                -DOUTPUT=${_header}
                "-DARTIFACTS=${_artifacts}"
                -P "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/BakeShaders.cmake"
        DEPENDS ${_outputs} "${CMAKE_CURRENT_FUNCTION_LIST_DIR}/BakeShaders.cmake"
        COMMENT "Baking ${ARG_NAME} shader bytecode into ${ARG_NAME}.h"
        VERBATIM
    )

    add_custom_target(${target}_generate DEPENDS "${_header}")
    add_library(${target} INTERFACE)
    add_dependencies(${target} ${target}_generate)
    target_include_directories(${target} INTERFACE "${CMAKE_CURRENT_BINARY_DIR}/generated")
endfunction()
