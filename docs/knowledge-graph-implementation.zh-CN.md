# 知识图谱 — 方案设计

本文档描述 Markman 中 **工作区知识图谱** 的功能设计：基于行内 `#tag` 与 Wiki 链接 `[[path]]` 构建点线网状关系图，原生 GPUI 高性能渲染，支持力导向布局、节点拖拽与缩放平移。

[开发与构建](development.zh-CN.md) | [Hook 自动开发步骤](knowledge-graph-hook-steps.zh-CN.md) | [标签方案](hashtag-tag-implementation.zh-CN.md) | [Wiki 链接方案](wiki-link-implementation.zh-CN.md)

---

## 需求概述

用户打开工作区后，希望以 **关系图** 形式理解笔记之间的连接：

| 场景 | 期望行为 |
| --- | --- |
| **数据来源** | 扫描工作区内 Markdown：行内 `#tag` + `[[wiki 链接]]` |
| **节点类型** | 两类节点，**不同颜色**：文档（文件名）与标签 |
| **标签大小** | 标签节点半径/字号随引用次数（`tag_index.counts`）变化 |
| **边** | 文档→标签（含该标签）、文档→文档（Wiki 链接） |
| **交互** | 拖拽节点、画布平移/缩放、点击节点跳转文件或过滤标签 |
| **性能** | 纯 Rust + GPUI 自定义绘制，无 WebView；大图视口裁剪 + 后台布局 |

本功能 **不** 替代现有 Tags 侧栏列表；图谱是 **可视化补充**，与之共享同一套索引数据。

---

## 图模型

### 节点（Node）

```rust
pub enum GraphNodeKind {
    /// 工作区内 .md 文件
    Document { path: PathBuf, label: String },
    /// canonical tag name（小写）
    Tag { name: String, count: usize },
}

pub struct GraphNode {
    pub id: GraphNodeId,
    pub kind: GraphNodeKind,
    /// 布局坐标（画布空间，px）
    pub position: Point<Pixels>,
    /// 交互用半径
    pub radius: Pixels,
}
```

| 类型 | 颜色来源（主题 token） | 大小 |
| --- | --- | --- |
| Document | 新增 `graph_node_document` / 复用 `text_link` | 固定半径 |
| Tag | 新增 `graph_node_tag` / 复用 `text_tag` | `radius = base + scale * sqrt(count)` |

### 边（Edge）

```rust
pub enum GraphEdgeKind {
    /// 文档正文含 #tag
    Tagged,
    /// 文档含 [[target]]
    WikiLink,
}

pub struct GraphEdge {
    pub source: GraphNodeId,
    pub target: GraphNodeId,
    pub kind: GraphEdgeKind,
}
```

**方向**：有向边（文档 → 标签，文档 → 目标文档）。渲染时可统一为细线，按 `kind` 可选不同透明度或虚线（Phase 2  polish）。

### 图 ID 约定

- 文档节点 ID：`doc:{canonical_path}`（工作区相对路径 UTF-8）
- 标签节点 ID：`tag:{canonical_name}`

---

## 与现有架构的映射

| 能力 | 现有模块 | 图谱用法 |
| --- | --- | --- |
| 标签索引 | `src/editor/tag_index.rs` | 标签节点 + 文档→标签边 |
| Wiki 解析 | `src/components/markdown/inline/wiki_link.rs` | 边提取须与 inline 规则一致 |
| 文件枚举 | `src/editor/markdown_files.rs` | 文档节点列表（仅含 `.md`） |
| 跳转 | `workspace.rs` — `PendingWorkspaceSearchJump` | 点击文档节点打开文件 |
| 标签过滤 | `filter_workspace_by_tag()` | 点击标签节点 |
| 侧栏 Tab | `WorkspaceTab` | 新增 `Graph` 或全屏 overlay |
| 自定义绘制 | `BlockTextElement` | 参考 `prepaint` / `paint` + `window.paint_quad` |
| 指针交互 | `render.rs` — `canvas()` | 平移、缩放、节点拖拽 |
| 异步索引 | `cx.spawn` + `tag_index_busy` | 同样模式构建 link index |

### 需新建模块

| 文件 | 职责 |
| --- | --- |
| `src/editor/link_index.rs` | 工作区 Wiki 链接反向/正向索引（类 `tag_index.rs`） |
| `src/editor/graph_model.rs` | 合并 tag + link 索引 → `KnowledgeGraph` |
| `src/editor/graph_layout.rs` | 力导向布局（纯 Rust，无新依赖） |
| `src/editor/graph_view.rs` | GPUI `KnowledgeGraphElement` + 面板 UI |

---

## 索引层设计

### Wiki 链接索引（新建）

```rust
pub struct LinkOccurrence {
    pub source_path: PathBuf,
    pub target_path: String,   // 工作区相对路径（未 resolve）
    pub line: usize,
    pub match_start_byte: usize,
}

pub struct WorkspaceLinkIndex {
    /// source → outgoing links
    pub by_source: BTreeMap<PathBuf, Vec<LinkOccurrence>>,
    /// resolved target (relative) → backlinks
    pub by_target: BTreeMap<String, Vec<LinkOccurrence>>,
    pub revision: u64,
}
```

**提取策略**（与 `tag_index` 一致）：

1. `collect_markdown_files(root)`
2. 逐文件读 UTF-8，按行扫描
3. 跳过 fence、ATX 标题行
4. 行内 `locate_wiki_link_in_str`（从 `wiki_link.rs` 抽出行级 API，与 inline 共用规则）
5. 增量：`refresh_link_index_for_file` / `remove_file_from_link_index`

**路径 resolve**：构建边时将 `target_path` 相对 `source_path` 的父目录解析；无法 resolve 的链接仍建边到「虚拟文档节点」或跳过（MVP：跳过并计数 `broken_links` 供 UI 提示）。

### 图构建

```rust
pub struct KnowledgeGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub revision: u64,
}

pub fn build_knowledge_graph(
    root: &Path,
    tag_index: &WorkspaceTagIndex,
    link_index: &WorkspaceLinkIndex,
) -> KnowledgeGraph { ... }
```

- 每个 `.md` 文件 → 1 个 Document 节点（即使无出边也显示，可选「仅有关联的节点」过滤）
- `tag_index.by_tag` 每个 key → 1 个 Tag 节点
- 每条 tag occurrence → Document→Tag 边（去重）
- 每条 resolved wiki link → Document→Document 边（去重）

---

## 布局与动画

### 力导向算法（MVP）

无外部依赖，实现简化 **Fruchterman-Reingold** 或 **spring-electrical**：

| 力 | 作用 |
| --- | --- |
| 斥力 | 所有节点对（或 Barnes-Hut 优化，>300 节点时再上） |
| 引力 | 沿边拉近距离，Wiki 边略强于 Tag 边（可调） |
| 中心引力 | 防止节点飞散 |
| 阻尼 | 每 tick 速度 × damping |

**执行方式**：

1. 后台线程 / `cx.spawn` 跑 N 次迭代（如 300），产出 `Vec<Point>`
2. 打开图谱时若 revision 未变，复用缓存 layout
3. 打开后可选 **入场动画**：前 60 帧继续迭代 + `cx.notify()` 刷新

**拖拽**：用户拖动节点时，该节点 `pinned = true`，每帧固定位置；释放后可取消 pin 或保持。

### 视口

- `GraphViewport { offset, scale }` — 滚轮缩放、中键/空白拖拽平移
- 变换：`screen = (world + offset) * scale`
- 裁剪：仅绘制与视口 AABB 相交的边与节点

---

## 渲染层设计

### `KnowledgeGraphElement`（GPUI `Element`）

```
prepaint:
  - 读取 graph + layout + viewport
  - 视口裁剪后的 draw list

paint:
  - 边：window.paint_quad 细矩形 或 Lyon path（Phase 2）
  - 节点：paint_quad + corner_radii 近似圆，或缓存 circle SVG
  - 标签：节点下方/内部 truncate 文件名或 #tag
  - 高亮：hover / selected 加描边
```

**性能要点**：

- 不在 `paint` 内分配 Vec；draw list 在 `prepaint` 缓存
- 边数 > 2000 时降低 alpha 或聚合（仅显示选中节点邻边）
- 节点数 > 500 时默认启用「隐藏孤立文档」过滤器

### 交互（`canvas` 子层）

| 手势 | 行为 |
| --- | --- |
| 空白拖拽 | 平移 viewport |
| 滚轮 / pinch | 缩放（以指针为中心） |
| 节点按下拖拽 | 移动节点，标记 pinned |
| 单击节点 | Document → `open_workspace_path`；Tag → `filter_workspace_by_tag` |
| 双击空白 | 重置 viewport / 重新布局（可选） |

---

## UI 接入方案

**推荐：工作区第四 Tab `Graph`**（与 Files / Outline / Tags 并列）

理由：

- 与标签、文件树同一上下文，切换自然
- 侧栏宽度可拖拽；图谱区域不足时提供 **「展开到编辑区」** 按钮（Phase 2）

**状态字段**（`WorkspaceState`）：

```rust
graph: Option<KnowledgeGraph>,
graph_layout: Option<GraphLayoutCache>,
graph_viewport: GraphViewport,
graph_busy: bool,
graph_filter: GraphFilter,  // All | ConnectedOnly | TagsOnly
selected_graph_node: Option<GraphNodeId>,
```

**同步时机**：与 `sync_workspace_tag_index` 并行 `sync_workspace_link_index`；二者 revision 变化后 `sync_knowledge_graph`。

---

## 主题与 i18n

### 主题（`theme.rs`）

```rust
graph_node_document: Hsla,
graph_node_tag: Hsla,
graph_edge: Hsla,
graph_edge_wiki: Hsla,  // 可选
graph_background: Hsla,
graph_label: Hsla,
```

### i18n 键（示例）

- `workspace.tab.graph`
- `workspace.graph.empty`
- `workspace.graph.building`
- `workspace.graph.filter.connected`
- `workspace.graph.reset_layout`

---

## 分阶段实施（Phase 概览）

| Phase | 内容 | Hook 步骤 |
| --- | --- | --- |
| 1 | Wiki 链接工作区索引 | 步骤 1 |
| 2 | 图模型合并 tag + link | 步骤 2 |
| 3 | 力导向布局引擎 + 单元测试 | 步骤 3 |
| 4 | 自定义 Element 静态渲染 | 步骤 4 |
| 5 | 拖拽 / 平移 / 缩放 / 点击 | 步骤 5 |
| 6 | 侧栏 Graph Tab + 同步 + i18n | 步骤 6 |
| 7 | 动画、过滤、性能与收尾 | 步骤 7 |

---

## 验收标准（整体）

1. 工作区含 `#tag` 与 `[[link]]` 的 `.md` 文件，Graph Tab 显示两类色点与连线
2. 标签点大小随 count 明显变化
3. 拖拽节点流畅；滚轮缩放、空白平移正常
4. 点击文档节点打开文件；点击标签节点选中 Tags 面板对应项
5. 保存/编辑文件后索引增量更新，图谱 revision 刷新
6. `cargo test` 通过；无新增 npm/WebView 依赖

---

## 风险与对策

| 风险 | 对策 |
| --- | --- |
| 大图卡顿 | 视口裁剪、边简化、默认隐藏孤立节点 |
| 布局不稳定 | 固定随机种子；足够迭代次数；缓存 layout |
| 断链 Wiki | resolve 失败跳过或灰色虚线（Phase 2） |
| 侧栏太窄 | 提供展开到主区域；最小宽度提示 |

---

## 实现状态（2026-06）

Hook 队列 7 步已全部落地。与初稿的主要偏差：

- 图谱状态保存在 `Editor::knowledge_graph_view`，而非 `WorkspaceState` 内嵌 `knowledge_graph` / `graph_layout`
- 边线使用 `PathBuilder::stroke` 绘制（`Path::new().line_to()` 单段无法 tessellate，会导致连线不可见）
- 默认过滤器为 `ConnectedOnly`（仅显示至少一条边的节点）
- 入场动画约 90 帧力导向 tick；大图边数 > 3000 时仅绘制 hover 节点邻边

---

## 参考

- `docs/hashtag-tag-implementation.zh-CN.md` — 标签索引
- `docs/wiki-link-implementation.zh-CN.md` — Wiki 链接
- `src/components/block/element.rs` — 自定义 Element 绘制
- `src/editor/tag_index.rs` — 扫描与增量索引模式
