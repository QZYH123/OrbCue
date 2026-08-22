# Design Language — Agent Activity Dock

designer 子代理的持久记忆。新的设计决策用一两行追加到对应小节。

## 基调

- 深青灰壳 + 语义色：绿 `#3ee089` 工作、琥珀 `#f4b860` 注意、红 `#f47878` 失败。不换色系。
- 字体 Inter / 系统无衬线；等宽用 ui-monospace 栈（会话 id、路径）。

## 小球（64px 窗，OS region 硬裁成圆）

- CSS 球固定 60px 居中，留 2px 透明缓冲给硬裁边；圆外禁止任何绘制：无 drop-shadow、外扩辉光、translateY、向外 scale。所有光效只用 border + inset box-shadow。
- 状态环 = 2px 彩色 border + 1.5px 同色 inset 内描边（约 3.5px 总带宽）；旧的 3px+2px 在 60px 球上像胶圈，太厚。
- `pulse` 只动 inset box-shadow（1.5px→4px 琥珀），0.28s 单次。
- 计数 16px/800 tabular-nums；`.wide`（如 `10/12`）降到 13px，不再挤边框。
- 角标：14px 圆点，`right/top: 9px`（中心离窗心约 22.6px），骑在环带内缘、整颗落在 60px 球内；1.5px 深色描边把它从同色环上分离出来。角标不许回到方窗四角。
- 闲置边框 `#8494a6`：足够亮以便在浅色桌面上有轮廓，但不抢状态色。

## 面板（360×500 窗）

- `.panel` 是 flex column + `height: 100%` + `overflow-y: auto`；`.sessions` / `.audit-list` 用 `flex: 1 1 auto; min-height: 0` 内部滚动，footer 永远在屏内。不要再用写死的 max-height 分配高度。
- 间距刻度按 360 宽收紧：面板 padding 14px、卡片 padding 10px 11px、列表 gap 7px、小节间 8–10px。
- 圆角层级：面板 16 → 卡片 11 → 胶囊/按钮 7–8。
- 字号层级：h1 17px、eyebrow 9px/宽字距、卡片标题 12px、正文 11px、等宽细节 10px、辅助 10px。
- 卡片右上角未读标记与球上角标同语言：15px 圆点 + mark 色；有标记时顶行 `padding-right: 18px` 防碰撞（`:has` 选择器）。
- 连接页第三态文案「未连接」+ 右侧「Windows PATH」胶囊说明原因，状态词与「已连接/可连接」同族，不重复。
- 会话卡片：`dock:` 精确跳回胶囊与连接页 side-badge 同尺寸（9px/800、绿胶囊）；窗口级成功用绿字短注「已回到最近交互的窗口」，失败仍用红字 `session-focus-error`。
- 滚动条 6px 细条，透明轨道。

## 自查方式

- `.scratch/design/shoot.py`：mock `__TAURI_INTERNALS__` 后用 playwright 分别以 64×64 / 360×500 视口截球和面板；球页注入 `clip-path: circle(32px at 32px 32px)` 模拟 OS region。
