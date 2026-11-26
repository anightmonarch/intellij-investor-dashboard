# Stocker for OpenIDEA

A 1:1 functional port of the [Stocker](https://plugins.jetbrains.com/plugin/15443-stocker)
JetBrains plugin to OpenIDEA. Same clean, minimal investor dashboard — A-Shares,
Hong Kong stocks, US stocks and cryptocurrencies with real-time quotes.

## Architecture

- **Rust/WASM** (`src/`): state, persistence, network and quote parsing. Talks to the
  UI over the `openidea.postMessage` JSON bridge.
- **WebView** (`web/`): the entire UI — tool window toolbar, 5 market tabs (ALL/CN/HK/US/Crypto),
  the 13-column table, index bar, search and manage dialogs. Consumes OpenIDEA's
  `--openidea-*` theme variables so it follows light/dark automatically.

The quote parsers (`src/parser.rs`), search suggestion parsers (`src/suggest.rs`),
URL/prefix rules (`src/fetcher.rs`) and pinyin conversion (`src/pinyin.rs`) are
ported directly from the original Kotlin/Java plugin, preserving its robustness
behavior (a single bad symbol never discards the whole batch).

## Build & install

```sh
cargo test                       # unit tests against the SDK's mock host
openidea-plugin build            # -> dist/plugin.wasm, validated like an install
openidea-plugin link             # load this folder in the IDE (Settings > Plugins)
```

Then in OpenIDEA: **Settings › Plugins** → allow the plugin (approve the network
and storage permissions) → open the **Stocker** tool window on the left.

## Permissions

- `storage`: watchlist, cost price, holdings and preferences (local only).
- `net:hq.sinajs.cn`, `net:qt.gtimg.cn`: quote data (Sina requires a Referer header).
- `net:suggest3.sinajs.cn`, `net:smartbox.gtimg.cn`: symbol search suggestions.

## Data sources

Public quote endpoints from Sina and Tencent (same as the original plugin).
No data leaves the machine other than the quote/search HTTP requests.

## Notes

- The visible tab refreshes every 5s via the WebView timer; `on-schedule:60`
  keeps a background refresh alive (OpenIDEA's minimum scheduled interval is 60s).
- Sina quotes are GBK-encoded and decoded with `encoding_rs`.
- Settings for provider/color pattern/pinyin live in **Settings › Plugins › Stocker**.

## License

Apache-2.0 (same as the original plugin).
