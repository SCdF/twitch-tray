// Settings page JavaScript
const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;

// Streamer mode detection
const urlParams = new URLSearchParams(window.location.search);
const streamerParam = urlParams.get('streamer');

// State
let config = null;
let searchTimeout = null;
let followedChannels = [];
let selectedStreamer = null;
let streamerSearchTimeout = null;

// DOM Elements
const tabs = document.querySelectorAll('.tab');
const panes = document.querySelectorAll('.pane');
const pollIntervalInput = document.getElementById('poll_interval');
const notifyMaxGapInput = document.getElementById('notify_max_gap');
const scheduleLookaheadInput = document.getElementById('schedule_lookahead');
const notifyOnLiveInput = document.getElementById('notify_on_live');
const notifyOnCategoryInput = document.getElementById('notify_on_category');
const liveMenuLimitInput = document.getElementById('live_menu_limit');
const scheduleMenuLimitInput = document.getElementById('schedule_menu_limit');
const categorySearchInput = document.getElementById('category_search');
const searchResultsDiv = document.getElementById('search_results');
const categoryListDiv = document.getElementById('category_list');
const streamerSearchInput = document.getElementById('streamer_search');
const streamerSearchResultsDiv = document.getElementById('streamer_search_results');
const streamerListDiv = document.getElementById('streamer_list');
const streamerDetailDiv = document.getElementById('streamer_detail');
const closeBtn = document.getElementById('close_btn');

// === Debug tab state ===
const WEEK_SECS = 7 * 24 * 3600;
let debugWindowStart = Math.floor(Date.now() / 1000) - 3 * WEEK_SECS;
let debugWindowEnd = Math.floor(Date.now() / 1000) + 86400;
let debugAllEntries = [];
let debugFilter = '';
let debugLoading = false;
let debugDataLoaded = false;

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
  await loadConfig();

  closeBtn.addEventListener('click', async () => {
    await getCurrentWindow().close();
  });

  if (streamerParam) {
    enterStreamerMode(streamerParam);
  } else {
    await loadFollowedChannels();
    setupEventListeners();

    // Show debug tab in debug builds
    try {
      const isDebug = await invoke('is_debug_build');
      if (isDebug) {
        document.getElementById('tab-debug').style.display = '';
      }
    } catch (e) {
      console.error('Failed to check debug build:', e);
    }
  }
});

async function loadConfig() {
  try {
    config = await invoke('get_config');
    if (!config.streamer_settings) {
      config.streamer_settings = {};
    }
    populateForm();
  } catch (error) {
    console.error('Failed to load config:', error);
  }
}

async function loadFollowedChannels() {
  try {
    followedChannels = await invoke('get_followed_channels_list');
  } catch (error) {
    console.error('Failed to load followed channels:', error);
    followedChannels = [];
  }
}

function populateForm() {
  if (!config) return;

  pollIntervalInput.value = config.poll_interval_sec;
  notifyMaxGapInput.value = config.notify_max_gap_min;
  notifyOnLiveInput.checked = config.notify_on_live;
  notifyOnCategoryInput.checked = config.notify_on_category;
  scheduleLookaheadInput.value = config.schedule_lookahead_hours;
  liveMenuLimitInput.value = config.live_menu_limit;
  scheduleMenuLimitInput.value = config.schedule_menu_limit;

  renderCategoryList();
  renderStreamerList();
}

function renderCategoryList() {
  if (!config || !config.followed_categories) {
    categoryListDiv.innerHTML = '<div class="empty-state">No categories added yet</div>';
    return;
  }

  if (config.followed_categories.length === 0) {
    categoryListDiv.innerHTML = '<div class="empty-state">No categories added yet</div>';
    return;
  }

  categoryListDiv.innerHTML = config.followed_categories.map(cat => `
    <div class="category-item" data-id="${cat.id}">
      <span class="category-name">${escapeHtml(cat.name)}</span>
      <button class="category-remove" onclick="removeCategory('${cat.id}')">Remove</button>
    </div>
  `).join('');
}

// === Streamer Settings ===

function importanceIcon(importance) {
  switch (importance) {
    case 'favourite': return '\u2B50 ';
    case 'silent': return '\uD83D\uDD15 ';
    case 'ignore': return '\uD83D\uDEAB ';
    default: return '';
  }
}

function renderStreamerList() {
  const settings = config?.streamer_settings || {};
  const logins = Object.keys(settings).sort((a, b) =>
    settings[a].display_name.localeCompare(settings[b].display_name)
  );

  if (logins.length === 0) {
    streamerListDiv.innerHTML = '<div class="empty-state">No streamers configured</div>';
    renderStreamerDetail();
    return;
  }

  streamerListDiv.innerHTML = logins.map(login => {
    const s = settings[login];
    const selectedClass = selectedStreamer === login ? ' selected' : '';
    return `
      <div class="streamer-item${selectedClass}" data-login="${escapeHtml(login)}" onclick="selectStreamer('${escapeHtml(login)}')">
        <span class="streamer-item-name">${importanceIcon(s.importance)}${escapeHtml(s.display_name)}</span>
        <button class="streamer-item-remove" onclick="event.stopPropagation(); removeStreamer('${escapeHtml(login)}')">Remove</button>
      </div>
    `;
  }).join('');

  renderStreamerDetail();
}

function renderStreamerDetailInto(container) {
  if (!selectedStreamer || !config.streamer_settings[selectedStreamer]) {
    return false;
  }

  const s = config.streamer_settings[selectedStreamer];
  const importanceOptions = ['favourite', 'normal', 'silent', 'ignore'];
  const importanceLabels = {
    favourite: 'Favourite - Star prefix, sorted first',
    normal: 'Normal - Default behavior',
    silent: 'Silent - No notifications',
    ignore: 'Ignore - Hidden from menu',
  };

  container.innerHTML = `
    <div class="detail-header">${importanceIcon(s.importance)}${escapeHtml(s.display_name)}</div>
    <div class="detail-field">
      <label for="streamer_importance">Importance</label>
      <select id="streamer_importance" onchange="updateStreamerImportance(this.value)">
        ${importanceOptions.map(opt => `
          <option value="${opt}" ${s.importance === opt ? 'selected' : ''}>${importanceLabels[opt]}</option>
        `).join('')}
      </select>
    </div>
  `;
  return true;
}

function renderStreamerDetail() {
  if (!selectedStreamer || !config.streamer_settings[selectedStreamer]) {
    selectedStreamer = null;
    streamerDetailDiv.innerHTML = '<div class="empty-detail-state">Select a streamer to configure</div>';
    return;
  }

  renderStreamerDetailInto(streamerDetailDiv);
}

function enterStreamerMode(login) {
  // Hide tabs nav
  const tabsEl = document.querySelector('.tabs');
  if (tabsEl) tabsEl.style.display = 'none';

  // Replace content with streamer-mode container
  const contentEl = document.querySelector('.content');
  contentEl.innerHTML = `
    <div class="streamer-mode-container">
      <div class="streamer-mode-detail" id="streamer_detail_mode"></div>
    </div>
  `;

  // Auto-add streamer if missing (safety net)
  if (!config.streamer_settings[login]) {
    config.streamer_settings[login] = {
      display_name: login,
      importance: 'normal'
    };
  }

  selectedStreamer = login;
  const container = document.getElementById('streamer_detail_mode');
  renderStreamerDetailInto(container);
}

function selectStreamer(login) {
  selectedStreamer = login;
  renderStreamerList();
}

function addStreamer(login, displayName) {
  if (!config.streamer_settings) {
    config.streamer_settings = {};
  }

  if (config.streamer_settings[login]) {
    // Already exists, just select it
    selectedStreamer = login;
    renderStreamerList();
    return;
  }

  config.streamer_settings[login] = {
    display_name: displayName,
    importance: 'normal'
  };

  selectedStreamer = login;
  renderStreamerList();
  autoSave();

  // Clear search
  streamerSearchInput.value = '';
  streamerSearchResultsDiv.classList.remove('visible');
}

function removeStreamer(login) {
  if (!config.streamer_settings) return;

  delete config.streamer_settings[login];

  if (selectedStreamer === login) {
    selectedStreamer = null;
  }

  renderStreamerList();
  autoSave();
}

function updateStreamerImportance(value) {
  if (!selectedStreamer || !config.streamer_settings[selectedStreamer]) return;
  config.streamer_settings[selectedStreamer].importance = value;
  if (streamerParam) {
    const container = document.getElementById('streamer_detail_mode');
    if (container) renderStreamerDetailInto(container);
  } else {
    renderStreamerList();
  }
  autoSave();
}

function searchStreamers(query) {
  const lowerQuery = query.toLowerCase();
  const configuredLogins = new Set(Object.keys(config?.streamer_settings || {}));

  const results = followedChannels.filter(ch => {
    if (configuredLogins.has(ch.broadcaster_login)) return false;
    return ch.broadcaster_name.toLowerCase().includes(lowerQuery) ||
           ch.broadcaster_login.toLowerCase().includes(lowerQuery);
  });

  if (results.length === 0) {
    streamerSearchResultsDiv.innerHTML = '<div class="search-result-item">No results found</div>';
    streamerSearchResultsDiv.classList.add('visible');
    return;
  }

  // Limit to top 10
  const shown = results.slice(0, 10);
  streamerSearchResultsDiv.innerHTML = shown.map(ch => `
    <div class="search-result-item" onclick="addStreamer('${escapeHtml(ch.broadcaster_login)}', '${escapeHtml(ch.broadcaster_name)}')">
      ${escapeHtml(ch.broadcaster_name)}
    </div>
  `).join('');
  streamerSearchResultsDiv.classList.add('visible');
}

function setupEventListeners() {
  // Tab switching
  tabs.forEach(tab => {
    tab.addEventListener('click', async () => {
      const targetId = tab.dataset.tab;

      tabs.forEach(t => t.classList.remove('active'));
      panes.forEach(p => p.classList.remove('active'));

      tab.classList.add('active');
      document.getElementById(targetId).classList.add('active');

      // Load initial debug data on first open, then scroll to now
      if (targetId === 'debug' && !debugDataLoaded) {
        debugDataLoaded = true;
        await loadDebugChunk(debugWindowStart, debugWindowEnd);
        scrollToNow();
      }
    });
  });

  // Category search with debounce
  categorySearchInput.addEventListener('input', (e) => {
    const query = e.target.value.trim();

    if (searchTimeout) {
      clearTimeout(searchTimeout);
    }

    if (query.length < 2) {
      searchResultsDiv.classList.remove('visible');
      return;
    }

    searchTimeout = setTimeout(() => searchCategories(query), 300);
  });

  // Streamer search with debounce (client-side filtering)
  streamerSearchInput.addEventListener('input', (e) => {
    const query = e.target.value.trim();

    if (streamerSearchTimeout) {
      clearTimeout(streamerSearchTimeout);
    }

    if (query.length < 2) {
      streamerSearchResultsDiv.classList.remove('visible');
      return;
    }

    streamerSearchTimeout = setTimeout(() => searchStreamers(query), 150);
  });

  // Close search results when clicking outside
  document.addEventListener('click', (e) => {
    if (!e.target.closest('.search-container')) {
      searchResultsDiv.classList.remove('visible');
      streamerSearchResultsDiv.classList.remove('visible');
    }
  });

  // Auto-save on general settings changes
  [pollIntervalInput, notifyMaxGapInput, scheduleLookaheadInput, liveMenuLimitInput, scheduleMenuLimitInput].forEach(input => {
    input.addEventListener('change', () => autoSave());
  });
  [notifyOnLiveInput, notifyOnCategoryInput].forEach(input => {
    input.addEventListener('change', () => autoSave());
  });
}

async function searchCategories(query) {
  try {
    const results = await invoke('search_categories', { query });
    displaySearchResults(results);
  } catch (error) {
    console.error('Search failed:', error);
    searchResultsDiv.innerHTML = '<div class="search-result-item">Search failed</div>';
    searchResultsDiv.classList.add('visible');
  }
}

function displaySearchResults(results) {
  if (!results || results.length === 0) {
    searchResultsDiv.innerHTML = '<div class="search-result-item">No results found</div>';
    searchResultsDiv.classList.add('visible');
    return;
  }

  // Filter out already followed categories
  const followedIds = new Set((config?.followed_categories || []).map(c => c.id));
  const filtered = results.filter(r => !followedIds.has(r.id));

  if (filtered.length === 0) {
    searchResultsDiv.innerHTML = '<div class="search-result-item">All results already added</div>';
    searchResultsDiv.classList.add('visible');
    return;
  }

  searchResultsDiv.innerHTML = filtered.map(cat => `
    <div class="search-result-item" onclick="addCategory('${cat.id}', '${escapeHtml(cat.name)}')">
      ${escapeHtml(cat.name)}
    </div>
  `).join('');
  searchResultsDiv.classList.add('visible');
}

function addCategory(id, name) {
  if (!config.followed_categories) {
    config.followed_categories = [];
  }

  // Check if already exists
  if (config.followed_categories.some(c => c.id === id)) {
    return;
  }

  config.followed_categories.push({ id, name });
  renderCategoryList();
  autoSave();

  // Clear search
  categorySearchInput.value = '';
  searchResultsDiv.classList.remove('visible');
}

function removeCategory(id) {
  if (!config.followed_categories) return;

  config.followed_categories = config.followed_categories.filter(c => c.id !== id);
  renderCategoryList();
  autoSave();
}

async function autoSave() {
  try {
    if (streamerParam) {
      // Streamer mode: re-fetch current config and merge only this streamer's settings
      const currentConfig = await invoke('get_config');
      if (!currentConfig.streamer_settings) {
        currentConfig.streamer_settings = {};
      }
      currentConfig.streamer_settings[streamerParam] = config.streamer_settings[streamerParam];
      await invoke('save_config', { config: currentConfig });
    } else {
      // Full settings mode
      const newConfig = {
        poll_interval_sec: parseInt(pollIntervalInput.value, 10) || 60,
        notify_max_gap_min: parseInt(notifyMaxGapInput.value, 10) || 10,
        notify_on_live: notifyOnLiveInput.checked,
        notify_on_category: notifyOnCategoryInput.checked,
        schedule_lookahead_hours: parseInt(scheduleLookaheadInput.value, 10) || 6,
        live_menu_limit: parseInt(liveMenuLimitInput.value, 10) || 10,
        schedule_menu_limit: parseInt(scheduleMenuLimitInput.value, 10) || 5,
        followed_categories: config.followed_categories || [],
        streamer_settings: config.streamer_settings || {}
      };

      // Validate
      newConfig.poll_interval_sec = Math.max(30, Math.min(300, newConfig.poll_interval_sec));
      newConfig.notify_max_gap_min = Math.max(1, Math.min(60, newConfig.notify_max_gap_min));
      newConfig.schedule_lookahead_hours = Math.max(1, Math.min(72, newConfig.schedule_lookahead_hours));
      newConfig.live_menu_limit = Math.max(1, Math.min(50, newConfig.live_menu_limit));
      newConfig.schedule_menu_limit = Math.max(1, Math.min(20, newConfig.schedule_menu_limit));

      await invoke('save_config', { config: newConfig });
    }
  } catch (error) {
    console.error('Failed to auto-save config:', error);
  }
}

function escapeHtml(text) {
  const div = document.createElement('div');
  div.textContent = text;
  return div.innerHTML;
}

// Make functions available globally for onclick handlers
window.addCategory = addCategory;
window.removeCategory = removeCategory;
window.selectStreamer = selectStreamer;
window.addStreamer = addStreamer;
window.removeStreamer = removeStreamer;
window.updateStreamerImportance = updateStreamerImportance;

// === Debug tab functions ===

function debounce(fn, delayMs) {
  let timer = null;
  return (...args) => {
    clearTimeout(timer);
    timer = setTimeout(() => fn(...args), delayMs);
  };
}

async function loadDebugChunk(start, end) {
  try {
    const chunk = await invoke('get_debug_schedule_data', { start, end });

    // Build a dedup key set from existing entries
    const seen = new Set(
      debugAllEntries.map(e => `${e.is_inferred}|${e.broadcaster_login}|${e.started_at}`)
    );

    for (const entry of chunk) {
      const key = `${entry.is_inferred}|${entry.broadcaster_login}|${entry.started_at}`;
      if (!seen.has(key)) {
        seen.add(key);
        debugAllEntries.push(entry);
      }
    }

    // Keep sorted by started_at
    debugAllEntries.sort((a, b) => a.started_at - b.started_at);

    renderDebugTable();
  } catch (e) {
    console.error('Failed to load debug data:', e);
  }
}

function formatDebugRow(entry) {
  const nowSecs = Date.now() / 1000;

  // Week number (7-day periods relative to now; negative = past, positive = future)
  const weekN = Math.floor((entry.started_at - nowSecs) / WEEK_SECS);

  // Wall-clock date (YYYY-MM-DD local), time (HH:MM local), and day abbreviation
  const d = new Date(entry.started_at * 1000);
  const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
  const day = ['Sun', 'Mon', 'Tue', 'Wed', 'Thur', 'Fri', 'Sat'][d.getDay()];

  const icon = entry.is_inferred ? '\u2728' : '';

  return { icon, name: entry.broadcaster_name, date, time, day, weekN };
}

function renderDebugTable() {
  const tbody = document.getElementById('debug-tbody');
  if (!tbody) return;

  const lowerFilter = debugFilter.toLowerCase();
  const filtered = lowerFilter
    ? debugAllEntries.filter(
        e =>
          e.broadcaster_name.toLowerCase().includes(lowerFilter) ||
          e.broadcaster_login.toLowerCase().includes(lowerFilter)
      )
    : debugAllEntries;

  tbody.innerHTML = filtered
    .map(entry => {
      const { icon, name, date, time, day, weekN } = formatDebugRow(entry);
      return `<tr>
        <td>${icon}</td>
        <td>${escapeHtml(name)}</td>
        <td>${date}</td>
        <td>${time}</td>
        <td>${day}</td>
        <td>${weekN >= 0 ? '+' : ''}${weekN}</td>
      </tr>`;
    })
    .join('');
}

function scrollToNow() {
  const nowSecs = Date.now() / 1000;
  const container = document.getElementById('debug-table-container');
  const tbody = document.getElementById('debug-tbody');
  if (!tbody || !container) return;

  const rows = tbody.querySelectorAll('tr');
  if (rows.length === 0) return;

  const lowerFilter = debugFilter.toLowerCase();
  const filtered = lowerFilter
    ? debugAllEntries.filter(
        e =>
          e.broadcaster_name.toLowerCase().includes(lowerFilter) ||
          e.broadcaster_login.toLowerCase().includes(lowerFilter)
      )
    : debugAllEntries;

  // First row at or after now; fall back to last row if all entries are in the past
  const firstFutureIdx = filtered.findIndex(e => e.started_at >= nowSecs);
  const targetIdx = firstFutureIdx >= 0 ? firstFutureIdx : rows.length - 1;
  const targetRow = rows[targetIdx];
  if (!targetRow) return;

  // Use getBoundingClientRect so the calculation works regardless of offsetParent chain
  const containerRect = container.getBoundingClientRect();
  const rowRect = targetRow.getBoundingClientRect();
  const rowOffsetInContainer = rowRect.top - containerRect.top + container.scrollTop;
  container.scrollTop = Math.max(0, rowOffsetInContainer - container.clientHeight / 3);
}

// Set up debug filter and scroll handlers once the DOM is ready
document.addEventListener('DOMContentLoaded', () => {
  const filterInput = document.getElementById('debug-filter');
  if (filterInput) {
    filterInput.addEventListener('input', e => {
      debugFilter = e.target.value;
      renderDebugTable();
    });
  }

  const container = document.getElementById('debug-table-container');
  if (container) {
    container.addEventListener(
      'scroll',
      debounce(async () => {
        if (debugLoading) return;
        debugLoading = true;
        try {
          if (container.scrollTop < 200) {
            const newStart = debugWindowStart - 86400;
            const prevHeight = container.scrollHeight;
            await loadDebugChunk(newStart, debugWindowStart);
            container.scrollTop += container.scrollHeight - prevHeight;
            debugWindowStart = newStart;
          } else if (
            container.scrollTop + container.clientHeight >
            container.scrollHeight - 200
          ) {
            const newEnd = debugWindowEnd + 86400;
            await loadDebugChunk(debugWindowEnd, newEnd);
            debugWindowEnd = newEnd;
          }
        } finally {
          debugLoading = false;
        }
      }, 150)
    );
  }
});
