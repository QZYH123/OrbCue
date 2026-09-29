export type Lang = 'zh' | 'en';

/** `zh*` stays Chinese. Any other explicit tag is English. Empty stays Chinese. */
export function langFromTag(tag: string | null | undefined): Lang {
  const value = (tag ?? '').trim().toLowerCase();
  if (!value) return 'zh';
  const primary = value.split(/[-_]/)[0]?.split('.')[0] ?? '';
  return primary === 'zh' ? 'zh' : 'en';
}

/** `?lang=en` or `?lang=zh` overrides the system language. Used by the browser preview. */
export function langFromLocation(search: string, navigatorLanguage?: string | null): Lang {
  const query = new URLSearchParams(search).get('lang');
  if (query === 'en' || query === 'zh') return query;
  return langFromTag(navigatorLanguage);
}

export function uiLang(): Lang {
  if (typeof window === 'undefined') return 'zh';
  return langFromLocation(window.location.search, navigator.language);
}

export function applyDocumentLang(lang: Lang = uiLang()) {
  if (typeof document === 'undefined') return;
  document.documentElement.lang = lang === 'en' ? 'en' : 'zh-CN';
}
