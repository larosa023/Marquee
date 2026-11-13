import { ago, bytes, countdown, credit, daysUntil, noun, plural, price, reasonParts, releaseLabel, runtimeLabel, typeBadge } from './format';

const now = new Date(2026, 10, 13, 15, 30); // Friday 13 November 2026, mid-afternoon

describe('release dates', () => {
  it('names the days around today', () => {
    expect(releaseLabel('2026-11-13', now)).toBe('Today');
    expect(releaseLabel('2026-11-14', now)).toBe('Tomorrow');
    expect(releaseLabel('2026-11-12', now)).toBe('Yesterday');
    expect(releaseLabel(null, now)).toBe('TBA');
  });

  it('uses the weekday this week, then the date, adding the year when it differs', () => {
    expect(releaseLabel('2026-11-17', now)).toBe(new Date(2026, 10, 17).toLocaleDateString(undefined, { weekday: 'long' }));
    expect(releaseLabel('2026-12-25', now)).toBe(new Date(2026, 11, 25).toLocaleDateString(undefined, { month: 'short', day: 'numeric' }));
    expect(releaseLabel('2027-03-05', now)).toContain('2027');
  });

  it('counts whole local days, whatever the time of day', () => {
    expect(daysUntil('2026-11-14', new Date(2026, 10, 13, 23, 59))).toBe(1);
    expect(daysUntil('2026-11-14', new Date(2026, 10, 13, 0, 1))).toBe(1);
  });

  it('counts down in days, weeks, then months, and says nothing for the past', () => {
    expect(countdown('2026-11-16', now)).toBe('In 3 days');
    expect(countdown('2026-12-11', now)).toBe('In 4 weeks');
    expect(countdown('2027-05-13', now)).toBe('In 6 months');
    expect(countdown('2026-11-01', now)).toBeNull();
  });
});

describe('labels', () => {
  it('formats lengths per medium', () => {
    expect(runtimeLabel({ media: 'movie', runtime: 134 })).toBe('2h 14m');
    expect(runtimeLabel({ media: 'tv', runtime: 48 })).toBe('48 min episodes');
    expect(runtimeLabel({ media: 'game', runtime: 600 })).toBe('~10 h');
    expect(runtimeLabel({ media: 'movie', runtime: null })).toBeNull();
  });

  it('formats prices, including free and unknown', () => {
    expect(price(0)).toBe('Free');
    expect(price(null)).toBeNull();
    expect(price(2499, 'USD')).toContain('24.99');
  });

  it('tells EPs and singles from albums', () => {
    expect(typeBadge({ media: 'album', meta: { album_type: 'ep' } })).toBe('EP');
    expect(typeBadge({ media: 'album', meta: {} })).toBe('Album');
    expect(typeBadge({ media: 'tv', meta: {} })).toBe('TV');
  });

  it('credits the artist first, then whoever made it', () => {
    expect(credit({ meta: { artist: 'Burial', director: 'x' } })).toBe('Burial');
    expect(credit({ meta: { developer: 'Mobius Digital' } })).toBe('Mobius Digital');
    expect(credit({ meta: {} })).toBeNull();
  });

  it('splits a reason into plain text and titles', () => {
    expect(reasonParts('Because you rated *Arrival* 5★ and play *Outer Wilds*')).toEqual([
      { text: 'Because you rated ', em: false },
      { text: 'Arrival', em: true },
      { text: ' 5★ and play ', em: false },
      { text: 'Outer Wilds', em: true },
    ]);
  });

  it('pluralises with and without the number', () => {
    expect(plural(1, 'title')).toBe('1 title');
    expect(plural(1200, 'match', 'matches')).toBe(`${(1200).toLocaleString()} matches`);
    expect(noun(1, 'entry', 'entries')).toBe('entry');
    expect(noun(0, 'entry', 'entries')).toBe('entries');
  });

  it('describes sizes and how long ago', () => {
    expect(bytes(52 * 1024 * 1024)).toBe('52 MB');
    expect(bytes(1.5 * 1024 ** 3)).toBe('1.50 GB');
    const t = Date.UTC(2026, 10, 13, 12);
    expect(ago(null, t)).toBe('never');
    expect(ago(new Date(t - 20_000).toISOString(), t)).toBe('just now');
    expect(ago(new Date(t - 5 * 3_600_000).toISOString(), t)).toBe('5 h ago');
    expect(ago(new Date(t - 3 * 86_400_000).toISOString(), t)).toBe('3 days ago');
  });
});
