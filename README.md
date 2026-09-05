# VoidUI

组件文档：[通用侧边栏](docs/sidebar.md)（四方向布局、弹性拖动、状态绑定与键盘控制）。

C++23 UI 库，项目与第三方库均使用原生 xmake 构建，无需 CMake。Windows 使用 D3D11，macOS 使用 Metal，Linux 使用 Vulkan。

## 准备环境

- [xmake](https://xmake.io/guide/quick-start) 3.1.1 或更新版本，并加入 `PATH`。
- Git。首次配置需要联网下载依赖源码和 Slang 编译器。
- Windows：Visual Studio / Build Tools 的 C++ 桌面开发工具及 Windows SDK；xmake 自动检测 MSVC，无需从开发者终端启动。
- macOS：Xcode Command Line Tools；Linux：支持 C++23 的编译器、Vulkan SDK、X11/Xext 开发库。Linux 窗口使用 X11，Wayland 会话需要 XWayland；当前原生构建配置不包含 SDL 的原生 Wayland 后端。
- [clangd](https://clangd.llvm.org/installation)，建议与所用编译器匹配的较新版本，并加入 `PATH`。

依赖固定为 SDL 3.4.14、FreeType 2.14.3、HarfBuzz 14.4.0 和 Slang 2026.16.1。前三项使用 `xmake/ports/` 中的脚本从源码构建为静态库，不调用其上游构建系统。Slang 和 C/C++ 编译器一样是构建工具，自动下载已编译的宿主机版本，也可指定已有的 `slangc`。

## 构建与运行

```sh
xmake f -m debug -y
xmake
xmake run voidui_example_counter
xmake test -v
```

`examples/*.cpp` 自动注册为 `voidui_example_<文件名>`；`tools/*_selftest.cpp` 自动注册为 `voidui_<文件名>` 并加入 `xmake test`。Release 自测也保留断言。`image_probe` 和 Windows 的 `font_probe_win` 是诊断工具，不作为测试执行。

```sh
xmake f -m release
xmake
# 只构建库
xmake f --examples=n --tools=n
xmake
# 恢复示例和工具
xmake f --examples=y --tools=y
# 使用已安装的 Slang（传入可执行文件路径）
xmake f --slangc=/absolute/path/to/slangc
```

输出位于 `build/<平台>/<架构>/<模式>/`，示例位于其 `examples/` 子目录。Windows 默认架构名为 `x64`。着色器头文件位于 xmake 自动生成目录，由构建过程生成；修改 `.slang` 文件会重新生成。

## VS Code 与 clangd

打开项目根目录，安装工作区推荐的 xmake、clangd 扩展。Windows 调试安装 Microsoft C/C++ 扩展，Linux/macOS 调试安装 CodeLLDB。Microsoft IntelliSense 已在工作区禁用，代码补全和诊断由 clangd 提供。

1. 运行任务 `xmake: configure debug`，然后按 `Ctrl+Shift+B` 构建。
2. 构建会在根目录自动更新 `compile_commands.json`，`.clangd` 从这里读取真实编译参数、依赖和生成头文件路径。首次完整构建后即可解析渲染代码中的着色器头文件。
3. 可通过任务 `xmake: compile commands` 手动刷新数据库（例如切换配置后）。数据库包含本机路径，不提交 Git。
4. 按 F5，选择相应平台的调试配置，输入示例名并选择与 `xmake f` 一致的架构。调试前自动切换 Debug 并构建；自定义了构建目录时需同步修改 `launch.json` 的程序路径。
5. 运行任务 `xmake: test` 构建并执行全部自测。

clangd 不要求用 Clang 编译，Windows 默认使用 MSVC。编译数据库规则按 [xmake 官方 clangd 集成方式](https://xmake.io/guide/extensions/ide-integration-plugins.html) 设置；VS Code 扩展的重复数据库生成已关闭。若刚安装工具，重启 VS Code 以刷新 `PATH`；必要时运行 `clangd: Restart language server`。

依赖构建脚本随项目维护。升级依赖版本时，应同步检查原生脚本的源码清单、平台配置及包验证；修改原生构建脚本后，可使用 `xmake require -f -y` 强制重建依赖。

本次迁移已在 Windows x64 / MSVC 上验证：三个第三方库的原生源码构建、Debug/Release 全量构建、两种模式各 18 项自测、clangd 解析及 D3D11 示例启动。Linux/macOS 配置尚未在对应系统实测。

Select 单值下拉组件、浮层行为及 VSS/C++ 样式用法见 [docs/select.md](docs/select.md)，运行 `xmake run voidui_example_select` 查看示例。
