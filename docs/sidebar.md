# 通用侧边栏

`#include "voidui/widgets/sidebar.h"`

`sidebar(panel, content)` 管理侧栏和主内容两个视口。它默认填满父容器，可放在任意容器中，也可嵌套组合多个方向。两个槽位可传普通控件、容器或函数组件；内部自动裁剪，长内容可包裹 `scrollable(...)`。

```cpp
auto page = component([] {
  auto showing = use_state(true);
  auto panel_size = use_state(260.0f);

  return sidebar(
      column(text("工具面板"),
             button("关闭").on_click([showing] { showing.set(false); })),
      column(button("切换侧栏").on_click([showing] {
               showing.set(!showing.get());
             }),
             text("主内容")))
      .placement(SidebarPlacement::Right)
      .open(showing)
      .extent(panel_size)
      .limits(160, 480)
      .drag_mode(SidebarDragMode::ElasticOpenClose)
      .drag_threshold(48)
      .collapse_threshold(40);
});
```

上例还需包含 `component.h`、`button.h`、`column.h`。`open(State<bool>)` 和 `extent(State<float>)` 都是双向绑定：拖动、键盘操作会同步写入状态，外部按钮或其他组件调用 `set(...)` 则更新侧栏。`on_open_change`、`on_extent_change` 可附加监听，不会覆盖状态绑定；监听仅在交互改变相应值时触发，外部声明更新不会反向触发。

## 方向与布局

| API | 行为 |
| --- | --- |
| `placement(Left / Right / Top / Bottom)` | 默认 Left；竖向侧栏调整宽度，顶部/底部调整高度 |
| `mode(SidebarMode::Docked)` | 默认，占用布局空间，主内容随尺寸调整 |
| `mode(SidebarMode::Overlay)` | 主内容保持原大小，侧栏覆盖在其上；不创建模态遮罩 |
| `extent(float)` | 默认 280，展开尺寸，不含拖动边缘 |
| `limits(min, max)` | 默认 160～560，展开尺寸的上下限 |
| `collapsed_extent(float)` | 默认 0；设为 56 等值可保留窄栏，最大不超过最小展开尺寸 |
| `handle_size(float)` | 默认 8，拖动边缘占用的厚度；关闭时仍保留，方便重新拉开 |
| `size / width / height / margin` | 整个组件的尺寸与外边距；侧栏宽/高用 `extent` 调整 |

关闭时保留子组件状态；`collapsed_extent(0)` 排除隐藏内容的鼠标输入和 Tab 焦点。窄栏显示同一份内容的裁剪视口；需要图标/完整导航两种内容时，可依据绑定的 `showing` 状态在面板组件中切换显示。窗口空间不足时临时压缩可见尺寸，空间恢复后恢复原尺寸，不会改写持久尺寸状态。

## 拖动与键盘

`SidebarDragMode` 提供四种预设：

- `Immediate`（默认）：展开、调整尺寸和收起都不等待弹性阈值，也不产生阻力曲线；向内缩到最小展开尺寸时直接收起。到达最大尺寸后反向拖动立即跟随，不积累越界距离。
- `Elastic`：有效拖动距离达到 `drag_threshold` 才展开或开始调整尺寸；超过阈值的部分计入连续尺寸调整。未达阈值释放时，尺寸不变。启用 `.edge_visible(true)` 时，边缘弯曲提供阻力反馈，释放后曲线约 250 ms 内回弹。
- `ElasticOpenClose`：仅展开和收起有弹性。展开后追上新边界即可连续调整尺寸，不再等待第二次阈值；在边界空隙中反向拖动也立即缩小。已展开时重新按下拖动同样立即跟随；收缩到最小尺寸后再进入弹性收起阶段。
- `Disabled`：移除拖动边缘及其键盘焦点，仍可通过状态和按钮开关。

也可分别覆盖三个阶段，参数均为 `SidebarDragBehavior::Immediate / Elastic`：

| API | 控制范围 |
| --- | --- |
| `open_behavior(...)` | 关闭时是否等待 `drag_threshold` 并产生弹性反馈 |
| `resize_behavior(...)` | 已展开时是否等待 `drag_threshold` 才开始调整，以及超过最大尺寸时是否产生阻力 |
| `collapse_behavior(...)` | 缩到最小尺寸后是否再拉动 `collapse_threshold` 才收起，并产生阻力 |

未覆盖的阶段使用 `drag_mode` 预设；显式覆盖不受 setter 调用顺序影响。`Disabled` 始终禁止拖动。比如只在展开时使用弹性，其余直接跟随：

```cpp
sidebar(panel, content)
    .drag_mode(SidebarDragMode::Immediate)
    .open_behavior(SidebarDragBehavior::Elastic);
```

完全关闭弹性只需 `.drag_mode(SidebarDragMode::Immediate)`，无需把各个阈值设成零。`edge_visible` 仍只控制线条是否可见，不控制交互弹性。

拖动分为“等待阈值”和“连续调整”两个阶段；即时行为的阈值为零。关闭时向展开方向拖到阈值，弹性展开使用配置的初始 `extent`，不使用拖动记忆；即时展开使用记忆尺寸。这次事件剩余位移不会再把面板撑大。展开/收起会让边界跳到新位置，因此每次跳变后都重新处理边界空隙，分别处理两个方向：

- 以左侧栏为例，鼠标在新边界左侧时，右移先追上边界，跨过边界后的距离才计入右拖阈值；中途反向左移可以从最新转折点计算左拖阈值，无需回到最初按下的位置。
- 鼠标在新边界右侧时，对称处理：左移先追上边界，右移则可从最新转折点计算阈值。追赶边界的空隙不会产生尺寸变化或弹性反馈。
- 达到当前方向阈值后，进入连续调整，尺寸从面板当前大小平滑变化。此时反向移动立即跟随，不再重复等待阈值；只有再次展开/收起的跳变才重新等待。

例如，使用 `Elastic` 时，左侧栏关闭时从 0 开始拖，配置宽度 260、阈值 48（忽略拖动条内的抓取偏移）：拖到 48 时展开为 260；继续到 200 时仍为 260；超过边界 260 后才计入扩宽阈值，拖到 328 时宽度为 280。若在 200 处反向左拖，则从 200 计数，拖到 132 时宽度为 240，不会突然跳到鼠标所在的宽度。

同样的尺寸使用 `ElasticOpenClose`：拖到 48 时展开为 260；继续到 260 时宽度仍为 260；拖到 280 时宽度就是 280。如果改在 200 处反向左拖到 190，宽度立即变为 250。每次从关闭状态重新展开都会使用一次展开弹性。

向关闭方向连续调整，启用收起弹性时目标尺寸到达 `最小展开尺寸 - collapse_threshold` 才收起，否则到最小展开尺寸就收起。两者都记住开始这段收缩前的展开尺寸，供即时拖动、按钮或键盘重新打开时使用；弹性展开始终忽略这份记忆。例如 `.extent(240)` 拉到 360 后不松手收起，再次弹性拉出仍为 240，松手后重新拖动也是如此。双向绑定的交互回写和普通重建不会改变弹性展开尺寸；外部显式修改 `extent` 才更新它。该尺寸仍受 `limits` 和可用空间约束。

`collapse_threshold` 默认为 48；启用边缘线和相应阶段的弹性时，在尺寸边界处继续拉动会显示有上限的阻力曲线。四个方向使用相同规则，只改变坐标轴和展开方向。

父组件重建不会重置当前阶段、边界或转折点；相同位置的重复移动/释放事件不会撤销刚完成的展开。外部修改开关、尺寸或拖动配置会取消当前手势。窗口失焦结束拖动并保留已提交的尺寸；Escape 则恢复本次按下时的开关和尺寸，即使一次拖动中已经经历多次展开/收起。

Tab 可聚焦边缘，并显示焦点底色。左右/上下方向键每次调整 16 px（取决于方向）；向外缩到最小值以下会关闭，关闭时向内调整会打开。Enter 或 Space 切换开关。键盘操作不受鼠标拖动阈值影响。

## 样式与状态

边缘线默认隐藏（Debug 和 Release 均如此）。`.edge_visible(true)` 同时显示静止边缘线和拖动时的弹性曲线，`.edge_visible(false)` 隐藏二者。隐藏只影响线条绘制，不改变拖动区域、阈值、鼠标指针或键盘操作；键盘焦点提示仍然保留。隐藏时也不会为线条回弹持续请求动画帧。

例如，仅在调试构建中显示：

```cpp
auto view = sidebar(panel, content);
#ifndef NDEBUG
view.edge_visible(true);
#endif
```

交互示例采用上述调试配置。使用 `.handle_color(Brush)`、`.elastic_color(Brush)` 设置启用后的边缘与拖动曲线颜色；也可通过 VSS 的 `sidebar { handle-color: ...; elastic-color: ...; }` 设置。`.background(Brush)` 设置整个组件背景，面板和主内容背景可直接设置在传入控件上。

内部视口暴露 `sidebar::part(panel)`、`sidebar::part(content)` 和 `sidebar::part(handle)`。建议把 padding、滚动等布局样式设置在传入的槽位内容上；槽位本身的几何由 Sidebar 管理。

未绑定时，侧栏自行保存交互状态。`.open(true)`、`.extent(260)` 等不变声明在父组件重建时不会覆盖用户拖动结果；声明值真正改变才应用。需要从任意组件明确控制开关或尺寸时使用 `State` 绑定。重建时保持组件位置或设置稳定的 `.key(...)`，即可保留槽位内容与交互状态。

```sh
xmake run voidui_example_sidebar
xmake run voidui_sidebar_selftest
```

示例可切换四个方向、占位/覆盖、即时/仅展开收起弹性/全程弹性拖动、关闭后保留窄栏，并演示内外按钮开关。
