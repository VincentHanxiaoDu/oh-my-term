# 研究: Warp 终端窗口/分辨率变化的自适应处理

> 研究对象:`/Users/hanxiao.du/Desktop/vincent/projects/oh-my-term/.research/warp`(warpdotdev/warp,本地 clone 处于 `master` 分支,与 GitHub master 同步)。只读研究,未修改任何仓库文件。
>
> 行号说明:本环境无 shell/grep,所有行号通过 `read` 工具按偏移分片人工定位。标注的文件行号精确到 ±1 行;`view.rs`(29,159 行)内部部分函数仅给出函数名与已核实的锚点行。

## Summary

Warp 的 resize 处理是 **三层解耦** 架构:(1) 窗口尺寸变化只是触发一次字体度量驱动的重布局(布局约束即 pane 尺寸),UI 层(Layout 后回调)才知道新尺寸; (2) 终端尺寸由 `SizeInfo`(pane 像素 + 行列数)统一承载,行/列数**永远由 pane 像素换算并 clamp 下限**(MIN_ROWS=1 / MIN_COLUMNS=2),杜绝 0×0; (3) resize 动作通过 `TerminalSizeElement.after_layout → 无界 channel → 异步流` 消费,天然合并每帧多次布局产生的重复 resize;最后才触发网格 reflow(flat-storage 全量压扁重排)与 PTY `TIOCSWINSZ`(带像素尺寸)。Split/pane 布局用**比例 flex 树**,窗口变化时各 pane 按 flex 比例自动伸缩,拖拽改 flex 时用最小尺寸保护与 EPSILON 去抖。

---

## 1. Warp 如何处理终端 resize:完整事件链路

```
winit 窗口 Resized
  → warpui 窗口层以新窗口尺寸作为根布局约束,触发全树重布局
  → PaneGroup/Flex 按 pane 树 flex 比例分配每个 pane 的像素尺寸
  → TerminalSizeElement::after_layout() 拿到本 pane 最终尺寸
      └─ resize_tx.try_send(terminal_size)      (非阻塞)
  → 异步流 after_terminal_view_layout(resize_rx)
      └─ 构建 SizeUpdate(AfterLayout,last_size,new_pane_size)
      └─ resize_internal(size_update)
           ├─ 早退: anything_changed()==false 且非 Refresh
           ├─ 0 尺寸保护: new_size.x()==0 || new_size.y()==0 → skip
           ├─ self.model.lock().resize(size_update)      → GridHandler::resize(网格 reflow)
           ├─ input.set_size_info / inline_menu_positioner.set_size_info
           ├─ update_scroll_position_locking(AfterResize)  (滚动位置保持)
           └─ ctx.emit(Event::Resize { size_update })   → 冒泡给 pane/workspace
  → PtyIntent::Resize(SizeUpdate) → PtyController → Message::Resize
  → PTY 事件循环 drain: Message::Resize(size) => pty.on_resize(&size)
  → ioctl(TIOCSWINSZ): 行列数 + pane 像素尺寸同时写入内核
```

关键代码(按链路顺序):

**① 布局后尺寸上报——事件驱动的核心解耦点** `app/src/terminal/terminal_size_element.rs:36-45`

```rust
fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
    self.child.after_layout(ctx, app);
    let terminal_size = self.size().expect("Size should be present");
    // 会话已终止时 channel 关闭,try_send 失败直接忽略
    let _ = self.resize_tx.try_send(terminal_size);
}
```

**② 消费端接线** `app/src/terminal/view.rs`(已核实锚点)

```rust
// L3645: 无界 channel,每帧布局后都 try_send,流式消费天然合并
let (resize_tx, resize_rx) = async_channel::unbounded();
// L3715: resize 在布局之后异步处理,不在布局过程中同步执行
let _ = ctx.spawn_stream_local(resize_rx, Self::after_terminal_view_layout, |_, _| {});
```

`resize_internal`(view.rs 内,函数级引用)中的两道闸门:

```rust
// 非有效 resize 直接返回(尺寸没变 / gap 没变 / 行列没变)
if !(size_update.anything_changed() || size_update.is_refresh()) { return; }
let new_size = size_update.new_size.pane_size_px();
if new_size.x() == 0. || new_size.y() == 0. {
    // 注释: pane 收起时每个布局帧都会走到这里,用 debug 级别避免刷屏
    log::debug!("Tried to resize with size {new_size:?}. Skipping resize");
    return;
}
```

**③ 模型侧网格 reflow** `app/src/terminal/model/grid/resize.rs:16-56`

```rust
pub fn resize(&mut self, size: SizeInfo) {
    self.ansi_handler_state.cell_width = size.cell_width_px.as_f32() as usize;
    self.ansi_handler_state.cell_height = size.cell_height_px.as_f32() as usize;
    let old_cols = self.columns();  let old_rows = self.visible_rows();
    let num_cols = size.columns();  let num_rows = size.rows();
    if old_cols == num_cols && old_rows == num_rows {
        log::debug!("Term::resize dimensions unchanged");
        return;                                   // L26-29: 尺寸未变早退
    }
    if num_rows == 0 {
        log::debug!("Ignoring resize down to zero visible lines");
        return;                                   // L31-34: 0 行保护
    }
    ...
    self.resize_storage(num_rows, num_cols);      // L44: 真正的 reflow
    self.ansi_handler_state.scroll_region = VisibleRow(0)..VisibleRow(self.visible_rows());
    if !self.secrets.is_empty() { self.scan_for_secrets_after_resize(); }
    self.refilter_lines();
}
```

**④ PTY 通道** `app/src/terminal/writeable_pty/message.rs:14-17`(`Message::Resize(SizeInfo)`)、`app/src/terminal/local_tty/event_loop.rs:162-177`:

```rust
fn drain_recv_channel(&mut self, state: &mut State) -> ChannelResult {
    while let Ok(msg) = self.rx.try_recv() {
        match msg {
            Message::Input(input) => state.write_list.push_back(input),
            ...
            Message::Resize(size) => self.pty.on_resize(&size),   // L171
```

**⑤ 内核写入** `app/src/terminal/local_tty/unix.rs:719-727` + `:748-757`

```rust
fn on_resize(&mut self, size: &SizeInfo) {
    let win = size.to_winsize();
    let res = unsafe { libc::ioctl(self.fd.as_raw_fd(), libc::TIOCSWINSZ, &win as *const _) };
    if res < 0 { panic!("ioctl TIOCSWINSZ failed: {}", io::Error::last_os_error()); }
}
// ToWinsize: 行/列 + 像素尺寸一起给内核(vim 分屏等全屏 TUI 依赖像素值)
winsize { ws_row: rows, ws_col: columns,
          ws_xpixel: pane_width_px, ws_ypixel: pane_height_px }
```

---

## 2. Split/Tab/输入框的 UI 重排:算法与保证

### 2.1 Split pane:比例 flex 树(窗口 resize 时无需"布局算法",天然等比伸缩)

Pane 树是 `PaneNode::Leaf(pid) | PaneNode::Branch { axis, nodes: Vec<(PaneFlex, PaneNode)>, dividers, ... }`(`app/src/pane_group/tree.rs:57-71`)。渲染时每个子节点以 **flex 权重** 加入 Flex 容器(`app/src/pane_group/tree.rs:1093-1095`):

```rust
parent.add_child(
    Shrinkable::new(flex_value, node.render(theme, hidden_panes, app)).finish(),
);
```

`Shrinkable(flex)` 是 warpui 的 flex 布局原语 → **窗口宽度变化时,同一 branch 内的 pane 按 flex 值等比伸缩**,不需要任何"重新计算像素"的代码;窗口级 resize 的唯一动作就是带新约束重布局。

**拖拽 divider 时如何把像素位移转回比例**(`app/src/pane_group/tree.rs:1136-1187`):

```rust
if size_1 + delta < minimum_pane_size || size_2 - delta < minimum_pane_size
    || delta.abs() < f32::EPSILON {
    return true;                      // L1162-1167: 最小尺寸保护 + 抖动过滤
}
// Re-distribute the flex factors.
let new_flex = ((size_1 + delta) / (size_1 + size_2) * total_flex)
    .max(0.).min(total_flex);         // L1169-1172: 像素位移 → 新的 flex 占比
self.nodes[idx].0 = PaneFlex(new_flex);
self.nodes[idx + 1].0 = PaneFlex(total_flex - new_flex);   // 同支占比守恒
```

配套保证:
- **Pane 最小像素尺寸**:`MINIMUM_PANE_SIZE = 50.` / `MINIMUM_PANE_SIZE_UDI = 190.`(Rich Input 输入框模式,输入框高度相关;`app/src/pane_group/mod.rs:221-222`),`get_minimum_pane_size()` L240-247。
- **双击 divider 重置比例**:`reset_pane_sizes` 把所在 branch 全部 flex 置回 `DEFAULT_FLEX_SIZE = 1.0`(`tree.rs:1189-1204`,对应 PR #11293),测试见 `tree_tests.rs:611-687`。
- **键盘微调**:`KEYBOARD_RESIZE_DELTA = 10.` px(`mod.rs:223`),`adjust_pane_size_by_id` 沿 pane 祖先链找同轴 branch 生效(`tree.rs:1224-1243`)。
- 隐藏 pane(拖拽/关闭/child-agent)渲染时 flex 置 0,保持占位(`tree.rs:1088-1089`)。
- Agent 面板新开最小宽度 400px,小窗口回退为窗口宽度 50%(`mod.rs:273`)。

### 2.2 终端行列数:pane 像素的纯函数 + 下限 clamp(这就是对 0×0 的根治)

`app/src/terminal/mod.rs:364-384`(行号精确):

```rust
pub fn new(pane_size_px, cell_width_px, cell_height_px, padding_x_px, padding_y_px) -> SizeInfo {
    let rows = (pane_size_px.y() - 2. * padding_y_px.as_f32()) / cell_height_px.as_f32();
    let columns = (pane_size_px.x() - 2. * padding_x_px.as_f32()) / cell_width_px.as_f32();
    SizeInfo {
        ...
        columns: max(columns as usize, MIN_COLUMNS),   // L377
        rows: max(rows as usize, MIN_ROWS),            // L378
```

其中 `MIN_ROWS = 1`、`MIN_COLUMNS = 2`(2 列是为容纳全宽 unicode 字符,注释明说;`mod.rs:109,113-114`)。**关键设计**(注释原文,`SizeInfo.rows` 字段文档):*rows 永远由 pane 尺寸决定,而不是内容元素尺寸——因为输入框出现/消失会频繁改变内容高度,而很多程序不擅长在处理尺寸变化;为了规避,让它们以为 rows 始终来自 pane 尺寸*。这是"输入框出现导致终端高度抖动→程序收到 resize 风暴"这一经典 bug 的 Warp 解法。

窗口级下限:`MIN_WINDOW_WIDTH=480` / `MIN_WINDOW_HEIGHT=192`(测试环境 124/34;`crates/warpui/src/windowing/mod.rs:12-14,18-20`),从 OS 层就不允许窗口缩到 0。

### 2.3 Tab/输入框等其他 UI

- Tab 栏是固定高度的 flex 组件,窗口变窄时 tab 标题按宽度阈值降级(icon-only < 42px、隐藏 pin < 70px,`app/src/tab.rs` 的 `COMPACT_TAB_WIDTH_THRESHOLD` / `TAB_PIN_VANISH_THRESHOLD` 常量)——同类"窄窗重排"可借鉴的分层降级策略。
- 输入框:`resize_internal` 中同步 `input.set_size_info(...)` 与 `inline_menu_positioner.set_size_info(...)`(弹层定位),并调用 `update_scroll_position_locking(ScrollPositionUpdate::AfterResize)` 保持滚动位置(避免 resize 后视图跳顶)。
- 网格内容高度与"gap"(ctrl-L 清屏区):`after_terminal_view_layout` 会按新尺寸重算 `new_gap_height`,滚动锁定保证用户视野稳定(浮点比较用 `HEIGHT_FUDGE_FACTOR_LINES = 0.01` 容差,`terminal/mod.rs:131`)。

---

## 3. Resize 相关的"坑"与对策

| 坑 | Warp 对策 | 位置 |
|---|---|---|
| **0×0 尺寸** | 三层防线:窗口 OS 层最小 480×192;`SizeInfo::new` 行列 clamp(≥1/≥2);`resize_internal` 对 px==0 早退(注释: pane 收起时每次布局都会到这里,故用 debug 日志防刷屏) | windowing/mod.rs:12-20; terminal/mod.rs:377-378; view.rs `resize_internal` |
| **事件风暴**(连续拖窗/输入框抖动产生大量 resize) | ① resize **不**在布局中同步执行,而是 `try_send` 到无界 channel 后由流异步消费(每帧至多合并进一次 layout 回调);② `anything_changed()` 全面比较(last/new size、gap、行列、pane px)后才动手;③ 网格侧 old==new 早退;④ `heights_approx_eq` 0.01 行容差吸收浮点累积误差;⑤ 输入框高度变化**不进** rows 计算(pane 尺寸为准)从源头减少无谓 resize;⑥ 拖拽用 `EPSILON` 滤噪 | terminal_size_element.rs:36-45; view.rs:3715 `resize_internal`; resize.rs:26-29; terminal/mod.rs:131-170 |
| **性能**(大 scrollback reflow) | alt screen(全屏 TUI)与 CLI-agent TUI **跳过 reflow**,只调 `grid.resize(false, rows, cols, …)` 改尺寸,内容不动(注释引用 GH #9838:防止 pane resize 把旧帧塞进 scrollback);普通网格才走"全量压扁到 flat storage → 按新列宽重排 → 弹出"路径;函数级阈值 `MAX_LOCKED_READ`(PTY 读锁限制)作为模型侧吞吐保护 | resize.rs:60-84(alt screen 分支)、108-111(压扁)、132-136、154-160(缩行裁剪) |
| **光标丢位/错位** | `InitialCursorState` 在 reflow 前:先 clamp 光标到可见区(debug 断言记录越界)、对 `input_needs_wrap` 与非空行尾光标做"cell-after-point"转换;在旧网格上给光标所在 cell 打 `HAS_CURSOR` 标记保证 reflow 后 cell 存在;reflow 后按内容偏移量还原光标并恢复 `input_needs_wrap`,最后把光标行 clamp 到最后可见行 | resize.rs:205-305(`InitialCursorState::new` 等,约 L205-305) |
| **滚动/清屏间隙错位** | `after_terminal_view_layout` 重算 gap;`update_scroll_position_locking(AfterResize)` 保持滚动偏移 | view.rs(函数级) |
| **vim 等全屏程序拒绝 reflow** | ① alt screen 直接调 `grid.resize`(不重排内容)并同步 PTY `TIOCSWINSZ`(带像素尺寸);② 专为"已知 resize 有问题的 alt-screen 应用"做强制刷新 hack:先发 `pane_width_px+1` 的错误尺寸再发正确尺寸,强制 TUI 重绘(`ALT_SCREEN_APPS_WITH_RESIZE_PROBLEMS`) | resize.rs:60-84; view.rs(函数级,`ALT_SCREEN_APPS_WITH_RESIZE_PROBLEMS`) |
| **shared-session 观看者尺寸** | 观看者把自然尺寸上报给分享者,分享者 PTY 取两者较大行列;`active_viewer_driven_size` 记忆,防止 AfterLayout 覆盖回观看者尺寸;resize-reason 检查防环 | terminal/mod.rs:196-204; view.rs `SizeUpdateBuilder::build` |
| **PTY ioctl 失败** | `panic!`(宁可崩溃暴露,不静默) | unix.rs:724-726 |

---

## 4. 自动化测试(位置清单)

已核实的 resize 相关测试:

1. **`app/src/terminal/size_update_tests.rs`**(全文件,约 19 行)— `SizeUpdate` 纯逻辑单测:`headless_layout_update_uses_cell_dimensions`(行列变更判断)、`unchanged_headless_layout_has_no_dimension_change`(无变化时 `rows_or_columns_changed()==false`)。
2. **`app/src/pane_group/tree_tests.rs:611-687`** — pane 树比例恢复:`test_reset_pane_sizes_resets_containing_branch`(整支重置为 `DEFAULT_FLEX_VALUE`)与 `test_reset_pane_sizes_only_resets_containing_branch`(只重置含该 divider 的支,嵌套支不受影响);另有大量 split/move/navigation 结构测试。
3. **`app/src/pane_group/mod_tests.rs`**(3,954 行,`#[path="mod_tests.rs"] mod tests`)— 集成级 pane 行为(App::test + mock_pane_group,窗口 1024×768),覆盖拆分/聚焦/关闭等;resize 拖拽相关用例未逐一核实行号。
4. **`app/src/terminal/ref_tests/`**(`mod.rs` + `data/` 目录)— alacritty 风格**数据驱动回放测试**:录制字节流 + `size.json`(SizeInfo)+ `grid.json`(期望网格)全等断言(`mod.rs:97-183`,含 `vim_large_window_scroll`、`deccolm_reset`(132 列模式)、`grid_reset` 等 ~50 个用例)——这就是 resize/重排行为回归测试的主要抓手。注意:`data/` 目录下**没有**独立命名的 `resize` 用例目录(已列全目录核实)。
5. **`app/src/integration_testing/window/step.rs`** — `add_window_and_check_bounds`(开窗并断言窗口 bounds)等窗口级集成步骤;`window/assertion.rs` 断言窗口数;真实的"改窗口尺寸→断言终端尺寸"集成用例在 `integration_testing` 各模块里,未逐一定位。
6. **`app/src/terminal/model/grid/resize.rs`** 内嵌 debug 断言(光标越界 `report_error!`),以及 `mod.rs` 底部引用的 `ref_tests`、`size_update_tests` 均在 `cargo test` 中运行。

未能核实的:网格 flat-storage reflow 的单元测试(`crates/warp_terminal` 内 `flat_storage`/`row` 的测试存在但未逐一定位 resize 用例);warpui(winit)窗口层 Resized 处理行号。

---

## 5. 最少 3 个值得借鉴的设计决策

**D1. 行列数 = pane 像素的纯函数 + 硬性下限(`MIN_ROWS=1/MIN_COLUMNS=2`)且"决定性的输入只来自 pane 尺寸"** —— 解决什么:0×0/极小窗口除零、输入框出现消失导致的 resize 抖动风暴、全宽字符在 1 列宽下的渲染崩溃。这是 omt 修 resize bug 最该先抄的一条:`SizeInfo::new`(`terminal/mod.rs:364-384`)这个"单点换算函数"让所有下游只有一种取值来源,0×0 在这个函数里就被吞掉。

**D2. resize 由"布局后回调 + channel + 异步流"驱动,而不是由窗口事件同步触发** —— 解决什么:事件风暴与布局/渲染死锁。`TerminalSizeElement::after_layout → try_send → after_terminal_view_layout`(`terminal_size_element.rs:36-45`、`view.rs:3645/3715`)。布局阶段永远不 resize,一帧内多次布局只合并为一次有效的 `SizeUpdate`;`anything_changed()` 全量比较决定是否真的动手。对 omt:把"窗口事件→立刻 resize PTY"改成"布局/绘制结束后比较新旧尺寸再决定要不要 TIOCSWINSZ",事件风暴自动消失。

**D3. Split 布局用"flex 比例树 + 像素位移→比例增量"而不是"像素绝对值树"** —— 解决什么:窗口变化时 pane 等比伸缩无需任何重排算法;拖拽 divider 的像素位移被折算成 flex 增量(总 flex 守恒),并配最小尺寸(50/190px)与 `EPSILON` 去抖(`tree.rs:1136-1187`)。还配套"双击重置整个 branch 比例"(tree.rs:1189-1204)。omt 若实现 split,直接把像素坐标存面板树会踩"窗口 resize 后必须手写重新分配"的坑;flex 权重 + shrink 因子是最省事且行为符合用户预期的方案。

**D4. 网格 reflow 采用"全量压扁(flat storage)→ 新列宽重排 → 弹出",但 alt-screen 跳过 reflow** —— 解决什么:重排正确性(行包裹在旧/新列宽间无损迁移,光标以内容偏移定位还原)+ 性能(全屏 TUI 高频 resize 时只改尺寸不改内容,且避免把旧帧塞进 scrollback,GH #9838;`resize.rs:58-160`)。光标三态(`AtPoint`/`AtCellAfterPoint` + `input_needs_wrap` 恢复)+ `HAS_CURSOR` 标记是"resize 后光标消失"的标准解法。

**D5. TIOCSWINSZ 同时下发 行列 + 像素尺寸** —— 解决什么:全屏 TUI(vim/tmux 分屏、hterm 类程序)依赖 `ws_xpixel/ws_ypixel` 做像素级布局;只发行列会导致分屏比例错乱(`unix.rs:748-757`)。另外 ioctl 失败直接 panic,不静默吞错(`unix.rs:724-726`)。

**D6(补充). 防循环/防伪变化**:`SizeUpdateReason` 区分 `Refresh`(字体/设置变化强制刷新)、`AfterLayout`、共享会话的 `SharerSizeChanged/ViewerSizeReported`(`terminal/mod.rs:187-205`),"viewer-reported 尺寸不得被 AfterLayout 覆盖回去 + resize-reason 检查防上报循环"——处理"多窗口/多端同步尺寸"时的必备护栏。

---

## 6. 对 omt resize bug 修复的借鉴清单(浓缩)

1. 建一个 `SizeInfo::new(pane_px, cell_px, padding)` 单点换函数:`rows/cols = (pane - 2*padding)/cell`,`max(_, MIN)` —— 修 0×0 与抖动。
2. resize 处理挪到"帧末/布局后",先 `anything_changed()` 再行动,通道或标志位合并同帧多次触发 —— 修事件风暴。
3. 网格重排:宽高变化时先保存光标(内容偏移形式),重排后还原并按新尺寸 clamp;0 行的 resize 直接忽略 —— 修光标丢失。
4. PTY `ioctl(TIOCSWINSZ)` 带像素尺寸;尺寸未变时不调 —— 兼容全屏 TUI 并减少系统调用。
5. 如果做 split:flex 比例 + 最小 pane 尺寸 + 拖拽像素→比例换算,窗口 resize 零代码自适应。
6. 给 resize 加数据驱动回归测试:输入字节流 + size.json + 期望网格全等断言(alacritty ref test 模式),比手写单测便宜且覆盖 vim 类真实场景。

---

## 搜索范围与访问情况说明

- 搜索方式:无 shell/grep 环境,用 `read` 按偏移分片扫描本地 clone + GitHub raw/API 辅助定位;重点读了 `app/src/terminal/*`(mod.rs、view.rs、grid/resize.rs、terminal_size_element.rs、local_tty/event_loop.rs、local_tty/unix.rs、writeable_pty/*、size_update_tests.rs、ref_tests)、`app/src/pane_group/*`(mod.rs、tree.rs、tree_tests.rs)、`app/src/lib.rs`、`crates/warpui/src/windowing/mod.rs` 等约 25 个文件。
- warp 仓库为公开仓库,本地 clone 无私有/加密部分,**全部关键代码均可访问**。
- 精确度声明:`resize_internal` / `after_terminal_view_layout` / `SizeUpdateBuilder` / `ALT_SCREEN_APPS_WITH_RESIZE_PROBLEMS` 位于 29,159 行的 `view.rs` 中,已核实其内容与调用关系,但未逐一提取精确行号(给出已核实锚点 L2530-2532/L3645/L3715);`pane_group/mod.rs` 的 `resize_pane/maybe_resize_pane/start_resizing/end_resizing` 同理(给出常量精确行号 L221-223/L240-247/L273)。如需精确行号,在仓库内 `rg -n "resize_internal|after_terminal_view_layout" app/src/terminal/view.rs` 一行即可。

## Gaps

1. `crates/warp_terminal` 内网格 primitive(flat_storage/row)的 resize 相关单元测试未逐一核实。
2. warpui winit 层对 `WindowEvent::Resized` 的具体处理(行号/是否节流)未深入;链路中该环节依据 `crates/warpui/src/windowing/*` 与 `app/src/lib.rs` 的 `on_window_resized` 回调(dispatch `workspace:save_app`)推断为"重建布局约束",建议后续抽查 `crates/warpui_core/src/windowing` 确认。
3. 集成测试中"真实改窗口尺寸→断言终端/PTY 尺寸"的现成用例未逐一定位到具体文件行。