# block 0.1.6 本地 patch

上游 [`block`](https://crates.io/crates/block) 自 2016 年起未维护，通过 gpui → cocoa/metal 等间接依赖引入。Rust 编译器即将拒绝其 `extern static _NSConcreteStackBlock: Class` 中对不可实例化类型 `enum Class {}` 的用法（[rust-lang/rust#74840](https://github.com/rust-lang/rust/issues/74840)）。

改动：将 `enum Class {}` 替换为 `#[repr(C)] struct Class;`，保持 ABI 兼容并消除 future-incompat 警告。

上游 issue：<https://github.com/SSheldon/rust-block/issues/21>
