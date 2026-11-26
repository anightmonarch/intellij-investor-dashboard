/* Stocker WebView — UI logic. All data comes from Rust via openidea.postMessage.
   Rendering mirrors StockerTableView.java exactly: centered columns, DecimalFormat
   "0.00####" prices, all numeric columns tinted by the row's change%, one index
   selected via a combo box, and price colors hidden on the selected row. */

// ---- column definitions: key, Chinese label, align (all centered in the original) ----
const COLUMNS = [
  { key: 'symbol',        label: '代码' },
  { key: 'name',          label: '名称' },
  { key: 'current',       label: '现价' },
  { key: 'opening',       label: '开盘' },
  { key: 'close',         label: '收盘' },
  { key: 'low',           label: '最低' },
  { key: 'high',          label: '最高' },
  { key: 'change',        label: '涨跌额' },
  { key: 'changePercent', label: '涨跌幅' },
  { key: 'cost',          label: '成本' },
  { key: 'holdings',      label: '持仓' },
  { key: 'netProfit',     label: '净收益额' },
  { key: 'dailyProfit',   label: '当日盈亏' },
];
const DEFAULT_VISIBLE = ['name', 'current', 'changePercent'];
// Columns tinted by the row's change% sign (NumericCellRenderer: current/open/close/low/high,
// plus change, changePercent, cost(inverted), netProfit, dailyProfit). Holdings & name stay neutral.
const PRICE_COLS = new Set(['current', 'opening', 'close', 'low', 'high']);

// ---- state ----
let state = { visibleColumns: DEFAULT_VISIBLE.slice(), colorPattern: 'rugd' };
let quotes = {};              // market -> quotes[]
let activeMarket = 'all';
let sort = { column: -1, dir: 0 };
let selectedCode = null;
let indexSelectedCode = null; // remember the chosen index across refreshes

// ---- bridge ----
// The host injects window.openidea via a script at the top of <head>. It is
// normally there before this file runs, but we wait for it rather than assume.
let bridgeReady = !!(window.openidea && typeof window.openidea.postMessage === 'function');
let previewMode = false;
let handshakeDone = false;
let gotReply = false;

function hasBridge() {
  return !!(window.openidea && typeof window.openidea.postMessage === 'function'
            && typeof window.openidea.onMessage === 'function');
}

function send(msg) {
  if (hasBridge()) {
    window.openidea.postMessage(JSON.stringify(msg));
    if (msg.op === 'suggest') {
      setTimeout(() => {
        const box = document.getElementById('search-results');
        if (box && box.textContent.trim() === '搜索中…') {
          box.innerHTML = '<div class="search-hint">搜索超时：请确认 设置 › 插件 › Stocker 已允许网络权限</div>';
        }
      }, 6000);
    }
  } else if (msg.op === 'suggest') {
    previewSuggest(msg);
  }
}

// ---- refresh loop -------------------------------------------------------
// One market per request. OpenIDEA gives a plugin call a hard 5 s budget and
// stops a plugin that keeps timing out, so the page — not the plugin — drives
// the round: ask for cn, wait for its quotes, then hk, and so on.
const MARKETS = ['cn', 'hk', 'us', 'crypto'];
let roundQueue = [];
let inflight = null;
let inflightTimer = null;

function startRound() {
  // Never stack rounds: a full queue is already a round in progress.
  if (roundQueue.length || inflight) return;
  roundQueue = MARKETS.slice();
  pump();
}

function pump() {
  if (inflight) return;
  if (!roundQueue.length) return;
  const m = roundQueue.shift();
  inflight = m;
  send({ op: 'refresh', market: m });
  // A silent market (no network, provider error) must not stall the round.
  inflightTimer = setTimeout(() => {
    if (inflight === m) { inflight = null; pump(); }
  }, 4000);
}

function doneMarket(m) {
  if (inflight !== m) return;
  clearTimeout(inflightTimer);
  inflight = null;
  setTimeout(pump, 80);
}

function rebuildAll() {
  let all = [];
  for (const m of MARKETS) all = all.concat(quotes[m] || []);
  quotes.all = all;
  let allIdx = [];
  for (const m of MARKETS) allIdx = allIdx.concat(quotes[m + '_index'] || []);
  quotes.all_index = allIdx;
}

function warnNoPlugin() {
  flash('插件未响应：请确认已在 设置 › 插件 中允许 Stocker 运行');
}

function previewSuggest(msg) {
  setTimeout(() => {
    renderSuggestions(msg.mode === 'crypto' ? [] : [
      { code: 'SH600887', name: '伊利股份', market: 'cn' },
      { code: 'ETN', name: '伊顿', market: 'us' },
      { code: 'ITW', name: '伊利诺工具', market: 'us' },
      { code: 'WYNN', name: '永利', market: 'us' },
      { code: 'AVY', name: '艾利', market: 'us' },
    ]);
  }, 250);
}

function registerBridge() {
  if (!hasBridge()) return false;
  window.openidea.onMessage(function (raw) {
    gotReply = true;
    let msg; try { msg = JSON.parse(raw); } catch (e) { return; }
    switch (msg.op) {
      case 'state':
        handshakeDone = true; updateBridgeBadge();
        state = Object.assign({}, state, msg.state);
        applyTheme(); renderHead(); renderTable();
        break;
      case 'quotes':
        handshakeDone = true; updateBridgeBadge();
        quotes[msg.market] = msg.items || [];
        quotes[msg.market + '_index'] = msg.indices || [];
        rebuildAll();
        renderTable(); renderIndexBar();
        doneMarket(msg.market);
        break;
      case 'tick':
        startRound();
        break;
      case 'cleared':
        quotes = {};
        renderTable(); renderIndexBar();
        break;
      case 'error': flash(msg.message); break;
      case 'suggestions': renderSuggestions(msg.items || []); break;
      case 'openSettings': openSettings(); break;
      case 'openSearch': openSearch(); break;
      case 'openManage': openManage(); break;
    }
  });
  bridgeReady = true;
  updateBridgeBadge();
  send({ op: 'ready' });
  // If the plugin never answers, the page is alive but the plugin is not: it is
  // disabled, waiting for permission, or stopped after repeated timeouts.
  setTimeout(() => { if (!gotReply) { warnNoPlugin(); updateBridgeBadge(); } }, 4000);
  return true;
}

// Wait briefly for the injected bridge; only fall back to preview if truly absent.
if (!registerBridge()) {
  let tries = 0;
  const t = setInterval(() => {
    tries++;
    if (registerBridge()) { clearInterval(t); }
    else if (tries >= 20) { clearInterval(t); previewMode = true; updateBridgeBadge(); }
  }, 50);
}

function updateBridgeBadge() {
  const el = document.getElementById('bridge-badge');
  if (!el) return;
  // Hidden once the plugin answers: the toolbar stays exactly as the original.
  // Shown only when something is wrong, with the reason in the tooltip.
  if (handshakeDone) { el.style.display = 'none'; return; }
  el.style.display = '';
  if (bridgeReady) {
    el.textContent = '◐';
    el.title = '插件未响应：请确认 设置 › 插件 › Stocker 已启用并已授权';
    el.style.color = '#ef9f27';
  } else {
    el.textContent = '○';
    el.title = '预览模式（未在 IDE 中运行）';
    el.style.color = '#888';
  }
}

function applyTheme() {
  const root = document.documentElement;
  const cp = state.colorPattern;
  if (cp === 'gurd') {
    root.style.setProperty('--up', '#43a047');
    root.style.setProperty('--down', '#e53935');
  } else if (cp === 'none') {
    root.style.setProperty('--up', 'var(--fg)');
    root.style.setProperty('--down', 'var(--fg)');
  } else {
    root.style.setProperty('--up', '#e53935');
    root.style.setProperty('--down', '#43a047');
  }
}

// ---- formatting: mirror Java DecimalFormat ----
// "0.00####" — at least 2 decimals, up to 6, trailing '#' zeros dropped.
function decimalFormat(v, minDecimals, maxDecimals) {
  if (v === null || v === undefined || isNaN(v)) return null;
  let fixed = Number(v).toFixed(maxDecimals);
  let [int, dec] = fixed.split('.');
  while (dec.length > minDecimals && dec.endsWith('0')) dec = dec.slice(0, -1);
  return int + (dec ? '.' + dec : '');
}
function price(v) { return decimalFormat(v, 2, 6); }
function signed(v, minDec, maxDec) {
  if (v === null || v === undefined || isNaN(v)) return '-';
  const text = decimalFormat(v, minDec, maxDec);
  return (v > 0 ? '+' : '') + text;
}
// Double.toString() semantics used by the Change% cell value in the original:
// quote.percentage + "%" keeps only meaningful decimals (e.g. 1.9, 0.4).
function plainDouble(v) {
  if (v === null || v === undefined || isNaN(v)) return '';
  return String(Number(v));
}
function valueFor(q, key) {
  switch (key) {
    case 'symbol': {
      let code = q.code || '';
      if (code.startsWith('BTC') && code.length > 3) code = code.slice(3);  // strip BTC prefix
      return code;
    }
    case 'name': return q.displayName || q.name || '';
    case 'current': return price(q.current) ?? '-';
    case 'opening': return price(q.opening) ?? '-';
    case 'close': return price(q.close) ?? '-';
    case 'low': return price(q.low) ?? '-';
    case 'high': return price(q.high) ?? '-';
    case 'change': return signed(q.change, 2, 6);
    case 'changePercent': {
      const p = q.percentage;
      if (p === null || p === undefined || isNaN(p)) return '-';
      return (p > 0 ? '+' : '') + plainDouble(p) + '%';
    }
    case 'cost': return (q.cost == null) ? '-' : Number(q.cost).toFixed(3);
    case 'holdings': return (q.holdings == null) ? '-' : String(q.holdings);
    case 'netProfit': return (q.cost == null || q.holdings == null) ? '-'
      : Number((q.current - q.cost) * q.holdings).toFixed(3);
    case 'dailyProfit': return (q.holdings == null) ? '-'
      : Number(q.change * q.holdings).toFixed(3);
    default: return '';
  }
}
// Price color is driven by the row's change% for every numeric column.
function colorClass(q, key) {
  if (!PRICE_COLS.has(key) && key !== 'change' && key !== 'changePercent'
      && key !== 'netProfit' && key !== 'dailyProfit') return '';
  let v;
  if (key === 'change') v = q.change;
  else if (key === 'changePercent') v = q.percentage;
  else if (key === 'netProfit') v = (q.cost == null || q.holdings == null) ? null : (q.current - q.cost) * q.holdings;
  else if (key === 'dailyProfit') v = (q.holdings == null) ? null : q.change * q.holdings;
  else v = q.percentage;  // price columns follow change%
  if (v === null || v === undefined || isNaN(v)) return '';
  return v > 0 ? 'up' : (v < 0 ? 'down' : 'zero');
}
// Cost column uses inverted color: cost > current => loss (down), cost < current => profit (up).
function costColor(q) {
  if (q.cost == null) return '';
  return q.cost > q.current ? 'down' : (q.cost < q.current ? 'up' : 'zero');
}
function rawFor(q, key) {
  switch (key) {
    case 'symbol': return q.code || '';
    case 'name': return q.displayName || q.name || '';
    case 'netProfit': return (q.cost == null || q.holdings == null) ? null : (q.current - q.cost) * q.holdings;
    case 'dailyProfit': return (q.holdings == null) ? null : q.change * q.holdings;
    default: return q[key];
  }
}

// ---- rendering ----
function visibleColumns() {
  let cols = COLUMNS.filter(c => state.visibleColumns.includes(c.key));
  if (cols.length === 0) cols = COLUMNS.filter(c => DEFAULT_VISIBLE.includes(c.key));
  return cols;
}
function renderHead() {
  const cols = visibleColumns();
  const head = document.getElementById('head-row');
  head.innerHTML = '';
  cols.forEach((c, i) => {
    const th = document.createElement('th');
    th.style.width = c.key === 'symbol' ? '80px' : (c.key === 'name' ? '100px' : '66px');
    th.textContent = c.label + (sort.column === i ? (sort.dir === 1 ? ' ▲' : ' ▼') : '');
    th.onclick = () => onSort(i);
    head.appendChild(th);
  });
}
function currentQuotes() {
  let list = (quotes[activeMarket] || []).slice();
  if (sort.column >= 0 && sort.dir !== 0) {
    const cols = visibleColumns();
    const key = cols[sort.column] ? cols[sort.column].key : null;
    if (key) {
      const asc = sort.dir === 1;
      list.sort((a, b) => {
        const av = rawFor(a, key), bv = rawFor(b, key);
        if (typeof av === 'number' && typeof bv === 'number') return asc ? av - bv : bv - av;
        return asc ? String(av ?? '').localeCompare(String(bv ?? ''))
                   : String(bv ?? '').localeCompare(String(av ?? ''));
      });
    }
  }
  return list;
}
function renderTable() {
  const cols = visibleColumns();
  const list = currentQuotes();
  const tbody = document.getElementById('body-row');
  tbody.innerHTML = '';
  document.getElementById('empty').classList.toggle('hidden', list.length > 0);
  list.forEach(q => {
    const tr = document.createElement('tr');
    if (q.code === selectedCode) tr.classList.add('selected');
    cols.forEach((c) => {
      const td = document.createElement('td');
      td.textContent = valueFor(q, c.key);
      const cc = c.key === 'cost' ? costColor(q) : colorClass(q, c.key);
      if (cc) td.classList.add(cc);
      tr.appendChild(td);
    });
    tr.onclick = () => { selectedCode = q.code; renderTable(); };
    tr.oncontextmenu = (e) => {
      e.preventDefault(); selectedCode = q.code; renderTable(); showCtxMenu(e.clientX, e.clientY);
    };
    tbody.appendChild(tr);
  });
}
function onSort(col) {
  if (sort.column !== col) sort = { column: col, dir: 1 };
  else if (sort.dir === 1) sort.dir = 2;
  else if (sort.dir === 2) sort = { column: -1, dir: 0 };
  else sort = { column: col, dir: 1 };
  renderHead(); renderTable();
}
function renderIndexBar() {
  const indices = quotes[activeMarket + '_index'] || quotes['cn_index'] || [];
  const sel = document.getElementById('index-select');
  const old = indexSelectedCode;
  sel.innerHTML = '';
  indices.forEach(q => {
    const opt = document.createElement('option');
    opt.value = q.code;
    opt.textContent = q.displayName || q.name;
    sel.appendChild(opt);
  });
  if (old && indices.some(q => q.code === old)) sel.value = old;
  else if (indices.length) indexSelectedCode = sel.value;
  sel.onchange = () => { indexSelectedCode = sel.value; renderIndexBar(); };
  if (!sel.options.length) {
    ['idx-value', 'idx-extent', 'idx-percent'].forEach(id => document.getElementById(id).textContent = '');
    return;
  }
  const q = indices.find(x => x.code === sel.value) || indices[0];
  const v = q.percentage > 0 ? 'up' : (q.percentage < 0 ? 'down' : 'zero');
  const ev = document.getElementById('idx-value');
  const ee = document.getElementById('idx-extent');
  const ep = document.getElementById('idx-percent');
  ev.textContent = Number(q.current).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
  ee.textContent = signed(q.change, 2, 2);
  ep.textContent = signed(q.percentage, 2, 6) + '%';
  [ev, ee, ep].forEach(el => { el.className = 'idx-cell ' + v; });
}

// ---- tabs / toolbar ----
document.querySelectorAll('.tab').forEach(tab => {
  tab.onclick = () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    tab.classList.add('active');
    activeMarket = tab.dataset.market;
    sort = { column: -1, dir: 0 };
    indexSelectedCode = null;
    renderTable(); renderIndexBar();
  };
});
document.getElementById('btn-add').onclick = () => openSearch();
document.getElementById('btn-refresh').onclick = () => send({ op: 'refresh' });
document.getElementById('btn-stop').onclick = () => send({ op: 'stop' });
document.getElementById('btn-manage').onclick = () => openManage();
document.getElementById('btn-settings').onclick = () => openSettings();
document.getElementById('btn-close').onclick = () => flash('可从工具窗口栏再次打开 Stocker');
document.getElementById('search-close').onclick = () => closeModal('search-dialog');
document.getElementById('manage-cancel').onclick = () => closeModal('manage-dialog');
document.getElementById('manage-ok').onclick = () => {
  // Commit each market's working list as a whole (order preserved), then metadata.
  for (const m of ['cn','hk','us','crypto']) {
    send({ op: 'reorder', market: m, codes: (manageWork[m] || []).map(x => x.code) });
  }
  for (const code of Object.keys(manageEdits)) {
    const e = manageEdits[code];
    send({ op: 'setMeta', code, customName: e.customName, cost: e.cost, holdings: e.holdings });
  }
  closeModal('manage-dialog');
};

// ---- context menu ----
function showCtxMenu(x, y) {
  const menu = document.getElementById('ctx-menu');
  menu.style.left = x + 'px'; menu.style.top = y + 'px';
  menu.classList.remove('hidden');
}
document.addEventListener('click', () => document.getElementById('ctx-menu').classList.add('hidden'));
document.getElementById('ctx-delete').onclick = (e) => {
  e.stopPropagation();
  if (selectedCode) send({ op: 'remove', code: selectedCode });
  document.getElementById('ctx-menu').classList.add('hidden');
};

// ---- search dialog ----
let searchTimer = null;
const searchInput = document.getElementById('search-input');
const searchMode = document.getElementById('search-mode');
function openSearch() {
  document.getElementById('search-dialog').classList.remove('hidden');
  searchInput.value = '';
  document.getElementById('search-clear').classList.add('hidden');
  onModeChange();
  document.getElementById('search-results').innerHTML = '<div class="search-hint">输入以搜索</div>';
  searchInput.focus();
}
function onModeChange() {
  searchInput.placeholder = searchMode.value === 'crypto' ? '输入以搜索加密货币' : '输入以搜索股票（A股/港股/美股）';
}
searchInput.oninput = () => {
  clearTimeout(searchTimer);
  document.getElementById('search-clear').classList.toggle('hidden', searchInput.value.length === 0);
  const key = searchInput.value.trim();
  if (!key) { document.getElementById('search-results').innerHTML = '<div class="search-hint">输入以搜索</div>'; return; }
  document.getElementById('search-results').innerHTML = '<div class="search-hint">搜索中…</div>';
  searchTimer = setTimeout(() => send({ op: 'suggest', key, mode: searchMode.value }), 300);
};
searchMode.onchange = () => { onModeChange(); searchInput.dispatchEvent(new Event('input')); };
document.getElementById('search-clear').onclick = () => {
  searchInput.value = '';
  searchInput.dispatchEvent(new Event('input'));
  searchInput.focus();
};
function renderSuggestions(items) {
  const box = document.getElementById('search-results');
  box.innerHTML = '';
  if (!items.length) { box.innerHTML = '<div class="search-hint">无结果</div>'; return; }
  items.forEach(s => {
    const row = document.createElement('div'); row.className = 'search-row';
    const added = isAdded(s.code, s.market);
    const name = s.name.length > 25 ? s.name.slice(0, 25) + '…' : s.name;
    row.innerHTML =
      `<span class="col-code">${s.code}</span>` +
      `<span class="col-market">${marketTitle(s.market)}</span>` +
      `<span class="col-name" title="${s.name}">${name}</span>` +
      `<span class="col-action"><button class="row-btn">${added ? '删除' : '添加'}</button></span>`;
    row.querySelector('button').onclick = (e) => {
      if (added) { send({ op: 'remove', code: s.code }); e.target.textContent = '添加'; }
      else { send({ op: 'add', market: s.market, code: s.code, name: s.name }); e.target.textContent = '删除'; }
    };
    box.appendChild(row);
  });
}
function isAdded(code) {
  const lists = (state && state.watchlists) || {};
  for (const m of ['cn','hk','us','crypto']) {
    if ((lists[m] || []).some(c => c.toUpperCase() === String(code).toUpperCase())) return true;
  }
  return false;
}
function marketTitle(k) { return ({cn:'CN',hk:'HK',us:'US',crypto:'Crypto'})[k] || k; }

// ---- manage dialog ----
// The dialog edits a working copy of each market's watchlist. OK commits the
// whole list back (order included), mirroring StockerManagementDialog's OkAction.
let manageMarket = 'cn';
let manageWork = {};          // market -> [{code,name,customName,cost,holdings}]
let manageSelected = -1;
let manageEdits = {};         // committed metadata changes at OK time

function openManage() {
  manageEdits = {};
  manageWork = {};
  for (const m of ['cn','hk','us','crypto']) {
    manageWork[m] = (quotes[m] || []).map(q => ({
      code: q.code, name: q.name,
      customName: (q.displayName && q.displayName !== q.name) ? q.displayName : '',
      cost: q.cost != null ? q.cost : null,
      holdings: q.holdings != null ? q.holdings : null,
    }));
    // Keep stored codes that failed to load as placeholders (like the original).
    const loaded = new Set(manageWork[m].map(x => x.code.toUpperCase()));
    for (const code of ((state.watchlists || {})[m] || [])) {
      if (!loaded.has(String(code).toUpperCase())) {
        manageWork[m].push({ code, name: code, customName: '', cost: null, holdings: null });
      }
    }
  }
  manageSelected = -1;
  document.getElementById('manage-dialog').classList.remove('hidden');
  renderManage();
}
document.querySelectorAll('.mtab').forEach(tab => {
  tab.onclick = () => {
    document.querySelectorAll('.mtab').forEach(t => t.classList.remove('active'));
    tab.classList.add('active'); manageMarket = tab.dataset.market;
    manageSelected = -1; renderManage();
  };
});
function renderManage() {
  const box = document.getElementById('manage-list'); box.innerHTML = '';
  const list = manageWork[manageMarket] || [];
  if (!list.length) { box.innerHTML = '<div class="search-hint">该市场暂无自选</div>'; updateManageToolbar(); return; }
  list.forEach((q, i) => {
    const row = document.createElement('div');
    row.className = 'manage-row' + (i === manageSelected ? ' selected' : '');
    row.innerHTML =
      `<span class="m-code">${q.code}</span>` +
      `<span class="m-name" title="${q.name}">${q.name}</span>` +
      `<span class="m-custom">${q.customName || '-'}</span>` +
      `<span class="m-cost">${q.cost != null ? Number(q.cost).toFixed(3) : '-'}</span>` +
      `<span class="m-hold">${q.holdings != null ? q.holdings : '-'}</span>` +
      `<span class="m-ops"></span>`;
    row.onclick = () => { manageSelected = i; renderManage(); };
    box.appendChild(row);
  });
  updateManageToolbar();
}
function updateManageToolbar() {
  const list = manageWork[manageMarket] || [];
  const hasSel = manageSelected >= 0 && manageSelected < list.length;
  document.getElementById('mt-edit').disabled = !hasSel;
  document.getElementById('mt-delete').disabled = !hasSel;
  document.getElementById('mt-up').disabled = !hasSel || manageSelected === 0;
  document.getElementById('mt-down').disabled = !hasSel || manageSelected === list.length - 1;
}
document.getElementById('mt-edit').onclick = () => {
  const q = (manageWork[manageMarket] || [])[manageSelected];
  if (q) openEdit(q);
};
document.getElementById('mt-delete').onclick = () => {
  const list = manageWork[manageMarket] || [];
  if (manageSelected < 0 || manageSelected >= list.length) return;
  list.splice(manageSelected, 1);
  if (manageSelected >= list.length) manageSelected = list.length - 1;
  renderManage();
};
document.getElementById('mt-up').onclick = () => moveSelected(-1);
document.getElementById('mt-down').onclick = () => moveSelected(1);
function moveSelected(delta) {
  const list = manageWork[manageMarket] || [];
  const j = manageSelected + delta;
  if (manageSelected < 0 || j < 0 || j >= list.length) return;
  [list[manageSelected], list[j]] = [list[j], list[manageSelected]];
  manageSelected = j;
  renderManage();
}

// ---- edit dialog ----
let editingCode = null;
function openEdit(q) {
  editingCode = q.code;
  document.getElementById('edit-title').textContent = '编辑 ' + q.code;
  document.getElementById('edit-name').value = q.customName || '';
  document.getElementById('edit-cost').value = q.cost != null ? Number(q.cost).toFixed(3) : '';
  document.getElementById('edit-holdings').value = q.holdings != null ? String(q.holdings) : '';
  document.getElementById('edit-error').textContent = '';
  document.getElementById('edit-dialog').classList.remove('hidden');
}
document.getElementById('edit-cancel').onclick = () => closeModal('edit-dialog');
document.getElementById('edit-ok').onclick = () => {
  const name = document.getElementById('edit-name').value.trim();
  const costText = document.getElementById('edit-cost').value.trim();
  const holdText = document.getElementById('edit-holdings').value.trim();
  const costVal = costText ? parseFloat(costText) : null;
  const holdVal = holdText ? parseInt(holdText, 10) : null;
  if (costText && isNaN(costVal)) { document.getElementById('edit-error').textContent = '请输入有效数字'; return; }
  if (holdText && isNaN(holdVal)) { document.getElementById('edit-error').textContent = '请输入整数'; return; }
  // Update the working-copy row so the manage list reflects the edit immediately.
  const list = manageWork[manageMarket] || [];
  const row = list.find(x => x.code === editingCode);
  if (row) {
    row.customName = name || '';
    row.cost = costVal;
    row.holdings = holdVal;
  }
  manageEdits[editingCode] = { customName: name || null, cost: costVal, holdings: holdVal };
  closeModal('edit-dialog');
  renderManage();
};

// ---- settings dialog (1:1 replica) ----
const COLUMN_LABELS = {
  symbol: '代码', name: '名称', current: '现价', opening: '开盘', close: '收盘',
  low: '最低', high: '最高', change: '涨跌额', changePercent: '涨跌幅',
  cost: '成本', holdings: '持仓', netProfit: '净收益额', dailyProfit: '当日盈亏',
};
const COLUMN_KEYS = Object.keys(COLUMN_LABELS);

function openSettings() {
  // Populate the 13 visible-column checkboxes.
  const box = document.getElementById('set-columns');
  box.innerHTML = '';
  const visible = state.visibleColumns || DEFAULT_VISIBLE;
  COLUMN_KEYS.forEach(key => {
    const label = document.createElement('label');
    label.className = 'set-check';
    const cb = document.createElement('input');
    cb.type = 'checkbox'; cb.value = key; cb.checked = visible.includes(key);
    cb.onchange = () => {
      const checked = Array.from(box.querySelectorAll('input:checked')).map(i => i.value);
      document.getElementById('set-columns-warning').classList.toggle('hidden', checked.length > 0);
    };
    const span = document.createElement('span');
    span.textContent = COLUMN_LABELS[key];
    label.appendChild(cb); label.appendChild(span);
    box.appendChild(label);
  });
  // Populate current values.
  document.getElementById('set-language').value = state.languageOverride || '';
  document.getElementById('set-provider').value = state.quoteProvider || 'sina';
  document.getElementById('set-crypto-provider').value = state.cryptoQuoteProvider || 'sina';
  document.getElementById('set-pinyin').checked = !!state.displayNameWithPinyin;
  const cp = state.colorPattern || 'rugd';
  const radios = document.querySelectorAll('input[name="colorPattern"]');
  radios.forEach(r => { r.checked = (r.value === cp); });
  document.getElementById('set-columns-warning').classList.add('hidden');
  document.getElementById('settings-dialog').classList.remove('hidden');
}

function collectSettings() {
  const box = document.getElementById('set-columns');
  const columns = Array.from(box.querySelectorAll('input:checked')).map(i => i.value);
  const cp = document.querySelector('input[name="colorPattern"]:checked');
  return {
    languageOverride: document.getElementById('set-language').value,
    quoteProvider: document.getElementById('set-provider').value,
    cryptoQuoteProvider: document.getElementById('set-crypto-provider').value,
    displayNameWithPinyin: document.getElementById('set-pinyin').checked,
    colorPattern: cp ? cp.value : 'rugd',
    visibleColumns: columns,
  };
}

function applySettings(close) {
  const box = document.getElementById('set-columns');
  const checked = Array.from(box.querySelectorAll('input:checked'));
  if (checked.length === 0) {
    document.getElementById('set-columns-warning').classList.remove('hidden');
    return;
  }
  const patch = collectSettings();
  // Update local state immediately so the UI reflects the change without waiting.
  state = Object.assign({}, state, patch);
  applyTheme();
  renderHead(); renderTable(); renderIndexBar();
  send({ op: 'updateSettings', state: patch });
  if (close) closeModal('settings-dialog');
  else flash('设置已应用');
}

document.getElementById('settings-ok').onclick = () => applySettings(true);
document.getElementById('settings-apply').onclick = () => applySettings(false);
document.getElementById('settings-cancel').onclick = () => closeModal('settings-dialog');

function closeModal(id) { document.getElementById(id).classList.add('hidden'); }
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') document.querySelectorAll('.modal:not(.hidden)').forEach(m => m.classList.add('hidden'));
});
function flash(msg) {
  let el = document.getElementById('toast');
  if (!el) {
    el = document.createElement('div'); el.id = 'toast';
    el.style.cssText = 'position:fixed;bottom:12px;left:50%;transform:translateX(-50%);background:var(--bg-elevated);border:1px solid var(--border);border-radius:6px;padding:8px 14px;z-index:300;box-shadow:0 2px 8px rgba(0,0,0,.25);font-size:12px;';
    document.body.appendChild(el);
  }
  el.textContent = msg; el.style.display = 'block';
  clearTimeout(el._t); el._t = setTimeout(() => { el.style.display = 'none'; }, 2500);
}

// ---- boot ----
applyTheme(); renderHead(); renderTable(); updateBridgeBadge();
// Only load demo data after we are sure there is no bridge (checked in poll above).
setTimeout(() => { if (previewMode || !bridgeReady) loadPreviewData(); }, 1200);
// Inside the IDE the refresh round is driven by `ready` → `tick` → per-market
// replies; a timer only restarts a round that went quiet (e.g. after the
// window was hidden and the plugin instance was reclaimed). The period is the
// original plugin's `refreshInterval`, in seconds.
function scheduleRound() {
  const secs = Math.max(3, Number(state.refreshInterval) || 5);
  setTimeout(() => {
    if (bridgeReady && !inflight && !roundQueue.length) startRound();
    scheduleRound();
  }, secs * 1000);
}
scheduleRound();

function loadPreviewData() {
  const raw = [
    ['sh600887','伊利股份',33.29,-0.86],['sh600900','长江电力',31.85,-0.34],
    ['sz300750','比音勒芬',28.18,-1.09],['sz000333','美的集团',94.97,0.88],
    ['sh603288','牧原股份',43.49,-1.38],['sh513100','纳100ETF',1.966,2.88],
    ['sh600009','申通快递',14.00,0.29],['sz002648','卫星化学',23.32,2.73],
    ['sh601928','儒意电影',9.75,-1.22],['sh600977','中国电影',14.65,-1.08],
    ['sh600028','恒逸石化',10.95,0.74],['sh603993','洛阳钼业',11.82,1.9],
    ['sh601600','中国铝业',7.47,0.4],
  ];
  quotes.cn = raw.map(([code, name, cur, pct]) => ({
    code, name, displayName: name, current: cur, opening: cur, close: cur,
    low: cur, high: cur, change: Number((cur * pct / 100).toFixed(2)),
    percentage: pct, updateAt: '', cost: null, holdings: null,
  }));
  quotes.cn_index = [{
    code: 'SH000001', name: '上证指数', displayName: '上证指数',
    current: 3481.04, change: 4.57, percentage: 0.13,
  }];
  rebuildAll();
  renderTable(); renderIndexBar();
}
