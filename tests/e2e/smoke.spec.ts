import { expect, test, type Page } from '@playwright/test';

/** Starts the app with the fake engine in a given state. */
async function open(page: Page, state: { onboarded?: boolean; hasTaste?: boolean } = {}, route = '#/home') {
  await page.addInitScript(
    (s) => {
      (window as unknown as { __MOCK_STATE__: unknown }).__MOCK_STATE__ = s;
    },
    { onboarded: true, hasTaste: true, ...state },
  );
  await page.goto(`/${route}`);
}

test('first run: onboarding leads to a personalised home feed', async ({ page }) => {
  await open(page, { onboarded: false, hasTaste: false });
  await expect(page.getByRole('heading', { name: 'Welcome to Marquee' })).toBeVisible();
  await page.getByRole('button', { name: 'Get started' }).click();

  // Connect: Last.fm only needs a username.
  await expect(page.getByRole('heading', { name: 'Connect what you already use' })).toBeVisible();
  await page.getByRole('listitem').filter({ hasText: 'Last.fm' }).getByRole('button', { name: 'Connect' }).click();
  await page.getByLabel('Last.fm username').fill('tester');
  await page.getByRole('dialog').getByRole('button', { name: 'Connect', exact: true }).click();
  await page.getByRole('dialog').getByRole('button', { name: 'Close' }).click();
  await expect(page.getByRole('listitem').filter({ hasText: 'Last.fm' }).getByText('Connected', { exact: true })).toBeVisible();
  await page.getByRole('button', { name: 'Continue' }).click();

  // Import is skippable.
  await expect(page.getByRole('heading', { name: 'Bring in the rest' })).toBeVisible();
  await page.getByRole('button', { name: 'Continue' }).click();

  // Pick things you love.
  await expect(page.getByRole('heading', { name: 'Pick 10 things you love' })).toBeVisible();
  for (const title of ['Signal From Europa', 'Hollow Lanterns', 'Neon Rain']) await page.getByRole('checkbox', { name: new RegExp(title) }).click();
  await expect(page.getByText('3 picked')).toBeVisible();
  await page.getByRole('button', { name: 'Continue' }).click();

  // Region and services.
  await page.getByRole('checkbox', { name: 'Test+' }).click();
  await page.getByRole('button', { name: 'Continue' }).click();

  // Theme.
  await page.getByRole('radio', { name: /Midnight/ }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'midnight');
  await page.getByRole('button', { name: 'Build my feed' }).click();

  // The sync runs for real and then hands over.
  await expect(page.getByRole('heading', { name: "You're all set" })).toBeVisible();
  await page.getByRole('button', { name: 'Open Marquee' }).click();
  await expect(page.getByRole('region', { name: 'Out This Week' })).toBeVisible();
  await expect(page.getByRole('navigation', { name: 'Main' })).toBeVisible();
});

test('search, open a title, add it to a list', async ({ page }) => {
  await open(page);
  await page.getByRole('button', { name: 'Search movies, shows, games and music' }).click();
  await page.getByRole('combobox').fill('lantern');
  await expect(page.getByRole('option', { name: /Hollow Lanterns/ })).toBeVisible();
  await page.keyboard.press('Enter');

  const detail = page.getByRole('dialog', { name: 'Hollow Lanterns' });
  await expect(detail.getByRole('heading', { name: 'Hollow Lanterns', level: 1 })).toBeVisible();
  await expect(detail.getByRole('region', { name: "Why you'd like it" })).toContainText('Because you');
  await detail.getByRole('button', { name: /Add to/ }).click();
  await page.getByRole('menuitem', { name: 'Play Later' }).click();
  await expect(page.getByRole('status').filter({ hasText: 'Added Hollow Lanterns to Play Later' })).toBeVisible();
  await expect(detail.getByRole('button', { name: /Saved/ })).toBeVisible();

  // It shows up in the list.
  await page.keyboard.press('Escape');
  await page.getByRole('navigation', { name: 'Main' }).getByRole('button', { name: 'Lists', exact: true }).click();
  await page.getByRole('button', { name: /Play Later/ }).click();
  await expect(page.getByRole('button', { name: 'Open Hollow Lanterns' })).toBeVisible();
});

test('switching theme restyles the app at once', async ({ page }) => {
  await open(page, {}, '#/settings/appearance');
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'marquee');
  await page.getByRole('radio', { name: /Pure Light/ }).click();
  await expect(page.locator('html')).toHaveAttribute('data-theme', 'pure-light');
  await expect(page.locator('html')).toHaveAttribute('data-mode', 'light');
  const background = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue('--bg').trim());
  expect(background.toUpperCase()).toBe('#FFFFFF');

  // Motion can be turned off from the same screen.
  await page.getByRole('radiogroup', { name: 'Motion' }).getByRole('radio', { name: 'Off' }).click();
  await expect(page.locator('html')).toHaveAttribute('data-motion', 'off');
});

test('the command palette jumps between screens', async ({ page }) => {
  await open(page);
  await page.keyboard.press('Control+K');
  const palette = page.getByRole('dialog', { name: 'Command palette' });
  await expect(palette).toBeVisible();
  await palette.getByRole('combobox').fill('calendar');
  await page.keyboard.press('Enter');
  await expect(page.getByRole('heading', { name: 'Release Calendar', level: 1 })).toBeVisible();
});
