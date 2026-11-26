# Stocker → OpenIDEA 1:1 复刻：全局技术方案与实施计划

> 目标：在 OpenIDEA 中复刻 JetBrains Stocker 插件（v1.21.2）的全部功能与交互体验，
> 保持其"简洁、克制、信息密度高"的设计语言，不使用现有第三方股票插件。
>
> 文档版本：v1.0 · 2026-11-26
> 源码基线：`/Users/wangboshi/github/intellij-investor-dashboard` @ v1.21.2
> 目标平台：OpenIDEA（Rust core + WASM plugin API 1.17，仓库 `/Users/wangboshi/github/openidea`）

---

## 第一部分：原插件完整功能梳理

### 1.1 模块总览

Stocker 主源码 48 个文件、约 316 KB、约 4800 行（含测试），可分为 10 个功能模块：

| # | 模块 | 核心文件 | 职责 |
|---|------|----------|------|
| 1 | 应用生命周期与调度 | `StockerApp.kt` | 全局单例、定时器、多窗口引用计数、行情聚合发布 |
| 2 | 设置与持久化 | `StockerSetting.kt` / `StockerSettingState.kt` | 13 类配置项、自选股、成本/持仓、名称映射 |
| 3 | 行情数据源 | `StockerQuoteHttpUtil.kt` / `StockerHttpClientPool.kt` / `StockerQuoteParser.kt` | 新浪/腾讯双源、URL 拼接、GBK 解码、字段解析、代码校验 |
| 4 | 搜索联想 | `StockerSuggestHttpUtil.kt` / `StockerSuggestionDialog.kt` | 股票/加密货币双模式搜索、300ms 防抖、市场过滤 |
| 5 | 表格视图 | `StockerTableView.java`（1144 行）+ 4 个 Renderer + Model | 13 列、三态排序、涨跌配色、右键删除、指数栏 |
| 6 | 工具窗口 | `StockerToolWindow.kt` / `StockerSimpleToolWindow.kt` | ALL + CN/HK/US/Crypto 五个标签页、工具栏、消息订阅 |
| 7 | 动作系统 | 6 个 `*Action.kt` | 添加/刷新/停止/管理/设置/清空 |
| 8 | 对话框 | `StockerManagementDialog.kt`（370 行） | 分类标签、自选批量增删排序、自定义名称/成本/持仓编辑 |
| 9 | 设置界面 | `StockerSettingWindow.kt`（369 行） | 3 组设置、13 列可见性勾选、实时生效 |
| 10 | 通知与国际化 | `StockerNotification.kt` / 2 个 Bundle | 欢迎/版本通知、中英双语、语言覆盖设置 |

### 1.2 数据模型

```kotlin
StockerQuote(               // 行情快照
    code, name,
    current, opening, close, low, high,
    change,                 // current - close（加密币为 current - opening）
    percentage,             // 涨跌百分比，保留 2 位
    updateAt                // "yyyy-MM-dd HH:mm:ss"
)
StockerSuggestion(code, name, market)   // 搜索结果
```

市场类型：`AShare` / `HKStocks` / `USStocks` / `Crypto`。
指数代码：沪指 `SH000001`、深成 `SZ399001`、创业板 `SZ399006`；恒生 `HSI`、恒生科技 `HSTECH`；道指 `DJI`、纳指 `IXIC`、标普 `INX`；比特币 `BTCBTCUSD`。

### 1.3 配置项完整清单（13 项）

| 配置项 | 类型 | 默认值 | 说明 |
|--------|------|--------|------|
| `version` | String | "" | 用于首次欢迎/版本更新通知 |
| `refreshInterval` | Long | 5 | 刷新周期（秒） |
| `quoteProvider` | enum | SINA | 股票行情源（SINA / TENCENT） |
| `cryptoQuoteProvider` | enum | SINA | 加密币行情源（设置中仅提供 SINA） |
| `quoteColorPattern` | enum | RED_UP_GREEN_DOWN | 三种：红涨绿跌 / 绿涨红跌 / 无色 |
| `displayNameWithPinyin` | Boolean | false | 中文名转拼音（如"平安银行"→PingAnYinHang） |
| `languageOverride` | String | "" | "" 跟随系统 / "en" / "zh_CN" |
| `visibleTableColumns` | List | NAME, CURRENT, CHANGE_PERCENT | 13 列中可见的子集，至少一个 |
| `aShareList` | List | [] | A 股自选代码 |
| `hkStocksList` | List | [] | 港股自选代码 |
| `usStocksList` | List | [] | 美股自选代码 |
| `cryptoList` | List | [] | 加密货币自选代码 |
| `customStockNames` / `stockCostPrices` / `stockHoldings` | Map | {} | 自定义名 / 成本价（3 位小数）/ 持仓股数 |

### 1.4 表格列定义（13 列）

`SYMBOL`、`NAME`、`CURRENT`、`OPENING`、`CLOSE`、`LOW`、`HIGH`、`CHANGE`、`CHANGE_PERCENT`、`COST_PRICE`、`HOLDINGS`、`NET_PROFIT`、`DAILY_PROFIT`。

派生列计算规则：
- `NET_PROFIT = (current - costPrice) × holdings`，保留 3 位小数
- `DAILY_PROFIT = change × holdings`，保留 3 位小数
- 缺失值统一显示 `-`（不是空白、不是 0）

### 1.5 渲染与交互细节（"简洁优雅"的关键）

1. **价格格式**：`0.00####`（至少两位小数，加密货币低价最多 6 位）；指数用 `#,##0.00` 带千分位。
2. **符号约定**：涨跌额、涨跌幅、净收益、当日盈亏均为正时前置 `+`。
3. **配色规则**：红涨绿跌（默认）/ 绿涨红跌 / 无色；跟随涨跌幅而非绝对价格。
   - 成本列反转配色：成本 < 现价（盈利）显示涨色，成本 > 现价显示跌色。
   - 持仓列不配色（股数不是价格变动）。
   - 选中行不应用涨跌色，保持 IDE 选中态。
4. **表格样式**：行高 26px、仅横线无竖线、表头高 30px、列宽 SYMBOL=80 / NAME=100 / 其他=66、无边框滚动Pane。
5. **三态排序**：点击表头 无→升→降→无；初始数据备份仅在排序时保留；数字列按数值、代码/名称列按忽略大小写字符串；新增/删除行时清除排序态。
6. **指数栏**：表格底部一行，含ComboBox选指数 + 点位 + 涨跌额 + 涨跌幅，字体加粗+1px，顶部 1px 分隔线。
7. **空状态**：无自选时提示"No favorites yet. Click + to add symbols."；加载中提示"Loading quotes…"。
8. **右键菜单**：单行删除，删除时精确记录目标代码/名称，避免弹窗期间选中态丢失造成误删。
9. **局部刷新**：逐单元格比对后再 `setValueAt`，未变化单元格不重绘，避免刷新时闪烁和选中态丢失。
10. **工具栏**：左侧 添加/刷新/停止/管理，右侧 设置；五个标签页（ALL/CN/HK/US/Crypto）共享同一消息总线。

### 1.6 数据源协议细节

**新浪行情** `https://hq.sinajs.cn/list=`：
- 必须带 `Referer: https://finance.sina.com.cn`
- 返回 GBK 编码，`var hq_str_sh600000="..."；`
- A股：字段 2/3/4/5/6 = 开/昨收/现/高/低，时间取 31+32
- 港股：代码去 `hk` 前缀，2/3/4/5/6/7 = 名称/开/昨收/高/低/现，涨跌幅取 9
- 美股：代码去 `gb_` 前缀，现价=2，开=6，高=7，低=8，昨收=27
- 加密币：代码去 `btc` 前缀（如 BTCBTCUSD），现价=9，开=6，涨跌幅按现价/开盘价计算

**腾讯行情** `https://qt.gtimg.cn/q=`：
- A股 `v_sh600000="..."`；港股/美股前缀 `hk`/`us`
- 统一字段：2=名称，4=现价，5=昨收，6=开盘，33=涨跌幅，34=高，35=低，31=时间
- 时间格式按市场不同：A股 `yyyyMMddHHmmss`、港股 `yyyy/MM/dd HH:mm:ss`、美股原样

**联想搜索**：
- 新浪 `https://suggest3.sinajs.cn/suggest/key=` → `var suggestvalue="..."`；`;` 分段、`,` 分列；类型码 11=A股,22=基金,31=港股,41=美股,71=加密币,81=指数
- 腾讯 `https://smartbox.gtimg.cn/s3/?v=2&t=all&c=1&q=` → `v_hint="..."`，`^` 分段、`~` 分列
- 加密币过滤：仅接受 `BTC{COIN}USD` 格式（9-15 字符），拒绝 USDT/CNY 对

### 1.7 关键健壮性设计（必须保留）

- 行情批量解析**逐行独立**，单行失败只丢弃该行，不影响同批其他代码。
- 未知/退市代码返回空占位记录时不使整页空白。
- 管理对话框：行情未加载完成前按 OK 不覆盖自选列表；行情失败的代码保留为占位行而非静默删除。
- 添加股票前用 `validateCode` 实际请求一次行情验证，失败弹出"不支持"提示。
- 全局单一定时器 + 引用计数：多个项目/窗口打开只拉取一次行情，最后一个窗口关闭时停止刷新。
- 设置变更时按变更类型分别刷新（列可见性 / 配色 / 数据源），避免整表重绘。

---

## 第二部分：目标平台能力核对（已逐项验证）

| 能力 | OpenIDEA 实现 | 来源 / 约束 |
|------|---------------|-------------|
| 插件运行时 | Rust → `wasm32-wasip2` 沙箱组件 | `register_plugin!` 宏 |
| 工具窗口 | `[[contributes.views]]` + `web = "web/index.html"` + `header = "none"` | 完整 HTML/CSS/JS 页面；`data-theme` + `--openidea-*` 自动跟随主题 |
| 双向通信 | `openidea.postMessage` / `openidea.onMessage` | Rust 侧为 `ViewEvent::Message` / `host::post_view_message` |
| 网络请求 | `host::fetch(&HttpRequest)` | 需 `net:<host>` 或 `net.http:<host>` capability；响应 body ≤ 8 MiB |
| 定时刷新 | `activation = ["on-schedule:<secs>"]` | **最小 60 秒**；收到 `Event::Scheduled` |
| 持久化 | `host::storage::{get,set,delete,keys}` | ≤ 64 MiB，key-value 字符串 |
| 设置页 | `[contributes.settings] schema = "..."` | JSON Schema 自动生成；`host::setting*` 读取；支持多页面 / 分组 / 表格 |
| 通知 | `host::notify(level,title,body)` | 无需 capability |
| 状态栏 | `host::set_status_item` | 无需 capability |
| 交互控件 | `host::pick`、`host::input`、`host::confirm` | 仅命令/事件上下文中可用 |
| 本地化 | `locales/<tag>.json` | manifest 与 view 字符串按 IDE 语言翻译 |
| 文件对话框 | `host::pick_file` / `pick_directory` | 仅命令/事件上下文 |

**两条硬约束**：
1. 插件实例空闲 5 分钟被回收（`deactivate`），内存状态不可靠 → 所有配置/自选必须写 `storage`。
2. 插件 API 是 preview；`on-schedule` 无法做到 5 秒刷新（见风险 R1）。

---

## 第三部分：全局技术方案

### 3.1 总体架构（分层映射）

```
┌─────────────────────────────────────────────────────────┐
│ 表现层：WebView 页面（web/index.html + app.js + style.css）│
│  · 5 标签表格 / 指数栏 / 工具栏 / 搜索对话框 / 管理对话框   │
│  · 涨跌配色、三态排序、右键菜单、空状态全部在页面内实现       │
│  · 通过 postMessage 与 Rust 双向通信；消费 --openidea-* 主题 │
├─────────────────────────────────────────────────────────┤
│ 状态层 StockerState                                      │
│  settings + watchlists + quotes + indices + viewState   │
│  ↔ host::storage（JSON 序列化，单一 key）                  │
├─────────────────────────────────────────────────────────┤
│ 业务层（纯 Rust，可单测，无 IDE 依赖）                       │
│  quote::parser / suggest::parser / pinyin /              │
│  finance（派生列计算）/ sort / formatter                  │
├─────────────────────────────────────────────────────────┤
│ 数据层 fetcher.rs                                        │
│  host::fetch + GBK 解码 + Referer + 批量 URL 拼接          │
├─────────────────────────────────────────────────────────┤
│ OpenIDEA Host API: fetch / storage / notify / views /     │
│                    settings / schedule / locale           │
└─────────────────────────────────────────────────────────┘
```

关键决策：**全界面 WebView**（见 D1）。Rust 侧只保留一个最小的 `Views` 实现来处理 WebView 的消息（`ViewEvent::Message`），不调用 `render` 生成原生 body；所有 UI 由 `web/` 目录中的页面绘制。这样避免原生组件在逐格配色、精确行高/边框、复合对话框上的表达力不足，直接达成 1:1 视觉目标。

### 3.2 文件规划

```
openidea-stocker/
├── plugin.toml                      # 清单：capabilities / views / commands / settings / activation
├── Cargo.toml                       # openidea-plugin SDK + serde/serde_json + encoding_rs + regex-lite
├── src/
│   ├── lib.rs                       # Plugin + Views + register_plugin!；消息路由
│   ├── model.rs                     # StockerQuote / Suggestion / MarketType / Settings（serde）
│   ├── settings.rs                  # 读写 host::setting 与 storage，默认值与迁移
│   ├── storage.rs                   # state JSON 序列化 / 反序列化
│   ├── provider.rs                  # Sina / Tencent 的 host、prefix、suggestHost 枚举
│   ├── fetcher.rs                   # host::fetch 封装、GBK 解码、validate_code
│   ├── parser.rs                    # 4 市场 × 2 数据源行情解析（移植自 StockerQuoteParser）
│   ├── suggest.rs                   # 新浪/腾讯联想解析 + 市场过滤 + 加密币校验
│   ├── pinyin.rs                    # 汉字转拼音（含常用股票用字表，结果首字母大写）
│   ├── finance.rs                   # 净收益/当日盈亏/数值格式化
│   └── protocol.rs                  # WebView ↔ Rust 的 JSON 消息定义与 serde 编解码
├── web/
│   ├── index.html                   # 页面骨架
│   ├── style.css                    # 全部视觉（消费 --openidea-* 主题变量）
│   ├── app.js                       # 表格/标签/排序/菜单/对话框/定时器/消息收发
│   └── icons/logo.svg               # 插件图标
├── settings.schema.json             # 设置页 Schema（3 组）
├── locales/en.json / zh-CN.json     # 文案
├── tests/                           # 解析器、排序、格式化、拼音单测（用原插件测试向量）
└── README.md
```

### 3.3 API 映射表

| Stocker（JVM） | OpenIDEA（Rust/WASM） | 备注 |
|----------------|----------------------|------|
| `@Service(Level.APP)` + `StockerApp` | `struct StockerPlugin: Plugin + Default` | 单实例由 host 管理 |
| `ScheduledExecutorService.scheduleAtFixedRate` | `activation = ["on-schedule:60"]` + `Event::Scheduled` | 周期从 5s 改为 60s |
| `Executors` 搜索防抖 | `host::fetch` 由按钮/输入事件直接触发，前端 JS 做 300ms 防抖 | 避免在 Rust 内起线程 |
| `Application messageBus` 多主题发布 | 内存 `Vec<StockerQuote>` + `host::refresh_view("stocker")` | 无多窗口消息总线概念 |
| `PersistentStateComponent` | `host::storage` + JSON | 单 key `state.v1` |
| `applicationConfigurable` + DSL panel | `[contributes.settings]` + `settings.schema.json` | 3 个 group 对应 schema properties |
| `ToolWindowFactory` + 5 个 Content | 5 个 `[[contributes.views]]`（`group = "stocker"`） | 共享一个工具窗口的 5 个标签 |
| `JBTable` + 9 个自定义 Renderer | WebView 内 `<table>` + CSS 完全自绘 | 逐格涨跌配色、行高 26px、仅横线、列宽全部还原 |
| `DialogWrapper` 管理/搜索 | WebView 内模态 `<dialog>` + JS | 表单校验在 JS 与 Rust 双端做 |
| `NotificationGroup` + HTML 通知 | `host::notify` + `host::open_url` | 欢迎/版本说明简化为一行文本 + 链接 |
| `StockerBundle` + `DynamicBundle` | `locales/en.json` / `zh-CN.json` | `host::locale()` 决定读哪个 |
| `BrowserUtil.browse` | `host::open_url` | GitHub / Donate 链接 |
| `Messages.showErrorDialog` | `host::notify(Error, ...)` 或 HTML 内联提示 | 添加失败提示 |

### 3.4 关键设计决策

**D1 — 表格渲染方案：选定 WebView（2026-11-26 决策）**

用户明确目标为**与原插件效果保持一致**，因此直接采用 WebView 方案，不走"先原生后切换"的弯路：

```
[[contributes.views]]
id = "stocker"
title = "Stocker"
icon = "icons/logo.svg"
location = "left"
secondary = true
web = "web/index.html"
header = "none"      # 隐藏标准工具窗口头部，自己绘制原插件的紧凑工具栏
```

决策依据（已验证 SDK/文档）：
1. **完整渲染控制**：WebView 是打包进插件的独立页面（目录内任意 HTML/CSS/JS/图标），可逐像素复刻原插件的表格布局、行高 26px、仅横线无竖线、列宽、表头样式、指数栏、空状态。原生 `sortable_table` 的 rows 只是 `list<list<string>>`，无法逐单元格配色。
2. **主题适配由平台解决**：WebView 根节点自动带 `data-theme="light|dark"`，并提供 13 个具体色值的 CSS 变量（`--openidea-bg`/`fg`/`border`/`accent`/`danger`/`success`/`font-ui` 等），`openidea.onThemeChange(fn)` 可监听切换。"自己处理主题适配"的顾虑不成立——直接消费这些变量即可。
3. **双向通信**：页面 `openidea.postMessage(JSON)` → Rust `ViewEvent::Message`；Rust `host::post_view_message(view, JSON)` → 页面 `openidea.onMessage(fn)`。Rust 侧只做数据（fetch / storage / 解析 / 计算），所有渲染逻辑在 TS/JS。
4. **高频刷新**：页面内 `setInterval(5000)` 发 `{op:"refresh"}`，Rust 调 `host::fetch` 后用 `post_view_message` 回传行情 JSON——这同时解决了 D2 的 5 秒刷新问题，无需依赖 60 秒的 `on-schedule`（后者仅作后台保活）。
5. **所有交互在页面内完成**：搜索联想、管理对话框、编辑表单、排序、右键菜单全部用 DOM 实现，无需 `DialogWrapper` 等价物。

代价：需要打包一个前端页面（原生 JS 即可，不引入 React 等框架以保持轻量）。这与"简洁优雅"的目标一致。

> 原 D1 中"原生 sortable_table 优先"的方案作废，避免 Phase 1→4 的重复劳动。

**D2 — 刷新频率（R1 的应对）**
`on-schedule` 最小 60 秒。若需保持 5 秒体验：
- 在 `Body::Html` 的表格页内用 JS `setInterval(5000)` 发 `postMessage("refresh")`，Rust 收到后调用 `host::fetch` 并 `host::post_view_message` 回传数据，JS 局部更新 DOM。
- 此路径下 HTML 是沙箱、无网络，所有请求仍经 Rust host，权限模型不受影响。
- 结论：**60 秒定时用于后台静默刷新（后台保活），5 秒高频由打开的可视视图中的 JS 定时器驱动**。这是最接近原插件行为的折中。

**D3 — 状态模型**
```rust
struct PersistentState {
    version: String,
    refresh_interval: u64,
    quote_provider: Provider,
    crypto_provider: Provider,
    color_pattern: ColorPattern,
    pinyin: bool,
    language_override: String,
    visible_columns: Vec<String>,
    watchlists: HashMap<MarketType, Vec<String>>,
    custom_names: HashMap<String, String>,
    cost_prices: HashMap<String, f64>,
    holdings: HashMap<String, i32>,
}
```
单 key `stocker.state.v1` 存于 `host::storage`；`activate()` 时读取，`deactivate()` 与每次变更后写回。设置页的 `host::setting*` 用于读取 schema 默认值（用户未改时回落到 schema default）。

**D4 — GBK 解码**
`host::fetch` 返回 `body: Vec<u8>`。新浪行情为 GBK 编码，使用 `encoding_rs::GBK.decode(&body)` 转 String；腾讯返回 UTF-8。一首请求可尝试按 provider 选择解码器。

**D5 — 5 个标签页的数据同步**
无消息总线。Rust 侧维护每个市场的 `Vec<StockerQuote>` 与合并的 `all` 列表；刷新一次拉取 4 个市场 + 4 组指数，写入内存后对 5 个 view 各调用一次 `host::refresh_view`。`Views::render` 根据 `view` 参数（`all`/`cn`/`hk`/`us`/`crypto`）返回对应数据切片。

### 3.5 风险与对策

| ID | 风险 | 影响 | 对策 |
|----|------|------|------|
| R1 | `on-schedule` 最小 60s，原插件 5s | 高频体验缺失 | D2：JS 定时器 + postMessage 驱动；后台用 60s 保活 |
| R2 | 沙箱无本地字体、HTML iframe 无网络 | 复杂 UI 需内联所有资源 | HTML/CSS/JS 全部内联在 Rust 常量中，图标用 `data:` URI 或 IDE 图标名 |
| R3 | 新浪 GBK + Referer | 乱码或被拒 | `encoding_rs` + 显式 `Referer` header（capability 允许自定义 header） |
| R4 | 权限模型：安装时需批准 `net:*` | 首次安装多一步 | 声明 required `net:hq.sinajs.cn` / `net:qt.gtimg.cn` / `net:suggest3.sinajs.cn` / `net:smartbox.gtimg.cn` / `net:storage` |
| R5 | 5 分钟空闲回收 | 自选/设置丢失 | 每次变更即时写 `storage`，`activate()` 恢复 |
| R6 | 拼音库缺失 | 拼音模式失效 | 内置常用 A 股/港股名称拼音映射表（几百条），覆盖场景；无需完整 pinyin4j |
| R7 | ~~原生表格不支持逐格配色~~ | 已消除 | D1 已选定 WebView 方案，逐格配色由 CSS 完整还原 |
| R8 | 上游行情接口变更 | 全面失效 | 解析器逐行容错；Sina/Tencent 双源可切换；保留原插件的 validate 机制 |
| R9 | 插件 API preview 变动 | 编译/运行失败 | SDK 依赖固定 tag（与 IDE 版本对齐）；持续关注 releases |

---

## 第四部分：详细实施计划

### 阶段 0：环境准备（已完成 / 进行中）
- [x] fork `WhiteVermouth/intellij-investor-dashboard` → `anightmonarch/...`，配置 upstream
- [x] 克隆 OpenIDEA 源码作为 SDK 参考
- [x] 安装 Rust stable + `wasm32-wasip2` target
- [x] 安装 `openidea-plugin` CLI（v0.1.53 / API 1.17）
- [ ] 在 fork 仓库新建分支 `openidea-port`

### 阶段 1：骨架与静态界面（可运行里程碑 M1）
1. `openidea-plugin new stocker --id <publisher>.stocker`
2. 编写 `plugin.toml`：
   - `engines.openidea = "^1.3"`（因使用 `header = "none"` 与多页面设置）
   - `capabilities.required = ["storage", "net:hq.sinajs.cn", "net:qt.gtimg.cn", "net:suggest3.sinajs.cn", "net:smartbox.gtimg.cn"]`
   - `activation = ["on-view:stocker", "on-schedule:60"]`
   - 单一 `[[contributes.views]]`：`id = "stocker"`、`location = "left"`、`secondary = true`、`web = "web/index.html"`、`header = "none"`；5 个标签由页面内 JS 渲染（对应原插件的 ALL/CN/HK/US/Crypto）
   - 6 个 `[[contributes.commands]]` 对齐原插件动作
   - `[contributes.settings] schema = "settings.schema.json"`
3. 实现 `model.rs` / `settings.rs` / `storage.rs`（含默认值、迁移逻辑）与 `protocol.rs` 消息结构。
4. 编写 `web/` 静态界面：5 标签、13 列表头（默认 3 列可见）、工具栏、指数栏占位、空状态文案；CSS 消费 `--openidea-*` 主题变量。
5. 实现最小 `Views` 实现（`render` 返回 `View::message("")`，`handle` 接收 `ViewEvent::Message` 并路由）。
6. `openidea-plugin build` + `link` 到 OpenIDEA，验证工具窗口出现、页面加载、5 个标签可切换。
7. **验收 M1**：打开 OpenIDEA 能看到与原插件布局一致的 5 标签空表格和指数栏。

### 阶段 2：数据链路打通（M2）
1. `provider.rs` + `fetcher.rs`：host::fetch + GBK 解码 + Referer + 批量 URL 拼接。
2. `parser.rs`：移植 4 市场 × 2 数据源解析，**逐行容错**。
3. 用原插件 `StockerQuoteParserTest.kt` 的测试向量写 Rust 单测，逐条对齐。
4. `finance.rs`：净收益/当日盈亏/3 位小数/`-` 占位。
5. `protocol.rs` 定义 `{op:"refresh"}` / `{op:"quotes",data:[...]}` 消息；页面 `setInterval(5000)` 触发拉取，Rust 回传 JSON，JS 更新表格；`on-schedule:60` 作后台保活。
6. **验收 M2**：添加几个自选代码后，表格显示实时行情；手动刷新、停止生效；重启 IDE 后自选仍在。

### 阶段 3：完整业务逻辑（M3）
1. `suggest.rs`：新浪/腾讯联想解析、市场过滤、加密币 BTC{COIN}USD 校验、`validateCode` 等价实现。
2. `web/app.js`：搜索对话框（双模式切换、300ms 防抖、加载态、添加/删除按钮状态同步、Esc 关闭）。
3. `web/app.js`：管理对话框（4 市场标签、列表排序、编辑自定义名/成本/持仓并校验、OK 时空加载保护）。
4. `pinyin.rs`：常用股票名称拼音映射 + 首字母大写（结果随行情 JSON 传给页面）。
5. `web/app.js`：三态排序 + 备份/恢复 + 新行清除排序态。
6. **验收 M3**：可搜索添加任意 A/HK/US/币，可批量管理与编辑元数据，拼音开关生效，排序正确。

### 阶段 4：视觉与细节对齐（M4）
1. WebView 表格逐单元格配色：红涨绿跌 / 绿涨红跌 / 无色三种模式，成本列反转配色，选中行不染色。
2. 对齐所有格式细节：`0.00####`、`#,##0.00`、正负号、价格小数位、数字右对齐。
3. 指数栏：下拉 + 点位/涨跌额/涨跌幅，加粗，顶部分隔线。
4. 空状态、加载态、错误提示文案中与英文逐条对齐（复用 `StockerBundle.properties` 的文案）。
5. 设置页 3 组 + 13 列勾选（至少一个） + 3 种配色 + 数据源 + 语言覆盖，Apply 实时生效。
6. 欢迎/版本通知、GitHub/Donate 链接。
7. **验收 M4**：与原插件截图逐项对照，视觉与交互达 1:1。

### 阶段 5：健壮性与发布（M5）
1. 实现 R5 状态恢复、R8 接口异常降级。
2. 多窗口/多项目行为验证。
3. `openidea-plugin check` 通过；package 签名。
4. 编写 README（安装、权限说明、数据源说明）。
5. **验收 M5**：`openidea-plugin build` 成功，安装到 OpenIDEA 后全流程通过。

### 里程碑时间线（以 AI 辅助开发的实际工作量估算）

| 阶段 | 主要产出 | 相对工作量 |
|------|----------|-----------|
| M1 骨架 | 可运行的 5 标签空壳 | 小 |
| M2 数据 | 行情显示 + 持久化 | 中 |
| M3 交互 | 搜索/管理/排序/拼音 | 大 |
| M4 视觉 | 表格渲染与设置页对齐 | 大 |
| M5 发布 | 打包与健壮性 | 小 |

---

## 附录 A：从原插件移植的测试向量

`src/test/kotlin/...` 下 5 个测试文件覆盖：行情解析（258 行，多市场多数据源）、列可见性迁移、拼音、表格模型工具。这些测试输入/期望输出可直接转为 Rust `#[test]`，是保证 1:1 语义一致的关键资产。

## 附录 B：不做的事（明确排除）

- 不引入现有股票插件的任何代码或 UI 设计。
- 不改变数据源（仍为新浪/腾讯公开接口）。
- 不新增原插件没有的功能（如技术指标、K 线图、预警）。
- 不使用网络图片资源（图标全部内联或使用 IDE 图标名）。
