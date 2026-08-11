# Ctrl+R 可搜索历史 + 通用覆盖层系统设计

**日期**: 2026-08-11  
**作者**: AI Assistant  
**状态**: 设计阶段  

## 概述

实现 Ctrl+R 可搜索历史功能，并建立通用的覆盖层（Overlay）系统，为未来的弹出式 UI 组件（会话选择器、审批弹窗、插件列表等）打下架构基础。

### 目标

1. **用户价值**：快速查找和重用历史输入，提升工作效率
2. **架构价值**：建立可扩展的覆盖层系统，支持未来的模态 UI 组件
3. **质量目标**：清晰的交互体验，良好的性能（<100ms 搜索响应）

### 非目标

- 语义搜索（作为未来增强，单独设计）
- 搜索历史持久化（Phase 2）
- 高级搜索算法（初期使用简单模糊匹配）

---

## 功能需求

### FR-1: 搜索历史触发

**需求**：用户按 Ctrl+R 弹出搜索覆盖层

**验收标准**：
- 在主界面任意时刻按 Ctrl+R，弹出搜索覆盖层
- 覆盖层居中显示，占据 80% 宽度 x 60% 高度
- 背景主界面仍然可见但被覆盖层遮挡

### FR-2: 双模式搜索

**需求**：支持两种搜索模式切换

**模式 1：用户输入模式（默认）**
- 只搜索 role == "user" 的消息
- 标题显示 "Search History (User Inputs)"

**模式 2：完整对话模式**
- 搜索所有消息（user + assistant + system）
- 标题显示 "Search History (All Messages)"

**验收标准**：
- 按 Tab 键在两种模式间切换
- 切换后立即使用当前查询重新搜索
- 标题栏清晰显示当前模式

### FR-3: 实时搜索

**需求**：输入查询关键词，实时过滤和显示结果

**验收标准**：
- 支持任意字符输入（字母、数字、符号、空格）
- 每次输入立即触发搜索（<100ms 响应）
- Backspace 删除字符并重新搜索
- 空查询显示最近的消息（按时间倒序）

### FR-4: 模糊匹配搜索

**需求**：使用模糊匹配算法查找包含关键词的消息

**算法（Phase 1）**：
- 子串匹配，大小写不敏感
- 计算匹配分数：
  - 匹配位置越靠前，分数越高
  - 连续匹配比分散匹配分数高
- 按分数排序，取 Top 50

**验收标准**：
- 查询 "hello" 能匹配 "Hello world"
- 查询 "git commit" 能匹配 "git commit -m 'fix'"
- 结果按相关性排序

### FR-5: 结果导航与选择

**需求**：上下键导航结果，Enter 选择

**验收标准**：
- Up/Down 键移动选中项（高亮显示）
- 选中项循环：到达底部后 Down 回到顶部
- Enter 键将选中的消息内容填充到输入框
- 填充后自动关闭覆盖层，光标在输入框末尾

### FR-6: 关闭覆盖层

**需求**：Esc 或 Ctrl+C 关闭覆盖层

**验收标准**：
- Esc 关闭覆盖层，不改变输入框内容
- Ctrl+C 关闭覆盖层，不改变输入框内容
- 关闭后焦点回到主输入框

---

## 架构设计

### 核心组件

#### 1. Overlay Trait（通用覆盖层接口）

```rust
/// 通用覆盖层 trait
/// 任何弹出式 UI 组件都实现此 trait
pub trait Overlay {
    /// 渲染覆盖层内容
    fn render(&self, frame: &mut Frame, area: Rect);
    
    /// 处理键盘事件
    /// 返回 OverlayAction 指示覆盖层的后续行为
    fn handle_key(&mut self, key: KeyEvent) -> OverlayAction;
    
    /// 覆盖层标题（显示在顶部边框）
    fn title(&self) -> &str;
}

/// 覆盖层处理键盘事件后的行为
pub enum OverlayAction {
    /// 继续显示覆盖层
    Continue,
    
    /// 关闭覆盖层（无返回值）
    Close,
    
    /// 关闭覆盖层并返回结果字符串
    CloseWithResult(String),
}
```

**设计理由**：
- **Trait 抽象**：统一所有覆盖层的接口，简化主循环逻辑
- **OverlayAction**：明确表达覆盖层的生命周期状态
- **CloseWithResult**：支持覆盖层向主界面传递数据（如搜索选中的内容）

#### 2. SearchOverlay（搜索覆盖层实现）

```rust
/// 搜索覆盖层
pub struct SearchOverlay {
    /// 搜索查询字符串
    query: String,
    
    /// 搜索结果列表
    results: Vec<SearchResult>,
    
    /// 当前选中的结果索引
    selected: usize,
    
    /// 搜索模式
    search_mode: SearchMode,
    
    /// 搜索源数据（当前会话的消息）
    source_messages: Vec<Message>,
}

/// 搜索模式
#[derive(Clone, Copy, PartialEq)]
pub enum SearchMode {
    /// 仅搜索用户输入
    UserInputs,
    
    /// 搜索所有消息
    AllMessages,
}

/// 搜索结果
pub struct SearchResult {
    /// 消息内容
    content: String,
    
    /// 消息角色（user/assistant/system）
    role: String,
    
    /// 消息时间戳
    timestamp: DateTime<Utc>,
    
    /// 在原消息列表中的索引（用于跳转上下文，预留）
    index: usize,
    
    /// 匹配分数（用于排序）
    match_score: f32,
}
```

**关键方法**：
- `new(messages: Vec<Message>) -> Self`：初始化搜索覆盖层
- `update_query(&mut self, query: String)`：更新搜索查询并重新搜索
- `toggle_mode(&mut self)`：切换搜索模式
- `select_next(&mut self)` / `select_prev(&mut self)`：导航结果
- `get_selected_content(&self) -> Option<String>`：获取选中的消息内容

#### 3. App 状态扩展

```rust
struct App {
    // ... 现有字段 ...
    
    /// 当前激活的覆盖层（如果有）
    active_overlay: Option<Box<dyn Overlay>>,
    
    /// 覆盖层关闭后的返回值
    overlay_result: Option<String>,
}
```

**状态转换**：
```
active_overlay == None（正常模式）
    ↓ Ctrl+R
active_overlay == Some(SearchOverlay)（搜索模式）
    ↓ Enter
active_overlay == None
overlay_result == Some("用户选中的内容")
    ↓ 填充到 input_buffer
overlay_result == None
```

---

## 数据流设计

### 搜索数据流

```
用户按 Ctrl+R
    ↓
创建 SearchOverlay::new(app.messages.clone())
    ↓ 初始化
空查询 → 显示最近 50 条消息（按时间倒序）
    ↓
用户输入查询字符
    ↓
SearchOverlay::update_query(query)
    ↓
根据 search_mode 过滤消息
    ↓
对每条消息执行模糊匹配
    ↓
计算 match_score
    ↓
按分数排序，取 Top 50
    ↓
更新 results 并重新渲染
```

### 事件路由

```
键盘事件
    ↓
有 active_overlay？
    ↓ 是
    overlay.handle_key(key) → OverlayAction
        ↓
        Continue → 继续显示覆盖层
        Close → active_overlay = None
        CloseWithResult(s) → 
            overlay_result = Some(s)
            active_overlay = None
    ↓ 否
    主界面正常处理键盘事件
```

**关键实现点**：
- 当 `active_overlay.is_some()` 时，所有键盘事件优先路由到覆盖层
- 覆盖层可以"吞掉"事件（返回 Continue）或传递给主界面（不常见）
- 主循环在每次迭代检查 `overlay_result`，如果有值则处理

---

## UI 设计

### 布局

```
┌──────────────────────────────────────────────────────┐
│ 主界面（标题栏、消息区、输入框）                      │
│                                                      │
│   ┌────────────────────────────────────────────┐   │
│   │ Search History (User Inputs) [Tab]         │   │  ← 覆盖层标题
│   ├────────────────────────────────────────────┤   │
│   │ Query: hello world_                        │   │  ← 搜索输入框
│   ├────────────────────────────────────────────┤   │
│   │ > /help                          (3 days)  │   │  ← 选中的结果（高亮）
│   │   explain the architecture       (2 days)  │   │
│   │   hello world example            (1 day)   │   │
│   │   /config                        (today)   │   │
│   │                                            │   │
│   │   [4 results]                              │   │  ← 底部状态栏
│   └────────────────────────────────────────────┘   │
│                                                      │
└──────────────────────────────────────────────────────┘
```

**样式规范**：
- 覆盖层边框：`Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan))`
- 选中项：背景色 `Color::DarkGray`，前缀 `>` 
- 未选中项：默认前景色
- 查询输入：光标显示为 `_`

### 结果显示格式

**用户输入模式**：
```
> /help                                          (3 days ago)
  explain the architecture                       (2 days ago)
```

**完整对话模式**：
```
> [user] explain the architecture                (2 days ago)
  [assistant] The architecture consists of...    (2 days ago)
  [user] /help                                   (3 days ago)
```

**时间格式**：
- 今天：`(today)`
- 昨天：`(yesterday)`
- 7 天内：`(N days ago)`
- 更早：`(YYYY-MM-DD)`

---

## 模糊搜索算法

### Phase 1: 简单子串匹配

```rust
/// 计算查询在内容中的匹配分数
/// 返回 None 表示不匹配
fn calculate_match_score(query: &str, content: &str) -> Option<f32> {
    let query_lower = query.to_lowercase();
    let content_lower = content.to_lowercase();
    
    // 查找第一次出现的位置
    let position = content_lower.find(&query_lower)?;
    
    // 分数计算：
    // - 基础分数 100.0
    // - 位置越靠前，分数越高（减去 position * 0.1）
    // - 内容越短，分数越高（除以 content.len() 再乘以 1000）
    let score = 100.0 - (position as f32 * 0.1) + (1000.0 / content.len() as f32);
    
    Some(score)
}
```

**算法特点**：
- 简单高效，零依赖
- 大小写不敏感
- 优先匹配位置靠前的结果
- 优先匹配较短的消息

**预留扩展点**：
- 未来可替换为 `nucleo` 或 `fuzzy-matcher` 库
- 可以添加缩写匹配（如 `gco` 匹配 `git commit`）
- 可以添加字符高亮信息

---

## 文件结构

```
kiana-tui/src/
├── main.rs                    (修改：集成 overlay 系统)
├── overlay/
│   ├── mod.rs                (新增：Overlay trait 定义，OverlayAction 枚举)
│   └── search.rs             (新增：SearchOverlay 实现)
├── search/
│   ├── mod.rs                (新增：搜索引擎抽象)
│   └── fuzzy.rs              (新增：模糊搜索算法)
├── acp.rs                    (无修改)
├── commands.rs               (无修改)
├── completion.rs             (无修改)
├── markdown.rs               (无修改)
├── sessions.rs               (无修改)
└── config.rs                 (无修改)
```

**模块职责**：
- `overlay/mod.rs`：定义 Overlay trait 和 OverlayAction
- `overlay/search.rs`：SearchOverlay 结构体及其 Overlay trait 实现
- `search/mod.rs`：搜索引擎接口（预留，支持未来切换搜索算法）
- `search/fuzzy.rs`：模糊搜索算法实现

---

## 性能考虑

### 搜索范围限制

**Phase 1**：
- 只搜索当前会话的消息（`app.messages`）
- 不包括历史会话（`session_manager.history`）
- 理由：简化实现，避免大量消息导致的性能问题

**Phase 2（可选）**：
- 提供配置选项，允许搜索历史会话
- 引入索引或缓存机制

### 结果数量限制

- 最多显示 50 条结果
- 如果超过 50 条，显示 `[50+ results]`

### 搜索性能目标

- **单次搜索延迟**：<100ms（对于 1000 条消息）
- **内存占用**：`source_messages` 克隆约 1-2 MB（可接受）

**优化手段（如果需要）**：
- 使用 Arc 共享消息数据，避免克隆
- 引入防抖（debounce），每 50ms 最多搜索一次

---

## 测试策略

### 单元测试

**overlay/search.rs**：
- `test_search_user_inputs_only()`：验证只搜索用户输入
- `test_search_all_messages()`：验证搜索所有消息
- `test_toggle_mode()`：验证模式切换逻辑
- `test_empty_query()`：验证空查询显示最近消息
- `test_no_results()`：验证无结果处理

**search/fuzzy.rs**：
- `test_exact_match()`：验证精确匹配
- `test_case_insensitive()`：验证大小写不敏感
- `test_substring_match()`：验证子串匹配
- `test_score_ordering()`：验证分数排序
- `test_no_match()`：验证不匹配返回 None

### 集成测试

**完整流程测试**：
1. 打开搜索覆盖层（Ctrl+R）
2. 输入查询
3. 导航结果（Up/Down）
4. 选择结果（Enter）
5. 验证输入框被填充

**边界情况**：
- 空消息列表
- 搜索结果为空
- 搜索超长消息（>1000 字符）
- 特殊字符查询（`[]{}\`）

### 手动测试场景

1. **基本流程**：Ctrl+R → 输入查询 → Enter → 验证填充
2. **模式切换**：Tab 切换 → 验证结果变化
3. **导航**：Up/Down 循环导航
4. **取消**：Esc/Ctrl+C 关闭
5. **性能**：1000 条消息，搜索响应速度

---

## 依赖项

### 新增依赖

无。使用现有依赖即可实现。

### 可选优化依赖（Phase 2）

- `fuzzy-matcher = "0.3"`：更好的模糊匹配算法
- `nucleo = "0.5"`：类似 fzf 的高性能搜索

---

## 未来扩展点

### 1. 语义搜索（单独设计）

**触发方式**：
- 输入框中输入 `@semantic query` 触发语义搜索
- 或通过配置启用语义搜索作为默认

**实现方式**：
- 扩展 ACP 协议，添加 `embedding` 和 `semantic_search` 命令
- SearchOverlay 调用 ACP 获取语义相似度
- 混合排序：关键词分数 + 语义相似度

### 2. 其他覆盖层组件

基于 Overlay trait，可以轻松添加：

- **SessionPickerOverlay**：会话选择器（替换现有的 `show_session_list`）
- **ApprovalOverlay**：审批弹窗（中期功能）
- **ConfigEditorOverlay**：配置编辑器（替换现有的 `config_mode`）
- **PluginListOverlay**：插件列表（长期功能）
- **HelpOverlay**：快捷键帮助

### 3. 覆盖层高级功能

- **覆盖层栈**：支持多层覆盖层（如在搜索中打开帮助）
- **自定义尺寸**：不同覆盖层使用不同的宽高比例
- **动画**：淡入淡出效果（可选，性能考虑）

### 4. 搜索增强

- **搜索历史持久化**：记录搜索关键词，Ctrl+R 二次按下显示历史
- **搜索结果高亮**：在结果中高亮匹配的字符
- **跳转到上下文**：选中结果后按 Ctrl+J 跳转到对话中的位置
- **跨会话搜索**：搜索所有历史会话的消息

---

## 风险与缓解

### 风险 1：性能问题（大量消息）

**场景**：用户有 10,000+ 条消息，搜索变慢

**缓解**：
- Phase 1 只搜索当前会话（通常 <1000 条）
- 限制结果数量（Top 50）
- 引入防抖（50ms）
- 未来可以添加索引或分页加载

### 风险 2：Overlay trait 的 dyn 复杂度

**场景**：`Box<dyn Overlay>` 可能导致生命周期问题

**缓解**：
- Overlay trait 设计为 `'static`，不包含引用
- 所有数据都 owned 或 cloned
- 如果确实需要引用，使用 `Arc`

### 风险 3：UI 闪烁或渲染问题

**场景**：覆盖层切换时出现闪烁

**缓解**：
- ratatui 的 `Clear` widget 正确使用
- 确保渲染顺序：主界面 → Clear → 覆盖层
- 测试不同终端模拟器的兼容性

---

## 实施计划

### 第 1 阶段：基础设施（预计 4-6 小时）

**任务**：
1. 创建 `overlay/mod.rs`：定义 Overlay trait 和 OverlayAction
2. 创建 `search/mod.rs` 和 `search/fuzzy.rs`：实现模糊搜索算法
3. 修改 `App` 结构体，添加 `active_overlay` 和 `overlay_result`
4. 实现事件路由逻辑（main.rs）

**验收**：
- 可以编译
- 单元测试通过（搜索算法）

### 第 2 阶段：SearchOverlay 实现（预计 6-8 小时）

**任务**：
1. 创建 `overlay/search.rs`
2. 实现 SearchOverlay 结构体和方法
3. 实现 Overlay trait for SearchOverlay
4. 实现渲染逻辑（搜索框 + 结果列表）
5. 实现键盘处理逻辑

**验收**：
- Ctrl+R 能弹出搜索覆盖层
- 可以输入查询并看到结果
- Up/Down 导航有效
- Enter 填充到输入框
- Esc 关闭覆盖层

### 第 3 阶段：模式切换与优化（预计 2-4 小时）

**任务**：
1. 实现 Tab 切换搜索模式
2. 优化搜索性能（如果需要）
3. 优化 UI 显示（时间格式、角色标签）
4. 添加单元测试和集成测试

**验收**：
- Tab 切换模式有效
- 搜索性能 <100ms
- 所有测试通过

### 第 4 阶段：文档与打磨（预计 1-2 小时）

**任务**：
1. 更新 README.md，添加 Ctrl+R 使用说明
2. 添加代码注释
3. 手动测试各种场景
4. 修复发现的 bug

**验收**：
- 功能完整可用
- 文档清晰
- 无明显 bug

**总计**：13-20 小时

---

## 成功标准

### 功能完整性

- ✅ Ctrl+R 打开搜索覆盖层
- ✅ 支持两种搜索模式切换（Tab）
- ✅ 实时搜索并显示结果
- ✅ 上下导航结果
- ✅ Enter 填充到输入框
- ✅ Esc/Ctrl+C 关闭覆盖层

### 性能指标

- ✅ 搜索延迟 <100ms（1000 条消息）
- ✅ UI 响应流畅，无明显卡顿
- ✅ 内存占用合理（<10 MB 额外开销）

### 代码质量

- ✅ 单元测试覆盖率 >80%
- ✅ 集成测试覆盖主要流程
- ✅ 代码结构清晰，符合 Rust 最佳实践
- ✅ 无 clippy 警告

### 用户体验

- ✅ 交互流畅自然
- ✅ 错误处理友好（空结果、无消息等）
- ✅ 快捷键符合直觉（Tab/Enter/Esc）

---

## 总结

本设计建立了一个可扩展的覆盖层系统，以 Ctrl+R 搜索历史作为第一个实现。通过 Overlay trait 抽象，未来可以轻松添加其他弹出式 UI 组件，为 Kiana TUI 的中长期功能打下坚实基础。

**核心设计决策**：
1. **Overlay trait**：统一覆盖层接口，简化主循环
2. **简单模糊搜索**：零依赖，快速实现，预留扩展点
3. **双模式搜索**：满足不同搜索需求，Tab 切换直观
4. **事件路由**：优先路由到覆盖层，清晰的控制流

**下一步**：进入实施计划（writing-plans skill）
