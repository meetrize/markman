# 知识图谱 — Hook 自动开发步骤

本文档为 Markman 添加 **工作区知识图谱（标签 + Wiki 链接关系图）** 的分步实施指南，配合 `.cursor/knowledge-graph-queue/` 与 stop hook **自动链式执行**。也可手动复制各步骤内容交给 Agent。

[方案设计](knowledge-graph-implementation.zh-CN.md) | [Hook 使用说明](#hook-自动链式执行) | [开发与构建](development.zh-CN.md)

---

## 方案概述

| 层次 | 内容 |
| --- | --- |
| **步骤 1** | Wiki 链接工作区索引（类 `tag_index.rs`） |
| **步骤 2** | `KnowledgeGraph` 图模型（节点/边合并） |
| **步骤 3** | 力导向布局引擎 + 单元测试 |
| **步骤 4** | GPUI 自定义 Element 静态绑图 |
| **步骤 5** | 拖拽、平移、缩放、点击跳转 |
| **步骤 6** | 侧栏 Graph Tab、索引同步、i18n |
| **步骤 7** | 入场动画、过滤、性能收尾 |

## 现有代码参考

| 模块 | 路径 | 职责 |
| --- | --- | --- |
| 标签索引 | `src/editor/tag_index.rs` | 扫描模式、增量更新、测试 |
| Wiki 解析 | `src/components/markdown/inline/wiki_link.rs` | `[[path]]` 规则 |
| 文件枚举 | `src/editor/markdown_files.rs` | `collect_markdown_files` |
| 侧栏 | `src/editor/workspace.rs` | `WorkspaceTab`、Tags 面板 |
| 自定义绘制 | `src/components/block/element.rs` | `Element` trait、`paint_quad` |
| canvas 交互 | `src/editor/render.rs` | 拖拽/指针事件 |
| 主题 | `src/theme/theme.rs` | 颜色 token |
| Hook 参考 | `.cursor/hooks/hashtag-queue-next.sh` | stop hook 链式投递 |

---

## Hook 自动链式执行

### 机制

| 文件 | 作用 |
| --- | --- |
| `.cursor/knowledge-graph-queue/steps.json` | 7 步标题与状态 |
| `.cursor/knowledge-graph-queue/state.json` | 队列开关、下一待执行步 |
| `.cursor/hooks/knowledge-graph-queue-next.sh` | Agent **stop** 时投递下一步 |
| `.cursor/hooks.json` | 注册 stop hook（**同时只启用一个开发队列**） |
| 本文档 | 每步任务说明与验收标准 |

### 启用

1. 将 `.cursor/refactor-queue/state.json` 与 `.cursor/hashtag-queue/state.json` 的 `enabled` 设为 `false`
2. 在 `.cursor/hooks.json` 的 `stop` 数组中追加：

```json
{
  "command": ".cursor/hooks/knowledge-graph-queue-next.sh",
  "loop_limit": 10
}
```

3. 设置 `.cursor/knowledge-graph-queue/state.json`：

```json
{
  "enabled": true,
  "nextStep": 1,
  "totalSteps": 7,
  "planDoc": "docs/knowledge-graph-hook-steps.zh-CN.md"
}
```

### 暂停 / 从某步继续

- `enabled: false` — 暂停
- `nextStep: N` — 从第 N 步开始

### 手动入队

```markdown
请阅读 `docs/knowledge-graph-hook-steps.zh-CN.md` 中的 **步骤 {N}/7：{标题}** 及「总览块」。

要求：
1. 只做该步骤范围内的改动
2. Wiki/标签解析规则保持单一来源（`wiki_link.rs` / `hashtag.rs`）
3. 完成后运行 `cargo test`（必要时 `./scripts/check.sh`）
4. 用中文总结：改了哪些文件、如何验证、建议的 commit message

开始执行。
```

---

## 总览（每步开头附上）

```markdown
# Markman 功能：工作区知识图谱

## 项目背景
Markman 是 Rust + GPUI 块级 Markdown 编辑器。已有工作区标签索引 `tag_index.rs` 与 Tags 侧栏；Wiki 链接 `[[path]]` 已支持打开文件，但无工作区级链接索引。需原生 GPUI 绘制点线关系图，无 WebView。

核心模块：
- `src/editor/tag_index.rs` — 标签索引（图谱标签节点数据源）
- `src/components/markdown/inline/wiki_link.rs` — Wiki 链接解析
- `src/editor/workspace.rs` — 侧栏 Tab、跳转
- `docs/knowledge-graph-implementation.zh-CN.md` — 完整方案

## 目标
1. 扫描工作区构建 Wiki 链接索引
2. 合并 tag + link 为 KnowledgeGraph（文档节点 + 标签节点 + 边）
3. 力导向布局、GPUI 自定义渲染
4. 拖拽/缩放/平移，点击跳转
5. 侧栏 Graph Tab

## 约束
- 不新增 WebView / npm 依赖；布局算法纯 Rust
- 解析规则与 inline 层单一来源
- 中英文 i18n 都要补（步骤 6 起）
- 新增 SVG 按 `.cursor/rules/icon-assets.mdc` 在 `src/main.rs` 注册
- 提交消息简体中文
- 每步结束 `cargo test` 通过

## 设计文档
详见 `docs/knowledge-graph-implementation.zh-CN.md`
```

---

## 步骤 1/7：Wiki 链接工作区索引

```markdown
# 步骤 1/7：Wiki 链接工作区索引

实现 `src/editor/link_index.rs`，模式对齐 `tag_index.rs`。

## 任务

### 1.1 行级定位 API

在 `src/components/markdown/inline/wiki_link.rs` 新增（或导出）：

```rust
/// 在单行文本中查找 `[[...]]`，返回 (start_byte, end_byte, path)。
pub fn locate_wiki_link_in_str(line: &str, search_from: usize) -> Option<(usize, usize, String)>;
```

规则与 inline `locate_wiki_link` 一致；供索引扫描复用。

### 1.2 link_index 模块

新建 `src/editor/link_index.rs`：

- `LinkOccurrence` — source_path, target_path, line, match_start_byte, preview
- `WorkspaceLinkIndex` — by_source, by_target, revision
- `extract_links_from_markdown(content, path)` — 跳过 fence、ATX 标题
- `build_workspace_link_index(root)`
- `refresh_link_index_for_file(index, path, content)`
- `remove_file_from_link_index(index, path)`

在 `src/editor/mod.rs` 声明 `mod link_index;`。

### 1.3 单元测试

- 单行 `see [[a.md]] and [[b/c.md]]`
- fence 内 `[[x]]` 不索引
- 增量 refresh 替换旧 occurrence

## 验收

- `cargo test link_index` 通过
- 不修改 UI

## 建议 commit

`feat(editor): 添加工作区 Wiki 链接索引引擎`
```

---

## 步骤 2/7：KnowledgeGraph 图模型

```markdown
# 步骤 2/7：KnowledgeGraph 图模型

新建 `src/editor/graph_model.rs`，将 tag_index + link_index 合并为图。

## 任务

### 2.1 类型定义

- `GraphNodeId`（String 或 newtype）
- `GraphNodeKind::Document | Tag`
- `GraphEdgeKind::Tagged | WikiLink`
- `GraphNode`, `GraphEdge`, `KnowledgeGraph`

### 2.2 构建函数

```rust
pub fn build_knowledge_graph(
    workspace_root: &Path,
    tag_index: &WorkspaceTagIndex,
    link_index: &WorkspaceLinkIndex,
) -> KnowledgeGraph;
```

- 文档节点：每个出现在 tag 或 link 中的 .md；MVP 可包含 `collect_markdown_files` 全部 md
- 标签节点：`tag_index.by_tag` 的 key + count
- 边：tag occurrence → Tagged；resolved wiki link → WikiLink
- 路径 resolve：相对 source 父目录；失败跳过并统计

### 2.3 测试

- 两文件 + 共享 tag + 一条 wiki link → 节点/边数量正确
- 重复 tag 同文件只一条边

## 验收

- `cargo test graph_model` 通过

## 建议 commit

`feat(editor): 添加知识图谱节点与边模型`
```

---

## 步骤 3/7：力导向布局引擎

```markdown
# 步骤 3/7：力导向布局引擎

新建 `src/editor/graph_layout.rs`。

## 任务

### 3.1 布局状态

```rust
pub struct GraphLayout {
    pub positions: HashMap<GraphNodeId, Point<f32>>,
    pub bounds: Bounds<f32>,
}

pub struct LayoutConfig {
    pub iterations: usize,
    pub repulsion: f32,
    pub attraction: f32,
    pub damping: f32,
    pub seed: u64,
}
```

### 3.2 算法

- 输入 `KnowledgeGraph` + `LayoutConfig`
- 初始化：随机位置（固定 seed 可复现）
- 迭代：斥力 + 边引力 + 弱中心引力
- Tag 节点半径传入 `GraphNode::radius` 供碰撞（可选）

### 3.3 API

```rust
pub fn compute_graph_layout(graph: &KnowledgeGraph, config: &LayoutConfig) -> GraphLayout;
pub fn layout_tick(state: &mut LayoutSimulation, dt: f32); // 供步骤 7 动画
```

### 3.4 测试

- 三角形三节点边应收敛非重叠
- 同 seed 两次结果一致

## 验收

- `cargo test graph_layout` 通过
- 无新 crate 依赖

## 建议 commit

`feat(editor): 实现知识图谱力导向布局`
```

---

## 步骤 4/7：KnowledgeGraphElement 静态渲染

```markdown
# 步骤 4/7：KnowledgeGraphElement 静态渲染

新建 `src/editor/graph_view.rs`（或 `src/components/graph/mod.rs`），实现 GPUI Element。

## 任务

### 4.1 主题色

`src/theme/theme.rs` 新增：

- `graph_node_document`
- `graph_node_tag`
- `graph_edge`
- `graph_background`

各内置主题补默认值。

### 4.2 Element 结构

```rust
pub struct KnowledgeGraphElement {
    graph: KnowledgeGraph,
    layout: GraphLayout,
    viewport: GraphViewport, // offset + scale，默认 fit
}
```

- `prepaint`：视口变换 + 裁剪 draw list
- `paint`：先边后节点；节点用 `paint_quad` + 圆角；标签文字 truncate
- Tag 半径：`base + k * sqrt(count)`

### 4.3 临时挂载

在 `workspace.rs` 或测试 harness 中 **dev-only** 渲染一块固定区域验证（可 feature gate），或单元测试仅测 layout→bounds。

本步 **不要求** 完整侧栏 Tab。

## 验收

- 编译通过；手动或测试验证能画出节点与边
- 无交互

## 建议 commit

`feat(graph): 添加知识图谱 GPUI 静态渲染 Element`
```

---

## 步骤 5/7：交互 — 拖拽、平移、缩放、点击

```markdown
# 步骤 5/7：交互 — 拖拽、平移、缩放、点击

在 `graph_view.rs` 扩展交互；用 `canvas()` 捕获指针（参考 `render.rs` 滚动条）。

## 任务

### 5.1 GraphViewport

- 滚轮缩放（以指针为中心）
- 空白区域拖拽平移
- `fit_to_bounds()` 重置

### 5.2 节点拖拽

- hit test：节点半径内
- 拖拽时 `pinned = true`，更新 layout position
- `MouseUp` 结束拖拽

### 5.3 点击

- Document 节点 → 回调 `open_workspace_path`
- Tag 节点 → 回调 `filter_workspace_by_tag`
- 在 `WorkspaceState` 预留 callback 或 `Editor` 方法

### 5.4 光标

- 节点上 `CursorStyle::PointingHand`；空白 `Grab` / `Grabbing`

## 验收

- 手动测试：拖拽单节点、平移缩放、点击有响应（可先 log）

## 建议 commit

`feat(graph): 支持图谱节点拖拽与视口平移缩放`
```

---

## 步骤 6/7：侧栏 Graph Tab 与索引同步

```markdown
# 步骤 6/7：侧栏 Graph Tab 与索引同步

接入工作区侧栏，与 tag/link 索引联动。

## 任务

### 6.1 WorkspaceTab

- `WorkspaceTab` 新增 `Graph`
- Tab 栏图标：`assets/icon/workspace/graph.svg` + `main.rs` 注册
- `render_workspace_graph_panel()`

### 6.2 状态与同步

`WorkspaceState` 增加：

- `link_index`, `link_index_busy`
- `knowledge_graph`, `graph_layout`, `graph_viewport`, `graph_busy`

`Editor`：

- `sync_workspace_link_index` — 模式同 tag_index
- `sync_knowledge_graph` — tag + link revision 变化时重建 graph + layout
- 保存文件时 `refresh_link_index_for_file`

### 6.3 UI

- building / empty / 图谱三态
- 工具栏：重置布局、适应窗口（调用 viewport fit）
- i18n：`locales/en.jsonc`、`locales/zh-CN.jsonc`

### 6.4 点击接通

- 文档节点 → `open_workspace_path`
- 标签节点 → Tags Tab + `select_workspace_tag`

## 验收

- 打开含 tag/wiki 的工作区，Graph Tab 可见关系图
- 编辑保存后图谱更新

## 建议 commit

`feat(workspace): 添加知识图谱侧栏 Tab 与索引同步`
```

---

## 步骤 7/7：动画、过滤与性能收尾

```markdown
# 步骤 7/7：动画、过滤与性能收尾

## 任务

### 7.1 入场动画

- 打开 Graph Tab 时跑 `layout_tick` 60–120 帧
- `cx.spawn` + `cx.notify()` 或 GPUI timer
- 动画期间降低边 alpha 可选

### 7.2 过滤器

`GraphFilter`：

- `All` — 全部 md 节点
- `ConnectedOnly` — 至少一条边（默认）
- 工具栏切换

### 7.3 性能

- 视口外节点/边不绘制
- 边 > 3000 时仅绘制 hover 节点邻边（可选）
- `graph_busy` 全量重建在后台线程

### 7.4 文档与收尾

- 更新 `docs/knowledge-graph-implementation.zh-CN.md` 实现状态（若与方案有偏差）
- `./scripts/check.sh` 或 `cargo test` 全绿

## 验收

- 大图（100+ 节点）可接受帧率
- 过滤器生效
- 队列全部 done

## 建议 commit

`feat(graph): 添加入场动画与工作区图谱性能优化`
```

---

## 队列完成后

1. `.cursor/knowledge-graph-queue/state.json` → `"enabled": false`
2. 从 `.cursor/hooks.json` 移除或保留 hook（disabled 即可）
3. 手动验证：多文件、多 tag、wiki 交叉引用、拖拽与跳转
4. 按需提交 PR

---

## 可选后续（不在本队列）

| 功能 | 说明 |
| --- | --- |
| 展开到主编辑区 | Graph 占满 `main_content` |
| 断链虚线 | 未 resolve 的 wiki 目标 |
| 按选中 tag 子图 | 从 Tags 面板「在图谱中显示」 |
| Barnes-Hut | 500+ 节点斥力优化 |
| YAML front matter tags | 与行内 tag 合并 |
