import { describe, expect, it } from 'vitest';
import { isLangPref, langFromLocation, langFromTag, readLangPref, resolveLang } from './locale';

describe('langFromTag', () => {
  it('keeps Chinese tags in Chinese and sends other languages to English', () => {
    expect(langFromTag('zh-CN')).toBe('zh');
    expect(langFromTag('zh-TW')).toBe('zh');
    expect(langFromTag('zh_HK.UTF-8')).toBe('zh');
    expect(langFromTag('en-US')).toBe('en');
    expect(langFromTag('en_GB')).toBe('en');
    expect(langFromTag('fr-FR')).toBe('en');
    expect(langFromTag('')).toBe('zh');
    expect(langFromTag(null)).toBe('zh');
  });
});

describe('langFromLocation', () => {
  it('lets the preview query override the system language', () => {
    expect(langFromLocation('?lang=en', 'zh-CN')).toBe('en');
    expect(langFromLocation('?page=settings&lang=zh', 'en-US')).toBe('zh');
    expect(langFromLocation('?page=settings', 'en-US')).toBe('en');
    expect(langFromLocation('', 'zh-CN')).toBe('zh');
    expect(langFromLocation('', '')).toBe('zh');
  });
});

describe('language setting', () => {
  it('keeps an explicit choice and otherwise follows the system', () => {
    expect(resolveLang('system', '', 'zh-CN')).toBe('zh');
    expect(resolveLang('system', '', 'en-US')).toBe('en');
    expect(resolveLang('en', '', 'zh-CN', false)).toBe('en');
    expect(resolveLang('zh', '', 'en-US', false)).toBe('zh');
    expect(resolveLang('zh', '?lang=en', 'en-US', true)).toBe('en');
    expect(resolveLang('zh', '?lang=en', 'en-US', false)).toBe('zh');
  });

  it('reads only system, zh, and en from storage', () => {
    expect(readLangPref(null)).toBe('system');
    expect(readLangPref({ getItem: () => 'en' })).toBe('en');
    expect(readLangPref({ getItem: () => 'nope' })).toBe('system');
    expect(isLangPref('system')).toBe(true);
    expect(isLangPref('fr')).toBe(false);
  });
});
