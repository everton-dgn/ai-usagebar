import assert from 'node:assert/strict';
import {
  accountSwitchFor,
  focusOnVisibility,
  applyCardNames,
  renameCard,
  formatDuration,
  nextUpdateLabel,
  parseHostPayload,
  projectCards,
  resetLabel,
  friendlyError,
  explainError,
  displayPlan,
  leftLabel,
  emptyLayout,
  memoryStorage,
  loadLayout,
  layoutForProviderView,
  updateProviderViewLayout,
  resetProviderViewLayout,
  prefsForCard,
  saveLayout,
  syncLayout,
  applyCardLayout,
  orderedCards,
  moveCardBefore,
  nudgeCard,
  visibleRowsFor,
  cardHasExtras,
  rowKey,
  mergeRowPrefs,
  defaultRowPrefs,
  quotaLabel,
  quotaAlternate,
  headlineLabel,
  headlineAlternate,
  lacksCredentials,
  seedLayout,
  hintPending,
  absorbPayload,
  setRowEnabled,
  moveRowToList,
  LAYOUT_KEY,
  normalizeLayout,
  normalizeColorThresholds,
  commitColorThresholds,
  usageColor,
  resetText,
  resetAlternate,
  formatResetExact,
  formatResetCreditDate,
  resetCreditDetails,
  condensedTextRowIndexes,
  providerIconId,
  initialsGlyph,
  metricCount,
  sendCommand,
  resolvedTheme,
  emptyPayload,
  pace,
  paceText,
  paceVisible,
  usageGoal,
  prettyMetricLabel,
  shortcutFromKeyEvent,
  defaultStars,
  toggleStar,
  isStarred,
  seedStars,
  stripCommand,
  normalizeMenuBarItems,
  MAX_STARS_PER_PROVIDER,
  expirySeverity,
  providerLinks,
  isHttpUrl,
} from './src/model.js';
import { measurePanelHeight, panelHeight } from './src/panel-size.js';

assert.equal(panelHeight(274, true), 274);
assert.equal(panelHeight(274, false), 360);
assert.equal(panelHeight(420, true), 420);
assert.equal(panelHeight(0, true), 0);
// A delayed close cannot clear a newer provider click; opening restores the
// native selection even when the click occurred before the JS handlers existed.
assert.equal(focusOnVisibility('openai@work', false, null), 'openai@work');
assert.equal(focusOnVisibility('', true, 'openai@work'), 'openai@work');
assert.equal(focusOnVisibility('openai@work', true, 'anthropic@home'), 'anthropic@home');
assert.equal(focusOnVisibility('openai@work', true, null), '');
assert.equal(focusOnVisibility('openai@work', false, undefined), 'openai@work');

// The native window can retain a tall viewport while WebView2 is hidden. Its
// stretched scroll region must not become the next requested panel height.
const intrinsicContent = { offsetHeight: 420, scrollHeight: 510 };
const stretchedScroller = { dataset: { scroll: '' }, offsetHeight: 900 };
const footerChrome = { dataset: {}, offsetHeight: 44 };
assert.equal(measurePanelHeight({
  children: [stretchedScroller, footerChrome],
  querySelector: () => intrinsicContent,
}), 554);

const report = {
  version: '1.10.0',
  generated_at: 1_000,
  next_refresh_at: 61_000,
  startup_enabled: true,
  host_error: null,
  menu_bar_show_all: false,
  menu_bar_hide_value: true,
  menu_bar_window: 'weekly',
  menu_bar_chart: true,
  primary: 'anthropic',
  entries: [
    {
      id: 'anthropic',
      display_name: 'Claude',
      short_name: 'cld',
      plan: 'Team 5x',
      status: 'ready',
      error: null,
      stale: false,
      sections: [
        {
          type: 'metric',
          label: 'Weekly',
          percent: 19,
          value: '19%',
          detail: 'Resets in 1d 16h',
          severity: 'low',
          reset_at: '2026-09-05T00:00:00Z',
        },
        {
          type: 'text',
          label: 'Balance',
          value: '$12.50',
        },
        { type: 'spacer' },
      ],
    },
  ],
};

const payload = parseHostPayload(report);
assert.equal(payload.version, '1.10.0');
assert.equal(payload.startupEnabled, true);
assert.equal(payload.menuBarShowAll, false);
assert.equal(payload.menuBarHideValue, true);
assert.equal(payload.menuBarWindow, 'weekly');
assert.equal(payload.menuBarChart, true);
assert.equal(payload.menuBarProvider, 'highest');
assert.equal(payload.notificationsEnabled, true);
assert.equal(payload.notificationsThreshold, 97);
assert.equal(parseHostPayload({ notifications_enabled: false, notifications_threshold: 85 }).notificationsEnabled, false);
assert.equal(parseHostPayload({ notifications_threshold: 85 }).notificationsThreshold, 85);
assert.equal(parseHostPayload({ notifications_threshold: 101 }).notificationsThreshold, 97);
assert.equal(parseHostPayload({ menu_bar_provider: 'openai@work' }).menuBarProvider, 'openai@work');
assert.equal(payload.entries.length, 1);
assert.equal(payload.entries[0].displayName, 'Claude');
assert.equal(payload.entries[0].sections.length, 2); // spacer dropped

assert.equal(formatDuration(0), 'now');
assert.equal(formatDuration(90_000), '1m');
assert.equal(formatDuration(3_600_000 + 120_000), '1h 2m');
assert.equal(formatDuration(2 * 86_400_000 + 3_600_000), '2d 1h');

const cards = projectCards(payload, Date.parse('2026-09-04T00:00:00Z'));
assert.equal(cards.length, 1);
assert.equal(cards[0].title, 'Claude');
assert.equal(cards[0].plan, 'Team 5x');
assert.equal(cards[0].rows[0].kind, 'metric');
assert.equal(cards[0].rows[0].leftPercent, 81);
assert.equal(cards[0].rows[0].usedPercent, 19);
assert.equal(cards[0].rows[0].reset, 'Resets in 1d 0h');
assert.equal(cards[0].rows[1].kind, 'text');
assert.equal(cards[0].rows[1].value, '$12.50');

assert.equal(
  resetLabel({ type: 'metric', resetAt: '', detail: 'Resets in 3h' }, 0),
  'Resets in 3h',
);

assert.equal(nextUpdateLabel(payload, 1_000), 'Next update in 1m');
assert.equal(nextUpdateLabel(payload, 61_000), 'Updating…');

const bad = parseHostPayload('{');
assert.equal(bad.hostError.includes('valid JSON'), true);
assert.equal(bad.entries.length, 0);

const stripped = parseHostPayload({
  version: '1',
  entries: [{
    id: 'x',
    display_name: 'Hi\u0007 there',
    sections: [{ type: 'metric', label: 'S', percent: 150, severity: 'nope' }],
  }],
});
assert.equal(stripped.entries[0].displayName.includes('\u0007'), false);
assert.equal(stripped.entries[0].sections[0].percent, 100);
assert.equal(stripped.entries[0].sections[0].severity, 'low');

const failed = parseHostPayload({
  host_error: 'no vendors enabled',
  entries: [{ id: 'openai', display_name: 'Codex', status: 'error', error: 'not signed in', sections: [] }],
});
assert.equal(failed.hostError, 'no vendors enabled');
assert.equal(failed.entries[0].status, 'error');
assert.equal(failed.entries[0].error, 'not signed in');

const errored = parseHostPayload({
  entries: [{
    id: 'openai',
    display_name: 'Codex',
    status: 'error',
    error: 'not signed in',
      sign_in: 'Run `codex login` in a terminal, then Refresh.',
    stale: true,
    sections: [],
  }],
});
const errorCards = projectCards(errored, 0);
assert.equal(errorCards[0].errorTitle, 'Sign-in expired');
assert.equal(errorCards[0].errorHint, 'This app needs a saved provider session. Signing in is not available here yet.');
assert.equal(errorCards[0].error, 'Sign-in expired. This app needs a saved provider session. Signing in is not available here yet.');
assert.equal(errorCards[0].errorDetail, 'not signed in');
assert.equal(errorCards[0].stale, true);
assert.equal(errorCards[0].rows.length, 0);

const claudeErrored = parseHostPayload({
  entries: [{
    id: 'anthropic',
    display_name: 'Claude',
    plan: 'Claude Max 5x',
    status: 'error',
    error: 'HTTP 401: authentication rejected — credentials may be missing, expired, or invalid',
    sign_in: 'Run `claude` in a terminal, then Refresh.',
    sections: [],
  }],
});
const claudeCards = projectCards(claudeErrored, 0);
assert.equal(claudeCards[0].title, 'Claude');
assert.equal(claudeCards[0].plan, 'Claude Max 5x');
assert.equal(claudeCards[0].errorTitle, 'Sign-in expired');
assert.equal(claudeCards[0].errorHint, 'This app needs a saved provider session. Signing in is not available here yet.');
assert.equal(claudeCards[0].rows.length, 0);

// Sign-in hints never hand the user a terminal step, whatever the host sends.
{
  const TERMINAL = /`|terminal|\brun\b|\bCLI\b|login --|config\.toml/i;
  const hostHints = [
    ['anthropic', 'HTTP 401: authentication rejected', 'Run `claude` in a terminal, then Refresh.'],
    ['openai', 'token refresh failed; run `codex login` to re-auth', 'Run `codex login` in a terminal, then Refresh.'],
    ['copilot', 'HTTP 403: forbidden', 'Run `gh auth login` in a terminal, then Refresh.'],
    ['antigravity', 'not signed in', 'Open Antigravity or run `agy`, then Refresh.'],
    ['modelstudio', 'not signed in', 'Install the official `bl` CLI and run `bl auth login --console`, then Refresh.'],
    ['zai', 'HTTP 401: authentication rejected', 'Set the API key in the config, then Refresh.'],
    ['anthropic', 'HTTP 401: authentication rejected', undefined],
    ['openai', 'not signed in', ''],
  ];
  for (const [id, error, signIn] of hostHints) {
    const explained = explainError(error, { signIn });
    assert.equal(explained.title, 'Sign-in expired', `${id}: ${error}`);
    assert.equal(explained.hint, 'This app needs a saved provider session. Signing in is not available here yet.', `${id}: ${signIn}`);
    assert.doesNotMatch(explained.hint, TERMINAL);
    assert.equal(explained.action, undefined);
  }
  // A graphical hint from the host is kept as written.
  assert.equal(
    explainError('HTTP 401: authentication rejected', { signIn: 'Sign in to the Cursor app, then Refresh.' }).hint,
    'Sign in to the Cursor app, then Refresh.',
  );
  // A product whose name ends in "CLI" is not a terminal step. These are the
  // Copilot and Kiro sentences from VendorId::sign_in_hint (src/vendor.rs).
  for (const signIn of [
    'Needs a saved GitHub CLI sign-in with Copilot access. Signing in is not available in this app.',
    'Needs a saved Kiro CLI sign-in. Signing in is not available in this app.',
  ]) {
    assert.equal(explainError('HTTP 401: authentication rejected', { signIn }).hint, signIn);
  }
  const copilotCards = projectCards(parseHostPayload({
    entries: [{
      id: 'copilot', display_name: 'Copilot', status: 'error', error: 'HTTP 403: forbidden', sections: [],
      sign_in: 'Needs a saved GitHub CLI sign-in with Copilot access. Signing in is not available in this app.',
    }],
  }), 0);
  assert.equal(copilotCards[0].errorHint, 'Needs a saved GitHub CLI sign-in with Copilot access. Signing in is not available in this app.');
  // A bare CLI is still an instruction to open a terminal.
  for (const signIn of [
    'Use the gh CLI to sign in, then Refresh.',
    'Sign in with the CLI, then Refresh.',
    'Sign in with the cli, then Refresh.',
    'Install the official bl CLI, then Refresh.',
  ]) {
    assert.equal(
      explainError('HTTP 401: authentication rejected', { signIn }).hint,
      'This app needs a saved provider session. Signing in is not available here yet.',
      signIn,
    );
  }
  // The rotated-token messages from anthropic::fetch and openai::fetch reach
  // the card as a warning. They name no command and still read as a sign-in.
  for (const [signIn, error] of [
    ['Needs a saved Claude Code sign-in on this Mac. Signing in is not available in this app.',
      'refreshed token could not be saved (disk full); the rotated refresh token is lost, so a new Claude Code sign-in is needed, which this app cannot do'],
    ['Sign in to the Codex app on this Mac, then Refresh.',
      'refreshed token could not be saved (disk full); the rotated refresh token is lost; sign in to the Codex app again'],
  ]) {
    const explained = explainError(error, { signIn });
    assert.equal(explained.title, 'Sign-in expired', error);
    assert.equal(explained.hint, signIn);
    assert.doesNotMatch(error, TERMINAL);
  }
  // Every provider's sign-in failure from the Rust side reads as a sign-in,
  // through one rule, and keeps the provider's own hint. Sources in comments.
  {
    const signIn = 'Provider sign-in hint.';
    for (const error of [
      // Refreshed credentials that could not be saved.
      'credentials error: refreshed Kiro CLI credentials could not be saved (disk full); a new Kiro CLI sign-in is needed if the refresh token was rotated, which this app cannot do', // kiro/fetch.rs
      'credentials error: the refreshed Kimi Code credentials could not be saved (disk full); a new Kimi Code sign-in is needed, which this app cannot do', // kimi/fetch.rs
      'credentials error: the refreshed Grok Bot credentials could not be saved (disk full); sign in to the Grok Bot desktop app again if the refresh token was rotated', // grokbot/fetch.rs
      // Explicit requests to sign in again.
      'credentials error: Kiro CLI token refresh failed (HTTP 400). A new Kiro CLI sign-in is needed, which this app cannot do.', // kiro/fetch.rs
      'credentials error: Command Code sign-in expired. A new sign-in is needed, which this app cannot do.', // commandcode/creds.rs
      'credentials error: Model Studio: console session expired; a new Model Studio console sign-in is needed, which this app cannot do', // modelstudio/types.rs
      "credentials error: Model Studio: the bl CLI's config.json carries no console token; a new Model Studio console sign-in is needed, which this app cannot do", // modelstudio/creds.rs
      'credentials error: Grok Build login file not found; a new Grok Build sign-in is needed, which this app cannot do', // supergrok/direct.rs
      'credentials error: GitHub Copilot: no token. A GitHub CLI sign-in with Copilot access is needed, which this app cannot do.', // copilot/credentials.rs
      'credentials error: could not parse Keychain: bad json. A new Claude Code sign-in is needed, which this app cannot do.', // anthropic/creds.rs
      'credentials error: Cursor session token is empty. Sign in to the Cursor IDE again.', // cursor/db.rs
      'credentials error: Grok Bot token refresh was rejected; sign in to the Grok Bot desktop app again', // grokbot/fetch.rs
      "credentials error: Antigravity's saved Google session expired; open Antigravity to sign in again", // antigravity/cloud.rs
      "credentials error: Antigravity's saved Google session expired and this app cannot refresh it; open Antigravity to sign in again", // antigravity/fetch.rs
    ]) {
      const explained = explainError(error, { signIn });
      assert.equal(explained.title, 'Sign-in expired', error);
      assert.equal(explained.hint, signIn, error);
      // "... CLI" here is a product name; no message names a command.
      assert.doesNotMatch(error, /`|terminal|\brun\b|login --|config\.toml/i, error);
    }
    // The rule does not swallow errors that have their own title.
    for (const [error, title] of [
      ['no local server found. Or sign in to Antigravity once, so its saved Google session can be used while it is closed.', "Antigravity isn't running"],
      ['HTTP 429: rate limited; next attempt in 5m', 'Too many requests'],
      ['HTTP 503: service unavailable', 'Provider is unavailable'],
      ['network transport error: timed out', "Can't reach the server"],
      ['usage response did not contain valid JSON', "Couldn't read usage data"],
      ['grokbot cache belongs to a different sign-in; refetching', "Couldn't update"],
      ['the refreshed usage payload was discarded; the cache could not be saved later', "Couldn't update"],
    ]) {
      assert.equal(explainError(error, { signIn }).title, title, error);
    }
  }
  // Caches written by older builds still carry the command wording.
  assert.equal(
    explainError('refreshed token could not be saved (x); the rotated refresh token is lost — re-run `claude` to log in again').title,
    'Sign-in expired',
  );
  // No hint the model can emit on its own mentions a terminal.
  for (const error of ['no vendors enabled', 'no API key', 'io error', 'did not contain valid JSON', 'no local server found', '']) {
    assert.doesNotMatch(explainError(error).hint || '', TERMINAL, error);
  }
}

const none = parseHostPayload({ version: '1', entries: [] });
assert.equal(none.entries.length, 0);
assert.equal(projectCards(none, 0).length, 0);
assert.equal(nextUpdateLabel({ nextRefreshAt: 0 }, 1), 'Updating…');

assert.equal(
  friendlyError('Zai: no API key. Either set an API key in a valid environment variable or set `api_key` under [zai] in C:\\Users\\dj4lm\\AppData\\Roaming\\ai-usagebar\\config\\config.toml.'),
  "No API key. API keys can't be added in this app yet.",
);
assert.equal(
  friendlyError('HTTP 429: Rate limited. Please try again later.'),
  'Too many requests. Try Refresh in a minute.',
);
assert.equal(
  explainError('network transport error: connection refused', 'openai').title,
  "Can't reach the server",
);
assert.equal(
  explainError('HTTP 503: provider unavailable', 'zai').title,
  'Provider is unavailable',
);
assert.ok(!friendlyError('Zai: no API key in C:\\Users\\dj4lm\\AppData\\Roaming\\ai-usagebar\\config\\config.toml.').includes('AppData'));

assert.equal(displayPlan('Claude', 'Claude Max 5x'), 'Max 5x');
assert.equal(displayPlan('Claude', 'Team 5x'), 'Team 5x');
assert.equal(displayPlan('Codex', 'Plus'), 'Plus');
assert.equal(leftLabel(81), '81% left');
assert.equal(leftLabel(0), 'Limit reached');
assert.equal(quotaLabel({ kind: 'metric', leftPercent: 81, usedPercent: 19 }, 'left'), '81% left');
assert.equal(quotaLabel({ kind: 'metric', leftPercent: 81, usedPercent: 19 }, 'used'), '19% used');
assert.equal(quotaLabel({ kind: 'metric', leftPercent: 0, usedPercent: 100 }, 'used'), 'Limit reached');

const exhausted = parseHostPayload({
  entries: [{
    id: 'cursor',
    display_name: 'Cursor',
    plan: 'Ultra',
    sections: [
      { type: 'metric', label: 'Total', percent: 27, severity: 'low' },
      { type: 'metric', label: 'Auto', percent: 2, severity: 'low' },
      { type: 'metric', label: 'API', percent: 100, severity: 'critical' },
      { type: 'text', label: 'Extra Usage', value: '$364.04 spent' },
    ],
  }],
});
const cursorCard = projectCards(exhausted, 0)[0];
assert.equal(cursorCard.rows[2].leftPercent, 0);
assert.equal(leftLabel(cursorCard.rows[2].leftPercent), 'Limit reached');
assert.equal(cardHasExtras(cursorCard, false), true);
assert.equal(visibleRowsFor(cursorCard, { collapsed: true }).map((r) => r.kind).join(','), 'metric,metric');
assert.equal(visibleRowsFor(cursorCard, { collapsed: false, hideExtras: true }).length, 3);
assert.equal(visibleRowsFor(cursorCard, { collapsed: false, hideExtras: false }).length, 4);
assert.equal(rowKey(cursorCard.rows[0]), 'metric:Total');

// A "Resets" text section repeats the countdown every meter already shows.
const withResets = parseHostPayload({
  entries: [
    {
      id: 'cursor',
      display_name: 'Cursor',
      sections: [
        { type: 'metric', label: 'Cursor Models', percent: 32, severity: 'low', reset_at: '2026-09-09T00:00:00Z' },
        { type: 'text', label: 'Resets', value: '4d 14h' },
      ],
    },
    {
      id: 'zai',
      display_name: 'Z.AI',
      sections: [
        { type: 'metric', label: 'Balance', percent: 10, severity: 'low' },
        { type: 'text', label: 'Resets', value: '12d 2h' },
      ],
    },
  ],
});
const [resetsFolded, resetsKept] = projectCards(withResets, 0);
assert.equal(resetsFolded.rows.map((r) => r.kind).join(','), 'metric');
assert.equal(resetsKept.rows.map((r) => r.label).join(','), 'Balance,Resets');

// Banked resets stay structured through normalization so the dashboard can
// keep the compact count in the card and put every expiry in its tooltip.
const bankedResets = parseHostPayload({
  entries: [{
    id: 'openai',
    display_name: 'Codex',
    reset_credits: {
      available: 3,
      credits: [
        { title: 'Full reset', expires_at: '2026-10-05T04:18:00Z' },
        { title: 'Full reset', expires_at: '2026-09-20T23:58:00Z' },
      ],
    },
    sections: [{ type: 'block', label: 'Reset credits', body: ['legacy text'] }],
  }],
});
assert.equal(bankedResets.entries[0].resetCredits.available, 3);
assert.equal(bankedResets.entries[0].resetCredits.credits[0].expiresAt, '2026-10-05T04:18:00Z');
const resetRow = projectCards(bankedResets, Date.parse('2026-09-15T10:00:00Z'))[0].rows[0];
assert.equal(resetRow.kind, 'resetCredits');
assert.equal(resetRow.label, 'Rate Limit Resets');
assert.equal(resetRow.available, 3);

const resetDetails = resetCreditDetails(resetRow, Date.parse('2026-09-15T10:00:00Z'), {
  locale: 'en-US',
  timeZone: 'UTC',
  timeFormat: '24',
});
assert.deepEqual(resetDetails.items.map((item) => item.date), [
  'Sep 20 at 23:58',
  'Oct 5 at 04:18',
  'Date unavailable',
]);
assert.deepEqual(resetDetails.items.map((item) => item.remaining), ['5d 13h', '19d 18h', '—']);
assert.equal(resetDetails.hidden, 0);
assert.equal(
  formatResetCreditDate('2026-09-20T23:58:00Z', { locale: 'en-US', timeZone: 'UTC', timeFormat: '12' }),
  'Sep 20 at 11:58 PM',
);

// Older hosts do not send `reset_credits`; their existing text block remains visible.
const legacyResetCard = projectCards(parseHostPayload({
  entries: [{
    id: 'openai',
    display_name: 'Codex',
    sections: [{ type: 'block', label: 'Reset credits', body: ['2 resets available'] }],
  }],
}), 0)[0];
assert.equal(legacyResetCard.rows[0].kind, 'block');

const extraOnAlways = moveRowToList(
  defaultRowPrefs(cursorCard.rows),
  'text:Extra Usage',
  'always',
  null,
);
assert.deepEqual(extraOnAlways.always.slice(-1), ['text:Extra Usage']);
assert.equal(visibleRowsFor(cursorCard, { collapsed: true, prefs: extraOnAlways }).length, 3);
const cursorPrefs = defaultRowPrefs(cursorCard.rows);
const disabledApi = setRowEnabled(cursorPrefs, 'metric:API', false);
assert.equal(disabledApi.off['metric:API'], true);
assert.deepEqual(disabledApi.always, cursorPrefs.always);
assert.deepEqual(disabledApi.demand, cursorPrefs.demand);
assert.equal(visibleRowsFor(cursorCard, { collapsed: false, prefs: disabledApi }).length, 3);
const reenabledApi = setRowEnabled(disabledApi, 'metric:API', true);
assert.equal(reenabledApi.off['metric:API'], undefined);
assert.deepEqual(reenabledApi.always, cursorPrefs.always);
assert.deepEqual(reenabledApi.demand, cursorPrefs.demand);
const mergedStored = mergeRowPrefs(cursorCard.rows, {
  always: ['metric:API'],
  demand: ['metric:Total'],
  off: { 'text:Extra Usage': true },
});
assert.ok(mergedStored.always.indexOf('metric:API') >= 0);
assert.ok(mergedStored.demand.indexOf('metric:Total') >= 0);
assert.equal(mergedStored.off['text:Extra Usage'], true);

const three = [
  { id: 'anthropic', title: 'Claude' },
  { id: 'openai', title: 'Codex' },
  { id: 'cursor', title: 'Cursor' },
];
assert.deepEqual(
  applyCardLayout(three, { cardOrder: ['cursor', 'anthropic'], hidden: { openai: true }, collapsed: {}, hideExtras: false }).map((c) => c.id),
  ['cursor', 'anthropic'],
);
assert.deepEqual(moveCardBefore(['a', 'b', 'c'], 'c', 'a'), ['c', 'a', 'b']);
assert.deepEqual(moveCardBefore(['a', 'b', 'c'], 'a', null), ['b', 'c', 'a']);
assert.deepEqual(nudgeCard(['a', 'b', 'c'], 'b', -1), ['b', 'a', 'c']);
assert.deepEqual(orderedCards(three, { cardOrder: ['cursor'] }).map((c) => c.id), ['cursor', 'anthropic', 'openai']);

const store = memoryStorage();
saveLayout(store, { cardOrder: ['cursor', 'openai'], hidden: { openai: true }, collapsed: { cursor: true }, hideExtras: true });
const loaded = loadLayout(store);
assert.equal(loaded.hideExtras, true);
assert.equal(loaded.hidden.openai, true);
assert.equal(loaded.collapsed.cursor, true);
assert.deepEqual(loaded.cardOrder, ['cursor', 'openai']);
const synced = syncLayout(loaded, ['anthropic', 'openai', 'cursor']);
assert.deepEqual(synced.cardOrder, ['cursor', 'openai', 'anthropic']);
assert.ok(store.getItem(LAYOUT_KEY).includes('cursor'));
assert.deepEqual(emptyLayout().cardOrder, []);

// A failed disk write is reported, while malformed saved data is left intact.
{
  const broken = { getItem() { return '{invalid'; }, setItem() { throw new Error('storage unavailable'); } };
  let readFailed = false;
  assert.deepEqual(loadLayout(broken, () => { readFailed = true; }), emptyLayout());
  assert.equal(readFailed, true);
  assert.equal(saveLayout(broken, emptyLayout()), false);
  assert.equal(saveLayout(memoryStorage(), emptyLayout()), true);
}

// A provider missing from one report must retain its customization on return.
{
  const remembered = {
    ...emptyLayout(), seeded: true,
    cardOrder: ['openai@work', 'zai'],
    hidden: { 'openai@work': true }, collapsed: { 'openai@work': true },
    names: { 'openai@work': 'Work' },
    rows: { 'openai@work': { always: ['metric:Weekly'], demand: [], off: { 'block:Credits': true, 'link:Status': true } } },
    stars: { 'openai@work': ['metric:Weekly'] },
  };
  const store = memoryStorage();
  saveLayout(store, absorbPayload(remembered, [{ id: 'zai' }]));
  const restored = absorbPayload(loadLayout(store), [{ id: 'zai' }, { id: 'openai@work' }]);
  for (const key of ['cardOrder', 'hidden', 'collapsed', 'names', 'rows', 'stars']) {
    assert.deepEqual(restored[key], remembered[key], `${key} survives a partial report and reload`);
  }
  const prefs = remembered.rows['openai@work'];
  assert.deepEqual(providerLinks('openai@work', prefs).map((link) => link.label), ['Dashboard']);
  assert.deepEqual(providerLinks('openai@work', { off: { 'link:Status': true, 'link:Dashboard': true } }), []);
  assert.equal(providerLinks('openai@personal').length, 2, 'links are configured per account');
}

// Individual dropdowns start fully expanded with every row and link enabled,
// regardless of overview restrictions. Later edits/resets remain independent.
{
  const id = 'openai@work';
  const otherId = 'openai@personal';
  const card = { id, rows: [
    { kind: 'metric', key: 'metric:weekly', label: 'Weekly' },
    { kind: 'metric', key: 'metric:session', label: 'Session' },
    { kind: 'block', label: 'Credits', body: ['balance: 20'] },
  ] };
  const legacy = {
    rows: {
      [id]: { always: ['metric:weekly'], demand: ['metric:session', 'block:Credits'],
        off: { 'block:Credits': true, 'link:Status': true, 'link:Dashboard': true } },
      [otherId]: { always: ['metric:weekly'], demand: [], off: {} },
    },
    collapsed: { [id]: true }, hideExtras: true, seeded: true,
    names: { [id]: 'Work' }, stars: { [id]: ['metric:weekly'] },
  };
  const store = memoryStorage({ [LAYOUT_KEY]: JSON.stringify(legacy) });
  const migrated = loadLayout(store);
  assert.deepEqual(migrated.individual, { rows: {}, collapsed: {}, hideExtras: false });
  assert.deepEqual(migrated.rows, legacy.rows, 'keep overview customization');
  const defaults = layoutForProviderView(migrated, 'individual');
  assert.deepEqual(visibleRowsFor(card, { prefs: prefsForCard(card, defaults), collapsed: defaults.collapsed[id], hideExtras: defaults.hideExtras }).map(rowKey), card.rows.map(rowKey));
  assert.deepEqual(providerLinks(id, prefsForCard(card, defaults)).map((link) => link.label), ['Status', 'Dashboard']);

  let prefs = prefsForCard(card, layoutForProviderView(migrated, 'individual'));
  prefs = setRowEnabled(prefs, 'block:Credits', true);
  prefs = setRowEnabled(prefs, 'link:Dashboard', true);
  prefs = setRowEnabled(prefs, 'link:Status', false);
  prefs = moveRowToList(prefs, 'metric:session', 'always', 'metric:weekly');
  const detailed = updateProviderViewLayout(migrated, 'individual', {
    rows: { ...migrated.individual.rows, [id]: prefs }, collapsed: {},
  });
  assert.deepEqual(detailed.rows, legacy.rows, 'individual editing leaves the overview untouched');
  assert.deepEqual(detailed.collapsed, legacy.collapsed);
  assert.deepEqual(detailed.individual.rows[otherId], migrated.individual.rows[otherId]);
  assert.deepEqual(providerLinks(id, detailed.individual.rows[id]).map((link) => link.label), ['Dashboard']);
  assert.deepEqual(providerLinks(id, detailed.rows[id]), []);

  const simpler = updateProviderViewLayout(detailed, 'overview', {
    rows: { ...detailed.rows, [id]: setRowEnabled(prefsForCard(card, detailed), 'metric:session', false) },
  });
  assert.deepEqual(simpler.individual, detailed.individual, 'overview editing leaves the dropdown untouched');
  saveLayout(store, absorbPayload(simpler, [{ id: otherId }]));
  const reloaded = absorbPayload(loadLayout(store), [{ id }, { id: otherId }]);
  assert.deepEqual(reloaded.rows, simpler.rows);
  assert.deepEqual(reloaded.individual, detailed.individual);
  const fullList = layoutForProviderView(reloaded, 'overview');
  const individual = layoutForProviderView(reloaded, 'individual');
  assert.deepEqual(visibleRowsFor(card, { prefs: prefsForCard(card, fullList), collapsed: fullList.collapsed[id] }).map(rowKey), ['metric:weekly']);
  assert.deepEqual(visibleRowsFor(card, { prefs: prefsForCard(card, individual), collapsed: individual.collapsed[id] }).map(rowKey), ['metric:session', 'metric:weekly', 'block:Credits']);
  assert.deepEqual(individual.names, legacy.names, 'names stay shared');
  assert.deepEqual(individual.stars, legacy.stars, 'menu-bar stars stay shared');

  for (const view of ['overview', 'individual']) {
    const reset = resetProviderViewLayout(reloaded, view, id);
    const own = layoutForProviderView(reset, view);
    const otherView = view === 'overview' ? 'individual' : 'overview';
    assert.equal(own.rows[id], undefined);
    assert.equal(own.collapsed[id], undefined);
    assert.deepEqual(own.rows[otherId], layoutForProviderView(reloaded, view).rows[otherId]);
    assert.deepEqual(layoutForProviderView(reset, otherView).rows, layoutForProviderView(reloaded, otherView).rows);
    assert.deepEqual(layoutForProviderView(reset, otherView).collapsed, layoutForProviderView(reloaded, otherView).collapsed);
    saveLayout(store, reset);
    assert.equal(layoutForProviderView(loadLayout(store), view).rows[id], undefined, 'reset survives reload without inheriting the other mode');
    if (view === 'individual') {
      const restored = layoutForProviderView(loadLayout(store), view);
      assert.deepEqual(visibleRowsFor(card, { prefs: prefsForCard(card, restored), collapsed: restored.collapsed[id] }).map(rowKey), card.rows.map(rowKey));
      assert.equal(providerLinks(id, prefsForCard(card, restored)).length, 2);
    }
  }
  assert.deepEqual(normalizeLayout({ ...legacy, individual: {} }).individual, { rows: {}, collapsed: {}, hideExtras: false });
}

// Clearing the last star is a saved choice, not a request to seed defaults again.
{
  const cards = [{ id: 'openai', rows: [{ kind: 'metric', label: 'Weekly' }] }];
  const initial = seedStars(emptyLayout(), cards);
  const store = memoryStorage();
  saveLayout(store, { ...initial, stars: {} });
  assert.deepEqual(seedStars(loadLayout(store), cards).stars, {});
}

// Language must survive storage normalization and every host payload refresh.
const languageStore = memoryStorage();
saveLayout(languageStore, { ...emptyLayout(), language: 'pt-BR' });
assert.equal(loadLayout(languageStore).language, 'pt-BR');
assert.equal(syncLayout(loadLayout(languageStore), ['anthropic']).language, 'pt-BR');
assert.equal(normalizeLayout({ language: 'invalid' }).language, 'en');
assert.equal(nextUpdateLabel({ nextRefreshAt: 120_000 }, 60_000, 'pt-BR'), 'Próxima atualização em 1m');
assert.equal(resetText({ resetAt: '2026-09-24T12:00:00Z' }, 'countdown', Date.parse('2026-09-24T11:00:00Z'), { locale: 'pt-BR' }), 'Redefine em 1h 0m');
assert.match(formatResetExact(Date.parse('2026-09-24T12:00:00Z'), Date.parse('2026-09-24T11:00:00Z'), { locale: 'pt-BR', timeZone: 'UTC', timeFormat: '24' }), /^hoje às 12:00$/);

// --- resetTimes layout field ------------------------------------

{
  const empty = emptyLayout();
  assert.equal(empty.resetTimes, 'countdown');
}

{
  const exact = normalizeLayout({ resetTimes: 'exact' });
  const junk = normalizeLayout({ resetTimes: 'never' });
  const missing = normalizeLayout({});
  assert.equal(exact.resetTimes, 'exact');
  assert.equal(junk.resetTimes, 'countdown');
  assert.equal(missing.resetTimes, 'countdown');
}

{
  const store = memoryStorage();
  saveLayout(store, { cardOrder: ['cursor'], resetTimes: 'exact' });
  const reloaded = loadLayout(store);
  const resynced = syncLayout(reloaded, ['anthropic', 'cursor']);
  assert.equal(reloaded.resetTimes, 'exact');
  assert.equal(resynced.resetTimes, 'exact');
  assert.deepEqual(resynced.cardOrder, ['cursor', 'anthropic']);
}

{
  const cleaned = syncLayout({ cardOrder: [], resetTimes: 'maybe' }, ['anthropic']);
  assert.equal(cleaned.resetTimes, 'countdown');
}

// --- usageColor / normalizeColorThresholds ------------------------------------

// Defaults match the Claude Code statusline: green, yellow from 70%, red from 85%.
assert.deepEqual(normalizeColorThresholds(undefined), { yellow: 70, red: 85 });
assert.equal(usageColor(0), 'green');
assert.equal(usageColor(69), 'green');
assert.equal(usageColor(70), 'yellow');
assert.equal(usageColor(84), 'yellow');
assert.equal(usageColor(85), 'red');
assert.equal(usageColor(10, undefined, true), 'red');
assert.equal(usageColor(55, { yellow: 50, red: 60 }), 'yellow');
assert.equal(usageColor(60, { yellow: 50, red: 60 }), 'red');
// Unusable input falls back per field; red always stays above yellow.
assert.deepEqual(normalizeColorThresholds({ yellow: 'x', red: 150 }), { yellow: 70, red: 85 });
assert.deepEqual(normalizeColorThresholds({ yellow: 90, red: 80 }), { yellow: 90, red: 91 });
assert.deepEqual(normalizeColorThresholds({ yellow: 100, red: 100 }), { yellow: 99, red: 100 });
assert.deepEqual(normalizeLayout({ colorThresholds: { yellow: 60 } }).colorThresholds, { yellow: 60, red: 85 });
// A settings edit commits the typed strings normalized, and only when they change something.
assert.deepEqual(commitColorThresholds({ yellow: '60', red: '80' }, { yellow: 70, red: 85 }), { next: { yellow: 60, red: 80 }, changed: true });
assert.deepEqual(commitColorThresholds({ yellow: '70', red: '85' }, { yellow: 70, red: 85 }), { next: { yellow: 70, red: 85 }, changed: false });
// Red typed at or below yellow is kept above it, which the inputs' bounds announce.
assert.deepEqual(commitColorThresholds({ yellow: '70', red: '60' }, { yellow: 70, red: 85 }), { next: { yellow: 70, red: 71 }, changed: true });
assert.deepEqual(commitColorThresholds({ yellow: '', red: 'x' }, { yellow: 50, red: 60 }), { next: { yellow: 70, red: 85 }, changed: true });

// --- resetText / resetAlternate / formatResetExact ---------------------------

const resetNow = Date.parse('2026-09-04T12:00:00Z');
const utc = { timeZone: 'UTC' };
const sameDayRow = { kind: 'metric', resetAt: '2026-09-04T18:38:00Z', reset: 'Resets in 6h 38m' };
const nextDayRow = { kind: 'metric', resetAt: '2026-09-05T18:38:00Z', reset: 'Resets in 1d 6h' };
const laterRow = { kind: 'metric', resetAt: '2026-09-12T18:38:00Z', reset: 'Resets in 8d 6h' };
const noStampRow = { kind: 'metric', resetAt: '', reset: 'Resets in 1d 16h' };
const badStampRow = { kind: 'metric', resetAt: 'not-a-date', reset: 'Resets in 2h' };

// countdown mode uses the live clock, not the row's cached text
assert.equal(resetText(sameDayRow, 'countdown', resetNow, utc), 'Resets in 6h 38m');
assert.equal(resetText(nextDayRow, 'countdown', resetNow, utc), 'Resets in 1d 6h');

// exact mode: today / tomorrow / other day
assert.equal(resetText(sameDayRow, 'exact', resetNow, utc), 'Resets today at 6:38 PM');
assert.equal(resetText(nextDayRow, 'exact', resetNow, utc), 'Resets tomorrow at 6:38 PM');
assert.equal(resetText(laterRow, 'exact', resetNow, utc), 'Resets Sep 12 at 6:38 PM');

// no usable timestamp falls back to the row text in either mode
assert.equal(resetText(noStampRow, 'countdown', resetNow, utc), 'Resets in 1d 16h');
assert.equal(resetText(noStampRow, 'exact', resetNow, utc), 'Resets in 1d 16h');
assert.equal(resetText(badStampRow, 'exact', resetNow, utc), 'Resets in 2h');
assert.equal(resetText({ kind: 'metric', resetAt: '', reset: '' }, 'exact', resetNow, utc), '');
assert.equal(resetText(null, 'exact', resetNow, utc), '');

// formatResetExact on its own, and the calendar-day comparison honors the zone
assert.equal(formatResetExact(Date.parse('2026-09-04T18:38:00Z'), resetNow, utc), 'today at 6:38 PM');
assert.equal(formatResetExact(Date.parse('2026-09-05T00:05:00Z'), resetNow, utc), 'tomorrow at 12:05 AM');
assert.equal(
  formatResetExact(Date.parse('2026-09-05T00:05:00Z'), resetNow, { timeZone: 'America/Sao_Paulo' }),
  'today at 9:05 PM',
);

// resetAlternate flips the mode; nothing to flip without a timestamp
assert.equal(resetAlternate(sameDayRow, 'countdown', resetNow, utc), 'Resets today at 6:38 PM');
assert.equal(resetAlternate(sameDayRow, 'exact', resetNow, utc), 'Resets in 6h 38m');
assert.equal(resetAlternate(noStampRow, 'countdown', resetNow, utc), '');
assert.equal(resetAlternate(badStampRow, 'exact', resetNow, utc), '');

// --- timeFormat: 12h / 24h override rides through resetText and resetAlternate --

{
  // ARRANGE: en-US would pick a 12-hour clock on its own
  const h24 = { locale: 'en-US', timeZone: 'UTC', timeFormat: '24' };
  const h12 = { locale: 'en-US', timeZone: 'UTC', timeFormat: '12' };
  const auto = { locale: 'en-US', timeZone: 'UTC', timeFormat: 'auto' };
  const at = Date.parse('2026-09-04T18:38:00Z');
  // ACT / ASSERT: the override wins in both directions, "auto" defers to the locale
  assert.equal(formatResetExact(at, resetNow, h24), 'today at 18:38');
  assert.equal(formatResetExact(at, resetNow, h12), 'today at 6:38 PM');
  assert.equal(formatResetExact(at, resetNow, auto), 'today at 6:38 PM');
  assert.equal(formatResetExact(Date.parse('2026-09-05T00:05:00Z'), resetNow, h24), 'tomorrow at 00:05');
  assert.equal(resetText(sameDayRow, 'exact', resetNow, h24), 'Resets today at 18:38');
  assert.equal(resetText(sameDayRow, 'exact', resetNow, h12), 'Resets today at 6:38 PM');
  assert.equal(resetAlternate(sameDayRow, 'countdown', resetNow, h24), 'Resets today at 18:38');
  assert.equal(resetAlternate(sameDayRow, 'exact', resetNow, h24), 'Resets in 6h 38m');
}

// --- layout: timeFormat / alwaysShowPace / usageGoal --------------------------

{
  // ARRANGE / ACT
  const empty = emptyLayout();
  const set = normalizeLayout({ timeFormat: '24', alwaysShowPace: true, usageGoal: true });
  const junk = normalizeLayout({ timeFormat: 'military', alwaysShowPace: 'yes', usageGoal: 'yes' });
  // ASSERT: defaults, valid values, and junk
  assert.equal(empty.timeFormat, 'auto');
  assert.equal(empty.alwaysShowPace, false);
  assert.equal(empty.usageGoal, false);
  assert.equal(set.timeFormat, '24');
  assert.equal(set.alwaysShowPace, true);
  assert.equal(set.usageGoal, true);
  assert.equal(junk.timeFormat, 'auto');
  assert.equal(junk.alwaysShowPace, false);
  assert.equal(junk.usageGoal, false);
  assert.equal(normalizeLayout({ timeFormat: '12' }).timeFormat, '12');

  // ASSERT: both survive storage and syncLayout
  const store = memoryStorage();
  saveLayout(store, { cardOrder: ['cursor'], timeFormat: '12', alwaysShowPace: true, usageGoal: true });
  const reloaded = loadLayout(store);
  assert.equal(reloaded.timeFormat, '12');
  assert.equal(reloaded.alwaysShowPace, true);
  assert.equal(reloaded.usageGoal, true);
  const synced = syncLayout(reloaded, ['cursor']);
  assert.equal(synced.timeFormat, '12');
  assert.equal(synced.alwaysShowPace, true);
  assert.equal(synced.usageGoal, true);
  const cleaned = syncLayout({ cardOrder: [], timeFormat: 'nope', alwaysShowPace: 1, usageGoal: 1 }, ['cursor']);
  assert.equal(cleaned.timeFormat, 'auto');
  assert.equal(cleaned.alwaysShowPace, false);
  assert.equal(cleaned.usageGoal, false);
}

// The goal follows wall-clock progress even when no usage has been reported.
{
  const end = Date.parse('2026-09-24T15:00:00Z');
  for (const seconds of [18_000, 604_800, 2_592_000]) {
    const row = { label: 'Session', resetAt: new Date(end).toISOString(), window: seconds, usedPercent: 0 };
    assert.deepEqual(usageGoal(row, end - seconds * 1000), { percent: 0, estimated: false });
    assert.deepEqual(usageGoal(row, end - seconds * 500), { percent: 50, estimated: false });
    assert.deepEqual(usageGoal(row, end), { percent: 100, estimated: false });
    assert.equal(usageGoal(row, end + 60_000), null);
  }
  const monthly = { label: 'Monthly', resetAt: '2026-03-31T12:00:00Z', window: 0 };
  assert.deepEqual(usageGoal(monthly, Date.parse('2026-02-28T12:00:00Z')), { percent: 0, estimated: true });
  assert.deepEqual(usageGoal(monthly, Date.parse('2026-03-31T12:00:00Z')), { percent: 100, estimated: true });
  assert.equal(usageGoal({ ...monthly, label: 'Weekly' }, end), null);
  assert.equal(usageGoal({ ...monthly, resetAt: 'bad' }, end), null);
}

// --- host payload: shortcut / retired update keys / window_secs ---------------

{
  // ARRANGE: every host key populated, plus the retired update keys an older host sent
  const full = parseHostPayload({
    version: '1.11.0',
    shortcut: 'Ctrl+Shift+U',
    shortcut_error: 'already taken',
    updates: 'auto',
    update: { state: 'downloading', version: 'v1.12.0', url: 'https://github.com/akitaonrails/ai-usagebar/releases/tag/v1.12.0', error: '' },
    update_checked_at: 1_700_000_000_000,
    entries: [{
      id: 'anthropic',
      display_name: 'Claude',
      sections: [
        { type: 'metric', label: 'Session', percent: 40, severity: 'low', reset_at: '2026-09-04T14:30:00Z', window_secs: 18000 },
        { type: 'metric', label: 'Weekly', percent: 10, severity: 'low', window_secs: -5 },
        { type: 'metric', label: 'Extra', percent: 5, severity: 'low', window_secs: 'lots' },
      ],
    }],
  });
  // ASSERT: the camelCase fields
  assert.equal(full.shortcut, 'Ctrl+Shift+U');
  assert.equal(full.shortcutError, 'already taken');
  for (const retired of ['updates', 'update', 'updateCheckedAt', 'repository']) {
    assert.equal(retired in full, false, retired);
  }
  assert.equal(full.entries[0].sections[0].window, 18000);
  assert.equal(full.entries[0].sections[1].window, 0);
  assert.equal(full.entries[0].sections[2].window, 0);
  // the window rides onto the projected row
  const rows = projectCards(full, 0)[0].rows;
  assert.equal(rows[0].window, 18000);
  assert.equal(rows[1].window, 0);

  // ASSERT: defensive fallbacks
  assert.equal(parseHostPayload({ shortcut: 's'.repeat(80) }).shortcut.length, 64);

  // ASSERT: emptyPayload carries the defaults
  const empty = emptyPayload();
  assert.equal(empty.shortcut, '');
  assert.equal(empty.shortcutError, '');
  assert.equal('update' in empty, false);

  // ASSERT: refresh_minutes keeps only the offered intervals, else the 5-minute default
  assert.equal(parseHostPayload({ refresh_minutes: 10 }).refreshMinutes, 10);
  assert.equal(parseHostPayload({ refresh_minutes: 1 }).refreshMinutes, 1);
  assert.equal(parseHostPayload({ refresh_minutes: 7 }).refreshMinutes, 5);
  assert.equal(parseHostPayload({ refresh_minutes: '10' }).refreshMinutes, 10);
  assert.equal(parseHostPayload({ refresh_minutes: 'often' }).refreshMinutes, 5);
  assert.equal(parseHostPayload({}).refreshMinutes, 5);
  assert.equal(empty.refreshMinutes, 5);
}

// --- pace / paceText / paceVisible --------------------------------------------

{
  // ARRANGE: a 5-hour window; `now` is the reference clock
  const window = 5 * 3600;
  const now = Date.parse('2026-09-04T12:00:00Z');
  const resetAfter = (ms) => new Date(now + ms).toISOString();
  const row = (usedPercent, elapsedMs, extra) => ({
    kind: 'metric',
    usedPercent,
    leftPercent: 100 - usedPercent,
    window,
    resetAt: resetAfter(window * 1000 - elapsedMs),
    ...extra,
  });

  // ACT / ASSERT: 40% spent at half the window projects 80% → ahead, 20% spare
  const ahead = pace(row(40, 2.5 * 3600_000), now);
  assert.equal(ahead.state, 'ahead');
  assert.equal(Math.round(ahead.projectedPercent), 80);
  assert.equal(ahead.sparePercent, 20);
  assert.equal(ahead.elapsedPercent, 50);
  assert.equal(ahead.runsOutMs, null);
  assert.equal(paceText(ahead, now), '~20% left at reset');
  assert.equal(paceVisible(ahead, { alwaysShowPace: false }), false);
  assert.equal(paceVisible(ahead, { alwaysShowPace: true }), true);
  assert.equal(paceVisible(ahead, emptyLayout()), false);

  // 45% at half the window is exactly the 90% boundary → still ahead
  assert.equal(pace(row(45, 2.5 * 3600_000), now).state, 'ahead');

  // 50% spent at half the window projects 100% → onTrack, nothing spare
  const close = pace(row(50, 2.5 * 3600_000), now);
  assert.equal(close.state, 'onTrack');
  assert.equal(close.sparePercent, 0);
  assert.equal(close.runsOutMs, null);
  assert.equal(paceText(close, now), '~0% spare');
  assert.equal(paceVisible(close, { alwaysShowPace: false }), true);

  // 48.5% at half the window projects 97% → onTrack with 3% spare
  const nearly = pace(row(48.5, 2.5 * 3600_000), now);
  assert.equal(nearly.state, 'onTrack');
  assert.equal(paceText(nearly, now), '~3% spare');
  // a negative spare never shows as "-N% spare"
  assert.equal(paceText({ state: 'onTrack', sparePercent: -1, projectedPercent: 101, runsOutMs: null, elapsedPercent: 50 }, now), '~0% spare');
  assert.equal(paceText({ state: 'ahead', sparePercent: 12, projectedPercent: 88, runsOutMs: null, elapsedPercent: 50 }, now), '~12% left at reset');

  // 80% spent after 2h projects 200% → behind, runs out at 2h30 (30m from now), before the reset
  const behind = pace(row(80, 2 * 3600_000), now);
  assert.equal(behind.state, 'behind');
  assert.equal(Math.round(behind.projectedPercent), 200);
  assert.equal(behind.sparePercent, -100);
  assert.equal(behind.elapsedPercent, 40);
  assert.ok(behind.runsOutMs > now);
  assert.ok(behind.runsOutMs < Date.parse(row(80, 2 * 3600_000).resetAt));
  assert.equal(Math.round((behind.runsOutMs - now) / 60_000), 30);
  assert.equal(paceText(behind, now), 'Limit in 30m');
  assert.equal(paceText(behind, now, { resetTimes: 'countdown' }), 'Limit in 30m');
  assert.equal(paceVisible(behind, { alwaysShowPace: false }), true);

  // exact reset times turn the countdown into a clock time (formatResetExact's wording)
  const exact = paceText(behind, now, { resetTimes: 'exact', timeZone: 'UTC' });
  assert.ok(exact.startsWith('Limit today at ') || exact.startsWith('Limit tomorrow at '), exact);
  assert.equal(exact, 'Limit ' + formatResetExact(behind.runsOutMs, now, { timeZone: 'UTC' }));
  // timeFormat flows through
  assert.equal(
    paceText(behind, now, { resetTimes: 'exact', timeZone: 'UTC', timeFormat: '24' }),
    'Limit today at 12:30',
  );
  assert.equal(
    paceText(behind, now, { resetTimes: 'exact', timeZone: 'UTC', timeFormat: '12' }),
    'Limit today at 12:30 PM',
  );

  // behind without a run-out instant (the limit is already spent) has no text: the flame alone
  const spent = pace(row(100, 2 * 3600_000), now);
  assert.equal(spent.state, 'behind');
  assert.equal(spent.runsOutMs, null);
  assert.equal(paceText(spent, now), '');
  assert.equal(paceText(spent, now, { resetTimes: 'exact', timeZone: 'UTC' }), '');

  // ASSERT: no signal → null
  assert.equal(pace(row(50, 30_000), now), null); // 30s in: under the 1% / 60s floor
  assert.equal(pace(row(50, 179_000), now), null); // 1% of 5h is 3m
  assert.notEqual(pace(row(50, 180_000), now), null);
  assert.equal(pace(row(50, 2 * 3600_000, { window: 0 }), now), null);
  assert.equal(pace({ kind: 'metric', usedPercent: 50, leftPercent: 50, resetAt: resetAfter(3600_000) }, now), null);
  assert.equal(pace(row(50, 2 * 3600_000, { resetAt: 'not-a-date' }), now), null);
  assert.equal(pace(row(50, 2 * 3600_000, { resetAt: '' }), now), null);
  assert.equal(pace({ ...row(50, 2 * 3600_000), resetAt: resetAfter(-1) }, now), null); // already reset
  assert.equal(pace(row(0, 2 * 3600_000), now), null); // nothing spent: no rate to project
  assert.equal(pace(null, now), null);
  assert.equal(pace({ kind: 'text', label: 'Balance', value: '$1' }, now), null);
  assert.equal(paceText(null, now), '');
  assert.equal(paceVisible(null, { alwaysShowPace: true }), false);

  // ASSERT: a projected card row carries enough for pace() straight from the host payload
  const hosted = parseHostPayload({
    entries: [{ id: 'anthropic', display_name: 'Claude', sections: [
      { type: 'metric', label: 'Session', percent: 40, severity: 'low', reset_at: resetAfter(2.5 * 3600_000), window_secs: window },
    ] }],
  });
  assert.equal(pace(projectCards(hosted, now)[0].rows[0], now).state, 'ahead');
}

// --- shortcutFromKeyEvent ----------------------------------------------------

{
  const press = (code, mods, key) => ({
    key: key || '',
    code,
    ctrlKey: !!(mods && mods.ctrl),
    altKey: !!(mods && mods.alt),
    shiftKey: !!(mods && mods.shift),
    metaKey: !!(mods && mods.meta),
  });
  assert.equal(shortcutFromKeyEvent(press('KeyU', { ctrl: true, shift: true }, 'U')), 'Ctrl+Shift+U');
  assert.equal(shortcutFromKeyEvent(press('F5', { alt: true }, 'F5')), 'Alt+F5');
  assert.equal(shortcutFromKeyEvent(press('Space', { meta: true }, ' ')), 'Win+Space');
  assert.equal(shortcutFromKeyEvent(press('Digit1', { ctrl: true, alt: true }, '1')), 'Ctrl+Alt+1');
  assert.equal(shortcutFromKeyEvent(press('F24', { ctrl: true }, 'F24')), 'Ctrl+F24');
  assert.equal(shortcutFromKeyEvent(press('ArrowUp', { ctrl: true }, 'ArrowUp')), 'Ctrl+Up');
  assert.equal(shortcutFromKeyEvent(press('ArrowLeft', { ctrl: true, meta: true }, 'ArrowLeft')), 'Ctrl+Win+Left');
  assert.equal(shortcutFromKeyEvent(press('Backslash', { ctrl: true }, '\\')), 'Ctrl+\\');
  assert.equal(shortcutFromKeyEvent(press('Backquote', { alt: true }, '`')), 'Alt+`');
  assert.equal(shortcutFromKeyEvent(press('BracketLeft', { ctrl: true }, '[')), 'Ctrl+[');
  assert.equal(shortcutFromKeyEvent(press('Quote', { ctrl: true }, "'")), "Ctrl+'");
  assert.equal(shortcutFromKeyEvent(press('PageDown', { ctrl: true }, 'PageDown')), 'Ctrl+PageDown');
  // full modifier order: Ctrl, Alt, Shift, Win
  assert.equal(
    shortcutFromKeyEvent(press('KeyA', { ctrl: true, alt: true, shift: true, meta: true }, 'A')),
    'Ctrl+Alt+Shift+Win+A',
  );
  // no Ctrl/Alt/Win → null, even with Shift
  assert.equal(shortcutFromKeyEvent(press('KeyU', { shift: true }, 'U')), null);
  assert.equal(shortcutFromKeyEvent(press('KeyU', {}, 'u')), null);
  // modifier-only presses and Escape → null
  assert.equal(shortcutFromKeyEvent(press('ShiftLeft', { shift: true }, 'Shift')), null);
  assert.equal(shortcutFromKeyEvent(press('ControlLeft', { ctrl: true }, 'Control')), null);
  assert.equal(shortcutFromKeyEvent(press('AltRight', { alt: true }, 'Alt')), null);
  assert.equal(shortcutFromKeyEvent(press('MetaLeft', { meta: true }, 'Meta')), null);
  assert.equal(shortcutFromKeyEvent(press('Escape', { ctrl: true }, 'Escape')), null);
  // unknown codes → null
  assert.equal(shortcutFromKeyEvent(press('F25', { ctrl: true }, 'F25')), null);
  assert.equal(shortcutFromKeyEvent(press('NumpadAdd', { ctrl: true }, '+')), null);
  assert.equal(shortcutFromKeyEvent(press('CapsLock', { ctrl: true }, 'CapsLock')), null);
  assert.equal(shortcutFromKeyEvent(press('', { ctrl: true }, '')), null);
  assert.equal(shortcutFromKeyEvent(press('toString', { ctrl: true }, '')), null);
  assert.equal(shortcutFromKeyEvent(null), null);
}

// --- SuperGrok labels / displayPlan equality ----------------------------------

{
  // ARRANGE: SuperGrok names the overall meter "<Window> usage" (older
  // reports used "<Window> Build credits"); Grok proper does not.
  const grok = parseHostPayload({
    entries: [
      { id: 'supergrok', display_name: 'SuperGrok', plan: 'SuperGrok', sections: [
        { type: 'metric', label: 'Weekly Build credits', percent: 12, severity: 'low' },
        { type: 'metric', label: 'Monthly Build credits', percent: 3, severity: 'low' },
        { type: 'metric', label: 'Build credits', percent: 3, severity: 'low' },
      ] },
      { id: 'supergrok@work', display_name: 'SuperGrok', plan: 'SuperGrok Heavy', sections: [
        { type: 'metric', label: 'Weekly Build credits', percent: 1, severity: 'low' },
      ] },
      { id: 'grok', display_name: 'Grok', sections: [
        { type: 'metric', label: 'Weekly Build credits', percent: 1, severity: 'low' },
      ] },
    ],
  });
  // ACT
  const [personal, work, plain] = projectCards(grok, 0);
  // ASSERT: only the trailing " Build credits" goes, only for supergrok
  assert.deepEqual(personal.rows.map((r) => r.label), ['Weekly', 'Monthly', 'Build credits']);
  assert.deepEqual(personal.rows.map(rowKey), ['metric:Weekly', 'metric:Monthly', 'metric:Build credits']);
  assert.deepEqual(work.rows.map((r) => r.label), ['Weekly']);
  assert.deepEqual(plain.rows.map((r) => r.label), ['Weekly Build credits']);

  const usageNamed = parseHostPayload({
    entries: [
      { id: 'supergrok', display_name: 'SuperGrok', plan: 'SuperGrok', sections: [
        { type: 'metric', label: 'Weekly usage', percent: 90, severity: 'critical' },
        { type: 'metric', label: 'Grok Build', percent: 87, severity: 'high' },
        { type: 'metric', label: 'Grok Chat', percent: 3, severity: 'low' },
      ] },
    ],
  });
  const [usageCard] = projectCards(usageNamed, 0);
  assert.deepEqual(usageCard.rows.map((r) => r.label), ['Weekly', 'Grok Build', 'Grok Chat']);

  // ASSERT: a plan equal to the title vanishes; a prefixed plan keeps its tail
  assert.equal(displayPlan(personal.title, personal.plan), '');
  assert.equal(displayPlan(work.title, work.plan), 'Heavy');
  assert.equal(displayPlan('SuperGrok', 'supergrok'), '');
  assert.equal(displayPlan('Claude', 'Claude'), '');
  assert.equal(displayPlan('Claude', 'Claude Max 5x'), 'Max 5x');
  assert.equal(displayPlan('', 'Plus'), 'Plus');
}

// --- quotaAlternate ----------------------------------------------------------

assert.equal(quotaAlternate({ kind: 'metric', leftPercent: 81, usedPercent: 19 }, 'left'), '19% used');
assert.equal(quotaAlternate({ kind: 'metric', leftPercent: 81, usedPercent: 19 }, 'used'), '81% left');
assert.equal(quotaAlternate({ kind: 'metric', leftPercent: 0, usedPercent: 100 }, 'left'), '');
assert.equal(quotaAlternate({ kind: 'metric', leftPercent: 0, usedPercent: 100 }, 'used'), '');
assert.equal(quotaAlternate({ kind: 'text', label: 'Balance', value: '$1' }, 'left'), '');
assert.equal(quotaAlternate(null, 'left'), '');

// --- first launch: lacksCredentials / seedLayout / hintPending ---------------

{
  // ARRANGE: two providers with keys, two without, one with an expired login
  const keyed = { id: 'anthropic', error: '' };
  const expired = { id: 'openai', error: 'HTTP 401: authentication rejected' };
  const zai = { id: 'zai', error: 'no API key configured for zai' };
  const openrouter = { id: 'openrouter', error: 'No API key' };
  const entries = [keyed, expired, zai, openrouter];

  // ACT
  const seeded = seedLayout(emptyLayout(), entries);

  // ASSERT: only the "No API key" providers start hidden, once
  assert.equal(lacksCredentials(zai), true);
  assert.equal(lacksCredentials(expired), false);
  assert.equal(lacksCredentials(keyed), false);
  assert.equal(lacksCredentials(null), false);
  assert.deepEqual(seeded.hidden, { zai: true, openrouter: true });
  assert.equal(seeded.seeded, true);
  assert.equal(hintPending(seeded), true);

  // ACT: the user re-enables Z.AI later; a second payload must not hide it again
  const reenabled = { ...seeded, hidden: { openrouter: true } };
  assert.deepEqual(seedLayout(reenabled, entries).hidden, { openrouter: true });
  assert.strictEqual(seedLayout(reenabled, entries), reenabled);

  // ASSERT: no entries (host error) is not a first launch
  assert.equal(seedLayout(emptyLayout(), []).seeded, false);
  assert.equal(seedLayout(emptyLayout(), null).seeded, false);
  assert.equal(hintPending(emptyLayout()), false);

  // ASSERT: when every provider lacks a credential the starter set stays visible
  const allMissing = seedLayout(emptyLayout(), [zai, openrouter]);
  assert.deepEqual(allMissing.hidden, {});
  assert.equal(allMissing.seeded, true);

  // ASSERT: the flags survive storage and syncLayout; the hint can be dismissed
  const store = memoryStorage();
  saveLayout(store, { ...seeded, hintDismissed: true });
  const loaded = loadLayout(store);
  assert.equal(loaded.seeded, true);
  assert.equal(loaded.hintDismissed, true);
  assert.equal(hintPending(loaded), false);
  const synced = syncLayout(loaded, ['anthropic', 'openai', 'zai', 'openrouter']);
  assert.equal(synced.seeded, true);
  assert.equal(synced.hintDismissed, true);
  assert.deepEqual(synced.hidden, { zai: true, openrouter: true });
  assert.equal(emptyLayout().seeded, false);
  assert.equal(emptyLayout().hintDismissed, false);
  assert.equal(normalizeLayout({ seeded: 'yes', hintDismissed: 1 }).seeded, false);
}

// --- absorbPayload ------------------------------------------------------------

{
  // ARRANGE: a remembered layout with hidden providers and a custom order
  const remembered = {
    ...emptyLayout(),
    cardOrder: ['openai', 'anthropic'],
    hidden: { zai: true },
    seeded: true,
    hintDismissed: true,
  };

  // ACT + ASSERT: the host's empty placeholder payload leaves it untouched
  assert.strictEqual(absorbPayload(remembered, []), remembered);
  assert.strictEqual(absorbPayload(remembered, null), remembered);

  // ACT + ASSERT: a real payload syncs (keeps hidden for known ids, appends new ids)
  const entries = [{ id: 'anthropic', error: '' }, { id: 'openai', error: '' }, { id: 'zai', error: 'no API key' }, { id: 'cursor', error: '' }];
  const synced = absorbPayload(remembered, entries);
  assert.deepEqual(synced.cardOrder, ['openai', 'anthropic', 'zai', 'cursor']);
  assert.deepEqual(synced.hidden, { zai: true });
  assert.equal(synced.seeded, true);

  // ACT + ASSERT: a fresh layout gets seeded by its first real payload
  const fresh = absorbPayload(emptyLayout(), entries);
  assert.deepEqual(fresh.hidden, { zai: true });
  assert.equal(fresh.seeded, true);
  assert.equal(hintPending(fresh), true);
}

// --- grouped metrics (Antigravity Session / Weekly) and duplicate labels -----

{
  // ARRANGE: two groups repeating the same metric names, plus a trailing text row
  const grouped = parseHostPayload({
    entries: [{
      id: 'antigravity',
      display_name: 'Antigravity',
      sections: [
        { type: 'spacer' },
        { type: 'text', label: 'Session', value: '' },
        { type: 'metric', label: 'Gemini', percent: 1, severity: 'low' },
        { type: 'metric', label: 'Claude & GPT OSS', percent: 0, severity: 'low' },
        { type: 'text', label: 'Weekly', value: '' },
        { type: 'metric', label: 'Gemini', percent: 4, severity: 'low' },
        { type: 'metric', label: 'Claude & GPT OSS', percent: 0, severity: 'low' },
        { type: 'text', label: 'Warning', value: 'credentials error: no local server found.' },
      ],
    }],
  });

  // ACT
  const card = projectCards(grouped, 0)[0];

  // ASSERT: headings vanish, metrics carry their group, keys stay unique
  assert.deepEqual(card.rows.map((r) => r.label), [
    'Gemini (Session)', 'Claude & GPT OSS (Session)', 'Gemini (Weekly)', 'Claude & GPT OSS (Weekly)',
  ]);
  // the Warning section is the card's warning, not a row
  assert.equal(card.rows.filter((r) => r.kind === 'text').length, 0);
  assert.equal(card.warning.title, "Antigravity isn't running");
  assert.equal(new Set(card.rows.map(rowKey)).size, card.rows.length);
  assert.equal(visibleRowsFor(card, { collapsed: false }).length, 4);
  assert.equal(metricCount(card), 4);

  // ASSERT: a vendor repeating a label outright still yields distinct rows
  const repeated = parseHostPayload({
    entries: [{ id: 'x', display_name: 'X', sections: [
      { type: 'metric', label: 'Quota', percent: 10, severity: 'low' },
      { type: 'metric', label: 'Quota', percent: 20, severity: 'low' },
    ] }],
  });
  const keys = projectCards(repeated, 0)[0].rows.map(rowKey);
  assert.deepEqual(keys, ['metric:Quota', 'metric:Quota #2']);
  assert.equal(visibleRowsFor(projectCards(repeated, 0)[0], { collapsed: false }).length, 2);
}

// --- vendor warnings become card.warning, and errors carry an action ------------

{
  // ARRANGE: cached data plus the Rust "Warning" row, and a Kimi-style warning label
  const warned = parseHostPayload({
    entries: [
      { id: 'antigravity', display_name: 'Antigravity', sections: [
        { type: 'metric', label: 'Gemini', percent: 4, severity: 'low' },
        { type: 'text', label: 'Warning', value: 'credentials error: Antigravity: no local server found. Quota is only served while Antigravity is running.' },
      ] },
      { id: 'kimi', display_name: 'Kimi', sections: [
        { type: 'metric', label: 'Weekly', percent: 10, severity: 'low' },
        { type: 'text', label: 'Kimi API schema drift', value: '' },
      ] },
    ],
  });

  // ACT
  const [agy, kimi] = projectCards(warned, 0);

  // ASSERT: no Warning row; the card carries a translated warning with the cleaned raw text
  assert.deepEqual(agy.rows.map((r) => r.label), ['Gemini']);
  assert.equal(agy.warning.title, "Antigravity isn't running");
  assert.equal(agy.warning.hint, 'Open the Antigravity app, then Refresh.');
  assert.ok(!agy.warning.raw.startsWith('credentials error'));
  assert.ok(agy.warning.raw.includes('no local server found'));
  assert.deepEqual(kimi.rows.map((r) => r.label), ['Weekly']);
  assert.equal(kimi.warning.title, "Couldn't update");
  assert.equal(cards[0].warning, null);

  // ASSERT: actions by error class
  assert.equal(explainError('no API key', 'zai').action, undefined);
  assert.deepEqual(explainError('no vendors enabled').action, { cmd: 'detect', label: 'Detect Providers' });
  assert.deepEqual(explainError('HTTP 503: down', 'zai').action, { cmd: 'refresh', label: 'Refresh' });
  assert.deepEqual(explainError('network transport error: connection refused', 'openai').action, { cmd: 'refresh', label: 'Refresh' });
  assert.equal(explainError('HTTP 401: authentication rejected', 'openai').action, undefined);
  assert.equal(explainError('HTTP 429: rate limited; next attempt in 4m', 'zai').action, undefined);
  assert.equal(explainError('Antigravity must be running', 'antigravity').title, "Antigravity isn't running");
}

// --- 429 backoff hint --------------------------------------------------------

assert.equal(
  explainError('HTTP 429: rate limited; next attempt in 4m', 'zai').hint,
  'Retrying automatically in 4m.',
);
assert.equal(
  explainError('HTTP 429: rate limited; next attempt in 1h 2m', 'zai').hint,
  'Retrying automatically in 1h 2m.',
);
assert.equal(explainError('HTTP 429: Rate limited. Please try again later.', 'zai').hint, 'Try Refresh in a minute.');

// --- headlineLabel / headlineAlternate ----------------------------------------

assert.equal(headlineLabel({ kind: 'metric', leftPercent: 81, usedPercent: 19 }, 'left'), '81% left');
assert.equal(headlineLabel({ kind: 'metric', leftPercent: 81, usedPercent: 19 }, 'used'), '19% used');
assert.equal(headlineLabel({ kind: 'metric', leftPercent: 0, usedPercent: 100 }, 'left'), '0% left');
assert.equal(headlineLabel({ kind: 'metric', leftPercent: 0, usedPercent: 100 }, 'used'), '100% used');
assert.equal(headlineLabel({ kind: 'text', label: 'Balance', value: '$1' }, 'left'), '');
assert.equal(headlineAlternate({ kind: 'metric', leftPercent: 0, usedPercent: 100 }, 'left'), '100% used');
assert.equal(headlineAlternate({ kind: 'metric', leftPercent: 81, usedPercent: 19 }, 'used'), '81% left');
assert.equal(headlineAlternate(null, 'left'), '');

// A metric that names `value` as its headline draws the money figure, like the
// Omarchy bar; the percentage and the report's detail move to the hover text.
// `percent` keeps the used/left toggle, and an older report without the field
// is a percentage.
const tank = (headline) => projectCards(parseHostPayload({
  version: '1.21.0',
  entries: [{
    id: 'deepseek',
    display_name: 'DeepSeek',
    sections: [{
      type: 'metric',
      label: 'Balance',
      percent: 40,
      value: headline === 'value' ? '$12.00' : '40%',
      detail: headline === 'value' ? '40% of $20.00 used ($12.00 left)' : '$12.00 of $20.00 left (40% used)',
      severity: 'low',
      ...(headline ? { headline } : {}),
    }],
  }],
}), 0)[0].rows[0];
const amountRow = tank('value');
assert.equal(amountRow.headline, 'value');
assert.equal(headlineLabel(amountRow, 'left'), '$12.00');
assert.equal(headlineLabel(amountRow, 'used'), '$12.00');
assert.equal(headlineAlternate(amountRow, 'used'), '40% used · 40% of $20.00 used ($12.00 left)');
assert.equal(headlineAlternate(amountRow, 'left'), '60% left · 40% of $20.00 used ($12.00 left)');
const percentRow = tank('percent');
assert.equal(percentRow.headline, 'percent');
assert.equal(headlineLabel(percentRow, 'used'), '40% used');
assert.equal(headlineAlternate(percentRow, 'used'), '60% left');
assert.equal(tank(undefined).headline, 'percent');
assert.equal(headlineLabel(tank(undefined), 'left'), '60% left');
// A `value` headline with no value to draw falls back to the percentage
// rather than an empty button.
assert.equal(projectCards(parseHostPayload({
  version: '1.21.0',
  entries: [{ id: 'deepseek', sections: [{ type: 'metric', label: 'Balance', percent: 40, value: '', headline: 'value' }] }],
}), 0)[0].rows[0].headline, 'percent');

// --- condensedTextRowIndexes -------------------------------------------------

assert.deepEqual(
  condensedTextRowIndexes([{ kind: 'metric' }, { kind: 'text' }, { kind: 'text' }]),
  [2],
);
assert.deepEqual(
  condensedTextRowIndexes([{ kind: 'text' }, { kind: 'block' }, { kind: 'metric' }, { kind: 'text' }]),
  [1],
);
assert.deepEqual(condensedTextRowIndexes([{ kind: 'text' }]), []);
assert.deepEqual(condensedTextRowIndexes([{ kind: 'metric' }, { kind: 'metric' }]), []);
assert.deepEqual(condensedTextRowIndexes([]), []);
assert.deepEqual(condensedTextRowIndexes(null), []);
assert.deepEqual(condensedTextRowIndexes('nope'), []);

// --- providerIconId / initialsGlyph ------------------------------------------

assert.equal(providerIconId('anthropic@work'), 'anthropic');
assert.equal(providerIconId('supergrok'), 'grok');
assert.equal(providerIconId('SuperGrok@personal'), 'grok');
assert.equal(providerIconId('grokbot'), 'grokbot');
assert.equal(providerIconId(' OpenAI '), 'openai');
assert.equal(providerIconId(''), '');
assert.equal(providerIconId(undefined), '');

assert.equal(initialsGlyph('Claude'), 'CL');
assert.equal(initialsGlyph('  codex'), 'CO');
assert.equal(initialsGlyph('x'), 'X');
assert.equal(initialsGlyph(''), '?');
assert.equal(initialsGlyph('   '), '?');
assert.equal(initialsGlyph(undefined), '?');

// --- metricCount -------------------------------------------------------------

assert.equal(metricCount(cursorCard), 3);
assert.equal(metricCount(cards[0]), 1);
assert.equal(metricCount({ rows: [] }), 0);
assert.equal(metricCount({}), 0);
assert.equal(metricCount(null), 0);

// --- sendCommand merges extra fields -----------------------------------------

{
  // ARRANGE: capture what the host would receive
  const sent = [];
  globalThis.window = { ipc: { postMessage(msg) { sent.push(msg); } } };
  try {
    // ACT
    sendCommand('resize', { height: 512, theme: 'dark' });
    sendCommand('refresh');
    sendCommand('quit', ['not', 'an', 'object']);
    sendCommand('open', { cmd: 'hijack', tab: 'settings' });
    // ASSERT: cmd comes first, extras follow, non-objects are ignored, cmd can't be overridden
    assert.deepEqual(sent.map((msg) => JSON.parse(msg)), [
      { cmd: 'resize', height: 512, theme: 'dark' },
      { cmd: 'refresh' },
      { cmd: 'quit' },
      { cmd: 'open', tab: 'settings' },
    ]);
    assert.equal(sent[0], '{"cmd":"resize","height":512,"theme":"dark"}');
  } finally {
    delete globalThis.window;
  }
}

{
  // ARRANGE: no host bridge at all
  // ACT / ASSERT: sending is a no-op rather than a throw
  assert.doesNotThrow(() => sendCommand('refresh', { x: 1 }));
}

// --- resolvedTheme --------------------------------------------

assert.equal(resolvedTheme('dark'), 'dark');
assert.equal(resolvedTheme('light'), 'light');
{
  // ARRANGE: an OS that prefers dark
  globalThis.window = { matchMedia: () => ({ matches: true }) };
  try {
    // ACT / ASSERT
    assert.equal(resolvedTheme('system'), 'dark');
    assert.equal(resolvedTheme('bogus'), 'dark');
  } finally {
    delete globalThis.window;
  }
}
{
  // ARRANGE: an OS that prefers light
  globalThis.window = { matchMedia: () => ({ matches: false }) };
  try {
    assert.equal(resolvedTheme('system'), 'light');
  } finally {
    delete globalThis.window;
  }
}
// without a window, "system" resolves to light
assert.equal(resolvedTheme('system'), 'light');

{
  const layout = emptyLayout();
  assert.equal(layout.stripStyle, 'bars');
  assert.deepEqual(layout.stars, {});
  const restored = normalizeLayout({
    stripStyle: 'text',
    stars: { anthropic: ['metric:Weekly', 'metric:Session', 'metric:Extra'] },
  });
  assert.equal(restored.stripStyle, 'bars');
  assert.deepEqual(restored.stars.anthropic, ['metric:Weekly', 'metric:Session']);
  assert.equal(MAX_STARS_PER_PROVIDER, 2);
}

{
  const payload = parseHostPayload({
    entries: [{
      id: 'anthropic',
      display_name: 'Claude',
      sections: [
        { type: 'metric', label: 'Weekly', percent: 19 },
        { type: 'metric', label: 'Session', percent: 41 },
        { type: 'text', label: 'Extra', value: '$1' },
      ],
    }],
  });
  const cards = projectCards(payload, 0);
  const stars = defaultStars(cards);
  assert.deepEqual(stars.anthropic, ['metric:Weekly', 'metric:Session']);
  assert.equal(isStarred(stars, 'anthropic', 'metric:Weekly'), true);
  const added = toggleStar(stars, 'anthropic', 'metric:Weekly');
  assert.equal(isStarred(added.stars, 'anthropic', 'metric:Weekly'), false);
  assert.equal(added.error, '');
  const capped = toggleStar(stars, 'anthropic', 'text:Extra');
  assert.equal(capped.error, 'Up to 2 stars per provider');
  assert.deepEqual(capped.stars, stars);
  const seeded = seedStars(emptyLayout(), cards);
  assert.deepEqual(seeded.stars, stars);
  const kept = seedStars(seeded, cards);
  assert.equal(kept, seeded);
  assert.deepEqual(stripCommand(seeded, cards), {
    style: 'bars',
    stars,
    order: ['anthropic'],
    names: {},
    language: 'en',
    thresholds: { yellow: 70, red: 85 },
  });
}

{
  const payload = parseHostPayload({
    entries: [
      { id: 'anthropic', display_name: 'Claude', sections: [{ type: 'metric', label: 'Weekly', percent: 10 }] },
      { id: 'openai', display_name: 'Codex', sections: [{ type: 'metric', label: 'Codex weekly', percent: 2 }] },
    ],
  });
  const cards = projectCards(payload, 0);
  const layout = {
    ...emptyLayout(),
    cardOrder: ['openai', 'anthropic'],
    stars: {
      anthropic: ['metric:Weekly'],
      openai: ['metric:Codex weekly'],
    },
  };
  assert.deepEqual(stripCommand(layout, cards).order, ['openai', 'anthropic']);
  // Only visible cards' custom titles go to the host, cleaned like the layout's.
  const named = { ...layout, names: { openai: 'Work\u202e Codex', gone: 'Hidden' } };
  assert.deepEqual(stripCommand(named, cards).names, { openai: 'Work Codex' });
  // The host builds its native provider menus in the popover's language.
  assert.equal(stripCommand({ ...layout, language: 'pt-BR' }, cards).language, 'pt-BR');
}

{
  // Per-provider menu-bar settings: known windows only, `null` for an unset value.
  assert.deepEqual(normalizeMenuBarItems({
    'openai@work': { window: 'session', hide_value: true },
    zai: { hidden: true, window: 'yearly' },
    '': { hidden: true },
    kimi: 'x',
  }), {
    'openai@work': { window: 'session', hideValue: true, hidden: false, colorValue: null },
    zai: { window: 'auto', hideValue: null, hidden: true, colorValue: null },
  });
  assert.deepEqual(normalizeMenuBarItems(null), {});
  const parsed = parseHostPayload({ menu_bar_items: { zai: { hidden: true } }, menu_bar_active_account_only: true });
  assert.equal(parsed.menuBarItems.zai.hidden, true);
  assert.equal(parsed.menuBarActiveAccountOnly, true);
  assert.equal(parseHostPayload({}).menuBarActiveAccountOnly, false);
  assert.equal(parseHostPayload({}).menuBarColorValue, true);
  assert.equal(parseHostPayload({ menu_bar_color_value: false }).menuBarColorValue, false);
  assert.equal(parseHostPayload({}).menuBarCentered, false);
  assert.equal(parseHostPayload({ menu_bar_centered: true }).menuBarCentered, true);
  assert.equal(normalizeMenuBarItems({ zai: { color_value: false } }).zai.colorValue, false);
}

{
  assert.equal(expirySeverity(1_000 + 8 * 24 * 3600 * 1000, 1_000), 'blue');
  assert.equal(expirySeverity(1_000 + 3 * 24 * 3600 * 1000, 1_000), 'yellow');
  assert.equal(expirySeverity(1_000 + 2 * 3600 * 1000, 1_000), 'red');
  assert.equal(expirySeverity(500, 1_000), 'red');
  // A host that sends `reset_credits` without the text block still gets the
  // row, and it starts On Demand like every non-metric row.
  const payload = parseHostPayload({
    entries: [{
      id: 'openai',
      display_name: 'Codex',
      reset_credits: { available: 2, credits: [{ expires_at: '2026-10-03T23:00:00Z' }] },
      sections: [
        { type: 'metric', label: 'Weekly', percent: 10, reset_at: '2026-10-01T00:00:00Z' },
      ],
    }],
  });
  const [card] = projectCards(payload, 0);
  assert.equal(card.resetCredits.available, 2);
  const resets = card.rows.find((row) => row.kind === 'resetCredits');
  assert.equal(resets.label, 'Rate Limit Resets');
  assert.equal(resets.available, 2);
  assert.equal(resets.credits.length, 1);
  assert.equal(card.rows.filter((row) => row.kind === 'resetCredits').length, 1);
  const prefs = defaultRowPrefs(card.rows);
  assert.ok(prefs.demand.indexOf('resetCredits:Rate Limit Resets') >= 0);
  assert.ok(prefs.always.indexOf('resetCredits:Rate Limit Resets') < 0);
}

{
  assert.equal(isHttpUrl('https://status.anthropic.com/'), true);
  assert.equal(isHttpUrl('javascript:alert(1)'), false);
  const claude = providerLinks('anthropic');
  assert.equal(claude.length, 2);
  assert.equal(claude[0].label, 'Status');
  assert.equal(claude[1].label, 'Dashboard');
  const codex = providerLinks('openai@work');
  assert.equal(codex[0].label, 'Status');
  assert.equal(codex[1].label, 'Dashboard');
  assert.equal(providerLinks('unknown-vendor').length, 0);
  const grok = providerLinks('supergrok');
  assert.equal(grok.length, 1);
  assert.equal(grok[0].label, 'Usage');
  assert.ok(grok[0].url.indexOf('https://grok.com/') === 0);
}

{
  assert.equal(prettyMetricLabel('anthropic', 'Session (5h)'), 'Session');
  assert.equal(prettyMetricLabel('anthropic', 'Weekly (7d)'), 'Weekly');
  assert.equal(prettyMetricLabel('anthropic', 'Fable (7d)'), 'Fable');
  assert.equal(prettyMetricLabel('openai', 'Codex weekly'), 'Weekly');
  assert.equal(prettyMetricLabel('openai', 'Codex 5h'), 'Session');
  assert.equal(prettyMetricLabel('cursor', 'Cursor Models'), 'Cursor Models');
  assert.equal(prettyMetricLabel('antigravity', 'Gemini', 'Session'), 'Gemini (Session)');
  const payload = parseHostPayload({
    entries: [{
      id: 'openai',
      display_name: 'Codex',
      sections: [
        { type: 'metric', label: 'Codex weekly', percent: 28 },
        { type: 'metric', label: 'Codex 5h', percent: 0 },
      ],
    }],
  });
  const [card] = projectCards(payload, 0);
  assert.equal(card.rows[0].label, 'Weekly');
  assert.equal(card.rows[0].key, 'metric:Codex weekly');
  assert.equal(card.rows[1].label, 'Session');
  assert.equal(card.rows[1].key, 'metric:Codex 5h');
}

// Account switch: only the macOS host reports switchable logins, keyed by
// vendor; each named card finds its own label and nothing else does.
{
  const payload = parseHostPayload(JSON.stringify({
    entries: [],
    accounts: {
      anthropic: { active: 'main', labels: ['main', 'work'], target: 'work', switching: false, error: 'no stored credential' },
      openai: { active: '', labels: ['main', 'work'], target: 'work', switching: true, error: '' },
      cursor: { active: 'x', labels: ['x'] },
      grok: 'not an object',
    },
  }));
  assert.deepEqual(Object.keys(payload.accounts).sort(), ['anthropic', 'openai']);

  const active = accountSwitchFor('anthropic@main', payload.accounts);
  assert.equal(active.active, true);
  assert.equal(active.error, '');

  const failed = accountSwitchFor('anthropic@work', payload.accounts);
  assert.equal(failed.active, false);
  assert.equal(failed.error, 'no stored credential');

  const running = accountSwitchFor('openai@work', payload.accounts);
  assert.equal(running.switching, true);
  assert.equal(running.busy, false);
  const waiting = accountSwitchFor('openai@main', payload.accounts);
  assert.equal(waiting.busy, true);
  assert.equal(waiting.switching, false);

  assert.equal(accountSwitchFor('anthropic', payload.accounts), null);
  assert.equal(accountSwitchFor('anthropic@unknown', payload.accounts), null);
  assert.equal(accountSwitchFor('cursor@x', payload.accounts), null);
  assert.equal(accountSwitchFor('openai@work', {}), null);
  assert.deepEqual(parseHostPayload(JSON.stringify({ entries: [] })).accounts, {});
}

// Custom card names: applied over the report's name, kept apart from it, cleared by an empty name.
{
  const cards = [{ id: 'openai@conta2', title: 'Codex · conta2' }, { id: 'zai', title: 'Z.AI' }];
  const names = renameCard({}, 'openai@conta2', '  Codex trabalho\n ');
  assert.deepEqual(names, { 'openai@conta2': 'Codex trabalho' });
  const named = applyCardNames(cards, names);
  assert.equal(named[0].title, 'Codex trabalho');
  assert.equal(named[0].defaultTitle, 'Codex · conta2');
  assert.equal(named[1].title, 'Z.AI');
  assert.equal(named[1].defaultTitle, undefined);
  assert.deepEqual(renameCard(names, 'openai@conta2', '   '), {});
  assert.deepEqual(normalizeLayout({ names: { 'zai': 'GLM', '': 'x', bad: 42 } }).names, { zai: 'GLM', bad: '42' });
  assert.equal(normalizeLayout({}).showPlan, true);
  assert.equal(normalizeLayout({ showPlan: false }).showPlan, false);
}

// Every provider id finds its own mark: the report slug uses a dash where the icon file has an underscore.
assert.equal(providerIconId('opencode-go'), 'opencode_go');
assert.equal(providerIconId('opencode-go@work'), 'opencode_go');

// macOS panel view: the list is the default, and only "tabs" switches it.
{
  assert.equal(normalizeLayout({}).panelView, 'list');
  assert.equal(normalizeLayout({ panelView: 'tabs' }).panelView, 'tabs');
  assert.equal(normalizeLayout({ panelView: 'grid' }).panelView, 'list');
}

// Every card that renders keeps its switch control: as many accounts as there
// are cards, and a label longer than a card id matched through the id's cut.
{
  const many = Array.from({ length: 63 }, (_, i) => `acct${i}`);
  const long = 'x'.repeat(200);
  const payload = parseHostPayload(JSON.stringify({
    entries: [{ id: `openai@${long}` }],
    accounts: {
      openai: { active: long, labels: [...many, long] },
      anthropic: { active: '', labels: [`${long}-a`, `${long}-b`] },
    },
  }));
  assert.equal(accountSwitchFor('openai@acct62', payload.accounts).label, 'acct62');
  const cardId = payload.entries[0].id;
  assert.notEqual(cardId, `openai@${long}`);
  const control = accountSwitchFor(cardId, payload.accounts);
  assert.equal(control.label, long);
  assert.equal(control.active, true);
  // Two labels that share the cut id are ambiguous, so neither card offers one.
  assert.equal(accountSwitchFor(cardId.replace('openai', 'anthropic'), payload.accounts), null);
}

console.log('ok');
