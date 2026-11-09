// Transaction intelligence page. Loaded as a same-origin script (CSP: script-src 'self').
// Renders KPI tiles and an AG Grid Community table from GET /api/analytics/transactions.
// All cell content is built with DOM APIs (textContent), never innerHTML, so seller-supplied
// strings cannot inject markup.
(function () {
  'use strict';

  var agGrid = window.agGrid;
  var gridEl = document.getElementById('grid');
  var statusEl = document.getElementById('grid-status');
  if (!gridEl) return;
  if (!agGrid || typeof agGrid.createGrid !== 'function') {
    if (statusEl) statusEl.textContent = 'The grid library failed to load. Raw data: /api/analytics/transactions';
    return;
  }

  var nonce = document.body.dataset.nonce || undefined;
  var preset = 'all';
  var highlightViolations = true;
  var gridApi = null;

  if (agGrid.ModuleRegistry && agGrid.AllCommunityModule) {
    try { agGrid.ModuleRegistry.registerModules([agGrid.AllCommunityModule]); } catch (e) { /* already registered */ }
  }

  function money(minor, currency) {
    if (typeof minor !== 'number' || !isFinite(minor)) return '—';
    try {
      return new Intl.NumberFormat('en-US', { style: 'currency', currency: currency || 'USD' }).format(minor / 100);
    } catch (e) {
      return 'USD ' + (minor / 100).toFixed(2);
    }
  }

  function el(tag, cls, text) {
    var node = document.createElement(tag);
    if (cls) node.className = cls;
    if (text !== undefined && text !== null) node.textContent = String(text);
    return node;
  }

  var TONES = {
    captured: 'good', voided: 'bad', declined: 'bad', reconciliation_required: 'warn',
    buyer_review: 'warn', quarantined: 'ai', authorized: 'info', approval_pending: 'info',
    capture_pending: 'info', void_pending: 'info',
    low: 'good', medium: 'warn', high: 'bad',
    approve: 'good', reject: 'bad', escalate: 'warn',
    contract: 'ai', catalog: 'info', legacy: 'neutral'
  };

  function human(s) {
    s = String(s).replace(/[_-]+/g, ' ');
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  function chipRenderer(p) {
    if (p.value === null || p.value === undefined || p.value === '') return el('span', 'muted', '—');
    var v = String(p.value);
    return el('span', 'cell-chip tone-' + (TONES[v] || 'neutral'), human(v));
  }

  function boolRenderer(yes, no, flagClass) {
    return function (p) {
      if (p.value === true) return el('span', flagClass || '', yes);
      if (p.value === false) return el('span', 'muted', no);
      return el('span', 'muted', '—');
    };
  }

  function monoRenderer(p) {
    return el('span', 'cell-mono', p.value || '—');
  }

  function idRenderer(p) {
    var row = p.data || {};
    var label = String(row.id || '').slice(0, 8) || '—';
    if (row.task_id) {
      var a = el('a', 'cell-mono', label);
      a.href = '/agent/' + encodeURIComponent(row.task_id);
      a.title = 'Open agent task ' + row.task_id;
      return a;
    }
    var span = el('span', 'cell-mono', label);
    span.title = String(row.id || '');
    return span;
  }

  function isFlagged(r) {
    return r.injection_detected === true || r.risk === 'high' || r.risk === 'medium' ||
      r.recommended_action === 'reject' || r.recommended_action === 'escalate';
  }
  function isHeld(r) {
    return ['approval_pending', 'authorized', 'quarantined', 'buyer_review', 'capture_pending', 'void_pending']
      .indexOf(r.state) !== -1;
  }
  function isViolation(r) {
    return !!r && (r.risk_override === true || (r.injection_detected === true && r.state === 'captured'));
  }

  var PRESETS = {
    all: function () { return true; },
    flagged: isFlagged,
    captured: function (r) { return r.state === 'captured'; },
    held: isHeld,
    reconciliation: function (r) { return r.state === 'reconciliation_required'; }
  };

  var columnDefs = [
    { headerName: 'Order', field: 'id', cellRenderer: idRenderer, width: 110, pinned: 'left', filter: false },
    {
      headerName: 'Created', field: 'created_at', sort: 'desc', width: 170,
      valueFormatter: function (p) {
        if (!p.value) return '—';
        var d = new Date(p.value);
        return isNaN(d) ? String(p.value) : d.toLocaleString();
      }
    },
    { headerName: 'Item', field: 'item', minWidth: 200, flex: 1 },
    { headerName: 'Seller', field: 'seller', width: 170 },
    {
      headerName: 'Amount', colId: 'amount', field: 'amount_minor', width: 120, type: 'numericColumn',
      filter: 'agNumberColumnFilter',
      valueFormatter: function (p) { return money(p.value, p.data && p.data.currency); }
    },
    { headerName: 'State', field: 'state', cellRenderer: chipRenderer, width: 170 },
    { headerName: 'Mode', field: 'mode', width: 100 },
    { headerName: 'Provider confirmed', field: 'provider_confirmed', cellRenderer: boolRenderer('✓ Confirmed', 'No'), width: 150 },
    { headerName: 'Structural', field: 'structural_passed', cellRenderer: boolRenderer('✓ Passed', '✗ Failed'), width: 120 },
    { headerName: 'AI risk', field: 'risk', cellRenderer: chipRenderer, width: 110 },
    { headerName: 'Injection', field: 'injection_detected', cellRenderer: boolRenderer('⚠ Detected', 'None', 'cell-flag'), width: 120 },
    { headerName: 'AI recommends', field: 'recommended_action', cellRenderer: chipRenderer, width: 140 },
    { headerName: 'Buyer decision', field: 'review_decision', cellRenderer: chipRenderer, width: 140 },
    { headerName: 'Risk override', field: 'risk_override', cellRenderer: boolRenderer('Overridden', 'No', 'cell-flag'), width: 130 },
    { headerName: 'Source', field: 'kind', cellRenderer: chipRenderer, width: 110 },
    { headerName: 'PayPal order', field: 'paypal_order', cellRenderer: monoRenderer, width: 190 },
    { headerName: 'Authorization', field: 'authorization_id', cellRenderer: monoRenderer, width: 190 }
  ];

  var options = {
    columnDefs: columnDefs,
    rowData: [],
    defaultColDef: { sortable: true, filter: true, resizable: true, minWidth: 90 },
    getRowId: function (p) { return String(p.data.id); },
    pagination: true,
    paginationPageSize: 25,
    paginationPageSizeSelector: [25, 50, 100],
    animateRows: !window.matchMedia('(prefers-reduced-motion: reduce)').matches,
    rowClassRules: {
      'row-violation': function (p) { return highlightViolations && isViolation(p.data); }
    },
    isExternalFilterPresent: function () { return preset !== 'all'; },
    doesExternalFilterPass: function (node) { return !!node.data && (PRESETS[preset] || PRESETS.all)(node.data); },
    overlayNoRowsTemplate: '<span>No transactions match.</span>',
    styleNonce: nonce
  };
  if (agGrid.themeQuartz) {
    options.theme = agGrid.themeQuartz.withParams({
      accentColor: '#087f5b',
      fontFamily: 'inherit',
      fontSize: 13,
      headerBackgroundColor: '#f7faf8',
      headerFontWeight: 600,
      borderColor: '#e5e5e5',
      rowHoverColor: '#f2faf6',
      selectedRowBackgroundColor: '#edf8f2',
      wrapperBorderRadius: 10
    });
  }
  gridApi = agGrid.createGrid(gridEl, options);

  function setText(id, value) {
    var node = document.getElementById(id);
    if (node) node.textContent = value;
  }

  function renderSummary(s, rows) {
    s = s || {};
    setText('kpi-authorized', money(s.authorized_minor || 0));
    setText('kpi-orders', (s.orders != null ? s.orders : rows.length) + ' orders in total');
    setText('kpi-captured', money(s.captured_minor || 0));
    setText('kpi-held', money(s.held_minor || 0));
    setText('kpi-flagged', String(s.flagged_count || 0));
    setText('kpi-blocked', (s.blocked_count || 0) + ' blocked · ' + (s.voided_count || 0) + ' voided');
    var violations = rows.filter(isViolation).length;
    var summary = document.getElementById('violations-summary');
    if (summary) {
      summary.textContent = violations === 0 ? 'No policy violations' : violations + ' policy violation' + (violations === 1 ? '' : 's');
      summary.className = 'chip ' + (violations === 0 ? 'tone-good' : 'tone-warn');
    }
  }

  function updateStatus() {
    if (!statusEl || !gridApi) return;
    var shown = gridApi.getDisplayedRowCount();
    var total = 0;
    gridApi.forEachNode(function () { total += 1; });
    statusEl.textContent = 'Showing ' + shown + ' of ' + total + ' transactions · updated ' + new Date().toLocaleTimeString();
  }

  function load() {
    return fetch('/api/analytics/transactions', { credentials: 'same-origin', headers: { Accept: 'application/json' } })
      .then(function (res) {
        if (res.status === 401 || res.status === 403) throw new Error('Sign in to view transaction analytics.');
        if (!res.ok) throw new Error('Analytics request failed (HTTP ' + res.status + ').');
        return res.json();
      })
      .then(function (data) {
        var rows = Array.isArray(data && data.rows) ? data.rows : [];
        gridApi.setGridOption('rowData', rows);
        renderSummary(data && data.summary, rows);
        updateStatus();
      })
      .catch(function (err) {
        if (statusEl) statusEl.textContent = err && err.message ? err.message : 'Could not load transactions.';
      });
  }

  var quick = document.getElementById('quick-filter');
  if (quick) {
    quick.addEventListener('input', function () {
      gridApi.setGridOption('quickFilterText', quick.value);
      updateStatus();
    });
  }

  Array.prototype.forEach.call(document.querySelectorAll('[data-preset]'), function (btn) {
    btn.addEventListener('click', function () {
      preset = btn.getAttribute('data-preset') || 'all';
      Array.prototype.forEach.call(document.querySelectorAll('[data-preset]'), function (other) {
        var on = other === btn;
        other.classList.toggle('active', on);
        other.setAttribute('aria-pressed', on ? 'true' : 'false');
      });
      gridApi.onFilterChanged();
      updateStatus();
    });
  });

  var toggle = document.getElementById('violations-toggle');
  if (toggle) {
    toggle.addEventListener('change', function () {
      highlightViolations = toggle.checked;
      gridApi.redrawRows();
    });
  }

  load();
  // Refresh in the background; the grid keeps sort, filter and scroll state across updates.
  setInterval(function () {
    if (document.visibilityState === 'visible') load();
  }, 20000);
})();
