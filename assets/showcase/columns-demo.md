# Columns Demo

This demo shows the minimal columns syntax:

```markdown
::: columns
--- column
Left column content

--- column
Right column content
:::
```

---

## 1. Basic Two Columns

::: columns
--- column
### Left Column

This is the left column. It can contain normal Markdown paragraphs and inline styles.

- Supports lists
- Supports **bold text**
- Supports `inline code`

--- column
### Right Column

This is the right column. Each column is parsed as an independent block area.

> Blockquotes can be used inside a column.

:::

---

## 2. Columns With Widths

::: columns
--- column width=35%
### Summary

The left column is narrower and is useful for descriptions, notes, or metadata.

- Owner: Product Team
- Status: Draft
- Updated: 2026-06-13

--- column width=65%
### Details

The right column is wider and can hold the main content.

This layout is useful for dashboards, reports, product specs, and side-by-side explanations.

:::

---

## 3. Table And Chart Mixed

::: columns
--- column width=40%
### Metrics Table

| Metric | Value | Change |
| --- | ---: | ---: |
| Page Views | 12,000 | +18% |
| Visitors | 3,200 | +9% |
| Orders | 860 | +12% |

--- column width=60%
### Flow Chart

```mermaid
flowchart LR
  Visit[Visit Page] --> Signup[Sign Up]
  Signup --> Trial[Start Trial]
  Trial --> Pay[Paid Order]
```

:::

---

## 4. Code And Explanation

::: columns
--- column
### Example Code

```rust
fn columns_enabled(markdown: &str) -> bool {
    markdown.contains("::: columns")
}
```

--- column
### Explanation

The code block can safely contain normal fenced content. Column markers inside fenced code are treated as code text, not as layout syntax.

```markdown
--- column
::: columns
```

:::

---

## 5. Report Style Layout

::: columns
--- column width=50%
### Product Notes

The columns block is intended for compact side-by-side writing.

Good use cases:

1. Comparing two options
2. Showing data and explanation together
3. Combining chart and table blocks

--- column width=50%
### Risk Notes

The current minimal implementation treats the columns source as one preserved raw Markdown block in the editor model, while HTML/PDF export renders it as real columns.

This keeps the first version simple and safe.

:::
