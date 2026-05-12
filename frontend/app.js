'use strict';

const state = {
  view: 'challenges',
  token: sessionStorage.getItem('feralctf_token') || '',
  user: null,
  challenges: [],
  selectedCategory: 'all',
  query: '',
  scoreboard: { teams: [] },
  profile: null,
  ws: null,
  reconnects: 0,
};

const app = document.getElementById('app');

document.addEventListener('DOMContentLoaded', init);

async function init() {
  renderShell();
  connectWebSocket();
  await loadSession();
  await Promise.all([loadChallenges(), loadScoreboard()]);
  navigate('challenges');
}

function renderShell() {
  app.innerHTML = `
    <header class="topbar">
      <div>
        <h1>FeralCTF</h1>
        <p class="muted">terminal capture console</p>
      </div>
      <nav class="nav">
        <button data-view="challenges">Challenges</button>
        <button data-view="scoreboard">Scoreboard</button>
        <button data-view="profile">Profile</button>
        <button data-view="admin">Admin</button>
      </nav>
      <form id="auth-form" class="auth-form">
        <input id="auth-username" autocomplete="username" placeholder="username">
        <input id="auth-password" autocomplete="current-password" type="password" placeholder="password">
        <button type="submit">Login</button>
      </form>
    </header>
    <main id="view"></main>
    <div id="modal" class="modal" aria-hidden="true"></div>
    <div id="toast" class="toast" role="status"></div>
  `;

  document.querySelectorAll('[data-view]').forEach((button) => {
    button.addEventListener('click', () => navigate(button.dataset.view));
  });
  document.getElementById('auth-form').addEventListener('submit', loginUser);
}

async function loadSession() {
  if (!state.token) {
    updateAuth();
    return;
  }
  try {
    state.user = await api('/api/auth/me');
  } catch (_) {
    state.token = '';
    sessionStorage.removeItem('feralctf_token');
  }
  updateAuth();
}

function updateAuth() {
  const form = document.getElementById('auth-form');
  if (!form) return;
  if (state.user) {
    form.innerHTML = `
      <span class="session-user">${escapeHtml(state.user.username)}</span>
      <button type="button" id="logout-button">Logout</button>
    `;
    document.getElementById('logout-button').addEventListener('click', logoutUser);
  }
}

async function loginUser(event) {
  event.preventDefault();
  const username = document.getElementById('auth-username').value.trim();
  const password = document.getElementById('auth-password').value;
  if (!username || !password) return;

  try {
    const result = await api('/api/auth/login', {
      method: 'POST',
      body: JSON.stringify({ username, password }),
    });
    state.token = result.token;
    state.user = result.user;
    sessionStorage.setItem('feralctf_token', result.token);
    updateAuth();
    await Promise.all([loadChallenges(), loadScoreboard()]);
    renderCurrent();
    toast('session opened');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function logoutUser() {
  try {
    await api('/api/auth/logout', { method: 'POST' });
  } catch (_) {}
  state.token = '';
  state.user = null;
  sessionStorage.removeItem('feralctf_token');
  renderShell();
  navigate('challenges');
}

function navigate(view) {
  state.view = view;
  document.querySelectorAll('[data-view]').forEach((button) => {
    button.classList.toggle('active', button.dataset.view === view);
  });
  renderCurrent();
}

function renderCurrent() {
  if (state.view === 'scoreboard') renderScoreboard();
  else if (state.view === 'profile') renderProfile();
  else if (state.view === 'admin') renderAdmin();
  else renderChallenges();
}

async function loadChallenges() {
  if (!state.token) return;
  try {
    const result = await api('/api/challenges');
    state.challenges = result.challenges || [];
  } catch (error) {
    state.challenges = [];
  }
}

function renderChallenges() {
  const view = document.getElementById('view');
  const categories = ['all', ...new Set(state.challenges.map((c) => c.category).filter(Boolean))];
  const challenges = filteredChallenges();

  view.innerHTML = `
    <section class="toolbar">
      <label>
        <span>Category</span>
        <select id="category-filter">
          ${categories.map((category) => `<option value="${escapeHtml(category)}">${escapeHtml(category)}</option>`).join('')}
        </select>
      </label>
      <label>
        <span>Search</span>
        <input id="challenge-search" value="${escapeHtml(state.query)}" placeholder="challenge name">
      </label>
    </section>
    <section class="challenge-grid">
      ${challenges.map(challengeCard).join('') || emptyState('No visible challenges.')}
    </section>
  `;

  const categoryFilter = document.getElementById('category-filter');
  categoryFilter.value = state.selectedCategory;
  categoryFilter.addEventListener('change', () => {
    state.selectedCategory = categoryFilter.value;
    renderChallenges();
  });
  const search = document.getElementById('challenge-search');
  search.addEventListener('input', () => {
    state.query = search.value;
    renderChallenges();
  });
  document.querySelectorAll('[data-challenge-id]').forEach((card) => {
    card.addEventListener('click', () => openChallenge(Number(card.dataset.challengeId)));
  });
}

function filteredChallenges() {
  const query = state.query.trim().toLowerCase();
  return state.challenges.filter((challenge) => {
    const categoryMatch = state.selectedCategory === 'all' || challenge.category === state.selectedCategory;
    const queryMatch = !query || challenge.title.toLowerCase().includes(query);
    return categoryMatch && queryMatch;
  });
}

function challengeCard(challenge) {
  const difficulty = difficultyFor(challenge.points);
  const solved = challenge.solved_by_team ? '<span class="solved">solved</span>' : '';
  return `
    <article class="card challenge-card" data-challenge-id="${challenge.id}">
      <div class="card-row">
        <h2>${escapeHtml(challenge.title)}</h2>
        ${solved}
      </div>
      <div class="card-row meta">
        <span class="category" style="--category-color:${categoryColor(challenge.category)}">${escapeHtml(challenge.category)}</span>
        <span><i class="dot ${difficulty}"></i>${difficulty}</span>
      </div>
      <div class="card-row">
        <strong>${challenge.points} pts</strong>
        <span>${challenge.solve_count} solves</span>
      </div>
    </article>
  `;
}

async function openChallenge(id) {
  try {
    const detail = await api(`/api/challenges/${id}`);
    const challenge = detail.challenge;
    const modal = document.getElementById('modal');
    modal.innerHTML = `
      <div class="modal-panel">
        <button class="modal-close" type="button" aria-label="Close">x</button>
        <h2>${escapeHtml(challenge.title)}</h2>
        <div class="meta">
          <span class="category" style="--category-color:${categoryColor(challenge.category)}">${escapeHtml(challenge.category)}</span>
          <span>${challenge.points} pts</span>
          <span>${challenge.solve_count} solves</span>
        </div>
        <p class="description">${escapeHtml(challenge.description)}</p>
        <div class="file-list">${(detail.files || []).map(fileLink).join('') || '<p class="muted">No files attached.</p>'}</div>
        <div class="hint-list">${(detail.hints || []).map((hint) => hintRow(challenge.id, hint)).join('') || '<p class="muted">No hints available.</p>'}</div>
        <form id="flag-form" class="flag-form">
          <input id="flag-input" placeholder="feralctf{...}" autocomplete="off" required>
          <button type="submit">Submit Flag</button>
        </form>
      </div>
    `;
    modal.classList.add('open');
    modal.setAttribute('aria-hidden', 'false');
    modal.querySelector('.modal-close').addEventListener('click', closeModal);
    modal.addEventListener('click', (event) => {
      if (event.target === modal) closeModal();
    }, { once: true });
    modal.querySelector('#flag-form').addEventListener('submit', (event) => submitFlag(event, challenge.id));
    modal.querySelectorAll('[data-hint-id]').forEach((button) => {
      button.addEventListener('click', () => unlockHint(challenge.id, Number(button.dataset.hintId)));
    });
  } catch (error) {
    toast(error.message, 'error');
  }
}

function fileLink(file) {
  return `
    <a class="file-link" href="/${encodeURI(file.storage_path)}" download>
      <span>${escapeHtml(file.filename)}</span>
      <small>${formatBytes(file.size_bytes)}</small>
    </a>
  `;
}

function hintRow(challengeId, hint) {
  if (hint.unlocked) {
    return `
      <details class="hint" open>
        <summary>Hint ${hint.sort_order} (${hint.cost_points} pts)</summary>
        <p>${escapeHtml(hint.content || '')}</p>
      </details>
    `;
  }
  return `
    <div class="hint locked">
      <span>Hint ${hint.sort_order} (${hint.cost_points} pts)</span>
      <button type="button" data-hint-id="${hint.id}" data-challenge-id="${challengeId}">Unlock</button>
    </div>
  `;
}

async function unlockHint(challengeId, hintId) {
  try {
    const result = await api(`/api/challenges/${challengeId}/hints/${hintId}/unlock`, { method: 'POST' });
    toast(`hint unlocked, -${result.points_deducted} pts`);
    openChallenge(challengeId);
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function submitFlag(event, challengeId) {
  event.preventDefault();
  const input = document.getElementById('flag-input');
  try {
    const result = await api(`/api/challenges/${challengeId}/submit`, {
      method: 'POST',
      body: JSON.stringify({ flag: input.value.trim() }),
    });
    if (!result.correct) {
      toast(result.message || 'incorrect flag', 'error');
      return;
    }
    toast(`correct, +${result.points_earned} pts`);
    closeModal();
    await Promise.all([loadChallenges(), loadScoreboard()]);
    renderCurrent();
  } catch (error) {
    toast(error.message, 'error');
  }
}

function closeModal() {
  const modal = document.getElementById('modal');
  modal.classList.remove('open');
  modal.setAttribute('aria-hidden', 'true');
  modal.innerHTML = '';
}

async function loadScoreboard() {
  try {
    state.scoreboard = await api('/api/scoreboard');
  } catch (_) {
    state.scoreboard = { teams: [] };
  }
}

function renderScoreboard() {
  const view = document.getElementById('view');
  const maxScore = Math.max(1, ...state.scoreboard.teams.map((team) => team.score));
  view.innerHTML = `
    <section class="panel">
      <h2>Scoreboard</h2>
      <table class="scoreboard">
        <thead><tr><th>Rank</th><th>Team</th><th>Solves</th><th>Progress</th><th>Score</th></tr></thead>
        <tbody>${state.scoreboard.teams.map((team) => scoreRow(team, maxScore)).join('') || '<tr><td colspan="5">No teams yet.</td></tr>'}</tbody>
      </table>
    </section>
  `;
}

function scoreRow(team, maxScore) {
  const isCurrent = state.user && state.user.team_id === team.team_id;
  const progress = Math.round((team.score / maxScore) * 100);
  return `
    <tr class="${isCurrent ? 'current-team' : ''}">
      <td>${team.rank}</td>
      <td>${escapeHtml(team.team_name)}</td>
      <td>${team.solve_count}</td>
      <td><div class="progress"><span style="width:${progress}%"></span></div></td>
      <td>${team.score}</td>
    </tr>
  `;
}

async function renderProfile() {
  const view = document.getElementById('view');
  if (!state.user) {
    view.innerHTML = emptyState('Login to view your profile.');
    return;
  }
  if (state.user.team_id && (!state.profile || state.profile.team.id !== state.user.team_id)) {
    try {
      state.profile = await api(`/api/teams/${state.user.team_id}`);
    } catch (_) {
      state.profile = null;
    }
  }
  const teamScore = state.scoreboard.teams.find((team) => team.team_id === state.user.team_id);
  const solves = state.profile ? state.profile.solve_history : [];
  view.innerHTML = `
    <section class="profile">
      <div class="avatar">${escapeHtml(state.user.username.slice(0, 1).toUpperCase())}</div>
      <div>
        <h2>${escapeHtml(state.user.username)}</h2>
        <p class="muted">${state.profile ? escapeHtml(state.profile.team.name) : 'No team'}</p>
      </div>
      <div class="stats">
        <div><strong>${teamScore ? teamScore.rank : '-'}</strong><span>rank</span></div>
        <div><strong>${teamScore ? teamScore.score : 0}</strong><span>score</span></div>
        <div><strong>${solves.length}</strong><span>solves</span></div>
        <div><strong>0</strong><span>hints used</span></div>
        <div><strong>0</strong><span>first bloods</span></div>
      </div>
    </section>
    <section class="panel">
      <h2>Solve History</h2>
      <div class="history">${solves.map(solveRow).join('') || '<p class="muted">No solves yet.</p>'}</div>
    </section>
  `;
}

function solveRow(solve) {
  return `
    <div class="history-row">
      <span>${escapeHtml(solve.category)}</span>
      <strong>${escapeHtml(solve.challenge_title)}</strong>
      <span>${solve.points} pts</span>
      <time>${formatTime(solve.solved_at)}</time>
    </div>
  `;
}

async function renderAdmin(section = 'overview') {
  const view = document.getElementById('view');
  view.innerHTML = `
    <section class="admin">
      <aside>
        ${['overview', 'challenges', 'users', 'teams', 'settings'].map((item) => `<button data-admin="${item}" class="${item === section ? 'active' : ''}">${item}</button>`).join('')}
      </aside>
      <div id="admin-content" class="panel"></div>
    </section>
  `;
  document.querySelectorAll('[data-admin]').forEach((button) => {
    button.addEventListener('click', () => renderAdmin(button.dataset.admin));
  });
  if (section === 'challenges') renderAdminChallenges();
  else if (section === 'users') renderAdminUsers();
  else if (section === 'teams') renderAdminTeams();
  else if (section === 'settings') renderAdminSettings();
  else renderAdminOverview();
}

async function renderAdminOverview() {
  const content = document.getElementById('admin-content');
  try {
    const stats = await api('/api/admin');
    const submissions = await api('/api/admin/submissions?per_page=8');
    content.innerHTML = `
      <div class="stat-grid">
        <div><strong>${stats.teams || 0}</strong><span>teams</span></div>
        <div><strong>${stats.challenges || 0}</strong><span>challenges</span></div>
        <div><strong>${stats.solves || 0}</strong><span>solves</span></div>
        <div><strong>${submissions.total || 0}</strong><span>submissions</span></div>
      </div>
      <h2>Recent Submissions</h2>
      <div class="history">${(submissions.submissions || []).map(submissionRow).join('') || '<p class="muted">No submissions yet.</p>'}</div>
    `;
  } catch (error) {
    content.innerHTML = emptyState(error.message);
  }
}

function submissionRow(submission) {
  return `
    <div class="history-row">
      <span>#${submission.id}</span>
      <strong>team ${submission.team_id}</strong>
      <span>challenge ${submission.challenge_id}</span>
      <span>${submission.is_correct ? 'correct' : 'wrong'}</span>
    </div>
  `;
}

function renderAdminChallenges() {
  const content = document.getElementById('admin-content');
  content.innerHTML = `
    <h2>Challenges</h2>
    <form id="challenge-form" class="admin-form">
      <input name="title" placeholder="title" required>
      <input name="category" placeholder="category" required>
      <input name="points" type="number" placeholder="points" required>
      <input name="flag" placeholder="flag" required>
      <textarea name="description" placeholder="description"></textarea>
      <button type="submit">Add Challenge</button>
    </form>
    <table class="scoreboard">
      <thead><tr><th>Title</th><th>Category</th><th>Points</th><th>Actions</th></tr></thead>
      <tbody>${state.challenges.map(adminChallengeRow).join('') || '<tr><td colspan="4">No challenges.</td></tr>'}</tbody>
    </table>
  `;
  document.getElementById('challenge-form').addEventListener('submit', createChallenge);
  document.querySelectorAll('[data-delete-challenge]').forEach((button) => {
    button.addEventListener('click', () => deleteChallenge(Number(button.dataset.deleteChallenge)));
  });
}

function adminChallengeRow(challenge) {
  return `
    <tr>
      <td>${escapeHtml(challenge.title)}</td>
      <td>${escapeHtml(challenge.category)}</td>
      <td>${challenge.points}</td>
      <td>
        <button type="button">Edit</button>
        <button type="button" data-delete-challenge="${challenge.id}">Delete</button>
      </td>
    </tr>
  `;
}

async function createChallenge(event) {
  event.preventDefault();
  const data = Object.fromEntries(new FormData(event.target).entries());
  try {
    await api('/api/admin/challenges', {
      method: 'POST',
      body: JSON.stringify({
        title: data.title,
        category: data.category,
        description: data.description || '',
        flag: data.flag,
        flag_type: 'static',
        flag_case_sensitive: true,
        points: Number(data.points),
        max_points: Number(data.points),
        min_points: Math.max(1, Math.floor(Number(data.points) / 5)),
        decay_rate: 10,
        author: null,
        tags: [],
        unlock_requires: null,
        is_hidden: false,
      }),
    });
    await loadChallenges();
    renderAdmin('challenges');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function deleteChallenge(id) {
  try {
    await api(`/api/admin/challenges/${id}`, { method: 'DELETE' });
    await loadChallenges();
    renderAdmin('challenges');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function renderAdminUsers() {
  const content = document.getElementById('admin-content');
  try {
    const users = await api('/api/admin/users');
    content.innerHTML = tableView('Users', ['ID', 'Username', 'Role', 'Team', 'Actions'], users.map((user) => [
      user.id,
      escapeHtml(user.username),
      escapeHtml(user.role),
      user.team_id || '-',
      `<button type="button">Ban</button>`,
    ]));
  } catch (error) {
    content.innerHTML = emptyState(error.message);
  }
}

async function renderAdminTeams() {
  const content = document.getElementById('admin-content');
  try {
    const teams = await api('/api/admin/teams');
    content.innerHTML = tableView('Teams', ['ID', 'Name', 'Score', 'Actions'], teams.map((team) => [
      team.id,
      escapeHtml(team.name),
      team.score || 0,
      `<button type="button">Disqualify</button>`,
    ]));
  } catch (error) {
    content.innerHTML = emptyState(error.message);
  }
}

function renderAdminSettings() {
  document.getElementById('admin-content').innerHTML = `
    <h2>Settings</h2>
    <form class="admin-form">
      <input placeholder="Competition name">
      <input type="datetime-local">
      <input type="datetime-local">
      <label><input type="checkbox"> Team mode</label>
      <label><input type="checkbox"> Dynamic scoring</label>
      <label><input type="checkbox"> Score freeze</label>
      <button type="button">Save</button>
    </form>
  `;
}

function tableView(title, headers, rows) {
  return `
    <h2>${title}</h2>
    <table class="scoreboard">
      <thead><tr>${headers.map((headerText) => `<th>${headerText}</th>`).join('')}</tr></thead>
      <tbody>${rows.map((row) => `<tr>${row.map((cell) => `<td>${cell}</td>`).join('')}</tr>`).join('')}</tbody>
    </table>
  `;
}

function connectWebSocket() {
  const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
  const socket = new WebSocket(`${protocol}//${window.location.host}/ws`);
  state.ws = socket;
  socket.addEventListener('open', () => {
    state.reconnects = 0;
  });
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    if (message.type === 'score_update') {
      state.scoreboard = { teams: message.scoreboard || [] };
      if (state.view === 'scoreboard') renderScoreboard();
    }
  });
  socket.addEventListener('close', () => {
    const delay = Math.min(30000, 500 * 2 ** state.reconnects);
    state.reconnects += 1;
    window.setTimeout(connectWebSocket, delay);
  });
}

async function api(path, options = {}) {
  const headers = {
    Accept: 'application/json',
    ...(options.body ? { 'Content-Type': 'application/json' } : {}),
    ...(state.token ? { Authorization: `Bearer ${state.token}` } : {}),
    ...(options.headers || {}),
  };
  const response = await fetch(path, { ...options, headers });
  if (!response.ok) {
    let message = `${response.status} ${response.statusText}`;
    try {
      const error = await response.json();
      message = error.message || error.error || message;
    } catch (_) {}
    throw new Error(message);
  }
  if (response.status === 204) return null;
  const text = await response.text();
  return text ? JSON.parse(text) : null;
}

function emptyState(message) {
  return `<section class="empty">${escapeHtml(message)}</section>`;
}

function toast(message, type = 'success') {
  const toastEl = document.getElementById('toast');
  toastEl.textContent = message;
  toastEl.className = `toast show ${type}`;
  window.setTimeout(() => toastEl.className = 'toast', 3500);
}

function escapeHtml(value) {
  const div = document.createElement('div');
  div.textContent = String(value ?? '');
  return div.innerHTML;
}

function difficultyFor(points) {
  if (points >= 400) return 'hard';
  if (points >= 200) return 'medium';
  return 'easy';
}

function categoryColor(category) {
  let hash = 0;
  for (const char of String(category || 'misc')) hash = char.charCodeAt(0) + ((hash << 5) - hash);
  const colors = ['#63d28c', '#60a5fa', '#f59e0b', '#f472b6', '#a78bfa', '#fb7185'];
  return colors[Math.abs(hash) % colors.length];
}

function formatTime(seconds) {
  if (!seconds) return '-';
  return new Date(seconds * 1000).toLocaleString();
}

function formatBytes(bytes) {
  if (!bytes) return '0 B';
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
