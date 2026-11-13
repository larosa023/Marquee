import { contrast } from './img';
import { appearanceFrom, defaultAppearance, readableAccent, resolveTheme, themeVars } from './theme';
import type { Theme } from './types';

function theme(id: string, mode: 'dark' | 'light', background: string, accent: string): Theme {
  return {
    schema: 1,
    id,
    name: id,
    description: '',
    mode,
    backdrop: 'mica',
    transparency: 0.2,
    surfaceOpacity: 1,
    colors: { background, sidebar: background, surface: '#151518', surfaceHover: '#1D1D21', surfaceStrong: '#28282E', text: '#F6F2EA', subtext: '#B9B4AA', faint: '#898680', accent },
    marquee: { radius: 14, controlRadius: 10, blur: 24, shadow: 0.6, font: 'Inter Variable', displayFont: 'Inter Variable', motion: 1, glow: 0.5 },
    builtin: true,
  };
}
const marquee = theme('marquee', 'dark', '#0A0A0C', '#F2C14E');
const system = theme('system', 'dark', '#101012', 'windows');
const systemLight = theme('system', 'light', '#FAFAFA', 'windows');
const suite = theme('hangar-suite', 'dark', '#0B1020', '#7AA2FF');

describe('choosing the theme', () => {
  it('uses the chosen theme, falling back to Marquee when it no longer exists', () => {
    expect(resolveTheme({ ...defaultAppearance, themeId: 'system' }, [marquee, system], systemLight, null, false, false).id).toBe('system');
    expect(resolveTheme({ ...defaultAppearance, themeId: 'deleted-theme' }, [marquee, system], systemLight, null, false, false).id).toBe('marquee');
  });

  it('follows Windows light mode only for "Follow Windows"', () => {
    const a = { ...defaultAppearance, themeId: 'system' };
    expect(resolveTheme(a, [marquee, system], systemLight, null, false, true).mode).toBe('light');
    expect(resolveTheme({ ...a, themeId: 'marquee' }, [marquee, system], systemLight, null, false, true).mode).toBe('dark');
  });

  it("follows Hangar's suite theme only when Hangar is installed and the option is on", () => {
    expect(resolveTheme(defaultAppearance, [marquee], systemLight, suite, true, false).id).toBe('hangar-suite');
    expect(resolveTheme(defaultAppearance, [marquee], systemLight, suite, false, false).id).toBe('marquee');
    expect(resolveTheme({ ...defaultAppearance, useSuiteTheme: false }, [marquee], systemLight, suite, true, false).id).toBe('marquee');
  });

  it('fills in defaults for settings that were never saved', () => {
    expect(appearanceFrom({})).toEqual(defaultAppearance);
    expect(appearanceFrom({ appearance: { motion: 'off' } }).motion).toBe('off');
    expect(appearanceFrom({ appearance: { motion: 'off' } }).scale).toBe(1);
  });
});

describe('theme colors', () => {
  it('keeps an accent visible against the page in both modes', () => {
    expect(contrast(readableAccent('#101018', '#0A0A0C', 'dark'), '#0A0A0C')).toBeGreaterThanOrEqual(3);
    expect(contrast(readableAccent('#FFF4B0', '#FFFFFF', 'light'), '#FFFFFF')).toBeGreaterThanOrEqual(3);
    expect(readableAccent('#F2C14E', '#0A0A0C', 'dark')).toBe('#F2C14E');
  });

  it('turns a theme into CSS variables, with the Windows accent and overrides applied', () => {
    expect(themeVars(marquee, null, null)['--accent']).toBe('#F2C14E');
    expect(themeVars(system, null, '#3B82F6')['--accent']).toBe('#3B82F6');
    expect(themeVars(system, null, null)['--accent']).toBe('#F2C14E');
    expect(themeVars(marquee, '#FF5577', null)['--accent']).toBe('#FF5577');
    const vars = themeVars(marquee, null, null);
    expect(vars['--radius']).toBe('14px');
    // Text placed on the accent must be readable.
    expect(contrast(vars['--on-accent'], vars['--accent'])).toBeGreaterThanOrEqual(4.5);
  });
});
