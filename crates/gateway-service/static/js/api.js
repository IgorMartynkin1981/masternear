const API = '/api';

async function request(path, options = {}) {
  const token = localStorage.getItem('token');
  const headers = { 'Content-Type': 'application/json', ...(options.headers || {}) };
  if (token) headers['Authorization'] = 'Bearer ' + token;

  let res;
  try {
    res = await fetch(API + path, { ...options, headers });
  } catch {
    const e = new Error('Нет связи с сервером');
    e.status = 0;
    throw e;
  }

  const data = await res.json().catch(() => ({}));
  if (!res.ok) {
    const e = new Error(data.error || 'Ошибка запроса');
    e.status = res.status;
    throw e;
  }
  return data;
}

export const api = {
  register: (body) => request('/auth/register', { method: 'POST', body: JSON.stringify(body) }),
  login: (body) => request('/auth/login', { method: 'POST', body: JSON.stringify(body) }),
  me: () => request('/auth/me'),
  settings: () => request('/auth/settings'),
  updateSettings: (body) => request('/auth/settings', { method: 'PUT', body: JSON.stringify(body) }),
  profile: () => request('/auth/profile'),
  updateProfile: (body) => request('/auth/profile', { method: 'PUT', body: JSON.stringify(body) }),

  categories: () => request('/categories'),
  masters: (params) => {
    const q = new URLSearchParams();
    if (params?.category_id) q.set('category_id', params.category_id);
    if (params?.lat != null) q.set('lat', params.lat);
    if (params?.lng != null) q.set('lng', params.lng);
    if (params?.radius_km != null) q.set('radius_km', params.radius_km);
    if (params?.sort) q.set('sort', params.sort);
    const s = q.toString();
    return request('/masters' + (s ? '?' + s : ''));
  },
  geocode: (place) => request('/geocode', { method: 'POST', body: JSON.stringify({ place }) }),
  saveProfile: (body) => request('/masters/me', { method: 'POST', body: JSON.stringify(body) }),
  myProfile: () => request('/masters/me'),
  setPrice: (body) => request('/masters/me/prices', { method: 'PUT', body: JSON.stringify(body) }),
  myPhotos: () => request('/masters/me/photos'),
  deletePhoto: (id) => request('/masters/me/photos/' + id, { method: 'DELETE' }),
  uploadPhoto: (file) => {
    const fd = new FormData();
    fd.append('file', file);
    const token = localStorage.getItem('token');
    return fetch(API + '/masters/me/photos', {
      method: 'POST',
      headers: token ? { Authorization: 'Bearer ' + token } : {},
      body: fd,
    }).then((res) =>
      res.json().then((data) => {
        if (!res.ok) throw new Error(data.error || 'Ошибка загрузки');
        return data;
      }),
    );
  },
  rateMaster: (masterUserId, score) =>
    request('/masters/' + masterUserId + '/rating', { method: 'POST', body: JSON.stringify({ score }) }),

  createChat: (body) => request('/chats', { method: 'POST', body: JSON.stringify(body) }),
  chats: () => request('/chats'),
  messages: (id) => request('/chats/' + id + '/messages'),
  sendMessage: (id, text) => request('/chats/' + id + '/messages', { method: 'POST', body: JSON.stringify({ text }) }),

  orders: () => request('/orders'),
  createOrder: (body) => request('/orders', { method: 'POST', body: JSON.stringify(body) }),
  placeOffer: (orderId, body) => request('/orders/' + orderId + '/offers', { method: 'POST', body: JSON.stringify(body) }),
  selectOffer: (orderId, offerId) =>
    request('/orders/' + orderId + '/select', { method: 'POST', body: JSON.stringify({ offer_id: offerId }) }),

  sendFeedback: (body) => request('/feedback', { method: 'POST', body: JSON.stringify(body) }),

  adminOverview: () => request('/admin/overview'),
  adminUsers: () => request('/admin/users'),
  adminGetUser: (id) => request('/admin/users/' + id),
  adminCreateUser: (body) => request('/admin/users', { method: 'POST', body: JSON.stringify(body) }),
  adminUpdateUser: (id, body) => request('/admin/users/' + id, { method: 'PUT', body: JSON.stringify(body) }),
  adminDeleteUser: (id) => request('/admin/users/' + id, { method: 'DELETE' }),
  adminOrders: () => request('/admin/orders'),
  adminGetOrder: (id) => request('/admin/orders/' + id),
  adminUpdateOrder: (id, body) => request('/admin/orders/' + id, { method: 'PUT', body: JSON.stringify(body) }),
  adminDeleteOrder: (id) => request('/admin/orders/' + id, { method: 'DELETE' }),
  adminFeedback: () => request('/admin/feedback'),
  adminResolveFeedback: (id) => request('/admin/feedback/' + id, { method: 'PATCH' }),
};