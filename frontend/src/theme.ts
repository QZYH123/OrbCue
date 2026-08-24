import prototypeHref from './themes/prototype.css?url';
import fluentHref from './themes/fluent.css?url';
import glyphHref from './themes/glyph.css?url';
import braunHref from './themes/braun.css?url';
import glassHref from './themes/glass.css?url';

export const THEMES = ['prototype', 'fluent', 'glyph', 'braun', 'glass'] as const;
export type DockTheme = (typeof THEMES)[number];

export const THEME_META: Record<DockTheme, { name: string; note: string }> = {
  prototype: { name: '原型', note: '石墨哑光圆球' },
  fluent: { name: 'Fluent', note: 'Win11 飞出层' },
  glyph: { name: 'Glyph', note: '点阵，球上显示数字' },
  braun: { name: 'Braun', note: 'ET66 仪器' },
  glass: { name: 'Glass', note: '白霜毛玻璃' },
};

const STORAGE_KEY = 'dock-theme';
const CHANNEL = 'dock-theme';

const HREFS: Record<DockTheme, string> = {
  prototype: prototypeHref,
  fluent: fluentHref,
  glyph: glyphHref,
  braun: braunHref,
  glass: glassHref,
};

type Listener = (theme: DockTheme) => void;
const listeners = new Set<Listener>();
let channel: BroadcastChannel | null = null;

export function parseTheme(value: string | null | undefined): DockTheme {
  return THEMES.includes(value as DockTheme) ? (value as DockTheme) : 'prototype';
}

export function readTheme(): DockTheme {
  try {
    return parseTheme(localStorage.getItem(STORAGE_KEY));
  } catch {
    return 'prototype';
  }
}

export function initialTheme(): DockTheme {
  if (typeof window === 'undefined') return 'prototype';
  const fromQuery = new URLSearchParams(window.location.search).get('theme');
  if (fromQuery) return parseTheme(fromQuery);
  return readTheme();
}

export function applyTheme(theme: DockTheme) {
  if (typeof document === 'undefined') return;
  document.documentElement.dataset.theme = theme;
  let el = document.getElementById('dock-theme') as HTMLLinkElement | null;
  if (!el) {
    el = document.createElement('link');
    el.id = 'dock-theme';
    el.rel = 'stylesheet';
    document.head.appendChild(el);
  }
  const href = HREFS[theme];
  if (el.dataset.current !== theme) {
    el.href = href;
    el.dataset.current = theme;
  }
}

export function subscribeTheme(fn: Listener) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function notify(theme: DockTheme) {
  for (const fn of listeners) fn(theme);
}

function themeChannel() {
  if (channel || typeof BroadcastChannel === 'undefined') return channel;
  channel = new BroadcastChannel(CHANNEL);
  channel.onmessage = (event) => {
    const next = parseTheme(typeof event.data === 'string' ? event.data : null);
    applyTheme(next);
    notify(next);
  };
  return channel;
}

export function persistTheme(theme: DockTheme) {
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    /* ignore quota */
  }
  applyTheme(theme);
  themeChannel()?.postMessage(theme);
  notify(theme);
}

export function initTheme() {
  const theme = initialTheme();
  applyTheme(theme);
  themeChannel();
  if (typeof window !== 'undefined') {
    window.addEventListener('storage', (event) => {
      if (event.key !== STORAGE_KEY) return;
      const next = parseTheme(event.newValue);
      applyTheme(next);
      notify(next);
    });
  }
  return theme;
}
