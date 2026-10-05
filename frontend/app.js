'use strict';

const state = {
  view: 'challenges',
  token: sessionStorage.getItem('feralctf_token') || '',
  user: null,
  challenges: [],
  selectedCategory: 'all',
  query: '',
  showSolved: true,
  announcements: [],
  competition: null,
  scoreboard: { teams: [] },
  profile: null,
  ws: null,
  reconnects: 0,
};

const app = document.getElementById('app');
const basePath = detectBasePath();
const appVersion = document.querySelector('meta[name="feralctf-version"]')?.content || '';

document.addEventListener('DOMContentLoaded', init);

async function init() {
  renderShell();
  connectWebSocket();
  await loadSession();
  await Promise.all([loadChallenges(), loadScoreboard(), loadCompetition(), loadAnnouncements()]);
  updateAuth();
  const returnView = sessionStorage.getItem('feralctf_return_view');
  sessionStorage.removeItem('feralctf_return_view');
  if (returnView === 'admin-settings' && state.user?.role === 'admin') {
    navigate('admin');
    renderAdmin('settings');
    toast('branding saved');
  } else {
    navigate('challenges');
  }
}

function renderShell() {
  app.innerHTML = `
    <header class="topbar">
      <div class="brand">
        <img id="brand-logo" src="${appPath('/feral10.jpg')}" class="brand-icon" alt="" title="Version ${escapeHtml(appVersion)}">
        <h1 id="brand-name">${escapeHtml(document.title || 'FeralCTF')}</h1>
      </div>
      <nav class="nav">
        <button data-view="challenges">Challenges</button>
        <button data-view="scoreboard">Scoreboard</button>
        <button data-view="profile">Profile</button>
      </nav>
      <form id="auth-form" class="auth-form">
        <input id="auth-username" autocomplete="username" placeholder="username">
        <input id="auth-password" autocomplete="current-password" type="password" placeholder="password">
        <button type="submit">Login</button>
        <button type="button" id="show-register-btn">Register</button>
      </form>
    </header>
    <div id="competition-banner" class="competition-banner" hidden></div>
    <main id="view"></main>
    <div id="modal" class="modal" aria-hidden="true"></div>
    <div id="toast" class="toast" role="status"></div>
  `;

  document.querySelectorAll('[data-view]').forEach((button) => {
    button.addEventListener('click', () => navigate(button.dataset.view));
  });
  document.getElementById('auth-form').addEventListener('submit', loginUser);
  document.getElementById('show-register-btn').addEventListener('click', showRegisterModal);
  const logo = document.getElementById('brand-logo');
  logo.addEventListener('error', () => {
    // A broken external logo falls back to the built-in one.
    const fallback = appPath('/feral10.jpg');
    if (!logo.src.endsWith(fallback)) logo.src = fallback;
  });
  applyBranding();
  renderCompetitionBanner();
}

async function loadCompetition() {
  try {
    state.competition = await api('/api/competition');
  } catch (_) {
    state.competition = null;
  }
  applyBranding();
  renderCompetitionBanner();
}

// Competition name and logo come from Admin -> Settings -> Branding.
function applyBranding() {
  const status = state.competition;
  if (!status) return;
  const name = status.name || 'FeralCTF';
  document.title = name;
  const heading = document.getElementById('brand-name');
  if (heading) heading.textContent = name;
  const logo = document.getElementById('brand-logo');
  if (logo) {
    const src = status.logo_url || appPath('/feral10.jpg');
    if (logo.getAttribute('src') !== src) logo.setAttribute('src', src);
    logo.alt = status.logo_url ? `${name} logo` : '';
  }
}

function renderCompetitionBanner() {
  const banner = document.getElementById('competition-banner');
  if (!banner) return;
  const status = state.competition;
  const now = Date.now() / 1000;
  let message = '';
  if (status && !status.started) {
    message = status.start_time ? `Competition starts ${formatTime(Date.parse(status.start_time) / 1000)}` : 'Competition has not started yet';
  } else if (status && status.ended) {
    message = 'Competition has ended — submissions are closed';
  } else if (status && status.frozen_at && status.frozen_at <= now) {
    message = 'Scoreboard is frozen';
  }
  banner.textContent = message;
  banner.hidden = !message;
}

async function loadAnnouncements() {
  try {
    state.announcements = (await api('/api/announcements')) || [];
  } catch (_) {
    state.announcements = [];
  }
}

function announcementStrip() {
  if (!state.announcements.length) return '';
  return `
    <section class="announcements">
      ${state.announcements.slice(0, 3).map((item) => `
        <article class="announcement">
          <strong>${escapeHtml(item.title)}</strong>
          <time class="muted">${formatTime(item.created_at)}</time>
          ${item.body ? `<p>${renderDescription(item.body)}</p>` : ''}
        </article>
      `).join('')}
    </section>
  `;
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
    const teams = state.scoreboard?.teams || [];
    const team = state.user.team_id ? teams.find((t) => t.team_id === state.user.team_id) : null;
    const displayName = team ? team.team_name : state.user.username;
    const score = team ? team.score.toLocaleString() : '0';
    form.innerHTML = `
      <div class="user-info">
        <span class="user-badge clickable" id="profile-badge">${escapeHtml(displayName)}</span>
        <span class="user-badge pts">${score} pts</span>
        <button type="button" id="logout-button">Logout</button>
      </div>
    `;
    document.getElementById('logout-button').addEventListener('click', logoutUser);
    document.getElementById('profile-badge').addEventListener('click', () => navigate('profile'));
  }
  updateAdminNav();
}

function updateAdminNav() {
  const nav = document.querySelector('nav.nav');
  if (!nav) return;
  const existing = nav.querySelector('[data-view="admin"]');
  const isAdmin = state.user && state.user.role === 'admin';
  if (isAdmin && !existing) {
    const btn = document.createElement('button');
    btn.dataset.view = 'admin';
    btn.textContent = 'Admin';
    btn.addEventListener('click', () => navigate('admin'));
    nav.appendChild(btn);
  } else if (!isAdmin && existing) {
    existing.remove();
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
    await Promise.all([loadChallenges(), loadScoreboard()]);
    updateAuth();
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
  clearLocalSession();
}

function clearLocalSession(message) {
  state.token = '';
  state.user = null;
  state.profile = null;
  sessionStorage.removeItem('feralctf_token');
  renderShell();
  navigate('challenges');
  if (message) toast(message);
}

function showRegisterModal() {
  const modal = document.getElementById('modal');
  modal.innerHTML = `
    <div class="modal-panel">
      <button class="modal-close" type="button" aria-label="Close">x</button>
      <h2>Register</h2>
      <form id="register-form" class="admin-form">
        <input name="username" autocomplete="username" placeholder="username" required>
        <input name="password" type="password" autocomplete="new-password" placeholder="password (min 8 chars)" required>
        <input name="password_confirm" type="password" autocomplete="new-password" placeholder="confirm password" required>
        <input name="team_name" placeholder="new team name (optional)">
        <input name="invite_code" placeholder="team invite code (optional)">
        <button type="submit">Register</button>
      </form>
    </div>
  `;
  modal.classList.add('open');
  modal.setAttribute('aria-hidden', 'false');
  modal.querySelector('.modal-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (event) => {
    if (event.target === modal) closeModal();
  }, { once: true });
  modal.querySelector('#register-form').addEventListener('submit', registerUser);
}

async function registerUser(event) {
  event.preventDefault();
  const data = Object.fromEntries(new FormData(event.target).entries());
  if (data.password !== data.password_confirm) {
    toast('passwords do not match', 'error');
    return;
  }
  const body = { username: data.username, password: data.password };
  if (data.team_name.trim()) body.team_name = data.team_name.trim();
  if (data.invite_code.trim()) body.invite_code = data.invite_code.trim();
  try {
    const result = await api('/api/auth/register', {
      method: 'POST',
      body: JSON.stringify(body),
    });
    state.token = result.token;
    state.user = result.user;
    sessionStorage.setItem('feralctf_token', result.token);
    closeModal();
    await Promise.all([loadChallenges(), loadScoreboard()]);
    updateAuth();
    renderCurrent();
    toast('account created');
  } catch (error) {
    toast(error.message, 'error');
  }
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
  if (!state.user) {
    view.innerHTML = emptyState('Log in to view challenges.');
    return;
  }
  const categories = ['all', ...new Set(state.challenges.map((c) => c.category).filter(Boolean))];
  const challenges = filteredChallenges();

  view.innerHTML = `
    ${announcementStrip()}
    <section class="toolbar">
      <input id="challenge-search" class="search-input" value="${escapeHtml(state.query)}" placeholder="search challenges...">
      <div class="category-pills">
        ${categories.map((cat) => `<button class="cat-pill${state.selectedCategory === cat ? ' active' : ''}" data-cat="${escapeHtml(cat)}">${escapeHtml(cat)}</button>`).join('')}
      </div>
      <label class="toggle-row">
        <span>Show solved</span>
        <span class="toggle-switch">
          <input type="checkbox" id="show-solved" ${state.showSolved ? 'checked' : ''}>
          <span class="toggle-slider"></span>
        </span>
      </label>
    </section>
    <section class="challenge-grid">
      ${challenges.map(challengeCard).join('') || emptyState('No visible challenges.')}
    </section>
  `;
  applyCategoryColors(view);

  const search = document.getElementById('challenge-search');
  search.addEventListener('input', (e) => {
    state.query = e.target.value;
    renderChallenges();
    const again = document.getElementById('challenge-search');
    again.focus();
    again.setSelectionRange(again.value.length, again.value.length);
  });
  document.getElementById('show-solved').addEventListener('change', (e) => {
    state.showSolved = e.target.checked;
    renderChallenges();
  });
  document.querySelectorAll('.cat-pill').forEach((btn) => {
    btn.addEventListener('click', () => {
      state.selectedCategory = btn.dataset.cat;
      renderChallenges();
    });
  });
  document.querySelectorAll('[data-challenge-id]').forEach((card) => {
    card.addEventListener('click', () => {
      const challenge = state.challenges.find((c) => c.id === Number(card.dataset.challengeId));
      if (challenge?.locked) {
        toast(`locked: solve ${prerequisiteTitle(challenge)} first`, 'error');
        return;
      }
      openChallenge(Number(card.dataset.challengeId));
    });
  });
}

function prerequisiteTitle(challenge) {
  const required = state.challenges.find((c) => c.id === challenge.unlock_requires);
  return required ? `"${required.title}"` : 'another challenge';
}

function filteredChallenges() {
  const query = state.query.trim().toLowerCase();
  return state.challenges.filter((challenge) => {
    if (challenge.solved_by_team && !state.showSolved) return false;
    const categoryMatch = state.selectedCategory === 'all' || challenge.category === state.selectedCategory;
    const queryMatch = !query || challenge.title.toLowerCase().includes(query);
    return categoryMatch && queryMatch;
  });
}

function challengeCard(challenge) {
  const difficulty = difficultyFor(challenge.points);
  return `
    <article class="card challenge-card${challenge.solved_by_team ? ' is-solved' : ''}${challenge.locked ? ' is-locked' : ''}" data-challenge-id="${challenge.id}">
      <div class="card-top">
        <span class="category" data-category-color="${categoryColor(challenge.category)}">${escapeHtml(challenge.category)}</span>
        ${challenge.solved_by_team ? '<span class="solved-flag">✓ solved</span>' : ''}
        ${challenge.locked ? `<span class="locked-flag" title="Solve ${escapeHtml(prerequisiteTitle(challenge))} first">🔒 locked</span>` : ''}
      </div>
      <h2 class="card-title">${escapeHtml(challenge.title)}</h2>
      <div class="card-bottom">
        <strong class="card-pts">${challenge.points} <span class="pts-label muted">pts</span></strong>
        <span class="card-meta">
          ${challenge.hint_count ? `<span title="${challenge.hint_count} hint(s)">💡${challenge.hint_count}</span>` : ''}
          ${challenge.file_count ? `<span title="${challenge.file_count} attachment(s)">📎${challenge.file_count}</span>` : ''}
          <i class="dot ${difficulty}"></i>${difficulty} · ${challenge.solve_count} solves
        </span>
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
          <span class="category" data-category-color="${categoryColor(challenge.category)}">${escapeHtml(challenge.category)}</span>
          <span>${challenge.points} pts</span>
          <span>${challenge.solve_count} solves</span>
          ${challenge.solved_by_team ? '<span class="solved">✓ solved</span>' : ''}
        </div>
        <p class="description">${renderDescription(challenge.description)}</p>
        ${detail.files?.length ? `<h3 class="section-label">Attachments</h3><div class="file-list">${detail.files.map(fileLink).join('')}</div>` : ''}
        ${detail.hints?.length ? `<h3 class="section-label">Hints</h3><div class="hint-list">${detail.hints.map((hint, index) => hintRow(challenge, hint, index)).join('')}</div>` : ''}
        <form id="flag-form" class="flag-form">
          <input id="flag-input" placeholder="FLAG{...}" autocomplete="off" required>
          <button type="submit">Submit Flag</button>
        </form>
      </div>
    `;
    applyCategoryColors(modal);
    modal.classList.add('open');
    modal.setAttribute('aria-hidden', 'false');
    modal.querySelector('.modal-close').addEventListener('click', closeModal);
    modal.addEventListener('click', (event) => {
      if (event.target === modal) closeModal();
    }, { once: true });
    modal.querySelector('#flag-form').addEventListener('submit', (event) => submitFlag(event, challenge.id));
    modal.querySelectorAll('button[data-hint-id]').forEach((button) => {
      button.addEventListener('click', () => unlockHint(challenge, button));
    });
  } catch (error) {
    toast(error.message, 'error');
  }
}

function isAbsoluteUrl(value) {
  try {
    const url = new URL(String(value || ''));
    return (url.protocol === 'http:' || url.protocol === 'https:') && Boolean(url.host);
  } catch (_) {
    return false;
  }
}

// Attachments are external links; FeralCTF does not host files.
function fileLink(file) {
  const size = file.size_bytes ? `<small>${formatBytes(file.size_bytes)}</small>` : '';
  if (!isAbsoluteUrl(file.storage_path)) {
    return `
      <div class="file-link unavailable">
        <span>${escapeHtml(file.filename)}</span>
        <small>link unavailable</small>
      </div>
    `;
  }
  return `
    <a class="file-link" href="${escapeHtml(file.storage_path)}" target="_blank" rel="noopener noreferrer">
      <span>${escapeHtml(file.filename)} ↗</span>
      ${size}
    </a>
  `;
}

function hintLabel(hint, index) {
  const cost = hint.cost_points ? `${hint.cost_points} pts` : 'free';
  return `Hint ${index + 1} (${cost})`;
}

function hintRow(challenge, hint, index) {
  if (hint.unlocked) {
    return `
      <details class="hint" open data-hint-id="${hint.id}">
        <summary>${hintLabel(hint, index)}</summary>
        <p>${renderDescription(hint.content || '')}</p>
      </details>
    `;
  }
  let action;
  if (challenge.solved_by_team) {
    action = '<small class="muted">solved — not needed</small>';
  } else if (!state.user?.team_id) {
    action = '<button type="button" disabled title="Hints are unlocked per team">Join a team to unlock</button>';
  } else {
    action = `<button type="button" data-hint-id="${hint.id}" data-hint-index="${index}" data-hint-cost="${hint.cost_points}">Unlock</button>`;
  }
  return `
    <div class="hint locked" data-hint-id="${hint.id}">
      <span>${hintLabel(hint, index)}</span>
      ${action}
    </div>
  `;
}

async function unlockHint(challenge, button) {
  const hintId = Number(button.dataset.hintId);
  const index = Number(button.dataset.hintIndex);
  const cost = Number(button.dataset.hintCost);
  if (cost > 0) {
    const team = state.scoreboard.teams.find((t) => t.team_id === state.user?.team_id);
    const score = team ? team.score : 0;
    if (!window.confirm(`Unlock Hint ${index + 1} for ${cost} pts? Team score: ${score}`)) return;
  }
  button.disabled = true;
  try {
    const result = await api(`/api/challenges/${challenge.id}/hints/${hintId}/unlock`, { method: 'POST' });
    // Replace just this row so a flag being typed is not lost.
    const row = button.closest('.hint');
    row.outerHTML = hintRow(challenge, {
      id: hintId,
      cost_points: cost,
      unlocked: true,
      content: result.content,
    }, index);
    toast(result.points_deducted ? `hint unlocked, -${result.points_deducted} pts` : 'hint unlocked');
    state.profile = null;
    await loadScoreboard();
  } catch (error) {
    button.disabled = false;
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
    state.profile = null;
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
    setScoreboard(await api('/api/scoreboard'));
  } catch (_) {
    setScoreboard({ teams: [] });
  }
}

function setScoreboard(scoreboard) {
  const previous = state.scoreboard || { teams: [], total_visible_points: 0 };
  const next = scoreboard || { teams: [] };
  state.scoreboard = {
    ...previous,
    ...next,
    teams: next.teams || [],
    total_visible_points: next.total_visible_points ?? previous.total_visible_points ?? 0,
  };
  updateAuth();
}

function renderScoreboard() {
  const view = document.getElementById('view');
  const totalVisiblePoints = state.scoreboard.total_visible_points || 0;
  view.innerHTML = `
    <section class="panel">
      <div class="scoreboard-header">
        <h2>Live Scoreboard</h2>
        <span class="live-dot">● live</span>
      </div>
      <div id="score-graph" class="score-graph"></div>
      <table class="scoreboard">
        <thead><tr><th>#</th><th>Team</th><th>Solves</th><th>Progress</th><th>Score</th></tr></thead>
        <tbody>${state.scoreboard.teams.map((team) => scoreRow(team, totalVisiblePoints)).join('') || '<tr><td colspan="5">No teams yet.</td></tr>'}</tbody>
      </table>
    </section>
  `;
  applyProgressWidths(view);
  renderScoreGraph();
}

async function renderScoreGraph() {
  const container = document.getElementById('score-graph');
  if (!container) return;
  let series;
  try {
    series = (await api('/api/scoreboard/graph')) || [];
  } catch (_) {
    return;
  }
  const top = state.scoreboard.teams.slice(0, 10).map((team) => team.team_id);
  series = series.filter((s) => top.includes(s.team_id) && s.points.length);
  if (!series.length || !document.body.contains(container)) return;

  const width = 800;
  const height = 220;
  const pad = 30;
  const times = series.flatMap((s) => s.points.map(([t]) => t));
  const minT = Math.min(...times);
  const maxT = Math.max(Math.max(...times), Math.floor(Date.now() / 1000));
  const maxScore = Math.max(1, ...series.flatMap((s) => s.points.map(([, score]) => score)));
  const x = (t) => pad + ((t - minT) / Math.max(1, maxT - minT)) * (width - pad * 2);
  const y = (score) => height - pad - (Math.max(0, score) / maxScore) * (height - pad * 2);
  const colors = ['#63d28c', '#60a5fa', '#f59e0b', '#f472b6', '#a78bfa', '#fb7185', '#34d399', '#fbbf24', '#38bdf8', '#e879f9'];

  const lines = series.map((s, index) => {
    const color = colors[top.indexOf(s.team_id) % colors.length] || colors[index % colors.length];
    let d = '';
    s.points.forEach(([t, score], i) => {
      d += i === 0 ? `M${x(t).toFixed(1)},${y(score).toFixed(1)}` : `H${x(t).toFixed(1)}V${y(score).toFixed(1)}`;
    });
    const last = s.points[s.points.length - 1];
    d += `H${x(maxT).toFixed(1)}`;
    return {
      color,
      name: s.team_name,
      svg: `<path d="${d}" fill="none" stroke="${color}" stroke-width="2"><title>${escapeHtml(s.team_name)}: ${last[1]}</title></path>`,
    };
  });

  container.innerHTML = `
    <svg viewBox="0 0 ${width} ${height}" role="img" aria-label="Score over time for the top teams">
      <line x1="${pad}" y1="${height - pad}" x2="${width - pad}" y2="${height - pad}" stroke="#2a3347"></line>
      <line x1="${pad}" y1="${pad}" x2="${pad}" y2="${height - pad}" stroke="#2a3347"></line>
      <text x="${pad - 4}" y="${pad + 4}" text-anchor="end" fill="#8b949e" font-size="10">${maxScore}</text>
      <text x="${pad - 4}" y="${height - pad}" text-anchor="end" fill="#8b949e" font-size="10">0</text>
      ${lines.map((line) => line.svg).join('')}
    </svg>
    <div class="graph-legend">
      ${lines.map((line) => `<span><i class="legend-swatch" data-swatch="${line.color}"></i>${escapeHtml(line.name)}</span>`).join('')}
    </div>
  `;
  container.querySelectorAll('[data-swatch]').forEach((el) => {
    el.style.background = el.dataset.swatch;
  });
}

function scoreRow(team, totalVisiblePoints) {
  const isCurrent = state.user && state.user.team_id === team.team_id;
  const progress = totalVisiblePoints > 0 ? Math.round((team.score / totalVisiblePoints) * 100) : 0;
  const barWidth = Math.min(100, Math.max(0, progress));
  const progressTitle = `${progress}%: ${team.score} of ${totalVisiblePoints} points scored`;
  const medals = ['🥇', '🥈', '🥉'];
  const rank = team.rank <= 3 ? medals[team.rank - 1] : team.rank;
  return `
    <tr class="${isCurrent ? 'current-team' : ''}">
      <td>${rank}</td>
      <td>${escapeHtml(team.team_name)}${isCurrent ? ' <span class="muted">(you)</span>' : ''}</td>
      <td>${team.solve_count}</td>
      <td><div class="progress" title="${escapeHtml(progressTitle)}"><span data-progress-width="${barWidth}"></span></div></td>
      <td>${team.score}</td>
    </tr>
  `;
}

// Inline style attributes are blocked by the CSP, so colours are applied here.
function applyCategoryColors(root) {
  root.querySelectorAll('[data-category-color]').forEach((el) => {
    el.style.setProperty('--category-color', el.dataset.categoryColor);
  });
}

function applyProgressWidths(root) {
  root.querySelectorAll('[data-progress-width]').forEach((bar) => {
    bar.style.width = `${bar.dataset.progressWidth}%`;
  });
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
  const hintUnlocks = state.profile?.hint_history || [];
  const history = [
    ...solves.map((solve) => ({ at: solve.solved_at, html: solveRow(solve) })),
    ...hintUnlocks.map((unlock) => ({ at: unlock.unlocked_at, html: hintHistoryRow(unlock) })),
  ].sort((a, b) => b.at - a.at);
  const inviteCode = state.profile?.team?.invite_code || '';

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
        <div><strong>${state.profile?.hints_used ?? 0}</strong><span>hints used</span></div>
        <div><strong>${state.profile?.first_bloods ?? 0}</strong><span>first bloods</span></div>
      </div>
    </section>
    ${!state.user.team_id ? `
      <section class="panel">
        <h2>Join or Create a Team</h2>
        <div class="team-setup">
          <form id="create-team-form" class="admin-form">
            <h3>Create Team</h3>
            <input name="team_name" placeholder="team name" required>
            <button type="submit">Create</button>
          </form>
          <form id="join-team-form" class="admin-form">
            <h3>Join Team</h3>
            <input name="invite_code" placeholder="invite code" required>
            <button type="submit">Join</button>
          </form>
        </div>
      </section>
    ` : `
      <section class="panel">
        <h2>Team</h2>
        <div class="invite-row">
          <span class="muted">Invite Code</span>
          <code class="invite-code">${escapeHtml(inviteCode)}</code>
          <button type="button" id="copy-invite-btn">Copy</button>
        </div>
      </section>
    `}
    <section class="panel">
      <h2>Change Password</h2>
      <form id="change-password-form" class="admin-form">
        <input name="current_password" type="password" autocomplete="current-password" placeholder="current password" required>
        <input name="new_password" type="password" autocomplete="new-password" placeholder="new password (min 8 chars)" required>
        <input name="new_password_confirm" type="password" autocomplete="new-password" placeholder="confirm new password" required>
        <button type="submit">Change Password</button>
      </form>
    </section>
    <section class="panel">
      <h2>Solve History</h2>
      <div class="history">${history.map((item) => item.html).join('') || '<p class="muted">No solves yet.</p>'}</div>
    </section>
  `;

  if (!state.user.team_id) {
    document.getElementById('create-team-form').addEventListener('submit', createTeam);
    document.getElementById('join-team-form').addEventListener('submit', joinTeam);
  } else if (inviteCode) {
    document.getElementById('copy-invite-btn').addEventListener('click', () => {
      navigator.clipboard.writeText(inviteCode).then(() => toast('invite code copied'));
    });
  }
  document.getElementById('change-password-form').addEventListener('submit', changePassword);
}

async function changePassword(event) {
  event.preventDefault();
  const data = Object.fromEntries(new FormData(event.target).entries());
  if (data.new_password !== data.new_password_confirm) {
    toast('passwords do not match', 'error');
    return;
  }
  try {
    await api('/api/auth/password', {
      method: 'PUT',
      body: JSON.stringify({
        current_password: data.current_password,
        new_password: data.new_password,
      }),
    });
    clearLocalSession('password changed; please log in again');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function createTeam(event) {
  event.preventDefault();
  const data = Object.fromEntries(new FormData(event.target).entries());
  try {
    await api('/api/teams', {
      method: 'POST',
      body: JSON.stringify({ name: data.team_name.trim() }),
    });
    state.user = await api('/api/auth/me');
    state.profile = null;
    await Promise.all([loadChallenges(), loadScoreboard()]);
    updateAuth();
    renderProfile();
    toast('team created');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function joinTeam(event) {
  event.preventDefault();
  const data = Object.fromEntries(new FormData(event.target).entries());
  try {
    await api('/api/teams/join', {
      method: 'POST',
      body: JSON.stringify({ invite_code: data.invite_code.trim() }),
    });
    state.user = await api('/api/auth/me');
    state.profile = null;
    await Promise.all([loadChallenges(), loadScoreboard()]);
    updateAuth();
    renderProfile();
    toast('joined team');
  } catch (error) {
    toast(error.message, 'error');
  }
}

function hintHistoryRow(unlock) {
  return `
    <div class="history-row hint-history">
      <span>hint</span>
      <strong>${escapeHtml(unlock.challenge_title)}</strong>
      <span>${unlock.points_deducted ? `−${unlock.points_deducted} pts` : 'free'}</span>
      <time>${formatTime(unlock.unlocked_at)}</time>
    </div>
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
        ${['overview', 'challenges', 'submissions', 'users', 'teams', 'settings'].map((item) => `<button data-admin="${item}" class="${item === section ? 'active' : ''}">■ ${item}</button>`).join('')}
      </aside>
      <div id="admin-content" class="panel"></div>
    </section>
  `;
  document.querySelectorAll('[data-admin]').forEach((button) => {
    button.addEventListener('click', () => renderAdmin(button.dataset.admin));
  });
  if (section === 'challenges') renderAdminChallenges();
  else if (section === 'submissions') renderAdminSubmissions();
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
      <strong>${escapeHtml(submission.team_name || `team ${submission.team_id}`)}</strong>
      <span>${escapeHtml(submission.username || `user ${submission.user_id}`)}</span>
      <span>${escapeHtml(submission.challenge_title || `challenge ${submission.challenge_id}`)}</span>
      <span class="${submission.is_correct ? 'ok' : 'bad'}">${submission.is_correct ? 'correct' : 'wrong'}</span>
      <time>${formatTime(submission.submitted_at)}</time>
    </div>
  `;
}

async function renderAdminSubmissions(filters = { page: 1 }) {
  const content = document.getElementById('admin-content');
  const params = new URLSearchParams({ page: filters.page || 1, per_page: 50 });
  if (filters.team_id) params.set('team_id', filters.team_id);
  if (filters.challenge_id) params.set('challenge_id', filters.challenge_id);
  if (filters.correct) params.set('correct', filters.correct);
  let result;
  let teams;
  let challenges;
  try {
    [result, teams, challenges] = await Promise.all([
      api(`/api/admin/submissions?${params}`),
      api('/api/admin/teams'),
      api('/api/admin/challenges'),
    ]);
  } catch (error) {
    content.innerHTML = emptyState(error.message);
    return;
  }
  const pages = Math.max(1, Math.ceil(result.total / result.per_page));
  const option = (value, label, selected) => `<option value="${value}" ${String(selected || '') === String(value) ? 'selected' : ''}>${escapeHtml(label)}</option>`;
  content.innerHTML = `
    <h2>Submissions</h2>
    <form id="submission-filters" class="filter-row">
      <select name="team_id">${option('', 'all teams', filters.team_id)}${teams.map((t) => option(t.id, t.name, filters.team_id)).join('')}</select>
      <select name="challenge_id">${option('', 'all challenges', filters.challenge_id)}${challenges.map((c) => option(c.id, c.title, filters.challenge_id)).join('')}</select>
      <select name="correct">${option('', 'any result', filters.correct)}${option('true', 'correct', filters.correct)}${option('false', 'wrong', filters.correct)}</select>
    </form>
    <p class="muted">${result.total} submissions · page ${result.page} of ${pages}</p>
    <div class="history">${result.submissions.map(submissionRow).join('') || '<p class="muted">No submissions.</p>'}</div>
    <div class="pager">
      <button type="button" data-page="${result.page - 1}" ${result.page <= 1 ? 'disabled' : ''}>← prev</button>
      <button type="button" data-page="${result.page + 1}" ${result.page >= pages ? 'disabled' : ''}>next →</button>
    </div>
  `;
  const form = document.getElementById('submission-filters');
  const current = () => Object.fromEntries(new FormData(form).entries());
  form.addEventListener('change', () => renderAdminSubmissions({ ...current(), page: 1 }));
  content.querySelectorAll('[data-page]').forEach((button) => {
    button.addEventListener('click', () => renderAdminSubmissions({ ...current(), page: Number(button.dataset.page) }));
  });
}

async function renderAdminChallenges() {
  const content = document.getElementById('admin-content');
  if (!content) return;
  let challenges;
  try {
    challenges = await api('/api/admin/challenges');
  } catch (error) {
    content.innerHTML = emptyState(error.message);
    return;
  }
  state.adminChallenges = challenges;
  content.innerHTML = `
    <h2>Challenges</h2>
    <details class="admin-create">
      <summary>Add challenge</summary>
      <form id="challenge-form" class="admin-form">
        ${challengeFormFields(null, challenges)}
        <button type="submit">Add Challenge</button>
      </form>
    </details>
    <table class="scoreboard">
      <thead><tr><th>Title</th><th>Category</th><th>Points</th><th>Hints</th><th>Attachments</th><th>Visible</th><th>Actions</th></tr></thead>
      <tbody>${challenges.map(adminChallengeRow).join('') || '<tr><td colspan="7">No challenges.</td></tr>'}</tbody>
    </table>
  `;
  const form = document.getElementById('challenge-form');
  wireChallengeForm(form);
  form.addEventListener('submit', createChallenge);
  document.querySelectorAll('[data-delete-challenge]').forEach((button) => {
    const challenge = challenges.find((c) => c.id === Number(button.dataset.deleteChallenge));
    button.addEventListener('click', () => deleteChallenge(challenge));
  });
  document.querySelectorAll('[data-toggle-hidden]').forEach((input) => {
    input.addEventListener('change', () => {
      toggleChallengeVisibility(Number(input.dataset.toggleHidden), !input.checked);
    });
  });
  document.querySelectorAll('[data-edit-challenge]').forEach((button) => {
    const id = Number(button.dataset.editChallenge);
    const challenge = challenges.find((c) => c.id === id);
    button.addEventListener('click', () => openEditChallengeModal(challenge));
  });
}

function adminChallengeRow(challenge) {
  const visible = !challenge.is_hidden;
  return `
    <tr>
      <td>${escapeHtml(challenge.title)}${challenge.unlock_requires ? ' <span class="muted" title="has a prerequisite">🔒</span>' : ''}</td>
      <td>${escapeHtml(challenge.category)}</td>
      <td>${challenge.points}${challenge.flag_type === 'dynamic' ? ' <span class="muted">dyn</span>' : ''}</td>
      <td>${challenge.hint_count}</td>
      <td>${challenge.file_count}</td>
      <td>
        <label class="toggle-switch" title="${visible ? 'visible' : 'hidden'}">
          <input type="checkbox" data-toggle-hidden="${challenge.id}" ${visible ? 'checked' : ''}>
          <span class="toggle-slider"></span>
        </label>
      </td>
      <td>
        <button type="button" data-edit-challenge="${challenge.id}">Edit</button>
        <button type="button" data-delete-challenge="${challenge.id}">Delete</button>
      </td>
    </tr>
  `;
}

// Shared by the create form and the edit modal. `challenge` is null on create.
function challengeFormFields(challenge, allChallenges) {
  const c = challenge || {};
  const flagType = c.flag_type || 'static';
  const points = c.points ?? '';
  const tags = (() => {
    try {
      return JSON.parse(c.tags || '[]').join(', ');
    } catch (_) {
      return '';
    }
  })();
  const prerequisites = allChallenges.filter((other) => other.id !== c.id);
  const flagHelp = challenge
    ? `<small class="muted">type: ${escapeHtml(flagType)} · re-enter the flag when changing type or case sensitivity.</small>
      <div class="reveal-row">
        <button type="button" id="reveal-flag-btn">Reveal current flag</button>
        <code id="revealed-flag" class="revealed-flag" hidden></code>
      </div>`
    : '';
  return `
    <label><span>Title</span><input name="title" value="${escapeHtml(c.title || '')}" required></label>
    <label><span>Category</span><input name="category" value="${escapeHtml(c.category || '')}" required></label>
    <label><span>Description</span><textarea name="description" rows="6">${escapeHtml(c.description || '')}</textarea></label>
    <div class="form-grid">
      <label><span>Flag type</span>
        <select name="flag_type">
          ${['static', 'regex', 'dynamic'].map((type) => `<option value="${type}" ${type === flagType ? 'selected' : ''}>${type}</option>`).join('')}
        </select>
      </label>
      <label class="toggle-row">
        <span>Case sensitive</span>
        <span class="toggle-switch">
          <input type="checkbox" name="flag_case_sensitive" ${c.flag_case_sensitive ? 'checked' : ''}>
          <span class="toggle-slider"></span>
        </span>
      </label>
    </div>
    <label>
      <span>${challenge ? 'New flag (leave blank to keep current)' : 'Flag'}</span>
      <input name="flag" placeholder="${flagType === 'regex' ? '^flag\\{[a-z]+\\}$' : 'flag{...}'}" autocomplete="off" ${challenge ? '' : 'required'}>
      ${flagHelp}
    </label>
    <div class="form-grid">
      <label><span>Points</span><input name="points" type="number" min="0" value="${points}" required></label>
      <label data-dynamic-field><span>Max points</span><input name="max_points" type="number" min="0" value="${c.max_points ?? ''}"></label>
      <label data-dynamic-field><span>Min points</span><input name="min_points" type="number" min="0" value="${c.min_points ?? ''}"></label>
      <label data-dynamic-field><span>Decay rate</span><input name="decay_rate" type="number" min="0" value="${c.decay_rate ?? 10}"></label>
    </div>
    <div class="form-grid">
      <label><span>Author</span><input name="author" value="${escapeHtml(c.author || '')}"></label>
      <label><span>Tags (comma separated)</span><input name="tags" value="${escapeHtml(tags)}"></label>
    </div>
    <label><span>Prerequisite (must be solved first)</span>
      <select name="unlock_requires">
        <option value="">none</option>
        ${prerequisites.map((other) => `<option value="${other.id}" ${other.id === c.unlock_requires ? 'selected' : ''}>${escapeHtml(other.title)}</option>`).join('')}
      </select>
    </label>
    <label class="toggle-row">
      <span>${challenge ? 'Visible' : 'Start visible'}</span>
      <span class="toggle-switch">
        <input type="checkbox" name="is_visible" ${challenge && !c.is_hidden ? 'checked' : ''}>
        <span class="toggle-slider"></span>
      </span>
    </label>
  `;
}

function wireChallengeForm(form) {
  const select = form.querySelector('[name="flag_type"]');
  const sync = () => {
    form.querySelectorAll('[data-dynamic-field]').forEach((field) => {
      field.hidden = select.value !== 'dynamic';
    });
  };
  select.addEventListener('change', sync);
  sync();
}

function challengeBody(form) {
  const data = Object.fromEntries(new FormData(form).entries());
  const points = Number(data.points);
  const dynamic = data.flag_type === 'dynamic';
  return {
    title: data.title.trim(),
    category: data.category.trim(),
    description: data.description || '',
    flag: data.flag.trim(),
    flag_type: data.flag_type,
    flag_case_sensitive: 'flag_case_sensitive' in data,
    points,
    max_points: dynamic && data.max_points !== '' ? Number(data.max_points) : points,
    min_points: dynamic && data.min_points !== '' ? Number(data.min_points) : Math.max(1, Math.floor(points / 5)),
    decay_rate: data.decay_rate !== '' ? Number(data.decay_rate) : 10,
    author: data.author.trim() || null,
    tags: data.tags.split(',').map((tag) => tag.trim()).filter(Boolean),
    unlock_requires: data.unlock_requires ? Number(data.unlock_requires) : null,
    is_hidden: !('is_visible' in data),
  };
}

async function toggleChallengeVisibility(id, isHidden) {
  try {
    await api(`/api/admin/challenges/${id}`, {
      method: 'PUT',
      body: JSON.stringify({ is_hidden: isHidden }),
    });
    await loadChallenges();
    renderAdminChallenges();
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function createChallenge(event) {
  event.preventDefault();
  try {
    await api('/api/admin/challenges', {
      method: 'POST',
      body: JSON.stringify(challengeBody(event.target)),
    });
    await loadChallenges();
    renderAdmin('challenges');
    toast('challenge added');
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function deleteChallenge(challenge) {
  if (!challenge) return;
  if (!window.confirm(`Delete "${challenge.title}"? Its solves, hints and attachments are removed and team scores recalculated.`)) return;
  try {
    await api(`/api/admin/challenges/${challenge.id}`, { method: 'DELETE' });
    await loadChallenges();
    renderAdmin('challenges');
    toast('challenge deleted');
  } catch (error) {
    toast(error.message, 'error');
  }
}

function openEditChallengeModal(challenge) {
  if (!challenge) return;
  const modal = document.getElementById('modal');
  modal.innerHTML = `
    <div class="modal-panel">
      <button class="modal-close" type="button" aria-label="Close">x</button>
      <h2>Edit Challenge</h2>
      <form id="edit-challenge-form" class="admin-form">
        ${challengeFormFields(challenge, state.adminChallenges || [])}
        <button type="submit">Save</button>
      </form>
      <section class="editor-section">
        <h3 class="section-label">Hints</h3>
        <div id="hint-editor"><p class="muted">loading…</p></div>
      </section>
      <section class="editor-section">
        <h3 class="section-label">Attachments (absolute URLs)</h3>
        <div id="file-editor"><p class="muted">loading…</p></div>
      </section>
    </div>
  `;
  modal.classList.add('open');
  modal.setAttribute('aria-hidden', 'false');
  modal.querySelector('.modal-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (event) => {
    if (event.target === modal) closeModal();
  }, { once: true });
  const form = modal.querySelector('#edit-challenge-form');
  wireChallengeForm(form);
  form.addEventListener('submit', (event) => updateChallenge(event, challenge));
  modal.querySelector('#reveal-flag-btn').addEventListener('click', () => revealFlag(challenge.id));
  renderHintEditor(challenge.id);
  renderFileEditor(challenge.id);
}

async function updateChallenge(event, challenge) {
  event.preventDefault();
  const body = challengeBody(event.target);
  if (!body.flag) delete body.flag;
  try {
    await api(`/api/admin/challenges/${challenge.id}`, {
      method: 'PUT',
      body: JSON.stringify(body),
    });
    closeModal();
    await loadChallenges();
    renderAdminChallenges();
    toast('challenge saved');
  } catch (error) {
    toast(error.message, 'error');
  }
}

// Decrypts the stored copy for verification (audited). Players' submissions
// are always checked against the hash, never against this value.
async function revealFlag(challengeId) {
  const output = document.getElementById('revealed-flag');
  try {
    const result = await api(`/api/admin/challenges/${challengeId}/flag`);
    if (result.error) {
      output.textContent = result.error;
    } else if (result.stored) {
      output.textContent = `${result.flag_type === 'regex' ? 'pattern: ' : ''}${result.flag}`;
    } else {
      output.textContent = 'not stored — re-enter the flag to enable reveal';
    }
    output.classList.toggle('muted', !result.flag);
    output.hidden = false;
  } catch (error) {
    toast(error.message, 'error');
  }
}

// ---- admin hint editor ----

async function renderHintEditor(challengeId) {
  const container = document.getElementById('hint-editor');
  if (!container) return;
  let hints;
  try {
    hints = await api(`/api/admin/challenges/${challengeId}/hints`);
  } catch (error) {
    container.innerHTML = emptyState(error.message);
    return;
  }
  container.innerHTML = `
    ${hints.map((hint, index) => `
      <div class="editor-row" data-hint-row="${hint.id}">
        <textarea name="content" rows="3" maxlength="4000">${escapeHtml(hint.content)}</textarea>
        <div class="editor-controls">
          <label><span>Cost</span><input name="cost_points" type="number" min="0" value="${hint.cost_points}"></label>
          <span class="muted">#${index + 1} · unlocked by ${hint.unlock_count} team${hint.unlock_count === 1 ? '' : 's'}</span>
          <button type="button" data-hint-move="-1" ${index === 0 ? 'disabled' : ''} title="Move up">↑</button>
          <button type="button" data-hint-move="1" ${index === hints.length - 1 ? 'disabled' : ''} title="Move down">↓</button>
          <button type="button" data-hint-save>Save</button>
          <button type="button" data-hint-delete>Delete</button>
        </div>
      </div>
    `).join('') || '<p class="muted">No hints yet.</p>'}
    <form id="add-hint-form" class="editor-row">
      <textarea name="content" rows="3" maxlength="4000" placeholder="new hint text" required></textarea>
      <div class="editor-controls">
        <label><span>Cost</span><input name="cost_points" type="number" min="0" value="0" required></label>
        <button type="submit">Add hint</button>
      </div>
    </form>
  `;

  const reload = () => {
    renderHintEditor(challengeId);
    loadChallenges();
    renderAdminChallenges();
  };
  container.querySelectorAll('[data-hint-row]').forEach((row) => {
    const hint = hints.find((h) => h.id === Number(row.dataset.hintRow));
    row.querySelector('[data-hint-save]').addEventListener('click', async () => {
      const cost = Number(row.querySelector('[name="cost_points"]').value);
      if (hint.unlock_count > 0 && cost !== hint.cost_points
        && !window.confirm('Existing unlocks keep their original deduction. Change the cost for future unlocks?')) return;
      try {
        await api(`/api/admin/hints/${hint.id}`, {
          method: 'PUT',
          body: JSON.stringify({ content: row.querySelector('[name="content"]').value, cost_points: cost }),
        });
        toast('hint saved');
        reload();
      } catch (error) {
        toast(error.message, 'error');
      }
    });
    row.querySelector('[data-hint-delete]').addEventListener('click', async () => {
      const message = hint.unlock_count > 0
        ? `${hint.unlock_count} team${hint.unlock_count === 1 ? '' : 's'} will be refunded what they paid (currently ${hint.cost_points} pts). Delete?`
        : 'Delete this hint?';
      if (!window.confirm(message)) return;
      try {
        await api(`/api/admin/hints/${hint.id}`, { method: 'DELETE' });
        toast('hint deleted');
        await loadScoreboard();
        reload();
      } catch (error) {
        toast(error.message, 'error');
      }
    });
    row.querySelectorAll('[data-hint-move]').forEach((button) => {
      button.addEventListener('click', () => moveHint(hints, hint, Number(button.dataset.hintMove), reload));
    });
  });
  container.querySelector('#add-hint-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const data = Object.fromEntries(new FormData(event.target).entries());
    try {
      await api(`/api/admin/challenges/${challengeId}/hints`, {
        method: 'POST',
        body: JSON.stringify({ content: data.content, cost_points: Number(data.cost_points) }),
      });
      toast('hint added');
      reload();
    } catch (error) {
      toast(error.message, 'error');
    }
  });
}

// Reorder by rewriting sort_order as positions 1..n for the hints that move.
async function moveHint(hints, hint, delta, reload) {
  const order = [...hints];
  const from = order.indexOf(hint);
  const to = from + delta;
  if (to < 0 || to >= order.length) return;
  [order[from], order[to]] = [order[to], order[from]];
  try {
    for (const [index, item] of order.entries()) {
      if (item.sort_order !== index + 1) {
        await api(`/api/admin/hints/${item.id}`, {
          method: 'PUT',
          body: JSON.stringify({ sort_order: index + 1 }),
        });
      }
    }
    reload();
  } catch (error) {
    toast(error.message, 'error');
  }
}

// ---- admin attachment editor ----

async function renderFileEditor(challengeId) {
  const container = document.getElementById('file-editor');
  if (!container) return;
  let files;
  try {
    files = await api(`/api/admin/challenges/${challengeId}/files`);
  } catch (error) {
    container.innerHTML = emptyState(error.message);
    return;
  }
  container.innerHTML = `
    ${files.map((file) => {
      const valid = isAbsoluteUrl(file.storage_path);
      return `
        <div class="editor-row attachment-row${valid ? '' : ' needs-fix'}" data-file-row="${file.id}">
          <div class="editor-controls">
            <label><span>Label</span><input name="label" value="${escapeHtml(file.filename)}"></label>
            <label class="grow"><span>URL${valid ? '' : ' — needs absolute URL'}</span><input name="url" type="url" value="${escapeHtml(file.storage_path)}" placeholder="https://..."></label>
            ${valid ? `<a href="${escapeHtml(file.storage_path)}" target="_blank" rel="noopener noreferrer">open ↗</a>` : ''}
            <button type="button" data-file-save>Save</button>
            <button type="button" data-file-delete>Delete</button>
          </div>
        </div>
      `;
    }).join('') || '<p class="muted">No attachments yet.</p>'}
    <form id="add-file-form" class="editor-row">
      <div class="editor-controls">
        <label><span>Label</span><input name="label" placeholder="capture.pcap" required></label>
        <label class="grow"><span>URL</span><input name="url" type="url" placeholder="https://files.example.com/capture.pcap" required></label>
        <button type="submit">Add attachment</button>
      </div>
    </form>
  `;
  const reload = () => {
    renderFileEditor(challengeId);
    loadChallenges();
    renderAdminChallenges();
  };
  container.querySelectorAll('[data-file-row]').forEach((row) => {
    const id = Number(row.dataset.fileRow);
    row.querySelector('[data-file-save]').addEventListener('click', async () => {
      try {
        await api(`/api/admin/files/${id}`, {
          method: 'PUT',
          body: JSON.stringify({
            label: row.querySelector('[name="label"]').value,
            url: row.querySelector('[name="url"]').value,
          }),
        });
        toast('attachment saved');
        reload();
      } catch (error) {
        toast(error.message, 'error');
      }
    });
    row.querySelector('[data-file-delete]').addEventListener('click', async () => {
      if (!window.confirm('Delete this attachment link?')) return;
      try {
        await api(`/api/admin/files/${id}`, { method: 'DELETE' });
        toast('attachment deleted');
        reload();
      } catch (error) {
        toast(error.message, 'error');
      }
    });
  });
  container.querySelector('#add-file-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const data = Object.fromEntries(new FormData(event.target).entries());
    try {
      await api(`/api/admin/challenges/${challengeId}/files`, {
        method: 'POST',
        body: JSON.stringify({ label: data.label, url: data.url }),
      });
      toast('attachment added');
      reload();
    } catch (error) {
      toast(error.message, 'error');
    }
  });
}

async function renderAdminUsers() {
  const content = document.getElementById('admin-content');
  try {
    const users = await api('/api/admin/users');
    content.innerHTML = `
      <div class="section-head">
        <h2>Users</h2>
        <button type="button" id="new-user-btn">New user</button>
      </div>
      <table class="scoreboard">
        <thead><tr><th>ID</th><th>Username</th><th>Role</th><th>Team</th><th>Admin</th><th>Ban</th><th>Actions</th></tr></thead>
        <tbody>${users.map(adminUserRow).join('') || '<tr><td colspan="7">No users.</td></tr>'}</tbody>
      </table>
    `;
    document.getElementById('new-user-btn').addEventListener('click', openNewUserModal);
    document.querySelectorAll('[data-user-team]').forEach((button) => {
      const user = users.find((item) => item.id === Number(button.dataset.userTeam));
      button.addEventListener('click', () => openUserTeamModal(user));
    });
    document.querySelectorAll('[data-user-admin]').forEach((input) => {
      input.addEventListener('change', () => {
        updateUserRole(Number(input.dataset.userAdmin), input.checked ? 'admin' : 'player');
      });
    });
    document.querySelectorAll('[data-user-ban]').forEach((input) => {
      input.addEventListener('change', () => {
        updateUserRole(Number(input.dataset.userBan), input.checked ? 'banned' : 'player');
      });
    });
    document.querySelectorAll('[data-user-password]').forEach((button) => {
      const id = Number(button.dataset.userPassword);
      const user = users.find((item) => item.id === id);
      button.addEventListener('click', () => openAdminPasswordModal(user));
    });
  } catch (error) {
    content.innerHTML = emptyState(error.message);
  }
}

function adminUserRow(user) {
  const role = String(user.role || 'player');
  return `
    <tr>
      <td>${user.id}</td>
      <td>${escapeHtml(user.username)}</td>
      <td>${escapeHtml(role)}</td>
      <td>${user.team_name ? escapeHtml(user.team_name) : '<span class="muted">-</span>'}</td>
      <td>${toggleCell('Admin', `data-user-admin="${user.id}"`, role === 'admin')}</td>
      <td>${toggleCell('Ban', `data-user-ban="${user.id}"`, role === 'banned')}</td>
      <td>
        <button type="button" data-user-team="${user.id}">Team</button>
        <button type="button" data-user-password="${user.id}">Password</button>
      </td>
    </tr>
  `;
}

async function updateUserRole(id, role) {
  try {
    await api(`/api/admin/users/${id}/role`, {
      method: 'PUT',
      body: JSON.stringify({ role }),
    });
    toast(`user role set to ${role}`);
    renderAdminUsers();
  } catch (error) {
    toast(error.message, 'error');
    renderAdminUsers();
  }
}

// Team picker shared by "New user" and "Team" (reassign). Returns markup.
function teamPickerFields(teams, currentTeamId) {
  return `
    <fieldset class="team-picker">
      <legend>Team</legend>
      <label class="radio-row"><input type="radio" name="team_mode" value="none" ${currentTeamId ? '' : 'checked'}> No team</label>
      <label class="radio-row"><input type="radio" name="team_mode" value="existing" ${currentTeamId ? 'checked' : ''} ${teams.length ? '' : 'disabled'}> Existing team</label>
      <select name="existing_id" ${teams.length ? '' : 'disabled'}>
        ${teams.map((team) => `<option value="${team.id}" ${team.id === currentTeamId ? 'selected' : ''}>${escapeHtml(team.name)}</option>`).join('')}
      </select>
      <label class="radio-row"><input type="radio" name="team_mode" value="new"> New team</label>
      <input name="new_name" placeholder="new team name">
    </fieldset>
  `;
}

function wireTeamPicker(form) {
  const sync = () => {
    const mode = form.querySelector('[name="team_mode"]:checked')?.value;
    form.querySelector('[name="existing_id"]').disabled = mode !== 'existing';
    form.querySelector('[name="new_name"]').disabled = mode !== 'new';
    if (mode === 'new') form.querySelector('[name="new_name"]').focus();
  };
  form.querySelectorAll('[name="team_mode"]').forEach((radio) => radio.addEventListener('change', sync));
  form.querySelector('[name="existing_id"]').addEventListener('focus', () => {
    form.querySelector('[name="team_mode"][value="existing"]').checked = true;
  });
  sync();
}

function teamAssignment(data) {
  if (data.team_mode === 'existing') return { existing_id: Number(data.existing_id) };
  if (data.team_mode === 'new') {
    if (!data.new_name?.trim()) throw new Error('enter a team name');
    return { new_name: data.new_name.trim() };
  }
  return null;
}

function openModalPanel(html) {
  const modal = document.getElementById('modal');
  modal.innerHTML = `<div class="modal-panel"><button class="modal-close" type="button" aria-label="Close">x</button>${html}</div>`;
  modal.classList.add('open');
  modal.setAttribute('aria-hidden', 'false');
  modal.querySelector('.modal-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (event) => {
    if (event.target === modal) closeModal();
  }, { once: true });
  return modal;
}

async function openNewUserModal() {
  let teams;
  try {
    teams = await api('/api/admin/teams');
  } catch (error) {
    toast(error.message, 'error');
    return;
  }
  const modal = openModalPanel(`
    <h2>New User</h2>
    <p class="muted">Create an account and hand out the credentials. Works even when registration is closed.</p>
    <form id="new-user-form" class="admin-form">
      <input name="username" autocomplete="off" placeholder="username (3–32: letters, digits, _ -)" required>
      <input name="password" type="password" autocomplete="new-password" placeholder="password (min 8 chars)" required>
      <input name="password_confirm" type="password" autocomplete="new-password" placeholder="confirm password" required>
      <label class="toggle-row">
        <span>Admin</span>
        <span class="toggle-switch">
          <input type="checkbox" name="is_admin">
          <span class="toggle-slider"></span>
        </span>
      </label>
      ${teamPickerFields(teams, null)}
      <button type="submit">Create User</button>
    </form>
  `);
  const form = modal.querySelector('#new-user-form');
  wireTeamPicker(form);
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const data = Object.fromEntries(new FormData(form).entries());
    if (data.password !== data.password_confirm) {
      toast('passwords do not match', 'error');
      return;
    }
    try {
      const user = await api('/api/admin/users', {
        method: 'POST',
        body: JSON.stringify({
          username: data.username.trim(),
          password: data.password,
          password_confirm: data.password_confirm,
          role: 'is_admin' in data ? 'admin' : 'player',
          team: teamAssignment(data),
        }),
      });
      closeModal();
      toast(`created ${user.username}${user.team_name ? ` on ${user.team_name}` : ''}`);
      await loadScoreboard();
      renderAdminUsers();
    } catch (error) {
      toast(error.message, 'error');
    }
  });
}

async function openUserTeamModal(user) {
  if (!user) return;
  let teams;
  try {
    teams = await api('/api/admin/teams');
  } catch (error) {
    toast(error.message, 'error');
    return;
  }
  const modal = openModalPanel(`
    <h2>Assign Team</h2>
    <p class="muted">${escapeHtml(user.username)} · currently ${user.team_name ? escapeHtml(user.team_name) : 'no team'}. Past solves stay with the team that earned them; the user is logged out.</p>
    <form id="user-team-form" class="admin-form">
      ${teamPickerFields(teams, user.team_id)}
      <button type="submit">Save</button>
    </form>
  `);
  const form = modal.querySelector('#user-team-form');
  wireTeamPicker(form);
  form.addEventListener('submit', async (event) => {
    event.preventDefault();
    const data = Object.fromEntries(new FormData(form).entries());
    try {
      const updated = await api(`/api/admin/users/${user.id}/team`, {
        method: 'PUT',
        body: JSON.stringify({ team: teamAssignment(data) }),
      });
      closeModal();
      if (state.user && state.user.id === user.id) {
        clearLocalSession('team changed; please log in again');
        return;
      }
      toast(`${updated.username}: ${updated.team_name || 'no team'}`);
      await loadScoreboard();
      renderAdminUsers();
    } catch (error) {
      toast(error.message, 'error');
    }
  });
}

function openAdminPasswordModal(user) {
  if (!user) return;
  const modal = document.getElementById('modal');
  modal.innerHTML = `
    <div class="modal-panel">
      <button class="modal-close" type="button" aria-label="Close">x</button>
      <h2>Set Password</h2>
      <p class="muted">${escapeHtml(user.username)}</p>
      <form id="admin-password-form" class="admin-form">
        <input name="password" type="password" autocomplete="new-password" placeholder="new password (min 8 chars)" required>
        <input name="password_confirm" type="password" autocomplete="new-password" placeholder="confirm new password" required>
        <button type="submit">Set Password</button>
      </form>
    </div>
  `;
  modal.classList.add('open');
  modal.setAttribute('aria-hidden', 'false');
  modal.querySelector('.modal-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (event) => {
    if (event.target === modal) closeModal();
  }, { once: true });
  modal.querySelector('#admin-password-form').addEventListener('submit', (event) =>
    updateUserPassword(event, user.id),
  );
}

async function updateUserPassword(event, id) {
  event.preventDefault();
  const data = Object.fromEntries(new FormData(event.target).entries());
  if (data.password !== data.password_confirm) {
    toast('passwords do not match', 'error');
    return;
  }
  try {
    await api(`/api/admin/users/${id}/password`, {
      method: 'PUT',
      body: JSON.stringify({
        password: data.password,
        password_confirm: data.password_confirm,
      }),
    });
    closeModal();
    if (state.user && state.user.id === id) {
      clearLocalSession('password updated; please log in again');
    } else {
      toast('password updated');
    }
  } catch (error) {
    toast(error.message, 'error');
  }
}

async function renderAdminTeams() {
  const content = document.getElementById('admin-content');
  try {
    const teams = await api('/api/admin/teams');
    content.innerHTML = `
      <h2>Teams</h2>
      <table class="scoreboard">
        <thead><tr><th>ID</th><th>Name</th><th>Score</th><th>Status</th><th>Ban</th></tr></thead>
        <tbody>${teams.map(adminTeamRow).join('') || '<tr><td colspan="5">No teams.</td></tr>'}</tbody>
      </table>
    `;
    document.querySelectorAll('[data-team-ban]').forEach((input) => {
      input.addEventListener('change', () => {
        updateTeamDisqualified(Number(input.dataset.teamBan), input.checked);
      });
    });
  } catch (error) {
    content.innerHTML = emptyState(error.message);
  }
}

function adminTeamRow(team) {
  const banned = Boolean(team.is_disqualified);
  return `
    <tr>
      <td>${team.id}</td>
      <td>${escapeHtml(team.name)}</td>
      <td>${team.score || 0}</td>
      <td>${banned ? 'banned' : 'active'}</td>
      <td>${toggleCell('Ban', `data-team-ban="${team.id}"`, banned)}</td>
    </tr>
  `;
}

async function updateTeamDisqualified(id, isDisqualified) {
  try {
    await api(`/api/admin/teams/${id}/disqualified`, {
      method: 'PUT',
      body: JSON.stringify({ is_disqualified: isDisqualified }),
    });
    await loadScoreboard();
    toast(isDisqualified ? 'team banned' : 'team unbanned');
    renderAdminTeams();
  } catch (error) {
    toast(error.message, 'error');
    renderAdminTeams();
  }
}

function toggleCell(label, dataAttribute, checked) {
  return `
    <label class="toggle-control" title="${label}">
      <span>${label}</span>
      <span class="toggle-switch">
        <input type="checkbox" ${dataAttribute} ${checked ? 'checked' : ''}>
        <span class="toggle-slider"></span>
      </span>
    </label>
  `;
}

async function renderAdminSettings() {
  const content = document.getElementById('admin-content');
  let settings;
  try {
    [settings] = await Promise.all([api('/api/admin/settings'), loadCompetition()]);
  } catch (error) {
    content.innerHTML = emptyState(error.message);
    return;
  }
  const c = settings.competition;
  const status = state.competition || {};
  const now = Date.now() / 1000;
  const statusText = !status.started ? 'not started' : status.ended ? 'ended' : status.frozen_at && status.frozen_at <= now ? 'running (scoreboard frozen)' : 'running';
  const row = (label, value) => `<tr><th>${label}</th><td>${escapeHtml(String(value ?? '-'))}</td></tr>`;
  const branding = settings.branding || {};
  content.innerHTML = `
    <h2>Settings</h2>
    <section class="settings-block">
      <h3 class="section-label">Branding</h3>
      <form id="branding-form" class="admin-form">
        <label><span>Competition name (blank uses config.toml: ${escapeHtml(c.name)})</span>
          <input name="name" maxlength="64" value="${branding.name && branding.name !== c.name ? escapeHtml(branding.name) : ''}" placeholder="${escapeHtml(c.name)}">
        </label>
        <label><span>Logo URL (absolute http/https; blank uses the built-in logo)</span>
          <input name="logo_url" type="url" value="${escapeHtml(branding.logo_url || '')}" placeholder="https://example.org/logo.png">
        </label>
        <div class="branding-preview">
          <img id="branding-preview-logo" class="brand-icon" alt="logo preview" src="${escapeHtml(branding.logo_url || appPath('/feral10.jpg'))}">
          <strong id="branding-preview-name">${escapeHtml(branding.name || c.name)}</strong>
        </div>
        <small class="muted">Square images around 120×120 px look best.</small>
        <button type="submit">Save branding</button>
      </form>
    </section>
    <section class="settings-block">
      <h3 class="section-label">Competition</h3>
      <p class="muted">These values come from config.toml and are read-only here; edit the file and restart to change them.</p>
      <table class="scoreboard settings-table">
        ${row('Default name', c.name)}
        ${row('Start time', c.start_time || 'not set')}
        ${row('End time', c.end_time || 'not set')}
        ${row('Team mode', c.team_mode ? 'on' : 'off')}
        ${row('Max team size', c.max_team_size)}
        ${row('Registration', c.registration_open ? 'open' : 'closed (admins create users)')}
        ${row('Dynamic scoring', c.dynamic_scoring ? 'on' : 'off')}
        ${row('Freeze before end', c.score_freeze_minutes_before_end ? `${c.score_freeze_minutes_before_end} min` : 'off')}
      </table>
    </section>
    <section class="settings-block">
      <h3 class="section-label">Controls · status: ${escapeHtml(statusText)}</h3>
      <div class="button-row">
        <button type="button" data-competition="start">Start / reopen</button>
        <button type="button" data-competition="freeze">Freeze scoreboard</button>
        <button type="button" data-competition="end">End</button>
      </div>
    </section>
    <section class="settings-block">
      <h3 class="section-label">Announcement</h3>
      <form id="announce-form" class="admin-form">
        <input name="title" placeholder="title" required>
        <textarea name="body" rows="3" placeholder="message"></textarea>
        <button type="submit">Send to everyone</button>
      </form>
    </section>
    <section class="settings-block">
      <h3 class="section-label">Export &amp; backup</h3>
      <div class="button-row">
        <button type="button" id="export-json">Export challenges (JSON)</button>
        <button type="button" id="backup-db">Backup database</button>
      </div>
    </section>
    <section class="settings-block">
      <h3 class="section-label">Import</h3>
      <form id="import-form" class="admin-form">
        <label><span>Bundle (FeralCTF or CTFd JSON)</span><input name="file" type="file" accept=".json,application/json" required></label>
        <label><span>Attachments zip (optional, legacy)</span><input name="attachments" type="file" accept=".zip,application/zip"></label>
        <label class="toggle-row"><span>Overwrite existing</span><span class="toggle-switch"><input type="checkbox" name="overwrite"><span class="toggle-slider"></span></span></label>
        <label class="toggle-row"><span>Dry run</span><span class="toggle-switch"><input type="checkbox" name="dry_run" checked><span class="toggle-slider"></span></span></label>
        <small class="muted">max ${settings.max_import_mb} MB</small>
        <button type="submit">Import</button>
      </form>
      <div id="import-result"></div>
    </section>
  `;
  const brandingForm = document.getElementById('branding-form');
  const previewLogo = document.getElementById('branding-preview-logo');
  const previewName = document.getElementById('branding-preview-name');
  previewLogo.addEventListener('error', () => {
    previewLogo.alt = 'logo could not be loaded';
  });
  brandingForm.addEventListener('input', () => {
    previewName.textContent = brandingForm.name.value.trim() || c.name;
    const url = brandingForm.logo_url.value.trim();
    // Preview only absolute URLs; the CSP allows the saved logo's origin
    // once branding is saved, so a new origin may not preview until then.
    previewLogo.src = isAbsoluteUrl(url) ? url : appPath('/feral10.jpg');
  });
  brandingForm.addEventListener('submit', async (event) => {
    event.preventDefault();
    const origin = (url) => (isAbsoluteUrl(url) ? new URL(url).origin : '');
    const previousOrigin = origin(branding.logo_url);
    try {
      const saved = await api('/api/admin/branding', {
        method: 'PUT',
        body: JSON.stringify({
          name: brandingForm.name.value.trim() || null,
          logo_url: brandingForm.logo_url.value.trim() || null,
        }),
      });
      // The CSP allows the logo's origin per page load, so a new origin
      // needs a reload before the browser will display it.
      if (origin(saved.logo_url) && origin(saved.logo_url) !== previousOrigin) {
        sessionStorage.setItem('feralctf_return_view', 'admin-settings');
        window.location.reload();
        return;
      }
      await loadCompetition();
      toast('branding saved');
      renderAdminSettings();
    } catch (error) {
      toast(error.message, 'error');
    }
  });
  content.querySelectorAll('[data-competition]').forEach((button) => {
    button.addEventListener('click', async () => {
      const action = button.dataset.competition;
      if (action === 'end' && !window.confirm('End the competition? Players can no longer submit.')) return;
      try {
        await api(`/api/admin/competition/${action}`, { method: 'POST' });
        toast(`competition: ${action}`);
        renderAdminSettings();
      } catch (error) {
        toast(error.message, 'error');
      }
    });
  });
  document.getElementById('announce-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const data = Object.fromEntries(new FormData(event.target).entries());
    try {
      await api('/api/admin/announce', { method: 'POST', body: JSON.stringify({ title: data.title, body: data.body }) });
      event.target.reset();
      toast('announcement sent');
    } catch (error) {
      toast(error.message, 'error');
    }
  });
  document.getElementById('export-json').addEventListener('click', () => downloadAuthed('/api/admin/export', 'feralctf-export.json'));
  document.getElementById('backup-db').addEventListener('click', () => downloadAuthed('/api/admin/backup', 'feralctf-backup.db'));
  document.getElementById('import-form').addEventListener('submit', async (event) => {
    event.preventDefault();
    const form = event.target;
    const body = new FormData();
    body.append('file', form.file.files[0]);
    if (form.attachments.files[0]) body.append('attachments', form.attachments.files[0]);
    body.append('overwrite', form.overwrite.checked ? 'true' : 'false');
    body.append('dry_run', form.dry_run.checked ? 'true' : 'false');
    try {
      const result = await api('/api/admin/import', { method: 'POST', body });
      document.getElementById('import-result').innerHTML = importResultView(result, form.dry_run.checked);
      if (!form.dry_run.checked) await loadChallenges();
    } catch (error) {
      toast(error.message, 'error');
    }
  });
}

function importResultView(result, dryRun) {
  const list = (title, items) => items?.length
    ? `<h4>${title}</h4><ul>${items.map((item) => `<li>${escapeHtml(item)}</li>`).join('')}</ul>`
    : '';
  return `
    <div class="import-result ${result.valid ? 'ok' : 'bad'}">
      <p><strong>${dryRun ? 'Dry run' : 'Imported'}:</strong>
        ${result.challenges_created} created · ${result.challenges_overwritten} overwritten · ${result.challenges_skipped} skipped
        ${result.valid ? '' : ' · <span class="bad">invalid bundle</span>'}</p>
      ${list('Validation errors', result.validation_errors)}
      ${list('Attachment warnings', result.attachment_warnings)}
    </div>
  `;
}

// Admin downloads need the Bearer token, so fetch and save via a blob URL.
async function downloadAuthed(path, fallbackName) {
  try {
    const response = await fetch(appPath(path), {
      headers: state.token ? { Authorization: `Bearer ${state.token}` } : {},
    });
    if (!response.ok) throw new Error(`${response.status} ${response.statusText}`);
    const disposition = response.headers.get('Content-Disposition') || '';
    const name = /filename="([^"]+)"/.exec(disposition)?.[1] || fallbackName;
    const url = URL.createObjectURL(await response.blob());
    const link = document.createElement('a');
    link.href = url;
    link.download = name;
    document.body.appendChild(link);
    link.click();
    link.remove();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  } catch (error) {
    toast(error.message, 'error');
  }
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
  const socket = new WebSocket(`${protocol}//${window.location.host}${appPath('/ws')}`);
  state.ws = socket;
  socket.addEventListener('open', () => {
    state.reconnects = 0;
  });
  socket.addEventListener('message', (event) => {
    const message = JSON.parse(event.data);
    if (message.type === 'score_update') {
      setScoreboard({
        teams: message.scoreboard || [],
        total_visible_points: message.total_visible_points ?? state.scoreboard.total_visible_points ?? 0,
      });
      if (state.view === 'scoreboard') renderScoreboard();
    } else if (message.type === 'announcement') {
      toast(`📣 ${message.title}`);
      loadAnnouncements().then(() => {
        if (state.view === 'challenges' && !document.getElementById('modal').classList.contains('open')) renderChallenges();
      });
    } else if (message.type === 'new_solve' && message.first_blood) {
      toast(`🩸 first blood: ${message.team} solved ${message.challenge}`);
    } else if (message.type === 'state_change') {
      loadCompetition();
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
    ...(options.body && !(options.body instanceof FormData) ? { 'Content-Type': 'application/json' } : {}),
    ...(state.token ? { Authorization: `Bearer ${state.token}` } : {}),
    ...(options.headers || {}),
  };
  const response = await fetch(appPath(path), { ...options, headers });
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

function normalizeBasePath(path) {
  const value = String(path || '').trim();
  if (!value || value === '/') return '';
  const prefixed = value.startsWith('/') ? value : `/${value}`;
  return prefixed.replace(/\/+$/, '');
}

function detectBasePath() {
  const metaPath = document.querySelector('meta[name="feralctf-base-path"]')?.content;
  const normalizedMetaPath = normalizeBasePath(metaPath || '');
  if (normalizedMetaPath) return normalizedMetaPath;

  const scriptSrc = document.currentScript?.getAttribute('src') || '';
  try {
    const scriptUrl = new URL(scriptSrc, window.location.href);
    return normalizeBasePath(scriptUrl.pathname.replace(/\/app\.js$/, ''));
  } catch (_) {
    return '';
  }
}

function appPath(path) {
  const value = String(path || '');
  if (/^[a-z][a-z0-9+.-]*:/i.test(value)) return value;
  const normalized = value.startsWith('/') ? value : `/${value}`;
  return `${basePath}${normalized}`;
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

function renderDescription(text) {
  const raw = String(text ?? '');
  const urlRegex = /https?:\/\/[^\s]+/g;
  let result = '';
  let lastIndex = 0;
  let match;
  while ((match = urlRegex.exec(raw)) !== null) {
    result += escapeHtml(raw.slice(lastIndex, match.index));
    const url = escapeHtml(match[0]);
    result += `<a href="${url}" target="_blank" rel="noopener noreferrer">${url}</a>`;
    lastIndex = urlRegex.lastIndex;
  }
  result += escapeHtml(raw.slice(lastIndex));
  return result;
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
