import prototypeHref from './themes/prototype.css?url';
import fluentHref from './themes/fluent.css?url';
import glyphHref from './themes/glyph.css?url';
import braunHref from './themes/braun.css?url';
import glassHref from './themes/glass.css?url';
import type { Lang } from './locale';

export const THEMES = ['prototype', 'fluent', 'glyph', 'braun', 'glass'] as const;
export type DockTheme = (typeof THEMES)[number];

const THEME_TEXT: Record<Lang, Record<DockTheme, { name: string; note: string }>> = {
  zh: {
    prototype: { name: '原型', note: '石墨哑光圆球' },
    fluent: { name: 'Fluent', note: 'Win11 飞出层' },
    glyph: { name: 'Glyph', note: '点阵，球上显示数字' },
    braun: { name: 'Braun', note: 'ET66 仪器' },
    glass: { name: 'Glass', note: '白霜毛玻璃' },
  },
  en: {
    prototype: { name: 'Prototype', note: 'Matte graphite orb' },
    fluent: { name: 'Fluent', note: 'Windows 11 flyout' },
    glyph: { name: 'Glyph', note: 'Dot matrix, with the count on the orb' },
    braun: { name: 'Braun', note: 'ET66 instrument' },
    glass: { name: 'Glass', note: 'Frosted glass' },
  },
};

export function themeMeta(lang: Lang = 'zh') {
  return THEME_TEXT[lang];
}

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
  const existing = document.getElementById('dock-theme');
  const el =
    existing instanceof HTMLLinkElement ? existing : document.createElement('link');
  if (existing !== el) {
    existing?.remove();
    el.id = 'dock-theme';
    el.rel = 'stylesheet';
    document.head.appendChild(el);
  }
  if (el.dataset.current !== theme) {
    el.href = HREFS[theme];
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
