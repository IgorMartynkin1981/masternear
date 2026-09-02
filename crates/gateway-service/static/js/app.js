import { api } from './api.js';

const state = { user: null, activeChat: null, pollTimer: null, unread: 0, orderAlerts: 0, ws: null, wsReconnectTimer: null };

const $ = (sel) => document.querySelector(sel);
const app = $('#app');

const BELL_SVG = `
<svg class="bell-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
  <path d="M18 8A6 6 0 0 0 6 8c0 7-3 9-3 9h18s-3-2-3-9"/>
  <path d="M13.73 21a2 2 0 0 1-3.46 0"/>
</svg>`;

const STAR_PATH = '<polygon points="12 2 15.09 8.26 22 9.27 17 14.14 18.18 21.02 12 17.77 5.82 21.02 7 14.14 2 9.27 8.91 8.26 12 2"/>';

function starSvg(filled) {
  return `<svg class="star${filled ? ' filled' : ''}" viewBox="0 0 24 24">${STAR_PATH}</svg>`;
}

function starsRow(value, count, interactive = false, userAttr = null) {
  const avg = Math.round((value || 0) * 2) / 2;
  const stars = [1, 2, 3, 4, 5]
    .map((n) => `<span class="star-slot" data-val="${n}">${starSvg(avg >= n)}</span>`)
    .join('');
  const info = `<span class="stars-info">${Number(value || 0).toFixed(1)} (${count || 0})</span>`;
  const cls = interactive ? 'stars interactive' : 'stars';
  const attr = interactive ? ` data-user="${userAttr}"` : '';
  return `<div class="${cls}"${attr}>${stars}${info}</div>`;
}

function esc(s) {
  return String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}

function toast(msg, isErr = false) {
  const t = $('#toast');
  t.textContent = msg;
  t.className = 'toast show' + (isErr ? ' err' : '');
  clearTimeout(t._t);
  t._t = setTimeout(() => (t.className = 'toast'), 2600);
}

function money(v) {
  const n = Number(v);
  return Number.isInteger(n) ? n + ' BYN' : n.toFixed(2) + ' BYN';
}

function initials(name) {
  return esc(String(name).trim().slice(0, 1).toUpperCase() || '?');
}

async function withError(fn) {
  try {
    return await fn();
  } catch (e) {
    toast(e.message, true);
    return null;
  }
}

/* ------------------------- Навигация ------------------------- */

function renderNav() {
  const links = $('#nav-links');
  const actions = $('#nav-actions');
  const cur = location.hash;
  const items = [['Главная', '#/'], ['Мастера', '#/catalog']];
  if (state.user) {
    items.push(['Заказы', '#/orders', state.user.role === 'master' ? 'orders' : false]);
    items.push(['Чаты', '#/chats', 'chat']);
  }
  if (state.user?.role === 'master') items.push(['Панель мастера', '#/profile']);
  if (state.user?.role === 'admin') items.push(['Админ-панель', '#/admin']);

  links.innerHTML = items
    .map(([t, h, badge]) => {
      const active = cur === h ? ' active' : '';
      if (badge === 'chat') {
        const b =
          state.unread > 0
            ? `<span class="nav-badge">${state.unread > 99 ? '99+' : state.unread}</span>`
            : '';
        return `<a class="nav-link${active}" href="${h}">${BELL_SVG}<span>${t}</span>${b}</a>`;
      }
      if (badge === 'orders') {
        const b =
          state.orderAlerts > 0
            ? `<span class="nav-badge orders">${state.orderAlerts > 99 ? '99+' : state.orderAlerts}</span>`
            : '';
        return `<a class="nav-link${active}" href="${h}"><svg class="bell-icon orders" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/></svg><span>${t}</span>${b}</a>`;
      }
      return `<a class="nav-link${active}" href="${h}">${t}</a>`;
    })
    .join('');

  document.title = state.unread > 0 ? `(${state.unread}) MasterNear — новые сообщения` : 'MasterNear — найди мастера на дом';

  if (state.user) {
    const role = state.user.role === 'master' ? 'Мастер' : 'Заказчик';
    actions.innerHTML = `
      <span class="chip" style="text-transform:capitalize;">${esc(state.user.name)} <em>${role}</em></span>
      <button class="btn ghost" id="logout">Выйти</button>`;
    $('#logout').onclick = () => {
      localStorage.removeItem('token');
      state.user = null;
      state.unread = 0;
      state.orderAlerts = 0;
      stopPoll();
      toast('Вы вышли');
      location.hash = '#/';
      renderNav();
      route();
    };
  } else {
    actions.innerHTML = `
      <a class="btn ghost" href="#/auth">Войти</a>
      <a class="btn primary" href="#/auth?mode=register">Регистрация</a>`;
  }
}

function route() {
  const [path, qs] = location.hash.replace(/^#/, '').split('?');
  const params = new URLSearchParams(qs || '');
  if (!path || path === '/') return renderHome();
  if (path === '/auth') return renderAuth(params.get('mode') === 'register');
  if (path === '/catalog') return renderCatalog();
  if (path === '/chat') return renderChatThread(params.get('id'));
  if (path === '/chats') return renderChats();
  if (path === '/orders') return renderOrders();
  if (path === '/profile') return renderProfile();
  if (path === '/admin') return renderAdmin();
  if (path === '/feedback') return renderFeedback();
  renderHome();
}

function navigate(hash) {
  location.hash = hash;
}

window.addEventListener('hashchange', () => {
  if (state.activeChat && !location.hash.startsWith('#/chat')) {
    stopPoll();
    state.activeChat = null;
  }
  renderNav();
  route();
});

function stopPoll() {
  if (state.pollTimer) clearInterval(state.pollTimer);
  state.pollTimer = null;
  disconnectWs();
}

function disconnectWs() {
  if (state.ws) {
    state.ws.close();
    state.ws = null;
  }
  if (state.wsReconnectTimer) {
    clearTimeout(state.wsReconnectTimer);
    state.wsReconnectTimer = null;
  }
}

function connectWebSocket(conversationId) {
  disconnectWs();
  const token = localStorage.getItem('token');
  if (!token) return;

  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const url = `${proto}//${location.host}/ws?token=${encodeURIComponent(token)}&conversation_id=${conversationId}`;

  const ws = new WebSocket(url);
  state.ws = ws;

  ws.onopen = () => {
    console.log('[WS] connected to conversation', conversationId);
  };

  ws.onmessage = (evt) => {
    try {
      const data = JSON.parse(evt.data);
      if (data.type === 'message' && data.conversation_id === state.activeChat) {
        appendMessage(data);
        refreshUnread();
      } else if (data.type === 'typing' && data.conversation_id === state.activeChat && data.user_id !== state.user.id) {
        const ind = $('#typing-indicator');
        if (ind) {
          ind.style.display = 'block';
          clearTimeout(ind._hide);
          ind._hide = setTimeout(() => { ind.style.display = 'none'; }, 2500);
        }
      }
    } catch {}
  };

  ws.onclose = () => {
    console.log('[WS] disconnected');
    if (state.activeChat && location.hash === '#/chat?id=' + state.activeChat) {
      state.wsReconnectTimer = setTimeout(() => connectWebSocket(conversationId), 3000);
    }
  };

  ws.onerror = (err) => {
    console.error('[WS] error', err);
  };
}

function appendMessage(m) {
  const box = $('#msgs');
  if (!box) return;
  const empty = box.querySelector('.empty');
  if (empty) empty.remove();

  const isMine = m.sender_id === state.user.id;
  const div = document.createElement('div');
  div.className = `msg ${isMine ? 'mine' : 'theirs'}`;
  const time = m.created_at ? new Date(m.created_at).toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' }) : '';
  div.innerHTML = `${esc(m.text)}<span class="time">${time}</span>`;
  box.appendChild(div);
  box.scrollTop = box.scrollHeight;
}

function sendWsMessage() {
  const input = $('#chat-input');
  const text = input.value.trim();
  if (!text || !state.ws || state.ws.readyState !== WebSocket.OPEN) return;
  input.value = '';
  state.ws.send(JSON.stringify({ type: 'message', text }));
}

async function refreshUnread() {
  if (!state.user) {
    if (state.unread) {
      state.unread = 0;
      renderNav();
    }
    return;
  }
  const convs = await api.chats().catch(() => null);
  if (!convs) return;
  const total = convs.reduce((s, c) => s + (c.unread_count || 0), 0);
  if (total !== state.unread) {
    state.unread = total;
    renderNav();
  }
}

async function refreshOrderAlerts() {
  if (!state.user || state.user.role !== 'master') return;
  const orders = await api.orders().catch(() => null);
  if (!orders) return;
  const newOnes = orders.filter((o) => o.status === 'open' && !o.my_offer).length;
  if (newOnes !== state.orderAlerts) {
    state.orderAlerts = newOnes;
    renderNav();
  }
}

/* ------------------------- Лендинг ------------------------- */

function renderHome() {
  const cta = state.user
    ? `<a class="btn primary big" href="#/catalog">Выбрать мастера</a>`
    : `<a class="btn primary big" href="#/auth?mode=register">Начать бесплатно</a>`;
  app.innerHTML = `
    <section class="hero">
      <div class="hero-badge">Ремонт и бытовые услуги без посредников</div>
      <h1>Найдите мастера <span class="grad">на дом</span><br>за пару минут</h1>
      <p>Сантехника, электрика, сборка мебели, уборка и многое другое. Выбирайте по честным ценникам и договаривайтесь прямо в чате.</p>
      <div class="hero-actions">
        ${cta}
        <a class="btn ghost big" href="#/catalog">Смотреть мастеров</a>
      </div>
    </section>
    <section class="features">
      <div class="feature"><div class="ico">1</div><h3>Честные ценники</h3><p>Мастер указывает стоимость работы заранее — никаких сюрпризов после выезда.</p></div>
      <div class="feature"><div class="ico">2</div><h3>Прямой чат</h3><p>Обсуждайте детали и время визита напрямую с мастером, без посредников.</p></div>
      <div class="feature"><div class="ico">3</div><h3>Выбор по рейтингу</h3><p>Сравнивайте мастеров по категориям, ценам и опыту.</p></div>
      <div class="feature"><div class="ico">4</div><h3>Без скрытых платежей</h3><p>Сервис не берёт комиссий и не требует предоплат.</p></div>
    </section>
    <footer>MasterNear — найдите мастера на дом за минуты · <a href="#/feedback" class="muted">Обратная связь</a></footer>`;
}

/* ------------------------- Каталог ------------------------- */

let activeCategory = null;
let geoSearch = { lat: null, lng: null, radius: 50, enabled: false };

function renderCatalog() {
  app.innerHTML = `
    <section class="page">
      <h1>Мастера рядом</h1>
      <p class="lead">Выберите категорию работ, укажите свой адрес — и найдите мастеров поблизости.</p>
      <div class="geo-search">
        <input id="g-place" placeholder="Ваш город или адрес — например «Минск, Немига»" />
        <input id="g-radius" type="number" min="1" max="300" value="${geoSearch.radius}" style="width:90px;" title="Радиус, км" />
        <button class="btn primary" id="g-go">Искать рядом</button>
        ${geoSearch.enabled ? '<button class="btn ghost" id="g-clear">Сбросить</button>' : ''}
      </div>
      <div class="cat-tabs" id="cat-tabs"></div>
      <div class="grid" id="grid">${'<div class="skeleton"></div>'.repeat(3)}</div>
    </section>`;

  if (geoSearch.enabled) {
    $('#g-place').value = geoSearch.place || '';
  }

  $('#g-go').onclick = async () => {
    const place = $('#g-place').value.trim();
    if (!place) return toast('Введите город или адрес', true);
    const btn = $('#g-go');
    btn.disabled = true;
    btn.textContent = 'Ищем…';
    const g = await withError(() => api.geocode(place));
    btn.disabled = false;
    btn.textContent = 'Искать рядом';
    if (!g) return;
    geoSearch.lat = g.lat;
    geoSearch.lng = g.lng;
    geoSearch.radius = Math.max(1, Number($('#g-radius').value) || 50);
    geoSearch.enabled = true;
    geoSearch.place = place;
    toast(`Найдено: ${esc(g.display)} — ищу мастеров в радиусе ${geoSearch.radius} км`);
    loadMasters();
  };

  const clearBtn = $('#g-clear');
  if (clearBtn) {
    clearBtn.onclick = () => {
      geoSearch = { lat: null, lng: null, radius: 50, enabled: false, place: '' };
      renderCatalog();
    };
  }

  withError(async () => {
    const cats = await api.categories();
    renderCatTabs(cats);
    loadMasters();
  });
}

function renderCatTabs(cats) {
  const tt = [{ id: null, name: 'Все' }, ...cats.map((c) => ({ id: c.id, name: c.name }))];
  $('#cat-tabs').innerHTML = tt
    .map((t) => {
      const isActive = (t.id === null && activeCategory === null) || t.id === activeCategory;
      return `<button class="btn ${isActive ? 'active' : 'ghost'}" data-id="${t.id ?? ''}">${esc(t.name)}</button>`;
    })
    .join('');
  document.querySelectorAll('#cat-tabs button').forEach((b) => {
    b.onclick = () => {
      activeCategory = b.dataset.id ? Number(b.dataset.id) : null;
      renderCatTabs(cats);
      loadMasters();
    };
  });
}

function bindRateWidgets(grid) {
  grid.querySelectorAll('.stars.interactive').forEach((row) => {
    const userAttr = row.dataset.user;
    const slots = row.querySelectorAll('.star-slot');
    const setIcons = (n) =>
      slots.forEach((s, i) => {
        s.querySelector('.star').classList.toggle('filled', i < n);
      });
    slots.forEach((slot, i) => {
      slot.addEventListener('mouseenter', () => setIcons(i + 1));
      slot.addEventListener('click', async () => {
        const score = i + 1;
        row.style.pointerEvents = 'none';
        try {
const res = await api.rateMaster(userAttr, score);
          toast(`Спасибо! Оценка сохранена (${Number(res.avg).toFixed(1)} ★)`);
          await loadMasters();
        } catch (e) {
          toast(e.message || 'Не удалось сохранить оценку', true);
          row.style.pointerEvents = '';
        }
      });
    });
    row.addEventListener('mouseleave', () => setIcons(0));
  });
}

async function loadMasters() {
  const grid = $('#grid');
  grid.innerHTML = '<div class="skeleton"></div>'.repeat(3);
  const params = { category_id: activeCategory };
  if (geoSearch.enabled) {
    params.lat = geoSearch.lat;
    params.lng = geoSearch.lng;
    params.radius_km = geoSearch.radius;
    params.sort = 'distance';
  }
  const masters = await withError(() => api.masters(params));
  if (!masters) return;
  if (masters.length === 0) {
    grid.innerHTML = `<div class="big-empty" style="grid-column:1/-1;"><h2>Пока никого нет</h2>${
      geoSearch.enabled ? 'В этом радиусе, увы, никого не нашлось. Попробуйте увеличить радиус.' : 'В этой категории пока нет мастеров'
    }</div>`;
    return;
  }
  grid.innerHTML = masters
    .map((m) => {
      const canRate = state.user && state.user.role === 'customer' && state.user.id !== m.user_id;
      return `
    <article class="card">
      <div class="card-top">
        <div class="avatar">${initials(m.name)}</div>
        <div style="flex:1;">
          <h3>${esc(m.name)}</h3>
          ${starsRow(m.rating, m.rating_count)}
        </div>
      </div>
      <p class="bio">${esc(m.bio || 'Мастер без описания')}</p>
      ${m.city ? `<div class="geo-line">📍 ${esc(m.city)}${m.distance_km != null ? ` — <b>${m.distance_km.toFixed(1)} км</b> от вас` : ''}</div>` : m.distance_km != null ? `<div class="geo-line">— <b>${m.distance_km.toFixed(1)} км</b> от вас</div>` : ''}
      ${m.photos && m.photos.length ? `<div class="photos">${m.photos.slice(0, 3).map((u) => `<img class="photo-thumb" src="${esc(u)}" loading="lazy" />`).join('')}</div>` : ''}
      <div class="prices">
        ${(m.prices || []).map((p) => `<span class="price-chip">${esc(p.category_name)} <b>${money(p.price)}</b></span>`).join('') || '<span class="muted" style="font-size:13px;">ценники не указаны</span>'}
      </div>
      ${canRate ? `<div class="rate-line"><span class="rate-hint">Ваша оценка:</span>${starsRow(0, 0, true, m.user_id)}</div>` : ''}
      <button class="btn primary block" data-master="${m.user_id}">${state.user ? 'Написать мастеру' : 'Войти и написать'}</button>
    </article>`;
    })
    .join('');

  bindRateWidgets(grid);

  grid.querySelectorAll('button[data-master]').forEach((b) => {
    b.onclick = async () => {
      const masterId = Number(b.dataset.master);
      if (!state.user) {
        toast('Сначала войдите в аккаунт', true);
        navigate('#/auth');
        return;
      }
      b.disabled = true;
      const conv = await withError(() => api.createChat({ master_id: masterId }));
      b.disabled = false;
      if (conv) {
        toast('Чат открыт');
        navigate('#/chat?id=' + conv.id);
      }
    };
  });
}

/* ------------------------- Авторизация ------------------------- */

function renderAuth(isRegister) {
  if (state.user) {
    navigate('#/');
    return;
  }
  app.innerHTML = `
    <section class="auth-wrap">
      <div class="auth-card">
        <div class="auth-tabs">
          <button id="tab-login" class="${isRegister ? '' : 'active'}">Вход</button>
          <button id="tab-register" class="${isRegister ? 'active' : ''}">Регистрация</button>
        </div>
        ${isRegister ? `
        <div class="field"><label>Имя</label><input id="f-name" placeholder="Иван" /></div>
        <div class="field"><label>Кто вы?</label>
          <select id="f-role">
            <option value="customer">Я заказчик — ищу мастера</option>
            <option value="master">Я мастер — ищу заказы</option>
          </select>
        </div>` : ''}
        <div class="field"><label>Email</label><input id="f-email" type="email" placeholder="you@mail.ru" /></div>
        <div class="field"><label>Пароль</label><input id="f-password" type="password" placeholder="минимум 8 символов" /></div>
        <button class="btn primary block" id="f-submit">${isRegister ? 'Создать аккаунт' : 'Войти'}</button>
      </div>
    </section>`;

  $('#tab-login').onclick = () => navigate('#/auth');
  $('#tab-register').onclick = () => navigate('#/auth?mode=register');

  $('#f-submit').onclick = async (e) => {
    const btn = e.currentTarget;
    const email = $('#f-email').value.trim();
    const password = $('#f-password').value;
    if (!email || !password) return toast('Заполните email и пароль', true);

    btn.disabled = true;
    const body = isRegister
      ? { name: $('#f-name').value.trim(), email, password, role: $('#f-role').value }
      : { email, password };

    const res = await withError(() => (isRegister ? api.register(body) : api.login(body)));
    btn.disabled = false;
    if (!res) return;

    localStorage.setItem('token', res.token);
    state.user = res.user;
    toast(`Добро пожаловать, ${res.user.name}!`);
    refreshUnread();
    renderNav();
    navigate('#/catalog');
  };
}

/* ------------------------- Профиль мастера ------------------------- */

function renderProfile() {
  if (!state.user) return navigate('#/auth');
  if (state.user.role !== 'master') {
    app.innerHTML = `
      <section class="page">
        <div class="big-empty">
          <h2>Раздел для мастеров</h2>
          <p class="muted">Профиль со сменой ценников доступен только мастерам.</p>
        </div>
      </section>`;
    return;
  }

  app.innerHTML = `
    <section class="page">
      <h1>Панель мастера</h1>
      <div class="panel">
        <div class="card">
          <h3 style="margin-bottom:16px;">Профиль</h3>
          <div class="field"><label>Имя</label><input id="p-name" placeholder="Ваше имя" /></div>
          <div class="field"><label>О себе</label><textarea id="p-bio" placeholder="Что делаете, опыт, район выезда"></textarea></div>
          <div class="field"><label>Ваш город / район (для поиска рядом)</label><input id="p-city" placeholder="Например: Минск, Партизанский район" /></div>
          <p class="muted" style="font-size:13px;margin-top:-6px;">Мы определим координаты автоматически — тогда клиенты смогут находить вас по расстоянию.</p>
          <button class="btn primary" id="p-save">Сохранить профиль</button>
        </div>
        <div class="card">
          <h3 style="margin-bottom:6px;">Ценники</h3>
          <p class="muted" style="font-size:13.5px;margin-bottom:14px;">Укажите стоимость работ по категориям.</p>
          <div id="p-price-list">${'<div class="skeleton" style="height:36px;margin:8px 0;"></div>'.repeat(4)}</div>
        </div>
        <div class="card">
          <h3 style="margin-bottom:6px;">Фото работ</h3>
          <p class="muted" style="font-size:13.5px;margin-bottom:14px;">Покажите клиентам свои работы.</p>
          <div class="upload-zone" id="p-upload">➕ Кликните, чтобы загрузить фото</div>
          <input type="file" id="p-file" accept="image/*" style="display:none;" />
          <div id="p-photos"><div class="skeleton" style="height:56px;margin:8px 0;"></div></div>
        </div>
      </div>
    </section>`;

  withError(async () => {
    const profile = await api.myProfile().catch(() => null);
    if (profile) {
      $('#p-name').value = profile.name;
      $('#p-bio').value = profile.bio;
      if (profile.city) $('#p-city').value = profile.city;
    }

    const cats = await api.categories();
    const prices = new Map((profile?.prices || []).map((p) => [p.category_id, p.price]));
    $('#p-price-list').innerHTML = cats
      .map(
        (c) => `
      <div class="price-row">
        <span class="name">${esc(c.name)}</span>
        <input type="number" min="0" step="0.5" data-cat="${c.id}" value="${prices.has(c.id) ? prices.get(c.id) : ''}" placeholder="—" />
        <button class="btn ghost" data-save="${c.id}">Сохранить</button>
      </div>`,
      )
      .join('');

    document.querySelectorAll('[data-save]').forEach((b) => {
      b.onclick = async () => {
        const input = document.querySelector(`input[data-cat="${b.dataset.save}"]`);
        const value = parseFloat(input.value);
        if (isNaN(value) || value < 0) return toast('Введите корректную цену', true);
        b.disabled = true;
        const saved = await withError(() => api.setPrice({ category_id: Number(b.dataset.save), price: value }));
        b.disabled = false;
        if (saved) toast(`Цена за «${esc(input.closest('.price-row').querySelector('.name').textContent)}»: ${money(saved.price)}`);
      };
    });

    loadPhotos();
  });

  const uploadZone = $('#p-upload');
  const fileInput = $('#p-file');
  uploadZone.onclick = () => fileInput.click();
  fileInput.onchange = async () => {
    const file = fileInput.files && fileInput.files[0];
    if (!file) return;
    if (!file.type.startsWith('image/')) return toast('Нужен файл изображения', true);
    if (file.size > 5 * 1024 * 1024) return toast('Файл больше 5 МБ', true);
    uploadZone.textContent = 'Загружаем...';
    const res = await withError(() => api.uploadPhoto(file));
    uploadZone.innerHTML = '➕ Кликните, чтобы загрузить фото';
    fileInput.value = '';
    if (res) {
      toast('Фото загружено');
      loadPhotos();
    }
  };

  async function loadPhotos() {
    const box = $('#p-photos');
    if (!box) return;
    const photos = await withError(() => api.myPhotos());
    if (!photos) return;
    if (!photos.length) {
      box.innerHTML = '<p class="muted" style="font-size:13.5px;">Пока нет фото</p>';
      return;
    }
    box.innerHTML = `<div class="photos">${photos
      .map(
        (p) => `
      <div class="photo-item">
        <img class="photo-thumb" src="${esc(p.url)}" loading="lazy" />
        <button class="photo-del" data-del="${p.id}" title="Удалить">✕</button>
      </div>`,
      )
      .join('')}</div>`;

    box.querySelectorAll('[data-del]').forEach((b) => {
      b.onclick = async () => {
        if (!confirm('Удалить фото?')) return;
        const ok = await withError(() => api.deletePhoto(Number(b.dataset.del)));
        if (ok) {
          toast('Фото удалено');
          loadPhotos();
        }
      };
    });
  }

  $('#p-save').onclick = async (e) => {
    const btn = e.currentTarget;
    btn.disabled = true;
    btn.textContent = 'Сохраняем…';
    const city = $('#p-city').value.trim();
    const body = { name: $('#p-name').value.trim(), bio: $('#p-bio').value.trim(), city };
    if (city) {
      const g = await withError(() => api.geocode(city));
      if (g) {
        body.lat = g.lat;
        body.lng = g.lng;
      }
    }
    const res = await withError(() => api.saveProfile(body));
    btn.disabled = false;
    btn.textContent = 'Сохранить профиль';
    if (res) toast(res.city ? `Профиль сохранён — вы найдёте клиентов рядом с «${res.city}»` : 'Профиль сохранён');
  };
}

/* ------------------------- Заказы ------------------------- */

let orderCategories = [];
let orderLocation = null;

function statusChip(status) {
  const labels = { open: 'Приём заявок', selected: 'Мастер выбран', closed: 'Закрыт' };
  return `<span class="status-chip ${status}">${labels[status] || status}</span>`;
}

function renderOrders() {
  if (!state.user) return navigate('#/auth');
  const isMaster = state.user.role === 'master';
  app.innerHTML = `
    <section class="page">
      <h1>${isMaster ? 'Новые заказы' : 'Мои заказы'}</h1>
      <p class="lead">${
        isMaster
          ? 'Клиенты публикуют работу и выбирают лучшее предложение. Поборитесь за заказ!'
          : 'Опишите работу и выбирайте мастера из тех, кто откликнется.'
      }</p>
      ${
        isMaster
          ? ''
          : `<div class="card" id="order-form-wrap" style="display:none;">
               <h3 style="margin-bottom:14px;">Новый заказ</h3>
               <div class="field"><label>Категория работ</label><select id="of-cat"></select></div>
               <div class="field"><label>Что нужно сделать</label><input id="of-title" placeholder="Например: починить кран на кухне" /></div>
               <div class="field"><label>Описание</label><textarea id="of-desc" placeholder="Подробности: что сломалось, когда, какой результат нужен"></textarea></div>
               <div class="field"><label>Ваш ценник (за что готовы заплатить, BYN)</label><input id="of-budget" type="number" min="0" step="1" placeholder="0" /></div>
               <div class="field"><label>Ваше место (город/адрес — чтобы подобрать мастеров поблизости)</label>
                 <div style="display:flex;gap:8px;">
                   <input id="of-place" placeholder="Например: Минск" style="flex:1;" />
                   <button class="btn ghost" id="of-geo" type="button">Определить</button>
                 </div>
               </div>
               <div id="of-geo-state"></div>
               <button class="btn primary" id="of-submit">Опубликовать заказ</button>
             </div>
             <button class="btn primary" id="order-new" style="margin-bottom:18px;">+ Новый заказ</button>`
      }
      <div id="order-list">${'<div class="skeleton"></div>'.repeat(2)}</div>
    </section>`;

  if (!isMaster) {
    $('#order-new').onclick = () => {
      $('#order-form-wrap').style.display = 'block';
      withError(async () => {
        const cats = await api.categories();
        orderCategories = cats;
        $('#of-cat').innerHTML = cats
          .map((c) => `<option value="${c.id}">${esc(c.name)}</option>`)
          .join('');
      });
    };
    $('#of-geo').onclick = async () => {
      const place = $('#of-place').value.trim();
      if (!place) return toast('Введите ваше место', true);
      const btn = $('#of-geo');
      btn.disabled = true;
      btn.textContent = '…';
      const g = await withError(() => api.geocode(place));
      btn.disabled = false;
      btn.textContent = 'Определить';
      if (!g) return;
      orderLocation = { lat: g.lat, lng: g.lng, place };
      $('#of-geo-state').innerHTML = `<span class="geo-ok">✓ Место определено: ${esc(place)}</span>`;
    };
    $('#of-submit').onclick = async (e) => {
      const btn = e.currentTarget;
      btn.disabled = true;
      const body = {
        category_id: Number($('#of-cat').value),
        title: $('#of-title').value.trim(),
        description: $('#of-desc').value.trim(),
        budget: Number($('#of-budget').value),
      };
      if (orderLocation) {
        body.lat = orderLocation.lat;
        body.lng = orderLocation.lng;
      }
      const res = await withError(() => api.createOrder(body));
      btn.disabled = false;
      if (res) {
        orderLocation = null;
        $('#of-geo-state').innerHTML = '';
        if (res.suggested_masters && res.suggested_masters.length) {
          toast(`Опубликовано. Поблизости найден ${res.suggested_masters.length} подходящи(й/х) мастер(а/ов)!`);
        } else {
          toast('Заказ опубликован — мастера увидят его!');
        }
        loadOrderList();
        api.orders();
      }
    };
  }

  loadOrderList();
}

async function loadOrderList() {
  const list = $('#order-list');
  if (!list) return;
  const orders = await withError(() => api.orders());
  if (!orders) return;
  if (orders.length === 0) {
    list.innerHTML = `<div class="big-empty"><h2>Заказов пока нет</h2></div>`;
    return;
  }
  const isMaster = state.user.role === 'master';
  list.innerHTML = orders.map((o) => orderCard(o, isMaster)).join('');

  document.querySelectorAll('[data-accept]').forEach((b) => {
    b.onclick = async () => {
      b.disabled = true;
      const res = await withError(() =>
        api.selectOffer(b.dataset.order, Number(b.dataset.accept)),
      );
      b.disabled = false;
      if (res) {
        toast('Отлично! Мастер получил заказ');
        loadOrderList();
      }
    };
  });

  document.querySelectorAll('[data-chat-master]').forEach((b) => {
    b.onclick = async () => {
      b.disabled = true;
      const conv = await withError(() => api.createChat({ master_id: Number(b.dataset.chatMaster) }));
      b.disabled = false;
      if (conv) navigate('#/chat?id=' + conv.id);
    };
  });

  document.querySelectorAll('[data-suggest-master]').forEach((b) => {
    b.onclick = async () => {
      b.disabled = true;
      const conv = await withError(() => api.createChat({ master_id: Number(b.dataset.suggestMaster) }));
      b.disabled = false;
      if (conv) navigate('#/chat?id=' + conv.id);
    };
  });

  document.querySelectorAll('[data-offer]').forEach((f) => {
    f.addEventListener('submit', async (e) => {
      e.preventDefault();
      const orderId = Number(f.dataset.offer);
      const price = parseFloat(f.querySelector('.bid-price').value);
      const comment = f.querySelector('.bid-comment').value.trim();
      const btn = f.querySelector('button');
      btn.disabled = true;
      const res = await withError(() => api.placeOffer(orderId, { price, comment }));
      btn.disabled = false;
      if (res) {
        toast(`Предложение на ${money(res.price)} отправлено!`);
        loadOrderList();
        refreshOrderAlerts();
      }
    });
  });
}

function orderCard(o, isMaster) {
  if (isMaster) return masterOrderCard(o);
  const accepted = o.offers.find((f) => f.accepted);
  return `
  <article class="card order-card">
    <div class="order-head">
      <span class="price-chip">${esc(o.category_name)}</span>
      ${statusChip(o.status)}
    </div>
    <h3>${esc(o.title)}</h3>
    <p class="bio">${esc(o.description || 'Без описания')}</p>
    <div class="order-budget"><b>${money(o.budget)}</b> — готовы заплатить</div>
    ${
      o.status === 'open'
        ? `<div class="offers">
             <div class="offers-title">Предложения мастеров (${o.offers_count})</div>
             ${
               o.offers.length
                 ? o.offers
                     .map((f) => `
               <div class="offer-item ${f.accepted ? 'picked' : ''}">
                 <div class="offer-master">
                   <span class="offer-name">${esc(f.master_name || 'Мастер #' + f.master_id)}</span>
                   <span class="stars-info">★ ${Number(f.master_rating || 0).toFixed(1)}</span>
                 </div>
                 <div class="offer-price">${money(f.price)}</div>
                 <p class="offer-comment">${esc(f.comment || '')}</p>
                 <button class="btn primary" data-accept="${f.id}" data-order="${o.id}">Выбрать</button>
               </div>`)
                     .join('')
                 : `<p class="muted" style="font-size:14px;">Пока никто не откликнулся. Мастера получат уведомление о новом заказе.</p>`
             }
             ${suggestedBlock(o)}
           </div>`
        : accepted
          ? `<div class="winner">
               <p>Выбран мастер: <b>${esc(accepted.master_name || 'Мастер #' + accepted.master_id)}</b> на ${money(accepted.price)}</p>
               <button class="btn primary" data-chat-master="${accepted.master_id}">Написать мастеру</button>
             </div>`
          : ''
    }
  </article>`;
}

function suggestedBlock(o) {
  if (!o.suggested_masters || !o.suggested_masters.length) return '';
  return `
    <div class="suggested">
      <div class="offers-title">Ближайшие мастера по этой работе (${o.suggested_masters.length})</div>
      ${o.suggested_masters
        .map(
          (m) => `
        <div class="suggested-item">
          <span class="offer-name">${esc(m.name)}</span>
          <span class="stars-info">★ ${Number(m.rating || 0).toFixed(1)}</span>
          <span class="suggested-dist">${m.distance_km.toFixed(1)} км</span>
          <button class="btn ghost" data-suggest-master="${m.master_id}">Написать</button>
        </div>`,
        )
        .join('')}
    </div>`;
}

function masterOrderCard(o) {
  const mine = o.my_offer;
  return `
  <article class="card order-card">
    <div class="order-head">
      <span class="price-chip">${esc(o.category_name)}</span>
      ${statusChip(o.status)}
    </div>
    <h3>${esc(o.title)}</h3>
    <p class="bio">${esc(o.description || 'Без описания')}</p>
    <div class="order-budget"><b>${money(o.budget)}</b> — бюджет клиента</div>
    ${o.status === 'open' ? `<div class="offers-title">Предложений уже: ${o.offers_count}</div>` : ''}
    ${
      o.status === 'open'
        ? mine
          ? `<div class="mine-offer">
               <p>Ваше предложение: <b>${money(mine.price)}</b> ${esc(mine.comment || '')}</p>
               <details><summary>Изменить предложение</summary>
                 <form data-offer="${o.id}" class="bid-form">
                   <input class="bid-price" type="number" min="0" step="1" value="${mine.price}" placeholder="Ваша цена, BYN" />
                   <input class="bid-comment" value="${esc(mine.comment)}" placeholder="Комментарий" />
                   <button class="btn primary">Обновить</button>
                 </form>
               </details>
             </div>`
          : `<form data-offer="${o.id}" class="bid-form">
               <input class="bid-price" type="number" min="0" step="1" required placeholder="Ваша цена, BYN" />
               <input class="bid-comment" placeholder="Комментарий (когда сможете приехать)" />
               <button class="btn primary">Бороться за заказ</button>
             </form>`
        : mine?.accepted
          ? `<div class="winner"><p><b>Вас выбрали для этого заказа!</b> Свяжитесь с клиентом по цене ${money(mine.price)}.</p></div>`
          : `<p class="muted" style="font-size:14px;">Эту работу поручили другому мастеру. Следите за новыми заказами!</p>`
    }
  </article>`;
}

/* ------------------------- Чаты ------------------------- */

async function masterNames() {
  try {
    const list = await api.masters();
    const map = new Map();
    list.forEach((m) => map.set(m.user_id, m.name));
    return map;
  } catch {
    return new Map();
  }
}

function renderChats() {
  if (!state.user) return navigate('#/auth');
  app.innerHTML = `
    <section class="page">
      <h1>Чаты</h1>
      <div class="chat-wrap">
        <div class="conv-list" id="conv-list"><div class="skeleton" style="margin:14px;"></div></div>
        <div class="thread">
          <div class="empty" id="thread-placeholder">
            <div><h3 style="margin-bottom:8px;">Выберите диалог</h3>Переписка с мастером появится здесь</div>
          </div>
        </div>
      </div>
    </section>`;
  loadChatList();
}

async function loadChatList() {
  const list = $('#conv-list');
  const names = state.user.role === 'master' ? new Map() : await masterNames();
  const convs = await withError(() => api.chats());
  if (!convs) return;
  if (convs.length === 0) {
    list.innerHTML = `<div class="empty">Диалогов пока нет</div>`;
    return;
  }
  list.innerHTML = convs
    .map((c) => {
      const title =
        state.user.id === c.master_id
          ? `Клиент #${c.customer_id}`
          : `Мастер: ${names.get(c.master_id) || '#' + c.master_id}`;
      const snippet = c.last_message ? esc(c.last_message) : 'нет сообщений';
      const unread = c.unread_count || 0;
      return `
        <div class="conv-item" data-id="${c.id}">
          <div class="t">${esc(title)}</div>
          <div class="s">${snippet}</div>
          ${unread ? `<span class="unread-dot">${unread > 99 ? '99+' : unread}</span>` : ''}
        </div>`;
    })
    .join('');

  list.querySelectorAll('.conv-item').forEach((el) => {
    el.onclick = () => navigate('#/chat?id=' + el.dataset.id);
  });
}

function renderChatThread(conversationId) {
  if (!state.user) return navigate('#/auth');
  app.innerHTML = `
    <section class="page">
      <h1><a class="muted" href="#/chats" style="margin-right:10px;">&#8592;</a> Чат</h1>
      <div class="chat-wrap">
        <div class="thread" style="grid-column:1 / -1;">
          <div class="thread-head" id="chat-title">Загрузка...</div>
          <div class="msgs" id="msgs"><div class="skeleton" style="height:36px;"></div></div>
          <div class="typing-indicator" id="typing-indicator" style="display:none;padding:0 20px 6px;color:var(--muted);font-size:13px;">печатает...</div>
          <div class="composer">
            <input id="chat-input" placeholder="Сообщение..." />
            <button class="btn primary" id="chat-send">Отправить</button>
          </div>
        </div>
      </div>
    </section>`;

  state.activeChat = Number(conversationId);
  $('#chat-title').textContent = `Диалог #${conversationId}`;

  loadMessages(state.activeChat);
  connectWebSocket(state.activeChat);

  let typingTimeout = null;
  const input = $('#chat-input');
  input.addEventListener('input', () => {
    if (state.ws && state.ws.readyState === WebSocket.OPEN) {
      state.ws.send(JSON.stringify({ type: 'typing' }));
    }
  });

  $('#chat-send').onclick = sendWsMessage;
  input.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') sendWsMessage();
  });
}

async function loadMessages(conversationId) {
  const msgs = await withError(() => api.messages(conversationId));
  if (!msgs) return;
  const box = $('#msgs');
  const cur = state.user.id;
  box.innerHTML = msgs.length
    ? msgs
        .map(
          (m) => `
        <div class="msg ${m.sender_id === cur ? 'mine' : 'theirs'}">
          ${esc(m.text)}
          <span class="time">${new Date(m.created_at).toLocaleTimeString('ru-RU', { hour: '2-digit', minute: '2-digit' })}</span>
        </div>`,
        )
        .join('')
    : '<div class="empty">Напишите первое сообщение</div>';
  box.scrollTop = box.scrollHeight;
  refreshUnread();
}

/* ------------------------- Обратная связь ------------------------- */

function renderFeedback() {
  app.innerHTML = `
    <section class="page">
      <h1>Обратная связь</h1>
      <p class="lead">Расскажите, что у нас хорошо, а что можно улучшить.</p>
      <div class="card" style="max-width:560px;">
        <div class="field">
          <label>Сообщение</label>
          <textarea id="fb-message" placeholder="Напишите ваш вопрос или предложение..."></textarea>
        </div>
        ${state.user ? '' : '<div class="field"><label>Email (необязательно)</label><input id="fb-email" placeholder="you@mail.ru" /></div>'}
        <button class="btn primary block" id="fb-submit">Отправить</button>
      </div>
    </section>`;

  $('#fb-submit').onclick = async (e) => {
    const btn = e.currentTarget;
    const message = $('#fb-message').value.trim();
    if (!message) return toast('Напишите сообщение', true);
    btn.disabled = true;
    const body = { message };
    if (!state.user) body.email = $('#fb-email').value.trim();
    const res = await withError(() => api.sendFeedback(body));
    btn.disabled = false;
    if (res) {
      toast('Спасибо! Ваше сообщение отправлено в службу поддержки.');
      $('#fb-message').value = '';
    }
  };
}

/* ------------------------- Админ-панель ------------------------- */

let adminTab = 'overview';

function renderAdmin() {
  if (!state.user) return navigate('#/auth');
  if (state.user.role !== 'admin') {
    app.innerHTML = `
      <section class="page">
        <div class="big-empty"><h2>Доступ ограничен</h2><p class="muted">Эта страница доступна только администратору.</p></div>
      </section>`;
    return;
  }

  app.innerHTML = `
    <section class="page">
      <h1>Админ-панель</h1>
      <div class="admin-tabs">
        <button data-tab="overview" class="${adminTab === 'overview' ? 'active' : ''}">Обзор</button>
        <button data-tab="users" class="${adminTab === 'users' ? 'active' : ''}">Пользователи</button>
        <button data-tab="orders" class="${adminTab === 'orders' ? 'active' : ''}">Заказы</button>
        <button data-tab="feedback" class="${adminTab === 'feedback' ? 'active' : ''}">Обращения</button>
      </div>
      <div id="admin-content"><div class="skeleton"></div></div>
    </section>`;

  document.querySelectorAll('.admin-tabs button').forEach((b) => {
    b.onclick = () => {
      adminTab = b.dataset.tab;
      renderAdmin();
    };
  });

  if (adminTab === 'overview') loadAdminOverview();
  else if (adminTab === 'users') loadAdminUsers();
  else if (adminTab === 'orders') loadAdminOrders();
  else loadAdminFeedback();
}

async function loadAdminOverview() {
  const box = $('#admin-content');
  const o = await withError(() => api.adminOverview());
  if (!o) return;
  box.innerHTML = `
    <div class="stats">
      ${stat('Пользователей', o.users)}
      ${stat('Мастеров', o.masters)}
      ${stat('Категорий', o.categories)}
      ${stat('Заказов', o.orders)}
      ${stat('Открытых', o.orders_open)}
      ${stat('Предложений', o.offers)}
      ${stat('Обращений', o.feedback)}
      ${stat('Новых обращений', o.feedback_new)}
    </div>`;
}

function stat(label, value) {
  return `<div class="stat"><div class="stat-num">${value}</div><div class="stat-label">${esc(label)}</div></div>`;
}

async function loadAdminUsers() {
  const box = $('#admin-content');
  const users = await withError(() => api.adminUsers());
  if (!users) return;
  box.innerHTML = `
    <div class="admin-toolbar">
      <button class="btn primary" id="admin-user-new">+ Пользователь</button>
    </div>
    <table class="admin-table">
      <thead><tr><th>ID</th><th>Имя</th><th>Email</th><th>Роль</th><th></th></tr></thead>
      <tbody>
        ${users.map((u) => `
          <tr data-uid="${u.id}">
            <td>${u.id}</td><td>${esc(u.name)}</td><td>${esc(u.email)}</td><td><span class="role-chip ${esc(u.role)}">${esc(u.role)}</span></td>
            <td class="row-actions">
              <button class="btn ghost sm" data-edit="${u.id}">Изменить</button>
              <button class="btn ghost sm danger" data-del="${u.id}">Удалить</button>
            </td>
          </tr>`).join('')}
      </tbody>
    </table>`;

  $('#admin-user-new').onclick = () => adminUserForm(null);
  box.querySelectorAll('[data-edit]').forEach((b) => (b.onclick = () => adminUserForm(Number(b.dataset.edit))));
  box.querySelectorAll('[data-del]').forEach((b) => {
    b.onclick = async () => {
      if (!confirm('Удалить пользователя?')) return;
      const ok = await withError(() => api.adminDeleteUser(Number(b.dataset.del)));
      if (ok) {
        toast('Пользователь удалён');
        loadAdminUsers();
      }
    };
  });
}

function adminUserForm(id) {
  const box = $('#admin-content');
  const isEdit = !!id;
  box.innerHTML = `
    <button class="btn ghost sm" id="admin-back">← Назад</button>
    <div class="card" style="max-width:480px;margin-top:14px;">
      <h3 style="margin-bottom:14px;">${isEdit ? 'Редактировать пользователя' : 'Новый пользователь'}</h3>
      <div class="field"><label>Имя</label><input id="au-name" /></div>
      <div class="field"><label>Email</label><input id="au-email" /></div>
      <div class="field"><label>Роль</label>
        <select id="au-role"><option value="customer">customer</option><option value="master">master</option><option value="admin">admin</option></select>
      </div>
      <div class="field"><label>${isEdit ? 'Новый пароль (пусто — не менять)' : 'Пароль'}</label><input id="au-pass" type="password" /></div>
      <button class="btn primary" id="au-save">Сохранить</button>
    </div>`;

  $('#admin-back').onclick = () => loadAdminUsers();

  if (isEdit) {
    withError(async () => {
      const u = await api.adminGetUser(id);
      $('#au-name').value = u.name;
      $('#au-email').value = u.email;
      $('#au-role').value = u.role;
    });
  }

  $('#au-save').onclick = async (e) => {
    const btn = e.currentTarget;
    btn.disabled = true;
    const body = {
      name: $('#au-name').value.trim(),
      email: $('#au-email').value.trim(),
      role: $('#au-role').value,
      password: $('#au-pass').value,
    };
    const res = await withError(() => (isEdit ? api.adminUpdateUser(id, body) : api.adminCreateUser(body)));
    btn.disabled = false;
    if (res) {
      toast('Сохранено');
      loadAdminUsers();
    }
  };
}

async function loadAdminOrders() {
  const box = $('#admin-content');
  const orders = await withError(() => api.adminOrders());
  if (!orders) return;
  box.innerHTML = `
    <table class="admin-table">
      <thead><tr><th>ID</th><th>Заголовок</th><th>Бюджет</th><th>Статус</th><th>Владелец</th><th></th></tr></thead>
      <tbody>
        ${orders.map((o) => `
          <tr>
            <td>${o.id}</td><td>${esc(o.title)}</td><td>${money(o.budget)}</td>
            <td><span class="status-chip ${esc(o.status)}">${esc(o.status)}</span></td>
            <td>#${o.customer_id}</td>
            <td class="row-actions">
              <button class="btn ghost sm" data-oedit="${o.id}">Изменить</button>
              <button class="btn ghost sm danger" data-odel="${o.id}">Удалить</button>
            </td>
          </tr>`).join('')}
      </tbody>
    </table>`;

  box.querySelectorAll('[data-oedit]').forEach((b) => (b.onclick = () => adminOrderForm(Number(b.dataset.oedit))));
  box.querySelectorAll('[data-odel]').forEach((b) => {
    b.onclick = async () => {
      if (!confirm('Удалить заказ?')) return;
      const ok = await withError(() => api.adminDeleteOrder(Number(b.dataset.odel)));
      if (ok) {
        toast('Заказ удалён');
        loadAdminOrders();
      }
    };
  });
}

function adminOrderForm(id) {
  const box = $('#admin-content');
  box.innerHTML = `
    <button class="btn ghost sm" id="ad-back">← Назад</button>
    <div class="card" style="max-width:480px;margin-top:14px;">
      <h3 style="margin-bottom:14px;">Заказ #${id}</h3>
      <div class="field"><label>Заголовок</label><input id="ao-title" /></div>
      <div class="field"><label>Описание</label><textarea id="ao-desc"></textarea></div>
      <div class="field"><label>Бюджет (BYN)</label><input id="ao-budget" type="number" step="1" min="0" /></div>
      <div class="field"><label>Статус</label>
        <select id="ao-status"><option value="open">open</option><option value="selected">selected</option><option value="closed">closed</option></select>
      </div>
      <button class="btn primary" id="ao-save">Сохранить</button>
    </div>`;

  $('#ad-back').onclick = () => loadAdminOrders();

  withError(async () => {
    const o = await api.adminGetOrder(id);
    $('#ao-title').value = o.title;
    $('#ao-desc').value = o.description;
    $('#ao-budget').value = o.budget;
    $('#ao-status').value = o.status;
  });

  $('#ao-save').onclick = async (e) => {
    const btn = e.currentTarget;
    btn.disabled = true;
    const res = await withError(() =>
      api.adminUpdateOrder(id, {
        title: $('#ao-title').value.trim(),
        description: $('#ao-desc').value.trim(),
        budget: Number($('#ao-budget').value),
        status: $('#ao-status').value,
      }),
    );
    btn.disabled = false;
    if (res) {
      toast('Заказ обновлён');
      loadAdminOrders();
    }
  };
}

async function loadAdminFeedback() {
  const box = $('#admin-content');
  const list = await withError(() => api.adminFeedback());
  if (!list) return;
  box.innerHTML = list.length
    ? list
        .map((f) => `
          <div class="fb-item ${esc(f.status)}">
            <div class="fb-head">
              <span class="status-chip ${esc(f.status)}">${esc(f.status)}</span>
              <span class="muted">${new Date(f.created_at).toLocaleString('ru-RU')} · ${esc(f.email || f.user_id ? ('ID ' + f.user_id) : 'аноним')}</span>
              ${f.status === 'new' ? '<button class="btn ghost sm" data-fbdone="' + f.id + '">Отметить выполненным</button>' : ''}
            </div>
            <p>${esc(f.message)}</p>
          </div>`)
        .join('')
    : '<div class="big-empty"><h2>Обращений пока нет</h2></div>';

  box.querySelectorAll('[data-fbdone]').forEach((b) => {
    b.onclick = async () => {
      const ok = await withError(() => api.adminResolveFeedback(Number(b.dataset.fbdone)));
      if (ok) loadAdminFeedback();
    };
  });
}

/* ------------------------- Инициализация ------------------------- */

async function init() {
  const token = localStorage.getItem('token');
  if (token) {
    const user = await api.me().catch(() => null);
    if (user) state.user = user;
    else localStorage.removeItem('token');
  }
  if (state.user) {
    refreshUnread();
    setInterval(refreshUnread, 6000);
    if (state.user.role === 'master') {
      refreshOrderAlerts();
      setInterval(refreshOrderAlerts, 10000);
    }
  }
  renderNav();
  route();
}

init();