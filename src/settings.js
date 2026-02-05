// Settings page JavaScript
const { invoke } = window.__TAURI__.core;
const { getCurrentWindow } = window.__TAURI__.window;

// State
let config = null;
let searchTimeout = null;

// DOM Elements
const tabs = document.querySelectorAll('.tab');
const panes = document.querySelectorAll('.pane');
const pollIntervalInput = document.getElementById('poll_interval');
const schedulePollInput = document.getElementById('schedule_poll');
const notifyMaxGapInput = document.getElementById('notify_max_gap');
const notifyOnLiveInput = document.getElementById('notify_on_live');
const notifyOnCategoryInput = document.getElementById('notify_on_category');
const categorySearchInput = document.getElementById('category_search');
const searchResultsDiv = document.getElementById('search_results');
const categoryListDiv = document.getElementById('category_list');
const saveBtn = document.getElementById('save_btn');
const cancelBtn = document.getElementById('cancel_btn');

// Initialize
document.addEventListener('DOMContentLoaded', async () => {
  await loadConfig();
  setupEventListeners();
});

async function loadConfig() {
  try {
    config = await invoke('get_config');
    populateForm();
  } catch (error) {
    console.error('Failed to load config:', error);
  }
}

function populateForm() {
  if (!config) return;

  pollIntervalInput.value = config.poll_interval_sec;
  schedulePollInput.value = config.schedule_poll_min;
  notifyMaxGapInput.value = config.notify_max_gap_min;
  notifyOnLiveInput.checked = config.notify_on_live;
  notifyOnCategoryInput.checked = config.notify_on_category;

  renderCategoryList();
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

  // Close search results when clicking outside
  document.addEventListener('click', (e) => {
    if (!e.target.closest('.search-container')) {
      searchResultsDiv.classList.remove('visible');
    }
  });

  // Save button
  saveBtn.addEventListener('click', saveConfig);

  // Cancel button
  cancelBtn.addEventListener('click', async () => {
    await getCurrentWindow().close();
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

  // Clear search
  categorySearchInput.value = '';
  searchResultsDiv.classList.remove('visible');
}

function removeCategory(id) {
  if (!config.followed_categories) return;

  config.followed_categories = config.followed_categories.filter(c => c.id !== id);
  renderCategoryList();
}

async function saveConfig() {
  try {
    // Build config from form
    const newConfig = {
      poll_interval_sec: parseInt(pollIntervalInput.value, 10) || 60,
      schedule_poll_min: parseInt(schedulePollInput.value, 10) || 5,
      notify_max_gap_min: parseInt(notifyMaxGapInput.value, 10) || 10,
      notify_on_live: notifyOnLiveInput.checked,
      notify_on_category: notifyOnCategoryInput.checked,
      followed_categories: config.followed_categories || []
    };

    // Validate
    newConfig.poll_interval_sec = Math.max(30, Math.min(300, newConfig.poll_interval_sec));
    newConfig.schedule_poll_min = Math.max(1, Math.min(60, newConfig.schedule_poll_min));
    newConfig.notify_max_gap_min = Math.max(1, Math.min(60, newConfig.notify_max_gap_min));

    await invoke('save_config', { config: newConfig });

    // Close window after successful save
    await getCurrentWindow().close();
  } catch (error) {
    console.error('Failed to save config:', error);
    alert('Failed to save settings: ' + error);
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
