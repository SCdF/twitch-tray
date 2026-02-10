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
const categorySearchInput = document.getElementById('category_search');
const searchResultsDiv = document.getElementById('search_results');
const categoryListDiv = document.getElementById('category_list');
const streamerSearchInput = document.getElementById('streamer_search');
const streamerSearchResultsDiv = document.getElementById('streamer_search_results');
const streamerListDiv = document.getElementById('streamer_list');
const streamerDetailDiv = document.getElementById('streamer_detail');
const closeBtn = document.getElementById('close_btn');

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
  scheduleLookaheadInput.value = config.schedule_lookahead_hours;
  notifyOnLiveInput.checked = config.notify_on_live;
  notifyOnCategoryInput.checked = config.notify_on_category;

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
    tab.addEventListener('click', () => {
      const targetId = tab.dataset.tab;

      tabs.forEach(t => t.classList.remove('active'));
      panes.forEach(p => p.classList.remove('active'));

      tab.classList.add('active');
      document.getElementById(targetId).classList.add('active');
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
  [pollIntervalInput, notifyMaxGapInput, scheduleLookaheadInput].forEach(input => {
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
        schedule_lookahead_hours: parseInt(scheduleLookaheadInput.value, 10) || 6,
        notify_on_live: notifyOnLiveInput.checked,
        notify_on_category: notifyOnCategoryInput.checked,
        followed_categories: config.followed_categories || [],
        streamer_settings: config.streamer_settings || {}
      };

      // Validate
      newConfig.poll_interval_sec = Math.max(30, Math.min(300, newConfig.poll_interval_sec));
      newConfig.notify_max_gap_min = Math.max(1, Math.min(60, newConfig.notify_max_gap_min));
      newConfig.schedule_lookahead_hours = Math.max(1, Math.min(72, newConfig.schedule_lookahead_hours));

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
