/** Shapes produced by `src/model.js`; the JS side references these through JSDoc `@returns`. */

export interface RowPrefs {
  always: string[];
  demand: string[];
  off: Record<string, boolean>;
}

export type TimeFormat = "12" | "24" | "auto";
export type Language = "en" | "pt-BR";

export type PanelView = "list" | "tabs";
export type ProviderView = "overview" | "individual";

export interface ProviderViewLayout {
  rows: Record<string, RowPrefs>;
  collapsed: Record<string, boolean>;
  hideExtras: boolean;
}

/** Percentages used where a usage bar turns yellow and red. */
export interface ColorThresholds {
  yellow: number;
  red: number;
}

export interface Layout extends ProviderViewLayout {
  alwaysShowPace: boolean;
  usageGoal: boolean;
  cardOrder: string[];
  hidden: Record<string, boolean>;
  /** Independent row/link visibility and expansion for menu-bar provider dropdowns. */
  individual: ProviderViewLayout;
  hintDismissed: boolean;
  language: Language;
  /** macOS dashboard: all providers stacked, or one at a time behind tabs. */
  panelView: PanelView;
  colorThresholds: ColorThresholds;
  /** Keep the macOS popover open when focus moves or a click lands outside it. */
  pinned: boolean;
  /** Card id → the name the user gave it. */
  names: Record<string, string>;
  /** Show the subscription plan beside each provider's name. */
  showPlan: boolean;
  resetTimes: string;
  seeded: boolean;
  showAs: string;
  /** Provider id → starred metric keys (max 2). */
  stars: Record<string, string[]>;
  starsSeeded: boolean;
  /** Menu-bar strip: compact bars glyph, or provider+values text. */
  stripStyle: "bars" | "text";
  theme: string;
  timeFormat: TimeFormat;
}

export interface MetricRow {
  /** The report's detail line; the hover text when `headline` is "value". */
  detail: string;
  /** Which number the headline shows: the percentage, or `value`. */
  headline: "percent" | "value";
  key?: string;
  kind: "metric";
  label: string;
  leftPercent: number;
  reset: string;
  resetAt: string;
  severity: string;
  usedPercent: number;
  /** The report's value text; the headline when `headline` is "value" (a money figure). */
  value: string;
  /** Reset window length in seconds; 0 when the host reports none. */
  window: number;
}

export type PaceState = "ahead" | "behind" | "onTrack";

export interface Pace {
  /** How much of the reset window has elapsed, 0..100. */
  elapsedPercent: number;
  projectedPercent: number;
  runsOutMs: number | null;
  sparePercent: number;
  state: PaceState;
}

export interface TextRow {
  key?: string;
  kind: "text";
  label: string;
  value: string;
}

export interface BlockRow {
  body: string[];
  key?: string;
  kind: "block";
  label: string;
}

export interface ResetCredit {
  expiresAt: string;
  title: string;
}

export interface ResetCredits {
  available: number;
  credits: ResetCredit[];
}

export interface ResetCreditsRow extends ResetCredits {
  key?: string;
  kind: "resetCredits";
  label: string;
}

export type Row = BlockRow | MetricRow | ResetCreditsRow | TextRow;

export interface ErrorAction {
  cmd: string;
  label: string;
}

export interface ExplainedError {
  action?: ErrorAction;
  hint: string;
  title: string;
}

export interface CardWarning {
  hint: string;
  raw: string;
  title: string;
}

export interface ResetCredit {
  expiresAt: string;
  title: string;
}

export interface ResetCredits {
  available: number;
  credits: ResetCredit[];
}

export interface Card {
  /** The report's name when the user renamed the card. */
  defaultTitle?: string;
  error: string;
  /** The action `explainError` chose for this entry, kept with its title and hint. */
  errorAction?: ErrorAction;
  errorDetail: string;
  errorHint: string;
  errorTitle: string;
  id: string;
  plan: string;
  resetCredits: ResetCredits | null;
  rows: Row[];
  stale: boolean;
  title: string;
  warning: CardWarning | null;
}

export interface MetricSection {
  detail: string;
  headline: "percent" | "value";
  label: string;
  percent: number;
  resetAt: string;
  severity: string;
  type: "metric";
  value: string;
  /** Reset window length in seconds; 0 when the host reports none. */
  window: number;
}

export interface TextSection {
  label: string;
  type: "text";
  value: string;
}

export interface BlockSection {
  body: string[];
  label: string;
  type: "block";
}

export type Section = BlockSection | MetricSection | TextSection;

export interface Entry {
  email: string;
  displayName: string;
  error: string;
  id: string;
  plan: string;
  resetCredits: ResetCredits | null;
  sections: Section[];
  shortName: string;
  stale: boolean;
  status: string;
}

/** One vendor's switchable logins, as the macOS host reports them. */
export interface AccountSwitchInfo {
  /** Label of the login in use, or "" when it is not a managed account. */
  active: string;
  labels: string[];
  /** Label of the last switch requested, running or finished. */
  target: string;
  switching: boolean;
  /** Why that switch failed, or "". */
  error: string;
}

/** The switch control on one account's card. */
export interface CardAccount {
  vendor: string;
  label: string;
  active: boolean;
  /** A switch to this account is running. */
  switching: boolean;
  /** Another switch for this vendor is running, so this one must wait. */
  busy: boolean;
  /** Why the last switch to this account failed, or "". */
  error: string;
}

export interface Payload {
  /** Switchable logins keyed by vendor slug ("anthropic", "openai"); empty off macOS. */
  accounts: Record<string, AccountSwitchInfo>;
  entries: Entry[];
  generatedAt: number;
  hostError: string;
  menuBarShowAll: boolean;
  menuBarHideValue: boolean;
  menuBarProvider: string;
  menuBarWindow: "auto" | "session" | "weekly" | "monthly";
  menuBarChart: boolean;
  /** Per-provider menu-bar settings, keyed by entry id; absent ids follow the menu bar. */
  menuBarItems: Record<string, MenuBarItem>;
  /** With several accounts of one provider, the menu bar shows only the one in use. */
  menuBarActiveAccountOnly: boolean;
  /** Color menu-bar percentages like the bars; providers may override it. */
  menuBarColorValue: boolean;
  /** Draw the providers in the middle of the menu bar instead of at its right. */
  menuBarCentered: boolean;
  notificationsEnabled: boolean;
  notificationsThreshold: number;
  /** Host OS: macos, or "" before the first payload. */
  os: string;
  nextRefreshAt: number;
  primary: string;
  /** Host refresh interval; one of 1, 5 or 10. */
  refreshMinutes: number;
  shortcut: string;
  shortcutError: string;
  startupEnabled: boolean;
  version: string;
}

export type Screen = "about" | "dashboard" | "provider" | "settings";

/** One provider's menu-bar settings. `hideValue: null` follows the menu bar. */
export interface MenuBarItem {
  window: "auto" | "session" | "weekly" | "monthly";
  hideValue: boolean | null;
  hidden: boolean;
  colorValue: boolean | null;
}
