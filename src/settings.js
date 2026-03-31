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
const notifyOnTitleInput = document.getElementById('notify_on_title');
const notifyOnHotInput = document.getElementById('notify_on_hot');
const hotnessMaxStreamAgeInput = document.getElementById('hotness_max_stream_age_min');
const hotnessZThresholdInput = document.getElementById('hotness_z_threshold');
const hotnessZCoolThresholdInput = document.getElementById('hotness_z_cool_threshold');
const hotnessMinObservationsInput = document.getElementById('hotness_min_observations');
const hotnessMinStreamsInput = document.getElementById('hotness_min_streams');
const hotnessLookbackDaysInput = document.getElementById('hotness_lookback_days');
const hotnessAgeWindowDivisorInput = document.getElementById('hotness_age_window_divisor');
const liveMenuLimitInput = document.getElementById('live_menu_limit');
const alwaysShowFavouritesInput = document.getElementById('always_show_favourites');
const alwaysShowHotInput = document.getElementById('always_show_hot');
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
let debugHotnessLoaded = false;
let debugProfilesFilter = '';
let debugAllProfiles = [];
let debugScheduleLoaded = false;

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
  notifyOnTitleInput.checked = config.notify_on_title !== false;
  notifyOnHotInput.checked = config.notify_on_hot;
  hotnessMaxStreamAgeInput.value = config.hotness_max_stream_age_min;
  hotnessZThresholdInput.value = config.hotness_z_threshold;
  hotnessZCoolThresholdInput.value = config.hotness_z_cool_threshold;
  hotnessMinObservationsInput.value = config.hotness_min_observations;
  hotnessMinStreamsInput.value = config.hotness_min_streams;
  hotnessLookbackDaysInput.value = config.hotness_lookback_days;
  hotnessAgeWindowDivisorInput.value = config.hotness_age_window_divisor;
  scheduleLookaheadInput.value = config.schedule_lookahead_hours;
  liveMenuLimitInput.value = config.live_menu_limit;
  alwaysShowFavouritesInput.checked = config.always_show_favourites;
  alwaysShowHotInput.checked = config.always_show_hot;
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

  const overrideValue = s.hotness_z_threshold_override != null ? s.hotness_z_threshold_override : '';
  const globalThreshold = config.hotness_z_threshold || 2.0;

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
    <div class="detail-field" style="margin-top: 16px;">
      <label for="streamer_hotness_override">Hotness Z-Score Override</label>
      <input type="number" id="streamer_hotness_override" min="0.5" max="5.0" step="0.1"
        value="${overrideValue}" placeholder="${globalThreshold} (global default)"
        onchange="updateStreamerHotnessOverride(this.value)">
      <span class="help-text">Leave empty to use the global threshold. Lower = more sensitive.</span>
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

function updateStreamerHotnessOverride(value) {
  if (!selectedStreamer || !config.streamer_settings[selectedStreamer]) return;
  const trimmed = value.trim();
  if (trimmed === '') {
    config.streamer_settings[selectedStreamer].hotness_z_threshold_override = null;
  } else {
    const parsed = parseFloat(trimmed);
    if (!isNaN(parsed)) {
      config.streamer_settings[selectedStreamer].hotness_z_threshold_override = Math.max(0.5, Math.min(5.0, parsed));
    }
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

      // Load hotness debug data on first open of the debug tab
      if (targetId === 'debug' && !debugHotnessLoaded) {
        debugHotnessLoaded = true;
        await loadDebugHotnessProfiles();
        await loadDebugHotness();
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
  [pollIntervalInput, notifyMaxGapInput, scheduleLookaheadInput, liveMenuLimitInput, scheduleMenuLimitInput, hotnessMaxStreamAgeInput, hotnessZThresholdInput, hotnessMinObservationsInput, hotnessMinStreamsInput, hotnessLookbackDaysInput, hotnessAgeWindowDivisorInput].forEach(input => {
    input.addEventListener('change', () => autoSave());
  });
  [notifyOnLiveInput, notifyOnCategoryInput, notifyOnTitleInput, notifyOnHotInput, alwaysShowFavouritesInput, alwaysShowHotInput].forEach(input => {
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
        notify_on_title: notifyOnTitleInput.checked,
        notify_on_hot: notifyOnHotInput.checked,
        hotness_max_stream_age_min: parseInt(hotnessMaxStreamAgeInput.value, 10) || 0,
        hotness_z_threshold: parseFloat(hotnessZThresholdInput.value) || 2.0,
        hotness_z_cool_threshold: parseFloat(hotnessZCoolThresholdInput.value) || 1.0,
        hotness_min_observations: parseInt(hotnessMinObservationsInput.value, 10) || 5,
        hotness_min_streams: parseInt(hotnessMinStreamsInput.value, 10) || 7,
        hotness_lookback_days: parseInt(hotnessLookbackDaysInput.value, 10) || 30,
        hotness_age_window_divisor: parseInt(hotnessAgeWindowDivisorInput.value, 10) || 2,
        schedule_lookahead_hours: parseInt(scheduleLookaheadInput.value, 10) || 6,
        live_menu_limit: parseInt(liveMenuLimitInput.value, 10) || 10,
        always_show_favourites: alwaysShowFavouritesInput.checked,
        always_show_hot: alwaysShowHotInput.checked,
        schedule_menu_limit: parseInt(scheduleMenuLimitInput.value, 10) || 5,
        followed_categories: config.followed_categories || [],
        streamer_settings: config.streamer_settings || {}
      };

      // Validate
      newConfig.poll_interval_sec = Math.max(30, Math.min(300, newConfig.poll_interval_sec));
      newConfig.notify_max_gap_min = Math.max(1, Math.min(60, newConfig.notify_max_gap_min));
      newConfig.hotness_max_stream_age_min = Math.max(0, Math.min(600, newConfig.hotness_max_stream_age_min));
      newConfig.hotness_z_threshold = Math.max(0.5, Math.min(5.0, newConfig.hotness_z_threshold));
      newConfig.hotness_z_cool_threshold = Math.max(0.0, Math.min(newConfig.hotness_z_threshold, newConfig.hotness_z_cool_threshold));
      newConfig.hotness_min_observations = Math.max(1, Math.min(50, newConfig.hotness_min_observations));
      newConfig.hotness_min_streams = Math.max(1, Math.min(30, newConfig.hotness_min_streams));
      newConfig.hotness_lookback_days = Math.max(7, Math.min(90, newConfig.hotness_lookback_days));
      newConfig.hotness_age_window_divisor = Math.max(1, Math.min(10, newConfig.hotness_age_window_divisor));
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
window.updateStreamerHotnessOverride = updateStreamerHotnessOverride;

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

// === Debug hotness functions ===

async function loadDebugHotness() {
  try {
    const entries = await invoke('get_debug_hotness_data');
    renderDebugHotnessTable(entries);
  } catch (e) {
    console.error('Failed to load hotness data:', e);
  }
}

function renderDebugHotnessTable(entries) {
  const tbody = document.getElementById('debug-hotness-tbody');
  if (!tbody) return;

  if (entries.length === 0) {
    tbody.innerHTML = '<tr><td colspan="11" style="text-align:center;color:#808080;padding:20px">No live streams with hotness data</td></tr>';
    return;
  }

  tbody.innerHTML = entries.map(e => {
    const window = e.age_min != null ? `${e.age_min}\u2013${e.age_max}m` : '\u2014';
    const mean = e.mean != null ? e.mean.toFixed(0) : '\u2014';
    const stddev = e.stddev != null ? e.stddev.toFixed(0) : '\u2014';
    const zScore = e.z_score != null ? e.z_score.toFixed(2) : '\u2014';
    const hotClass = e.is_hot ? ' class="debug-hot-row"' : '';
    const hotIcon = e.is_hot ? '\uD83D\uDD25' : '';
    return `<tr${hotClass}>
      <td>${escapeHtml(e.broadcaster_name)}</td>
      <td>${e.current_viewers.toLocaleString()}</td>
      <td>${window}</td>
      <td>${e.window_observations}</td>
      <td>${e.window_distinct_streams}</td>
      <td>${mean}</td>
      <td>${stddev}</td>
      <td>${zScore}</td>
      <td>${e.observation_count}</td>
      <td>${e.distinct_streams}</td>
      <td>${hotIcon}</td>
    </tr>`;
  }).join('');
}

// === Debug hotness profiles ===

const BUCKET_AGE_LABELS = {
  0: '0m', 5: '5m', 10: '10m', 15: '15m', 30: '30m', 45: '45m',
  60: '1h', 90: '1.5h', 120: '2h', 180: '3h', 240: '4h', 360: '6h'
};

async function loadDebugHotnessProfiles() {
  try {
    debugAllProfiles = await invoke('get_debug_hotness_profiles');
    renderDebugProfilesTable();
  } catch (e) {
    console.error('Failed to load hotness profiles:', e);
  }
}

function renderDebugProfilesTable() {
  const thead = document.getElementById('debug-profiles-thead');
  const tbody = document.getElementById('debug-profiles-tbody');
  if (!thead || !tbody) return;

  if (debugAllProfiles.length === 0) {
    thead.innerHTML = '';
    tbody.innerHTML = '<tr><td colspan="1" style="text-align:center;color:#808080;padding:20px">No observation data yet</td></tr>';
    return;
  }

  // Sort alphabetically by streamer name
  const sorted = [...debugAllProfiles].sort((a, b) =>
    a.broadcaster_name.localeCompare(b.broadcaster_name, undefined, { sensitivity: 'base' })
  );

  // Filter by streamer name
  const lowerFilter = debugProfilesFilter.toLowerCase();
  const profiles = lowerFilter
    ? sorted.filter(p => p.broadcaster_name.toLowerCase().includes(lowerFilter))
    : sorted;

  if (profiles.length === 0) {
    thead.innerHTML = '';
    tbody.innerHTML = '<tr><td colspan="1" style="text-align:center;color:#808080;padding:20px">No matches</td></tr>';
    return;
  }

  // Collect all age points from first profile (they're all the same)
  const agePoints = profiles[0].buckets.map(b => b.age_point);
  const minObs = config ? config.hotness_min_observations : 5;
  const minStreams = config ? config.hotness_min_streams : 7;

  // Header row
  thead.innerHTML = `<tr>
    <th>Streamer</th>
    ${agePoints.map(a => `<th>${BUCKET_AGE_LABELS[a] || a + 'm'}</th>`).join('')}
  </tr>`;

  // Body rows
  tbody.innerHTML = profiles.map(p => {
    const bucketCells = p.buckets.map(b => {
      const isEmpty = b.count === 0;
      const hasSufficientData = b.count >= minObs && b.distinct_streams >= minStreams;
      const isActiveBucket = p.is_live && p.current_bucket_age === b.age_point;

      if (isEmpty) {
        return `<td class="bucket-cell bucket-empty">\u2014</td>`;
      }

      const meanStr = b.mean.toFixed(0);
      const stdStr = b.stddev.toFixed(0);
      const cellText = `${meanStr}\u00B1${stdStr}`;

      const classes = ['bucket-cell'];
      if (hasSufficientData) {
        classes.push('bucket-bold');
      } else {
        classes.push('bucket-dim');
      }
      if (isActiveBucket) {
        classes.push('bucket-active');
      }

      // Tooltip data attributes
      const zScore = (isActiveBucket && p.current_viewers != null && b.stddev > 0)
        ? ((p.current_viewers - b.mean) / b.stddev).toFixed(2)
        : null;

      const tooltipLines = [
        `Age bucket: ${BUCKET_AGE_LABELS[b.age_point] || b.age_point + 'm'}`,
        `Mean: ${b.mean.toFixed(1)}`,
        `StdDev: ${b.stddev.toFixed(1)}`,
        `Observations: ${b.count}`,
        `Distinct streams: ${b.distinct_streams}`,
        `Sufficient data: ${hasSufficientData ? 'Yes' : 'No (need ' + minObs + ' obs, ' + minStreams + ' streams)'}`,
      ];
      if (isActiveBucket && p.current_viewers != null) {
        tooltipLines.push(`Current viewers: ${p.current_viewers.toLocaleString()}`);
        if (zScore != null) {
          tooltipLines.push(`Z-score: ${zScore}\u03C3`);
        }
      }

      return `<td class="${classes.join(' ')}"
        data-tooltip="${escapeHtml(tooltipLines.join('\n'))}"
        onmouseenter="showBucketTooltip(event)" onmouseleave="hideBucketTooltip()">${cellText}</td>`;
    }).join('');

    const namePrefix = p.is_live ? '\u{1F534} ' : '';
    return `<tr>${'<td>' + namePrefix + escapeHtml(p.broadcaster_name) + '</td>'}${bucketCells}</tr>`;
  }).join('');
}

let tooltipEl = null;

function showBucketTooltip(event) {
  hideBucketTooltip();
  const text = event.target.dataset.tooltip;
  if (!text) return;

  tooltipEl = document.createElement('div');
  tooltipEl.className = 'bucket-tooltip';
  tooltipEl.textContent = text;
  document.body.appendChild(tooltipEl);

  const rect = event.target.getBoundingClientRect();
  tooltipEl.style.left = rect.left + 'px';
  tooltipEl.style.top = (rect.bottom + 4) + 'px';

  // Keep tooltip in viewport
  requestAnimationFrame(() => {
    if (!tooltipEl) return;
    const tr = tooltipEl.getBoundingClientRect();
    if (tr.right > window.innerWidth) {
      tooltipEl.style.left = (window.innerWidth - tr.width - 8) + 'px';
    }
    if (tr.bottom > window.innerHeight) {
      tooltipEl.style.top = (rect.top - tr.height - 4) + 'px';
    }
  });
}

function hideBucketTooltip() {
  if (tooltipEl) {
    tooltipEl.remove();
    tooltipEl = null;
  }
}

window.showBucketTooltip = showBucketTooltip;
window.hideBucketTooltip = hideBucketTooltip;

// Set up debug filter, scroll handlers, and subtab switching once the DOM is ready
document.addEventListener('DOMContentLoaded', () => {
  // Debug subtab switching
  document.querySelectorAll('.debug-subtab').forEach(btn => {
    btn.addEventListener('click', async () => {
      const targetTab = btn.dataset.debugTab;

      document.querySelectorAll('.debug-subtab').forEach(b => b.classList.remove('active'));
      document.querySelectorAll('.debug-subpane').forEach(p => p.classList.remove('active'));

      btn.classList.add('active');
      document.getElementById(`debug-${targetTab}-pane`).classList.add('active');

      // Lazy-load schedule data on first open
      if (targetTab === 'schedule' && !debugScheduleLoaded) {
        debugScheduleLoaded = true;
        await loadDebugChunk(debugWindowStart, debugWindowEnd);
        scrollToNow();
      }
    });
  });

  const profilesFilterInput = document.getElementById('debug-profiles-filter');
  if (profilesFilterInput) {
    profilesFilterInput.addEventListener('input', e => {
      debugProfilesFilter = e.target.value;
      renderDebugProfilesTable();
    });
  }

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
