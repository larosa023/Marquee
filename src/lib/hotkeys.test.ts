import { renderHook } from '@testing-library/react';
import { SHORTCUTS, bindings, comboFrom, prettyCombo, useHotkeys } from './hotkeys';

const key = (init: KeyboardEventInit) => new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });

describe('shortcuts', () => {
  it('names a key combination the same way however it is typed', () => {
    expect(comboFrom(key({ key: 'k', ctrlKey: true }))).toBe('Ctrl+K');
    expect(comboFrom(key({ key: 'R', ctrlKey: true, shiftKey: true }))).toBe('Ctrl+Shift+R');
    expect(comboFrom(key({ key: 'ArrowLeft', altKey: true }))).toBe('Alt+ArrowLeft');
    expect(comboFrom(key({ key: 'Control', ctrlKey: true }))).toBeNull();
    expect(prettyCombo('Alt+ArrowLeft')).toEqual(['Alt', '←']);
  });

  it('has a unique default for every shortcut, and lets overrides replace them', () => {
    const defaults = Object.values(bindings(undefined));
    expect(new Set(defaults).size).toBe(SHORTCUTS.length);
    expect(bindings({ palette: 'Ctrl+P' }).palette).toBe('Ctrl+P');
    expect(bindings({ palette: 'Ctrl+P' }).search).toBe('Ctrl+F');
  });

  it('runs the handler for a combo, including a rebound one', () => {
    const palette = vi.fn();
    const { rerender } = renderHook(({ o }) => useHotkeys(o, { palette }), { initialProps: { o: undefined as Record<string, string> | undefined } });
    window.dispatchEvent(key({ key: 'k', ctrlKey: true }));
    expect(palette).toHaveBeenCalledTimes(1);
    rerender({ o: { palette: 'Ctrl+P' } });
    window.dispatchEvent(key({ key: 'k', ctrlKey: true }));
    expect(palette).toHaveBeenCalledTimes(1);
    window.dispatchEvent(key({ key: 'p', ctrlKey: true }));
    expect(palette).toHaveBeenCalledTimes(2);
  });

  it('leaves plain keys alone while the user is typing', () => {
    const search = vi.fn();
    renderHook(() => useHotkeys(undefined, { search }));
    const input = document.createElement('input');
    document.body.append(input);
    input.dispatchEvent(key({ key: '/' }));
    expect(search).not.toHaveBeenCalled();
    document.body.dispatchEvent(key({ key: '/' }));
    expect(search).toHaveBeenCalledTimes(1);
    input.remove();
  });
});
