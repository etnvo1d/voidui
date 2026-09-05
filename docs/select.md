# Select

`#include "voidui/widgets/select.h"`

单值下拉选择器；选项的 `value` 是稳定标识，`label` 是显示文本，第三项 `disabled` 默认为 `false`。建议 value 唯一；重复值取第一项显示。未设置 value 或没有匹配项时显示 placeholder，不自动选择第一项。

```cpp
select({{"cn", "中国"}, {"us", "美国"}, {"eu", "欧洲", true}})
    .placeholder("选择地区")
    .value("cn")
    .width(280)
    .picker_max_height(240)
    .on_change([](const std::string &value) { /* 保存 value */ });
```

在 function component 中可以绑定 `State<std::string>`：

```cpp
return component([] {
  auto region = use_state(std::string("cn"));
  return select({{"cn", "中国"}, {"us", "美国"}}, region);
  // 等价于 select({...}).value(region)
});
```

`.disabled(true)` 禁用整个控件，并从 Tab 顺序中移除。`on_change` 仅在用户提交不同值时触发一次。未显式传入 value 的新声明保留之前的用户选择；显式 `.value(...)` 在组件重建时采用新声明值。相同选项列表的重建会保留展开状态；选项列表改变或控件禁用时收起。按 lvalue 传入并克隆的 Select 各自拥有独立交互状态。

## 交互与浮层

- 点击、Space、Enter、↑/↓ 打开。展开后 ↑/↓ 浏览，Home/End 跳至首尾，跳过禁用项；浏览不会更改已选值。
- Enter/Space 或点击选项提交；选中项有勾选标记，浏览项有背景反馈，关闭后焦点保留在触发框。
- Escape、Alt+↑、点击外部、Tab 或窗口失焦关闭。Escape 取消浏览；Tab 继续移至下一控件。外部点击沿用 overlay 的防穿透行为。
- ASCII 字母/数字支持前缀查找，700 ms 内连续输入组成前缀，重复首字母循环查找。关闭时直接选中，展开时只移动浏览项。目前不提供 IME 搜索、多选或 optgroup。
- 默认 240 px 宽、最大 280 px 高；默认主题包含焦点环、禁用状态、圆角、阴影和上下翻转的箭头。长标签保持单行并裁切，避免覆盖箭头。
- picker 是内部 overlay，以窗口坐标独立绘制和命中，不继承 scrollable 的裁剪。面板匹配触发框宽度，受窗口宽度约束；优先向下，不够则翻转向上，并按可用空间限制高度。长列表在面板内滚动，键盘浏览自动滚动到目标项；面板滚轮不传给外层 scrollable。

## VSS 与 C++ 样式

支持以下 CSS 语法子集。相关标准见 [CSS Forms](https://www.w3.org/TR/css-forms-1/) 和 [CSS UI](https://www.w3.org/TR/css-ui-4/)。这里是 VoidUI 自绘控件的语义，不表示实现了浏览器完整表单样式规范。

```css
select {
  appearance: base-select;
  width: 280px;
  padding: 10px 12px;
  background: white;
  color: #263244;
  border-width: 1px;
  border-color: #cbd5e1;
  border-radius: 8px;
  font-size: 14px;
}
select:focus, select:open { border-color: #2563eb; }
select:disabled { color: #94a3b8; cursor: not-allowed; }
select::picker(select) { background: white; border-radius: 10px; }
select:open::picker-icon { color: #2563eb; }
select option:checked { background: #eff6ff; }
select option:focus { background: #dbeafe; }
select option:disabled { color: #94a3b8; }
select option:checked::checkmark { color: #2563eb; }
```

`:enabled` 也可用于 select 和 option。`:focus` 在 option 上代表键盘/指针正在浏览的项；系统焦点仍保留在 Select 上，选项不额外占用 Tab 停靠点。伪类写在伪元素前时测试宿主，写在后时测试该部件，例如 `select:open::picker-icon` 与 `select::picker(select):hover`。

`::picker(select)`、`::picker-icon`、`::checkmark` 分别复用 `::part(picker)`、`::part(picker-icon)`、`::part(checkmark)` 的命名部件。`select::part(label)` 可设置触发框文字。option 是公开的样式节点，可用 `.my-select option` 限定范围；内部文字和图形仍通过部件暴露。

`appearance` 支持 `auto`、`base-select`、`none`：前两者使用 VoidUI 默认外观，`none` 隐藏默认箭头；不会启用系统原生控件，也不会清除显式背景、边框等样式。未实现 CSS `content` 替换、锚点定位属性、`:not()` 和其它表单伪类。`picker_max_height()` 是组件配置，并非 CSS 标准属性；VSS 可通过 `select::picker(select) { height: 220px; }` 设置面板高度，但仍受该最大高度和窗口空间限制。

同一层叠系统支持 C++ 链式赋值，inline 设置高于应用 VSS：

```cpp
auto field = select({{"cn", "中国"}, {"us", "美国"}})
    .appearance(SelectAppearance::BaseSelect)
    .background(Color(255, 255, 255))
    .color(Color(38, 50, 68))
    .font_size(14)
    .padding({10, 12})
    .border_radius(8)
    .border_width(1)
    .border_color(Color(203, 213, 225));

auto sheet = std::make_shared<StyleSheet>();
sheet->add(Selectors::of<Select>().open().picker_icon(),
           StyleDeclaration{}.set<styles::Foreground>(Color(37, 99, 235)));
sheet->add(Select::option_selector().checked().checkmark(),
           StyleDeclaration{}.set<styles::Foreground>(Color(13, 148, 136)));
```

C++ 选择器提供 `.open()`、`.checked()`、`.disabled()`、`.enabled()`、`.picker()`、`.picker_icon()`、`.checkmark()`；也可组合 `.hovered()`、`.focused()`、`.part(...)`。

## 运行

```sh
xmake run voidui_example_select
xmake run voidui_select_selftest
```

示例从项目根目录加载 `examples/select.vss`，展示状态绑定、长列表、滚动视口、禁用项和自定义主题。
