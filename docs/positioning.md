# 定位、层叠与 Tooltip

框架使用 C++23。VSS 和 C++ 设置都写入同一套样式属性，C++ 实例设置优先于样式表；克隆组件会保留这些设置。

## 常用 API

```cpp
auto content = column(text("Save the current document"))
    .position(Position::Absolute)
    .left(Inset::percentage(50))
    .bottom(Inset::calc(100, 10)) // calc(100% + 10px)
    .z_index(1000)
    .white_space(WhiteSpace::Nowrap)
    .visibility(Visibility::Hidden)
    .pointer_events(PointerEvents::None)
    .opacity(0.0f);

VisualTransform offset;
offset.translate_x_percent = -50;
offset.translate_y = 4;
content.transform(offset);
```

这些方法定义在 Widget 基类，使用 C++23 显式对象参数保留派生组件类型和左/右值性质。组件作者不需要复制定位 setter。

`left/right/top/bottom` 接受 `Inset`，传入数字表示逻辑像素，`Inset{}` 或 `Inset::Auto{}` 表示 `auto`。`z_index(ZIndex{})` 恢复 `auto`，与 `z_index(0)` 不同。`inset(Spacing<Inset>(...))` 一次设置四边，其参数顺序沿用 C++ `Spacing`：左、上、右、下；VSS `inset` 使用 CSS 的上、右、下、左顺序。

过渡可用现有类型设置：

```cpp
content.transition_property(TransitionPropertyList::make({styles::Opacity::index()}))
       .transition_duration(StyleTimeList::make({0.15f}))
       .transition_timing_function(EasingList::make({Easing::ease()}));
```

完整的悬停样式与窗口示例见 [`examples/tooltip.cpp`](../examples/tooltip.cpp)。Tooltip 背景和 padding 放在 Column 容器上，Text 负责文本本身。

## 布局与层叠语义

| 属性 | 行为 |
| --- | --- |
| `position: static` | 普通布局，忽略四边偏移。 |
| `relative` | 保留原来的布局占位，再偏移自身及子树。 |
| `absolute` | 脱离普通布局，相对最近的定位或变换祖先的 padding box 定位；否则使用视口。 |
| `fixed` | 脱离普通布局，默认相对视口，滚动不改变其位置；变换祖先会建立新的包含块。 |
| `sticky` | 保留普通布局占位，在最近裁剪滚动容器内按指定边吸附，并受父容器边界限制。 |
| `left/right/top/bottom` | `auto`、`px`、`%`、无单位零，以及这些单位的 `calc()` 加减表达式，支持嵌套括号。 |
| `inset` | 1–4 个定位值的 CSS 简写；后续单边声明独立覆盖。 |
| `z-index` | `auto` 或有符号 32 位整数。相同层级保持声明顺序。定位组件及 Row/Column 的直接布局子项可设置层级。 |
| `visibility` | `hidden/collapse` 保留布局但不绘制、不接收指针命中；子项可显式恢复 `visible`。 |
| `pointer-events` | `none` 不成为指针目标；子项可显式恢复 `auto`。不会阻断正常事件冒泡。 |
| `white-space` | `nowrap/pre` 禁止自动折行，`normal/pre-wrap` 保留现有自动折行行为。 |

绝对或固定子项不参与容器尺寸、gap、Fill/Flex 分配；同时设置一对相对边且对应尺寸为 auto 时会拉伸。百分比 inset 参考包含块，百分比 translate 参考组件自身的 border box。

显式整数 z-index、fixed、sticky、非 none 变换和小于 1 的 opacity 会建立层叠上下文。上下文整体参与外层排序，内部的大 z-index 不会越过外层上下文。普通祖先以及 `position: relative; z-index: auto` 不会把高层级后代困在其子树内。绘制与命中测试共用排序；命中按反向绘制顺序检查。

`visibility` 过渡在可见与隐藏之间使用 CSS 的特殊插值：淡入期间可见，淡出结束才隐藏。`opacity: 0` 本身不禁止命中，可搭配 `visibility` 或 `pointer-events`。

## 实现与边界

- `widget_positioning.cpp` 管理布局后的定位，`widget_paint.cpp` 管理层叠、绘制和命中，`widget_geometry.h` 统一变换和包含块判断。
- Row/Column 共用 `linear_layout.cpp`，定位子项由 LayoutContext 从普通布局接口中排除；带命名插槽的组件可用 `flow_index()` 映射注册索引。
- 排序缓存在树上，不改变声明子项顺序；绘制复用祖先状态和路径缓冲。未使用脱流定位的 LayoutContext 不分配过滤列表。Inset 为两个 float，VisualTransform 保持在 PropertyValue 的 32 字节内联空间中。
- 普通绘制失效先比较层叠属性快照；颜色和透明度动画在层叠关系不变时复用排序。创建或移除 transform 会重新计算包含块，已有 transform 的位移仍只触发绘制。
- 这是原生 UI 布局，不是完整浏览器 CSS 引擎。暂不提供 DOM 行内排版、RTL/竖排布局、CSS 空白字符折叠、所有 CSS 长度单位或 `calc()` 乘除法。所有 inset 为 auto 时以父内容区起点为回退位置。
- 支持 translate/translateX/translateY、scale/scaleX/scaleY、rotate 及可由平移、旋转、缩放表达的组合，按 CSS 顺序合成。需要剪切或跨轴百分比系数的组合会报告解析错误；不支持 3D transform。
- opacity 沿用原生绘制命令的 alpha 调制；尚未提供浏览器对重叠后代的离屏整体透明度合成。visibility 的 collapse 在本框架中与 hidden 一致。

语义参考：[CSS Positioned Layout](https://www.w3.org/TR/css-position-3/) 与 [CSS 绘制顺序](https://www.w3.org/TR/CSS22/zindex.html)。

## 验证

```powershell
xmake f -m debug -y
xmake
xmake test -v
```

`positioning_selftest` 覆盖完整 tooltip VSS、过渡、百分比、嵌套层叠、负层级、命中、裁剪、滚动 fixed/sticky、透明组件及输入插槽；其他 selftest 覆盖原有布局、样式、动画、输入、资源和绘制路径。
