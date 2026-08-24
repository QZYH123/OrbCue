# Design Language — Agent Activity Dock

designer 子代理的持久记忆。新的设计决策用一两行追加到对应小节。

## 基调

- 石墨哑光：`#121416` 壳、`#eceef1` 字、细发丝线。不要黑金、不要磷光黄。
- 语义色克制：工作 `#5ecf8f`、注意 `#e0a257`、失败 `#e07a7a`。
- 字体只用系统无衬线。大数字也走同一套。

## 小球（56px 窗，OS region 硬裁成圆）

- CSS 球 52px；圆外禁止 drop-shadow、外扩辉光、translateY、向外 scale。
- 内圈 conic 表示工作/追踪。无旋转光泽层。
- `pulse` 只动 inset。点击 toggle 面板。

## 面板（300×400 窗）

- 顶栏大数字 + 底栏导航。列表区域 `overflow-x: hidden`，不要横向滚动条。卡片 hover 不要 translateX。
- 会话不显示 summary。标题是 Agent 名 + 同项目同 Agent 的两位序号 `01` `02`（全量列表编号，筛选不重排）。
- 项目行右侧可收起。
- 「回去」是返回箭头图标，不是「回」字。精确与否都用同一套细线圆钮，不要填工作绿。
- 连接页：一行名称+侧别，一行路径省略，右侧状态+按钮。不要字母方块图标。
- 连接页状态词仍是「已连接/可连接」。
- 审计：和动态页同一套 Agent + 序号；不显示 session hex。第二行是状态 · 项目。不记 working / idle。
