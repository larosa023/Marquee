import { inTauri } from './api';

/**
 * Artwork goes through the engine's on-disk cache (`mqimg://`), so it loads instantly the
 * second time and still shows offline. Outside the app (tests) the original URL is used.
 */
export function img(url: string | null | undefined): string | undefined {
  if (!url) return undefined;
  if (!inTauri || url.startsWith('data:') || url.startsWith('blob:')) return url;
  // Windows serves custom protocols as http://<scheme>.localhost.
  const base = navigator.userAgent.includes('Windows') ? 'http://mqimg.localhost/' : 'mqimg://localhost/';
  return `${base}?u=${encodeURIComponent(url)}`;
}

/** Pulls a vivid, readable accent color out of an image (poster or cover). */
export async function accentFrom(src: string, against: 'dark' | 'light'): Promise<string | null> {
  try {
    const image = new Image();
    image.crossOrigin = 'anonymous';
    image.decoding = 'async';
    image.src = src;
    await image.decode();
    const size = 24;
    const canvas = document.createElement('canvas');
    canvas.width = canvas.height = size;
    const ctx = canvas.getContext('2d', { willReadFrequently: true });
    if (!ctx) return null;
    ctx.drawImage(image, 0, 0, size, size);
    const { data } = ctx.getImageData(0, 0, size, size);
    // Bucket by hue; score buckets by how saturated and how common they are.
    const buckets = new Map<number, { score: number; h: number; s: number; l: number; n: number }>();
    for (let i = 0; i < data.length; i += 4) {
      const [h, s, l] = rgbToHsl(data[i], data[i + 1], data[i + 2]);
      if (s < 0.25 || l < 0.15 || l > 0.9) continue;
      const key = Math.round(h / 24);
      const b = buckets.get(key) ?? { score: 0, h: 0, s: 0, l: 0, n: 0 };
      b.score += s * s;
      b.h += h;
      b.s += s;
      b.l += l;
      b.n += 1;
      buckets.set(key, b);
    }
    const best = [...buckets.values()].sort((a, b) => b.score - a.score)[0];
    if (!best || best.n < 6) return null;
    const h = best.h / best.n;
    const s = Math.min(0.9, Math.max(0.55, best.s / best.n));
    // Keep it readable on the page: light on dark themes, deep on light ones.
    const l = against === 'dark' ? 0.66 : 0.4;
    return hslToHex(h, s, l);
  } catch {
    return null;
  }
}

function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  r /= 255;
  g /= 255;
  b /= 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = max === r ? (g - b) / d + (g < b ? 6 : 0) : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
  h *= 60;
  return [h, s, l];
}

export function hslToHex(h: number, s: number, l: number): string {
  const a = s * Math.min(l, 1 - l);
  const f = (n: number) => {
    const k = (n + h / 30) % 12;
    const c = l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1));
    return Math.round(255 * c)
      .toString(16)
      .padStart(2, '0');
  };
  return `#${f(0)}${f(8)}${f(4)}`.toUpperCase();
}

function luminance(hex: string): number {
  const h = hex.replace('#', '');
  const full = h.length === 3 ? [...h].map((c) => c + c).join('') : h;
  const n = parseInt(full, 16);
  const lin = (c: number) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin((n >> 16) & 255) + 0.7152 * lin((n >> 8) & 255) + 0.0722 * lin(n & 255);
}

/** WCAG contrast ratio between two hex colors. */
export function contrast(a: string, b: string): number {
  const [la, lb] = [luminance(a), luminance(b)];
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** Near-black or white, whichever reads better on the accent. */
export function onAccent(accent: string): string {
  return contrast(accent, '#FFFFFF') > contrast(accent, '#0A0A0C') ? '#FFFFFF' : '#0A0A0C';
}
