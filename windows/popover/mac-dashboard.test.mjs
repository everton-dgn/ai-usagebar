import assert from 'node:assert/strict';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

process.env.TZ = 'UTC';
const server = await createServer({ server: { middlewareMode: true, hmr: false, ws: false }, appType: 'custom' });

try {
  const { MacDashboard } = await server.ssrLoadModule('/src/screens/MacDashboard.tsx');
  const { ProviderSection } = await server.ssrLoadModule('/src/components/ProviderSection.tsx');
  const { Settings } = await server.ssrLoadModule('/src/screens/Settings.tsx');
  const { TooltipProvider } = await server.ssrLoadModule('/src/components/ui/tooltip.tsx');
  const { LanguageProvider } = await server.ssrLoadModule('/src/lib/i18n.tsx');
  const { emptyLayout, emptyPayload } = await server.ssrLoadModule('/src/model.js');
  const nowMs = Date.parse('2026-09-24T11:00:00Z');
  const card = {
    id: 'anthropic', title: 'Claude', plan: '', stale: false, error: '', rows: [{
      kind: 'metric', key: 'session', label: 'Session', headline: 'percent',
      usedPercent: 46, leftPercent: 54, resetAt: '2026-09-24T12:00:00Z',
      reset: '', detail: '', severity: 'low', value: '46%', window: 18_000,
    }],
  };
  const payload = {
    entries: [{ id: 'anthropic', shortName: 'cld', status: 'ready' }],
    primary: 'anthropic', generatedAt: nowMs, nextRefreshAt: nowMs + 60_000,
    hostError: '', version: 'test',
  };

  function note(resetTimes) {
    const html = renderToStaticMarkup(
      React.createElement(LanguageProvider, { language: 'pt-BR' },
        React.createElement(MacDashboard, {
          cards: [card], layout: { resetTimes, timeFormat: '24' }, nowMs, payload,
          onOpenCustomize() {}, onOpenSettings() {},
        })),
    );
    return html.match(/<div class="mac-metric-note"><span>([^<]+)<\/span>/)?.[1];
  }

  assert.equal(note('exact'), 'Redefine hoje às 12:00');
  // Tabs name the provider in full and draw its icon: never a short code or initials.
  const named = renderToStaticMarkup(React.createElement(LanguageProvider, { language: 'pt-BR' },
    React.createElement(MacDashboard, {
      cards: [{ ...card, id: 'anthropic@principal', title: 'Claude · principal' }], layout: emptyLayout(), nowMs,
      payload: { ...payload, entries: [{ id: 'anthropic@principal', shortName: 'cld', status: 'ready' }], primary: 'anthropic@principal' },
      onOpenCustomize() {},
    })));
  assert.match(named, /<span class="mac-tab-name">Claude · principal<\/span>/);
  assert.doesNotMatch(named, />cld</);
  assert.doesNotMatch(named, />CL</);
  assert.equal(note('countdown'), 'Redefine em 1h 0m');
  const withGoal = renderToStaticMarkup(React.createElement(LanguageProvider, { language: 'pt-BR' },
    React.createElement(MacDashboard, {
      cards: [card], layout: { ...emptyLayout(), usageGoal: true }, nowMs, payload,
      onOpenCustomize() {}, onOpenSettings() {},
    })));
  assert.match(withGoal, /Meta agora<\/span><strong>80%<\/strong>/);
  assert.match(withGoal, /class="mac-goal-meter" role="progressbar"/);
  assert.doesNotMatch(renderToStaticMarkup(React.createElement(LanguageProvider, { language: 'pt-BR' },
    React.createElement(MacDashboard, {
      cards: [card], layout: emptyLayout(), nowMs, payload,
      onOpenCustomize() {}, onOpenSettings() {},
    }))), /mac-goal-meter/);
  const settingsPayload = { ...emptyPayload(''), os: 'macos' };
  const settingsProps = {
    cards: [card], layout: emptyLayout(), nowMs, payload: settingsPayload,
    resetArmed: false,
    onAlwaysShowPace() {}, onUsageGoal() {}, onLanguage() {}, onOpenCustomize() {},
    onOpenProvider() {}, onReorderProviders() {}, onToggleProvider() {},
    onResetCustomization() {}, onResetTimes() {}, onShowAs() {},
    onTheme() {}, onTimeFormat() {}, onPanelView() {}, onShowPlan() {}, onColorThresholds() {}, onTabChange() {},
  };
  function settingsTab(tab) {
    return renderToStaticMarkup(React.createElement(TooltipProvider, {},
      React.createElement(LanguageProvider, { language: 'pt-BR' },
        React.createElement(Settings, { ...settingsProps, tab }))));
  }
  const general = settingsTab('general');
  const panelViewTab = ['general', 'preferences'].map(settingsTab).find((html) => /Visualização do painel/.test(html));
  assert.ok(panelViewTab, 'the Panel View picker is on a macOS settings tab');
  assert.match(general, /role="tablist"/);
  assert.match(general, /Iniciar ao entrar/);
  assert.doesNotMatch(general, /Alertas de limite/);
  assert.doesNotMatch(general, /Redefinir toda a personalização/);
  const providers = settingsTab('providers');
  assert.match(providers, /Redefinir toda a personalização/);
  assert.match(providers, /Claude/);
  assert.doesNotMatch(providers, /Iniciar ao entrar/);
  const alerts = settingsTab('alerts');
  assert.match(alerts, /Alertas de limite/);
  assert.doesNotMatch(alerts, /Iniciar ao entrar/);
  const menu = settingsTab('menu');
  assert.match(menu, /Barra de menus/);
  assert.doesNotMatch(menu, /Exibição do uso/);
  const preferences = settingsTab('preferences');
  assert.match(preferences, /Aparência/);
  assert.match(preferences, /Exibição do uso/);
  assert.match(preferences, /Meta de uso/);
  assert.doesNotMatch(preferences, /Barra de menus/);
  // Both views color the bar by what is used, with the thresholds from the layout.
  const at = (used) => ({ ...card, rows: [{ ...card.rows[0], usedPercent: used, leftPercent: 100 - used, value: `${used}%` }] });
  const tabsColor = (used, colorThresholds) => renderToStaticMarkup(React.createElement(LanguageProvider, { language: 'pt-BR' },
    React.createElement(MacDashboard, {
      cards: [at(used)], layout: { ...emptyLayout(), colorThresholds }, nowMs, payload,
      onOpenCustomize() {}, onOpenSettings() {},
    }))).match(/class="mac-meter-fill" data-color="(\w+)"/)?.[1];
  const listColor = (used, colorThresholds) => renderToStaticMarkup(React.createElement(TooltipProvider, {},
    React.createElement(LanguageProvider, { language: 'pt-BR' },
      React.createElement(ProviderSection, { card: at(used), layout: { ...emptyLayout(), colorThresholds }, nowMs })))).match(/class="meter-fill" data-color="(\w+)"/)?.[1];
  // The number beside each bar takes the same color.
  const tabsNumber = (used) => renderToStaticMarkup(React.createElement(LanguageProvider, { language: 'pt-BR' },
    React.createElement(MacDashboard, {
      cards: [at(used)], layout: emptyLayout(), nowMs, payload,
      onOpenCustomize() {}, onOpenSettings() {},
    }))).match(/<strong data-usage="(\w+)">\d+%<\/strong>/)?.[1];
  const listNumber = (used) => renderToStaticMarkup(React.createElement(TooltipProvider, {},
    React.createElement(LanguageProvider, { language: 'pt-BR' },
      React.createElement(ProviderSection, { card: at(used), layout: emptyLayout(), nowMs })))).match(/class="plain-btn[^"]*" data-usage="(\w+)"/)?.[1];
  for (const number of [tabsNumber, listNumber]) {
    assert.equal(number(46), 'green');
    assert.equal(number(75), 'yellow');
    assert.equal(number(90), 'red');
  }
  for (const color of [tabsColor, listColor]) {
    assert.equal(color(46, { yellow: 70, red: 85 }), 'green');
    assert.equal(color(75, { yellow: 70, red: 85 }), 'yellow');
    assert.equal(color(90, { yellow: 70, red: 85 }), 'red');
    assert.equal(color(75, { yellow: 80, red: 95 }), 'green');
  }
  // The threshold inputs carry the red-above-yellow rule as their bounds.
  const colors = renderToStaticMarkup(React.createElement(TooltipProvider, {},
    React.createElement(LanguageProvider, { language: 'pt-BR' },
      React.createElement(Settings, { ...settingsProps, layout: { ...emptyLayout(), colorThresholds: { yellow: 60, red: 80 } }, tab: 'preferences' }))));
  assert.match(colors, /min="1" max="79"[^>]*aria-label="Amarelo a partir de"/);
  assert.match(colors, /min="61" max="100"[^>]*aria-label="Vermelho a partir de"/);
  // The tabs view's card carries the account star: outline to switch, filled on the account in use.
  const accountsPayload = {
    ...payload,
    entries: [{ id: 'anthropic@work', shortName: 'cld', status: 'ready' }, { id: 'anthropic@home', shortName: 'cld', status: 'ready' }],
    primary: 'anthropic@work',
    accounts: { anthropic: { active: 'home', labels: ['work', 'home'], switching: false, error: '', target: '' } },
  };
  const tabsAccount = (selected) => renderToStaticMarkup(React.createElement(LanguageProvider, { language: 'pt-BR' },
    React.createElement(MacDashboard, {
      cards: [{ ...card, id: 'anthropic@work', title: 'Claude' }, { ...card, id: 'anthropic@home', title: 'Claude · 2' }],
      layout: emptyLayout(), nowMs, payload: accountsPayload, focusId: selected,
      onOpenCustomize() {}, onSwitchAccount() {},
    })));
  assert.match(tabsAccount('anthropic@work'), /aria-label="Use Claude \(switches Claude Code and the VS Code extension\)"/);
  assert.match(tabsAccount('anthropic@home'), /aria-label="Claude · 2 is the active account"/);
  console.log('macOS dashboard reset display: ok');
} finally {
  await server.close();
}
