import assert from 'node:assert/strict';
import React from 'react';
import { renderToStaticMarkup } from 'react-dom/server';
import { createServer } from 'vite';

const server = await createServer({ server: { middlewareMode: true, hmr: false, ws: false }, appType: 'custom' });

try {
  const { ShortcutRecorder, shortcutLabel } = await server.ssrLoadModule('/src/components/ShortcutRecorder.tsx');
  const { LanguageProvider } = await server.ssrLoadModule('/src/lib/i18n.tsx');

  // The host persists the Command key as "Win"; only the label changes.
  assert.equal(shortcutLabel('Ctrl+Win+U'), 'Ctrl+Command+U');
  assert.equal(shortcutLabel('Win+Left'), 'Command+Left');
  assert.equal(shortcutLabel('Ctrl+Alt+Shift+W'), 'Ctrl+Alt+Shift+W');
  assert.equal(shortcutLabel(''), '');

  const render = (value, language = 'en') => renderToStaticMarkup(React.createElement(LanguageProvider, { language },
    React.createElement(ShortcutRecorder, { error: '', value, onChange() {} })));

  const shown = render('Ctrl+Shift+Win+U');
  assert.match(shown, />Ctrl\+Shift\+Command\+U</);
  assert.match(shown, /aria-label="Global Shortcut Ctrl\+Shift\+Command\+U"/);
  assert.doesNotMatch(shown, /\bWin\b/);
  assert.match(render('Win+K', 'pt-BR'), />Command\+K</);
  assert.match(render('Ctrl+Alt+U'), />Ctrl\+Alt\+U</);
  console.log('shortcut recorder labels: ok');
} finally {
  await server.close();
}
