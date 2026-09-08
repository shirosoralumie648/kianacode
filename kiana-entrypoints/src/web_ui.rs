//! `kiana web` 使用的静态 loopback 页面资源。
//!
//! 页面只负责渲染工作区侧栏、对话、输入框和详情面板，并通过同一进程提供的本地 API
//! 请求状态或提交回合。HTML/JavaScript 不持有授权事实，也不应自行调用模型、工具或
//! 外部网络；所有有副作用的操作必须回到 Web handler 再进入 `DaemonHost`。
//!
//! 页面中的 token 是启动时注入的短期进程内凭据，配合服务端 loopback、Host 和 Origin
//! 校验使用。静态资源本身可以被重新生成，不能作为会话、receipt 或 EventLog 的持久来源。

/// 从编译时嵌入的 `web_page.html` 返回完整页面文本。
///
/// `include_str!` 使二进制启动后不依赖当前工作目录寻找前端文件；页面内容仍是展示层，
/// 不会因此获得额外的工具或网络能力。
pub const PAGE: &str = include_str!("web_page.html");
