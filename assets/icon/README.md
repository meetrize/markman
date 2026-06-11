# About Icons

These SVG icons are sourced from Iconify and stored locally so the app can embed
them through the GPUI asset source at build time.

## Adding a new icon

1. Save the SVG under `assets/icon/<category>/<name>.svg`.
2. Register it in `src/main.rs` inside `VelotypeAssets::load()` with `include_bytes!`.
3. Reference the same path string from UI code, e.g. `svg().path("icon/toolbar/foo.svg")`.

The match key in `load()` must exactly match the path passed to `svg().path(...)`.
Without step 2, the icon will not render.

| Local file | Iconify icon | Icon set | License |
| --- | --- | --- | --- |
| `workspace/folder.svg` | [`material-symbols:folder`](https://icon-sets.iconify.design/material-symbols/folder/) | Material Symbols | Apache-2.0 |
| `workspace/markdown.svg` | [`mdi:language-markdown`](https://icon-sets.iconify.design/mdi/language-markdown/) | Material Design Icons | Apache-2.0 |
| `titlebar/chrome-close.svg` | [`codicon:chrome-close`](https://icon-sets.iconify.design/codicon/chrome-close/) | Codicons by Microsoft Corporation | CC BY 4.0 |
| `titlebar/chrome-minimize.svg` | [`codicon:chrome-minimize`](https://icon-sets.iconify.design/codicon/chrome-minimize/) | Codicons by Microsoft Corporation | CC BY 4.0 |
| `titlebar/chrome-maximize.svg` | [`codicon:chrome-maximize`](https://icon-sets.iconify.design/codicon/chrome-maximize/) | Codicons by Microsoft Corporation | CC BY 4.0 |
| `titlebar/chrome-restore.svg` | [`codicon:chrome-restore`](https://icon-sets.iconify.design/codicon/chrome-restore/) | Codicons by Microsoft Corporation | CC BY 4.0 |

The exported SVGs keep `fill="currentColor"` so the app can color icons with the
active Velotype theme.
