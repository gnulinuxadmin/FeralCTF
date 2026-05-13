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
  updateAuth();
  navigate('challenges');
}

function renderShell() {
  app.innerHTML = `
    <header class="topbar">
      <div class="brand">
        <img src="/feral10.jpg" class="brand-icon" alt="">
        <h1>Feral CTF</h1>
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
    <main id="view"></main>
    <div id="modal" class="modal" aria-hidden="true"></div>
    <div id="toast" class="toast" role="status"></div>
  `;

  document.querySelectorAll('[data-view]').forEach((button) => {
    button.addEventListener('click', () => navigate(button.dataset.view));
  });
  document.getElementById('auth-form').addEventListener('submit', loginUser);
  document.getElementById('show-register-btn').addEventListener('click', showRegisterModal);
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
  state.token = '';
  state.user = null;
  sessionStorage.removeItem('feralctf_token');
  renderShell();
  navigate('challenges');
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
  const categories = ['all', ...new Set(state.challenges.map((c) => c.category).filter(Boolean))];
  const challenges = filteredChallenges();

  view.innerHTML = `
    <section class="toolbar">
      <input id="challenge-search" class="search-input" value="${escapeHtml(state.query)}" placeholder="search challenges...">
      <div class="category-pills">
        ${categories.map((cat) => `<button class="cat-pill${state.selectedCategory === cat ? ' active' : ''}" data-cat="${escapeHtml(cat)}">${escapeHtml(cat)}</button>`).join('')}
      </div>
    </section>
    <section class="challenge-grid">
      ${challenges.map(challengeCard).join('') || emptyState('No visible challenges.')}
    </section>
  `;

  document.getElementById('challenge-search').addEventListener('input', (e) => {
    state.query = e.target.value;
    renderChallenges();
  });
  document.querySelectorAll('.cat-pill').forEach((btn) => {
    btn.addEventListener('click', () => {
      state.selectedCategory = btn.dataset.cat;
      renderChallenges();
    });
  });
  document.querySelectorAll('[data-challenge-id]').forEach((card) => {
    card.addEventListener('click', () => openChallenge(Number(card.dataset.challengeId)));
  });
}

function filteredChallenges() {
  const query = state.query.trim().toLowerCase();
  return state.challenges.filter((challenge) => {
    if (challenge.solved_by_team) return false;
    const categoryMatch = state.selectedCategory === 'all' || challenge.category === state.selectedCategory;
    const queryMatch = !query || challenge.title.toLowerCase().includes(query);
    return categoryMatch && queryMatch;
  });
}

function challengeCard(challenge) {
  const difficulty = difficultyFor(challenge.points);
  return `
    <article class="card challenge-card" data-challenge-id="${challenge.id}">
      <div class="card-top">
        <span class="category" style="--category-color:${categoryColor(challenge.category)}">${escapeHtml(challenge.category)}</span>
        ${challenge.solved_by_team ? '<span class="solved-flag">✓</span>' : ''}
      </div>
      <h2 class="card-title">${escapeHtml(challenge.title)}</h2>
      <div class="card-bottom">
        <strong class="card-pts">${challenge.points} <span class="pts-label muted">pts</span></strong>
        <span class="card-meta"><i class="dot ${difficulty}"></i>${difficulty} · ${challenge.solve_count} solves</span>
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
        <p class="description">${renderDescription(challenge.description)}</p>
        ${detail.files?.length ? `<div class="file-list">${detail.files.map(fileLink).join('')}</div>` : ''}
        ${detail.hints?.length ? `<div class="hint-list">${detail.hints.map((hint) => hintRow(challenge.id, hint)).join('')}</div>` : ''}
        <form id="flag-form" class="flag-form">
          <input id="flag-input" placeholder="FLAG{...}" autocomplete="off" required>
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
      <div class="scoreboard-header">
        <h2>Live Scoreboard</h2>
        <span class="live-dot">● live</span>
      </div>
      <table class="scoreboard">
        <thead><tr><th>#</th><th>Team</th><th>Solves</th><th>Progress</th><th>Score</th></tr></thead>
        <tbody>${state.scoreboard.teams.map((team) => scoreRow(team, maxScore)).join('') || '<tr><td colspan="5">No teams yet.</td></tr>'}</tbody>
      </table>
    </section>
  `;
}

function scoreRow(team, maxScore) {
  const isCurrent = state.user && state.user.team_id === team.team_id;
  const progress = Math.round((team.score / maxScore) * 100);
  const medals = ['🥇', '🥈', '🥉'];
  const rank = team.rank <= 3 ? medals[team.rank - 1] : team.rank;
  return `
    <tr class="${isCurrent ? 'current-team' : ''}">
      <td>${rank}</td>
      <td>${escapeHtml(team.team_name)}${isCurrent ? ' <span class="muted">(you)</span>' : ''}</td>
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
        <div><strong>0</strong><span>hints used</span></div>
        <div><strong>0</strong><span>first bloods</span></div>
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
      <h2>Solve History</h2>
      <div class="history">${solves.map(solveRow).join('') || '<p class="muted">No solves yet.</p>'}</div>
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
        ${['overview', 'challenges', 'users', 'teams', 'settings'].map((item) => `<button data-admin="${item}" class="${item === section ? 'active' : ''}">■ ${item}</button>`).join('')}
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

async function renderAdminChallenges() {
  const content = document.getElementById('admin-content');
  let challenges;
  try {
    challenges = await api('/api/admin/challenges');
  } catch (error) {
    content.innerHTML = emptyState(error.message);
    return;
  }
  content.innerHTML = `
    <h2>Challenges</h2>
    <form id="challenge-form" class="admin-form">
      <input name="title" placeholder="title" required>
      <input name="category" placeholder="category" required>
      <input name="points" type="number" placeholder="points" required>
      <input name="flag" placeholder="flag" required>
      <textarea name="description" placeholder="description" rows="6"></textarea>
      <label class="toggle-row">
        <span>Start visible</span>
        <span class="toggle-switch">
          <input type="checkbox" name="is_visible">
          <span class="toggle-slider"></span>
        </span>
      </label>
      <button type="submit">Add Challenge</button>
    </form>
    <table class="scoreboard">
      <thead><tr><th>Title</th><th>Category</th><th>Points</th><th>Visible</th><th>Actions</th></tr></thead>
      <tbody>${challenges.map(adminChallengeRow).join('') || '<tr><td colspan="5">No challenges.</td></tr>'}</tbody>
    </table>
  `;
  document.getElementById('challenge-form').addEventListener('submit', createChallenge);
  document.querySelectorAll('[data-delete-challenge]').forEach((button) => {
    button.addEventListener('click', () => deleteChallenge(Number(button.dataset.deleteChallenge)));
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
      <td>${escapeHtml(challenge.title)}</td>
      <td>${escapeHtml(challenge.category)}</td>
      <td>${challenge.points}</td>
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
        flag_case_sensitive: false,
        points: Number(data.points),
        max_points: Number(data.points),
        min_points: Math.max(1, Math.floor(Number(data.points) / 5)),
        decay_rate: 10,
        author: null,
        tags: [],
        unlock_requires: null,
        is_hidden: !('is_visible' in data),
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

function openEditChallengeModal(challenge) {
  if (!challenge) return;
  const modal = document.getElementById('modal');
  modal.innerHTML = `
    <div class="modal-panel">
      <button class="modal-close" type="button" aria-label="Close">x</button>
      <h2>Edit Challenge</h2>
      <form id="edit-challenge-form" class="admin-form">
        <label><span>Title</span>
          <input name="title" value="${escapeHtml(challenge.title)}" required>
        </label>
        <label><span>Category</span>
          <input name="category" value="${escapeHtml(challenge.category)}" required>
        </label>
        <label><span>Points</span>
          <input name="points" type="number" value="${challenge.points}" required>
        </label>
        <label><span>New flag (leave blank to keep current)</span>
          <input name="flag" placeholder="flag{...}" autocomplete="off">
        </label>
        <label><span>Description</span>
          <textarea name="description" rows="6">${escapeHtml(challenge.description)}</textarea>
        </label>
        <label class="toggle-row">
          <span>Visible</span>
          <span class="toggle-switch">
            <input type="checkbox" name="is_visible" ${!challenge.is_hidden ? 'checked' : ''}>
            <span class="toggle-slider"></span>
          </span>
        </label>
        <button type="submit">Save</button>
      </form>
    </div>
  `;
  modal.classList.add('open');
  modal.setAttribute('aria-hidden', 'false');
  modal.querySelector('.modal-close').addEventListener('click', closeModal);
  modal.addEventListener('click', (event) => {
    if (event.target === modal) closeModal();
  }, { once: true });
  modal.querySelector('#edit-challenge-form').addEventListener('submit', (event) =>
    updateChallenge(event, challenge.id),
  );
}

async function updateChallenge(event, id) {
  event.preventDefault();
  const data = Object.fromEntries(new FormData(event.target).entries());
  const body = {
    title: data.title,
    category: data.category,
    points: Number(data.points),
    description: data.description || '',
    is_hidden: !('is_visible' in data),
  };
  if (data.flag.trim()) body.flag = data.flag.trim();
  try {
    await api(`/api/admin/challenges/${id}`, {
      method: 'PUT',
      body: JSON.stringify(body),
    });
    closeModal();
    renderAdminChallenges();
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
