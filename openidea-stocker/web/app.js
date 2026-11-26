/* Stocker WebView — UI logic. All data comes from Rust via openidea.postMessage. */

// ---- column definitions (13, matching StockerTableColumn) ----
const COLUMNS = [
  { key: 'symbol',        label: 'Symbol',   align: 'left'  },
  { key: 'name',          label: 'Name',     align: 'left'  },
  { key: 'current',       label: 'Current',  align: 'right' },
  { key: 'opening',       label: 'Opening',  align: 'right' },
  { key: 'close',         label: 'Close',    align: 'right' },
  { key: 'low',           label: 'Low',      align: 'right' },
  { key: 'high',          label: 'High',     align: 'right' },
  { key: 'change',        label: 'Change',   align: 'right' },
  { key: 'changePercent', label: 'Change%',  align: 'right' },
  { key: 'cost',          label: 'Cost',     align: 'right' },
  { key: 'holdings',      label: 'Holdings', align: 'right' },
  { key: 'netProfit',     label: 'Net Profit', align: 'right' },
  { key: 'dailyProfit',   label: 'Daily P/L', align: 'right' },
];
const NUMERIC = new Set(['current','opening','close','low','high','change','changePercent','cost','holdings','netProfit','dailyProfit']);
const COLORED = new Set(['change','changePercent','netProfit','dailyProfit']);
const DEFAULT_VISIBLE = ['name','current','changePercent'];

// ---- state ----
let state = {
  visibleColumns: DEFAULT_VISIBLE.slice(),
  colorPattern: 'rugd',
  languageOverride: '',
};
let quotes = {};           // market key -> array of quotes (camelCase from Rust)
let activeMarket = 'all';
let sort = { column: -1, dir: 0 };   // dir: 0 none, 1 asc, 2 desc
let sortBackup = [];
let selectedCode = null;

// ---- bridge ----
function send(msg) {
  if (window.openidea && typeof window.openidea.postMessage === 'function') {
    window.openidea.postMessage(JSON.stringify(msg));
  }
}
if (window.openidea && typeof window.openidea.onMessage === 'function') {
  window.openidea.onMessage(function (raw) {
    let msg;
    try { msg = JSON.parse(raw); } catch (e) { return; }
    switch (msg.op) {
      case 'state':
        state = Object.assign({}, state, msg.state);
        applyTheme();
        renderHead();
        renderTable();
        break;
      case 'quotes':
        quotes = msg.quotes || {};
        renderTable();
        renderIndexBar();
        break;
      case 'added':
      case 'removed':
        // Rust follows up with a quotes message; nothing to do here.
        break;
      case 'error':
        flash(msg.message);
        break;
      case 'suggestions':
        renderSuggestions(msg.items || []);
        break;
    }
  });
}

function applyTheme() {
  const cp = state.colorPattern;
  const root = document.documentElement;
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

// ---- formatting (ported from StockerQuoteUpdateListener + renderer) ----
function fmtPrice(v) {
  if (v === null || v === undefined || isNaN(v)) return '-';
  const abs = Math.abs(v);
  // 0.00#### : at least 2 decimals, up to 6 for small crypto prices
  let decimals = 2;
  if (abs > 0 && abs < 1) decimals = 6;
  else if (abs > 0 && abs < 10) decimals = 4;
  return Number(v).toFixed(decimals).replace(/\.?0+$/, m => m.includes('.') ? '' : m);
}
function fmtSigned(v, decimals = 2) {
  if (v === null || v === undefined || isNaN(v)) return '-';
  const s = Number(v).toFixed(decimals);
  return (v > 0 ? '+' : '') + s;
}
function fmtIndex(v) {
  if (v === null || v === undefined || isNaN(v)) return '-';
  return Number(v).toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}
function valueFor(q, key) {
  switch (key) {
    case 'symbol': return q.code || '';
    case 'name': return q.displayName || q.name || '';
    case 'current': return fmtPrice(q.current);
    case 'opening': return fmtPrice(q.opening);
    case 'close': return fmtPrice(q.close);
    case 'low': return fmtPrice(q.low);
    case 'high': return fmtPrice(q.high);
    case 'change': return fmtSigned(q.change, 2);
    case 'changePercent': return fmtSigned(q.percentage, 2) + '%';
    case 'cost': return (q.cost === null || q.cost === undefined) ? '-' : Number(q.cost).toFixed(3);
    case 'holdings': return (q.holdings === null || q.holdings === undefined) ? '-' : String(q.holdings);
    case 'netProfit': {
      if (q.cost == null || q.holdings == null) return '-';
      return Number((q.current - q.cost) * q.holdings).toFixed(3);
    }
    case 'dailyProfit': {
      if (q.holdings == null) return '-';
      return Number(q.change * q.holdings).toFixed(3);
    }
    default: return '';
  }
}
function rawFor(q, key) {
  switch (key) {
    case 'netProfit': return (q.cost == null || q.holdings == null) ? null : (q.current - q.cost) * q.holdings;
    case 'dailyProfit': return q.holdings == null ? null : q.change * q.holdings;
    default: return q[key];
  }
}
function colorClass(q, key) {
  if (!COLORED.has(key)) return '';
  const v = rawFor(q, key);
  if (v === null || v === undefined || v === 0 || isNaN(v)) return '';
  return v > 0 ? 'up' : 'down';
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
    th.style.width = c.key === 'symbol' ? '80px' : (c.key === 'name' ? '100px' : '');
    let label = c.label;
    if (sort.column === i) label += `<span class="sort">${sort.dir === 1 ? '▲' : '▼'}</span>`;
    th.innerHTML = label;
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
        if (typeof av === 'number' && typeof bv === 'number') {
          return asc ? av - bv : bv - av;
        }
        const as = String(av ?? ''), bs = String(bv ?? '');
        return asc ? as.localeCompare(bs) : bs.localeCompare(as);
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
      if (c.align === 'right' || NUMERIC.has(c.key)) td.classList.add('num');
      if (c.key === 'symbol') td.classList.add('code');
      const cc = colorClass(q, c.key);
      if (cc) td.classList.add(cc);
      if ((c.key === 'change' || c.key === 'changePercent') && (q.change > 0)) td.classList.add('positive');
      tr.appendChild(td);
    });
    tr.onclick = () => { selectedCode = q.code; renderTable(); };
    tr.oncontextmenu = (e) => {
      e.preventDefault();
      selectedCode = q.code;
      renderTable();
      showCtxMenu(e.clientX, e.clientY);
    };
    tbody.appendChild(tr);
  });
}

function onSort(col) {
  if (sort.column !== col) {
    sort = { column: col, dir: 1 };
  } else if (sort.dir === 1) {
    sort.dir = 2;
  } else if (sort.dir === 2) {
    sort = { column: -1, dir: 0 };
  } else {
    sort = { column: col, dir: 1 };
  }
  renderHead();
  renderTable();
}

function renderIndexBar() {
  // Prefer the active market's indices, then fall back to CN.
  let indices = quotes[activeMarket + '_index'] || quotes['cn_index'] || [];
  const bar = document.getElementById('index-bar');
  bar.innerHTML = '';
  indices.forEach(q => {
    const item = document.createElement('div');
    item.className = 'index-item';
    const up = q.change > 0, down = q.change < 0;
    item.innerHTML =
      `<span class="index-name">${q.code}</span>` +
      `<span class="index-value">${fmtIndex(q.current)}</span>` +
      `<span class="${up ? 'up' : down ? 'down' : ''}">${fmtSigned(q.change, 2)}</span>` +
      `<span class="${up ? 'up' : down ? 'down' : ''}">${fmtSigned(q.percentage, 2)}%</span>`;
    bar.appendChild(item);
  });
}

// ---- tabs / toolbar ----
document.querySelectorAll('.tab').forEach(tab => {
  tab.onclick = () => {
    document.querySelectorAll('.tab').forEach(t => t.classList.remove('active'));
    tab.classList.add('active');
    activeMarket = tab.dataset.market;
    sort = { column: -1, dir: 0 };
    renderTable();
    renderIndexBar();
  };
});

document.getElementById('btn-add').onclick = () => openSearch();
document.getElementById('btn-refresh').onclick = () => send({ op: 'refresh' });
document.getElementById('btn-stop').onclick = () => send({ op: 'stop' });
document.getElementById('btn-manage').onclick = () => openManage();
document.getElementById('btn-settings').onclick = () => flash('Configure columns/provider in Settings › Plugins › Stocker');
document.getElementById('search-close').onclick = () => closeModal('search-dialog');
document.getElementById('manage-cancel').onclick = () => closeModal('manage-dialog');
document.getElementById('manage-ok').onclick = () => closeModal('manage-dialog');

// ---- context menu ----
function showCtxMenu(x, y) {
  const menu = document.getElementById('ctx-menu');
  menu.style.left = x + 'px';
  menu.style.top = y + 'px';
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
  searchInput.placeholder = searchMode.value === 'crypto' ? 'Type to search crypto' : 'Type to search stocks (CN/HK/US)';
  document.getElementById('search-results').innerHTML = '<div class="search-hint">Type to search</div>';
  searchInput.focus();
}
searchInput.oninput = () => {
  clearTimeout(searchTimer);
  const key = searchInput.value.trim();
  if (!key) {
    document.getElementById('search-results').innerHTML = '<div class="search-hint">Type to search</div>';
    return;
  }
  document.getElementById('search-results').innerHTML = '<div class="search-hint">Searching…</div>';
  searchTimer = setTimeout(() => {
    send({ op: 'suggest', key, mode: searchMode.value });
  }, 300);
};
searchMode.onchange = () => {
  searchInput.placeholder = searchMode.value === 'crypto' ? 'Type to search crypto' : 'Type to search stocks (CN/HK/US)';
  searchInput.dispatchEvent(new Event('input'));
};
function renderSuggestions(items) {
  const box = document.getElementById('search-results');
  box.innerHTML = '';
  if (items.length === 0) {
    box.innerHTML = '<div class="search-hint">No results</div>';
    return;
  }
  items.forEach(s => {
    const row = document.createElement('div');
    row.className = 'search-row';
    const added = isAdded(s.code, s.market);
    const name = s.name.length > 25 ? s.name.slice(0, 25) + '…' : s.name;
    row.innerHTML =
      `<span class="col-code">${s.code}</span>` +
      `<span class="col-market">${marketTitle(s.market)}</span>` +
      `<span class="col-name" title="${s.name}">${name}</span>` +
      `<span class="col-action"><button class="row-btn">${added ? 'Delete' : 'Add'}</button></span>`;
    row.querySelector('button').onclick = (e) => {
      if (added) {
        send({ op: 'remove', code: s.code });
        e.target.textContent = 'Add';
      } else {
        send({ op: 'add', market: s.market, code: s.code, name: s.name });
        e.target.textContent = 'Delete';
      }
    };
    box.appendChild(row);
  });
}
function isAdded(code, market) {
  const lists = (state && state.watchlists) || {};
  for (const m of ['cn','hk','us','crypto']) {
    const list = lists[m] || [];
    if (list.some(c => c.toUpperCase() === String(code).toUpperCase())) return true;
  }
  // Fallback: scan loaded quotes
  for (const m of ['cn','hk','us','crypto']) {
    if ((quotes[m] || []).some(q => q.code === code)) return true;
  }
  return false;
}
function marketTitle(k) { return ({cn:'CN',hk:'HK',us:'US',crypto:'Crypto'})[k] || k; }

// ---- manage dialog ----
let manageMarket = 'cn';
let manageEdits = {};  // code -> {customName, cost, holdings}
function openManage() {
  manageEdits = {};
  // Seed from currently displayed quotes across markets
  document.getElementById('manage-dialog').classList.remove('hidden');
  renderManage();
}
document.querySelectorAll('.mtab').forEach(tab => {
  tab.onclick = () => {
    document.querySelectorAll('.mtab').forEach(t => t.classList.remove('active'));
    tab.classList.add('active');
    manageMarket = tab.dataset.market;
    renderManage();
  };
});
function renderManage() {
  const box = document.getElementById('manage-list');
  box.innerHTML = '';
  const list = quotes[manageMarket] || [];
  if (list.length === 0) {
    box.innerHTML = '<div class="search-hint">No favorites in this market yet</div>';
    return;
  }
  list.forEach(q => {
    const row = document.createElement('div');
    row.className = 'manage-row';
    const edit = manageEdits[q.code] || {};
    const custom = edit.customName !== undefined ? edit.customName : (q.cost !== undefined ? '' : '');
    row.innerHTML =
      `<span class="m-code">${q.code}</span>` +
      `<span class="m-name" title="${q.name}">${q.name}</span>` +
      `<span class="m-custom">${custom || '-'}</span>` +
      `<span class="m-cost">${q.cost != null ? Number(q.cost).toFixed(3) : '-'}</span>` +
      `<span class="m-hold">${q.holdings != null ? q.holdings : '-'}</span>` +
      `<span class="m-ops"><button class="mini-btn" title="Edit">&#9998;</button><button class="mini-btn danger" title="Delete">&#215;</button></span>`;
    row.querySelectorAll('.mini-btn')[0].onclick = () => openEdit(q);
    row.querySelectorAll('.mini-btn')[1].onclick = () => send({ op: 'remove', code: q.code });
    box.appendChild(row);
  });
}
document.getElementById('manage-ok').onclick = () => {
  Object.keys(manageEdits).forEach(code => {
    const e = manageEdits[code];
    send({ op: 'setMeta', code, customName: e.customName, cost: e.cost, holdings: e.holdings });
  });
  closeModal('manage-dialog');
};

// ---- edit dialog ----
let editingCode = null;
function openEdit(q) {
  editingCode = q.code;
  document.getElementById('edit-title').textContent = 'Edit ' + q.code;
  document.getElementById('edit-name').value = q.displayName && q.displayName !== q.name ? q.displayName : '';
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
  if (costText && (isNaN(costVal))) {
    document.getElementById('edit-error').textContent = 'Enter a valid number';
    return;
  }
  if (holdText && (isNaN(holdVal))) {
    document.getElementById('edit-error').textContent = 'Enter a whole number';
    return;
  }
  manageEdits[editingCode] = {
    customName: name || null,
    cost: costVal,
    holdings: holdVal,
  };
  closeModal('edit-dialog');
};

function closeModal(id) { document.getElementById(id).classList.add('hidden'); }
document.addEventListener('keydown', (e) => {
  if (e.key === 'Escape') {
    document.querySelectorAll('.modal:not(.hidden)').forEach(m => m.classList.add('hidden'));
  }
});

// ---- transient toast ----
function flash(msg) {
  let el = document.getElementById('toast');
  if (!el) {
    el = document.createElement('div');
    el.id = 'toast';
    el.style.cssText = 'position:fixed;bottom:12px;left:50%;transform:translateX(-50%);background:var(--bg);border:1px solid var(--border);border-radius:6px;padding:8px 14px;z-index:300;box-shadow:0 2px 8px rgba(0,0,0,.2);font-size:12px;';
    document.body.appendChild(el);
  }
  el.textContent = msg;
  el.style.display = 'block';
  clearTimeout(el._t);
  el._t = setTimeout(() => { el.style.display = 'none'; }, 2500);
}

// ---- boot ----
applyTheme();
renderHead();
renderTable();
send({ op: 'ready' });
setInterval(() => send({ op: 'refresh' }), 5000);
