---
inclusion: fileMatch
fileMatchPattern: ['assets/icon/**', 'src/main.rs', 'src/editor/**', 'src/components/**']
---

# SVG 图标资源注册

Velotype 通过 GPUI 的 `AssetSource` 在**编译期**嵌入 SVG。仅把文件放进 `assets/icon/` **不会**自动可用；未注册时 `svg().path(...)` 会加载失败，按钮区域空白。

## 添加新图标（必须三步）

1. **放置 SVG**
   - 路径：`assets/icon/<category>/<name>.svg`（如 `assets/icon/toolbar/undo-2.svg`）
   - 描边类图标使用 `stroke="currentColor"`，以便 `.text_color(...)` 着色

2. **在 `src/main.rs` 注册**
   - 在 `impl AssetSource for VelotypeAssets` 的 `load()` 中增加 `match` 分支
   - `match` 的 key 必须与 UI 中 `svg().path(...)` 的路径字符串**完全一致**

```rust
"icon/toolbar/undo-2.svg" => Ok(Some(Cow::Borrowed(include_bytes!(
    "../assets/icon/toolbar/undo-2.svg"
)))),
```

3. **在 UI 中引用**
   - 常量或字面量使用同一 path，例如 `"icon/toolbar/undo-2.svg"`

## 自检清单

- [ ] `assets/icon/...` 下已有 SVG 文件
- [ ] `src/main.rs` 的 `VelotypeAssets::load()` 已添加对应分支
- [ ] UI 中的 path 与 `match` key 一致（含子目录与文件名）
- [ ] 修改后重新编译/重启 dev 进程

## 常见错误

- 只加了 SVG + UI 常量，**忘记改 `main.rs`**（图标不显示的最常见原因）
- path 不一致，如 UI 写 `icon/toolbar/foo.svg`，注册写错文件名或目录
