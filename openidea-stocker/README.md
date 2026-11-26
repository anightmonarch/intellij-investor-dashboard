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

`dist/` is a build artifact and is not committed: after pulling, run
`openidea-plugin build` again, then reload the plugin in the IDE. A stale
`dist/plugin.wasm` keeps the old behaviour while the sources look right.

## How the refresh works

OpenIDEA gives a plugin call a **hard 5 s wall clock** (`Limits::timeout`) and
stops a plugin for the session after three consecutive timeouts. The original
plugin fetched four markets plus four index sets from a background
`ScheduledExecutorService`, which has no equivalent in a WASM component — doing
it inside one call is what made this port hang and then go silent.

So the **page owns the loop**: every `refreshInterval` seconds (5 by default) it
asks for one market at a time,

```
page  → {op:'refresh', market:'cn'}      one HTTP request, well inside the budget
plugin → {op:'quotes', market:'cn', items:[…], indices:[…]}
page  → {op:'refresh', market:'hk'}      …and so on, cn → hk → us → crypto
```

Every mutation (`add`, `remove`, `reorder`, `setMeta`, `updateSettings`) and the
`on-schedule:60` event only reply `{op:'tick'}`; the page then runs the round.
A plugin call therefore never makes more than one request.

## When nothing responds

The toolbar shows a small dot when the page is loaded but the plugin never
answers, and a search that times out says so in the results list. That means the
plugin is disabled, is waiting for the network permission, or was stopped after
repeated timeouts — check **Settings › Plugins › Stocker**. The dot disappears
once the plugin replies.

## Permissions

- `storage`: watchlist, cost price, holdings and preferences (local only).
- `net:hq.sinajs.cn`, `net:qt.gtimg.cn`: quote data (Sina requires a Referer header).
- `net:suggest3.sinajs.cn`, `net:smartbox.gtimg.cn`: symbol search suggestions.

## Data sources

Public quote endpoints from Sina and Tencent (same as the original plugin).
No data leaves the machine other than the quote/search HTTP requests.

## Notes

- Sina quotes are GBK-encoded and decoded with `encoding_rs`.
- Provider, color pattern and pinyin conversion are in **Settings › Plugins ›
  Stocker**; the tool window's own settings dialog mirrors the original's page.
- `refreshInterval` is deliberately absent from `settings.schema.json`: a native
  default there would override the original's 5 s on every activation.

## License

Apache-2.0 (same as the original plugin).
