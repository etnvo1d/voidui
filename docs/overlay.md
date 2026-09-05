# Overlay、Modal 与 Tooltip

## 普通用户

```cpp
#include "voidui/core/component.h"
#include "voidui/widgets/button.h"
#include "voidui/widgets/column.h"
#include "voidui/widgets/modal.h"
#include "voidui/widgets/tooltip.h"

using namespace voidui;

auto view = component([] {
  auto showing = use_state(false);
  return column(
      button("打开").on_click([showing] { showing.set(true); }),
      modal(column(
          tooltip(button("帮助"), "提示属于当前弹窗"),
          button("关闭").on_click([showing] { showing.set(false); })))
          .open(showing)
          .width(420)
          .close_on_outside_press(true));
});
```

`Modal` 默认关闭、居中、有遮罩，按 Escape 关闭，默认点击遮罩只阻断输入。
`.open(State<bool>)` 同时绑定显示值和关闭回调：Escape、点击外部或父浮层
关闭时，自动将状态设为 false。状态控制的关闭按钮直接调用 `set(false)`。
也可在打开时才声明 Modal，移除声明会自动完成焦点恢复和浮层清理。

Modal 内可继续声明 Modal、Overlay 和 Tooltip，不需要配置 z-index。
`examples/modal.cpp` 的每个弹窗都能继续打开下一层，没有固定层数。

Tooltip 用法：

```cpp
tooltip(button("保存"), "保存当前文档")
    .placement(OverlayPlacement::Top)
    .delay(std::chrono::milliseconds{500})
    .max_width(240);
```

默认悬停 500 ms 显示，触发控件获得焦点时立即显示。气泡不接收鼠标输入；
离开触发区域、窗口失焦、滚动、鼠标按下和 Escape 都可以关闭。
焦点仍在触发控件内时，离开鼠标可以继续显示。被主动关闭后须退出当前
触发状态再进入；一次新的焦点激活也能显示。空字符串不创建气泡。
Modal 关闭后的自动焦点恢复不会触发气泡。此时重新悬停仍按延迟显示，
移开鼠标即隐藏；通过 Tab 再次聚焦则恢复正常的焦点提示行为。

```css
modal { padding: 24px; border-radius: 12px; background: white; }
tooltip::part(bubble) { background: #1d4ed8; color: white; }
```

Modal 遮罩可用 `.backdrop(Color(0, 0, 0, 96))` 配置。

## 组件作者

`Overlay` 是同一机制的非模态入口，默认打开、可交互、以最近的非透明父
节点为锚点。放在按钮旁边或通过 `take_internal_child` 注册即可；普通
父布局会自动排除 Overlay。函数组件可直接返回 Overlay。

```cpp
auto anchor = column(
    button("菜单"),
    overlay(column(button("第一项"), button("第二项")))
        .open(menu_state)
        .placement(OverlayPlacement::Bottom)
        .close_on_escape(true)
        .close_on_outside_press(true));
```

独立 Widget 可覆盖 `overlay_options()` 返回自己的 `OverlayOptions`，
不必继承 Overlay。树会直接向浮层根发送 `OverlayDismissedEvent`，原因
包括 `Escape`、`OutsidePress`、`Press`、`Scroll`、`OwnerClosed` 和
`ModalOpened` 和 `WindowFocusLost`。这是通知，不是可取消的关闭请求。需要强制用户处理的
Modal 可关闭 Escape 与点击外部策略，再通过应用按钮控制状态。

使用 Overlay / Modal 时也可以手动绑定：

```cpp
modal(content)
    .open(showing.get())
    .on_close([showing](OverlayDismissReason reason) {
      showing.set(false);
    });
```

`.on_close(...)` 替换已有回调，包括 `.open(State<bool>)` 安装的回调；
同时使用时请在自定义回调中同步状态。程序主动设为 false 不重复通知本层，
但会向其附属浮层发送 `OwnerClosed`，使子组件状态同步。
回调在树完成关闭后分发，允许通过 State 更新组件。移除组件不会调用
已被销毁的组件回调，也不会保留失效的焦点或浮层指针。

未绑定状态的 `.open(true)` 被策略关闭后保持关闭；重新打开需让框架先
观察到 false，再观察到 true。直接修改已挂载 Widget 的参数后调用树的
`request_layout()`。推荐使用 State 声明式更新。

## 顺序、归属与模态范围

三者分开管理：组件树负责所有权与样式；活动浮层栈负责绘制顺序；最上面
的 Modal 限定允许交互的范围。

- 普通内容先画，活动浮层按**打开顺序**绘制。重新打开排到顶部；一次更新
  同时打开多层时按声明遍历顺序入栈。组件重建、key 重排和样式重算不会
  改变已经打开的顺序。普通内容的 z-index 不能跨越浮层边界。
- 每个 Modal 的遮罩紧贴在该 Modal 下面绘制，覆盖先前层级。嵌套弹窗关闭
  后可见前一层，而不是共享一块永远位于所有弹窗下方的遮罩。
- 声明在浮层中的内容自动归属它。没有声明父浮层的 Modal 若在另一 Modal
  激活期间打开，会归属当时的 Modal；因此兄弟声明也能正确逐层关闭。
  普通 Tooltip、菜单应声明在所属 Modal 内，不能从背景范围抢到前台。
- 新 Modal 打开时关闭不属于它的临时浮层，也取消尚未到期的背景 Tooltip。
  若 Modal 本来就在某个菜单中，该菜单作为所有者保留在其下方。
- 点击和滚轮不能穿透最上面 Modal 的遮罩，键盘事件不能冒泡到范围外。
  可交互的附属 Overlay 仍可接收输入；非交互气泡整棵子树被排除。
- Escape 只处理最上面可关闭浮层，遇到不允许 Escape 的 Modal 即停止。
  点击外部关闭一次至多关闭一个交互浮层，并吞掉该次按下及配对释放，
  防止刚露出的背景控件被同一次操作触发。
- Modal 打开时聚焦第一个可聚焦控件；Tab / Shift+Tab 在当前范围内循环。
  没有可聚焦子控件时仍保持模态输入边界。Button 支持 Enter / Space 激活。
  关闭 Modal 恢复打开前的焦点；目标已移除或不可交互则回退到当前 Modal。
- 关闭所有者会一起关闭附属浮层，释放其悬停、文本选择、焦点和鼠标捕获。
  移除任意层级同样清理运行状态和待发送通知。

## 定位与裁剪

浮层保留原来的样式继承、part 和组件生命周期，在窗口逻辑坐标中绘制，
不继承外部裁剪、变换与合成透明度。自己的样式变换和透明度仍有效。
锚点定位考虑祖先变换，随布局、窗口尺寸和绘制变换更新。

锚点型 Overlay 优先使用指定方向，相反方向越界更少时翻转，再限制在
窗口内。Modal 和 `Center` 定位使用窗口矩形，不因触发按钮滚出视口而
消失。普通锚点型浮层在锚点完全不可见时关闭。旋转裁剪的可见性判断使用
包围矩形。

浮层内容裁剪在自己的边界内，嵌套浮层再次脱离裁剪。长菜单或长对话框
请在内容中使用 Scrollable；窗口约束限制测量尺寸，不自动创建滚动条。
菜单高度目前不会自动按锚点某一侧的剩余空间分配。

## 性能与验证

没有额外原生窗口、离屏纹理或第二棵所有权树，仍输出一份 DisplayList。
普通 Widget、Node 和 PaintEntry 不增加浮层实例字段。只有浮层节点有稀疏
运行状态，活动栈存数组索引；入栈、出栈和结构变化时才重新排序活动索引。
绘制与命中测试复用各浮层的缓存绘制区间，开关浮层不重建整份绘制顺序。

未触发或被抑制的浮层跳过几何计算，隐藏内容首次打开前不测量。稳定显示
复用布局，定位变化才平移内容。延迟复用单次唤醒机制，等待或稳定显示不
请求连续帧；提前取消可能留下至多一次旧计时唤醒。默认参数无需组件作者
管理计时器、层号或焦点恢复句柄。

`overlay_selftest` 验证裁剪、变换、命中、定位、延迟与生命周期。
`modal_selftest` 验证打开顺序、嵌套输入范围、焦点循环与恢复、状态绑定、
遮罩点击、背景提示取消、兄弟声明、key 重排和移除后的引用清理。

另有 "overlay_allocation_selftest"：32 层 Modal 同时打开，预热后复用 Painter
和 DisplayList 生成 100 帧，验证 C++ operator new 分配次数为 0、稳定状态
不请求重绘，并验证逐层关闭后无遗留绘制命令。该测试不涵盖 GPU 驱动内部
分配或首次布局与打开时的分配。
