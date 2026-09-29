export type Lang = 'zh' | 'en';
export type LangPref = 'system' | Lang;

export const LANG_PREF_KEY = 'orbcue-ui-lang';

/** `zh*` stays Chinese. Any other explicit tag is English. Empty stays Chinese. */
export function langFromTag(tag: string | null | undefined): Lang {
  const value = (tag ?? '').trim().toLowerCase();
  if (!value) return 'zh';
  const primary = value.split(/[-_]/)[0]?.split('.')[0] ?? '';
  return primary === 'zh' ? 'zh' : 'en';
}

export function isLangPref(value: unknown): value is LangPref {
  return value === 'system' || value === 'zh' || value === 'en';
}

export function readLangPref(storage: { getItem(key: string): string | null } | null | undefined): LangPref {
  const raw = storage?.getItem(LANG_PREF_KEY) ?? '';
  return isLangPref(raw) ? raw : 'system';
}

/** `?lang=` wins while `honorQuery` is set. A saved choice wins over the system language. */
export function resolveLang(
  pref: LangPref,
  search: string,
  navigatorLanguage?: string | null,
  honorQuery = true,
): Lang {
  if (honorQuery) {
    const query = new URLSearchParams(search).get('lang');
    if (query === 'en' || query === 'zh') return query;
  }
  if (pref === 'zh' || pref === 'en') return pref;
  return langFromTag(navigatorLanguage);
}

/** `?lang=en` or `?lang=zh` overrides the system language. Used by the browser preview. */
export function langFromLocation(search: string, navigatorLanguage?: string | null): Lang {
  return resolveLang('system', search, navigatorLanguage, true);
}

export function uiLang(): Lang {
  if (typeof window === 'undefined') return 'zh';
  const storage = typeof localStorage === 'undefined' ? null : localStorage;
  return resolveLang(readLangPref(storage), window.location.search, navigator.language, true);
}

export function applyDocumentLang(lang: Lang = uiLang()) {
  if (typeof document === 'undefined') return;
  document.documentElement.lang = lang === 'en' ? 'en' : 'zh-CN';
}
