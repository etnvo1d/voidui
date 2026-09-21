# Source and compatibility notes

This package derives from [zed-industries/zed](https://github.com/zed-industries/zed) at commit `e2534d2357a80795d2c372d31268748e7ee992e5`.

It combines the WGPU renderer with the scene and text APIs needed to use it independently. The following notes describe the source of the included code and the differences that affect applications using this package.

## Source files

| Included files | Upstream location |
| --- | --- |
| `src/wgpu_{renderer,context,atlas}.rs`, `src/shaders*.wgsl` | `crates/gpui_wgpu/src/` |
| `src/{geometry,color,scene,bounds_tree,path_builder}.rs` | Matching files in `crates/gpui/src/` |
| Font descriptors/features/fallbacks in `src/text_system/` | Adapted from `crates/gpui/src/`; paragraph/cache implementation replaced locally |
| `src/types.rs` | Rendering and font types extracted from `crates/gpui/src/platform.rs`, `window.rs`, `style.rs`, and `gpui.rs` |
| `tests/fonts/IBMPlexSans-Regular.ttf` and its license | `assets/fonts/ibm-plex-sans/` |

The standalone manifest, `src/painter.rs`, `src/shared_string.rs`, `src/lib.rs`, example, and integration tests were added for this package.

## Dependencies and types

The package replaces Zed's collection helpers with `rustc-hash` and standard collections, its logging helper with a local implementation, and `block_on` with `pollster`. `SharedString` has a standalone immutable implementation whose equality and hashing depend on text content.

Geometry types retain the operations used by scenes and GPU data. GPUI application, display, and window queries, `Refineable` derives, and Taffy conversions are omitted. `Rems` conversion takes an explicit root font size.

Font matching now uses Fontique; shaping, wrapping and paragraph geometry use Parley. The original cosmic-text adapter and GPUI line wrappers have been removed. Swash rasterization retains the standalone painter/atlas contract.

## Text and drawing

Text painting accepts `Painter` without GPUI's `Window` or `App`. `TextSystem` shares Parley resources, `Paragraph` retains native layout, and `TextLayoutCache` provides weak paragraph reuse. See [the migration guide](../../docs/parley.md) for removed low-level APIs.

Native WGPU initialization enables all compiled backends, extending the upstream Vulkan/GL selection to allow standalone macOS and Windows use. Surface creation, replacement, and recovery retain the supplied window handle source through WGPU's safe surface API.

Native `render_to_rgba` uses the same drawing pipelines as window presentation and returns pixels for screenshots or inspection. The renderer now exposes a macOS transaction-presentation hook and a callback immediately before successful presentation. The parent voidui runtime supplies the AppKit resize and Wayland lifecycle integration; the standalone renderer does not own native window callbacks.

Atlas uploads validate dimensions, GPU texture limits, and pixel byte length. Image and SVG keys use resource IDs supplied by your application. The unimplemented video surface placeholder is retained without its CoreVideo object field; it cannot import video surfaces.

## Tests and attribution

Tests that can run independently are retained. Upstream line wrapper tests requiring `TestAppContext` and platform-specific font metrics are replaced with standalone font fixture tests. Unused proptest generators and obsolete platform feature gates are omitted. Documentation examples use the standalone crate name.

The package includes the Apache, MIT, and font license notices. It contains modified upstream code and is maintained separately. When evaluating an upgrade, compare these compatibility differences and run the shader, GPU layout, text, and window presentation checks for the platforms your application supports.

## Desktop runtime integration changes

The voidui runtime port adds a pre-present callback while retaining `draw` as a
compatibility wrapper. macOS can set CAMetalLayer transaction presentation and
allow bounded drawable acquisition through the guarded WGPU HAL surface. Resize
relies on WGPU synchronization instead of an extra unbounded device poll; device
recovery retry delays belong to the host scheduler instead of a blocking sleep.
Path/MSAA intermediate textures are allocated only when the scene contains paths.
Adapter selection prefers integrated GPUs after explicit/compositor matches to
avoid waking a discrete GPU for ordinary UI work. Existing licenses remain intact.
