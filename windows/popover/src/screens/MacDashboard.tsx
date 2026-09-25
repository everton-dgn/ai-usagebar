import { useEffect, useRef, useState, type MouseEvent, type PointerEvent } from "react";
import MdiCogOutline from "~icons/mdi/cog-outline";
import MdiPin from "~icons/mdi/pin";
import MdiPinOutline from "~icons/mdi/pin-outline";
import MdiRefresh from "~icons/mdi/refresh";
import MdiTab from "~icons/mdi/tab";
import MdiViewAgendaOutline from "~icons/mdi/view-agenda-outline";
import { ProviderIcon } from "@/components/ProviderIcon";
import { useI18n } from "@/lib/i18n";
import type { Card, Layout, MetricRow, PanelView, Payload, Row } from "@/lib/types";
import { nextUpdateLabel, providerIconId, resetText, sendCommand, usageGoal } from "../model.js";

interface MacDashboardProps {
  cards: Card[];
  layout: Layout;
  nowMs: number;
  payload: Payload;
  onOpenCustomize: () => void;
}

interface MacPanelHeaderProps {
  view: PanelView;
  pinned: boolean;
  onView: (view: PanelView) => void;
  onPin: (pinned: boolean) => void;
  onOpenSettings: () => void;
}

/** The macOS dashboard header, shared by both views: title, view switch, refresh, settings. */
export function MacPanelHeader({ view, pinned, onView, onPin, onOpenSettings }: MacPanelHeaderProps) {
  const { t } = useI18n();
  const views: [PanelView, string, typeof MdiTab][] = [
    ["list", t("List"), MdiViewAgendaOutline],
    ["tabs", t("Tabs"), MdiTab],
  ];
  return (
    <header className="mac-dashboard-header">
      <div>
        <h1>AI Usage</h1>
        <p>{t("Usage and balance")}</p>
      </div>
      <div className="mac-dashboard-actions">
        <div className="mac-view-switch" role="group" aria-label={t("Panel View")}>
          {views.map(([id, label, Icon]) => (
            <button
              key={id}
              type="button"
              className="mac-icon-button"
              aria-label={label}
              aria-pressed={view === id}
              data-active={view === id}
              title={label}
              onClick={() => onView(id)}
            >
              <Icon aria-hidden />
            </button>
          ))}
        </div>
        <button
          type="button"
          className="mac-icon-button mac-pin-button"
          aria-label={t(pinned ? "Unpin panel" : "Pin panel")}
          aria-pressed={pinned}
          data-active={pinned}
          title={t(pinned ? "Unpin panel" : "Pin panel")}
          onClick={() => onPin(!pinned)}
        >
          {pinned ? <MdiPin aria-hidden /> : <MdiPinOutline aria-hidden />}
        </button>
        <button type="button" className="mac-icon-button" aria-label={t("Refresh")} title={t("Refresh")} onClick={() => sendCommand("refresh")}>
          <MdiRefresh aria-hidden />
        </button>
        <button type="button" className="mac-icon-button" aria-label={t("Settings")} title={t("Settings")} onClick={onOpenSettings}>
          <MdiCogOutline aria-hidden />
        </button>
      </div>
    </header>
  );
}

function primaryMetric(card: Card): MetricRow | undefined {
  return card.rows.find((row): row is MetricRow => row.kind === "metric" && row.headline === "percent")
    ?? card.rows.find((row): row is MetricRow => row.kind === "metric");
}

function providerPreview(card: Card): string {
  const metric = primaryMetric(card);
  if (metric) return metric.headline === "value" ? metric.value : `${metric.usedPercent}%`;
  if (card.error) return "—";
  const balance = card.rows.find((row) => row.kind === "text" && /balance|credit/i.test(row.label));
  return balance?.kind === "text" ? balance.value : "—";
}

function Metric({ row, layout, nowMs }: { row: MetricRow; layout: Layout; nowMs: number }) {
  const { language, metricLabel, t } = useI18n();
  const percent = Math.min(100, Math.max(0, Number(row.usedPercent) || 0));
  const reset = resetText(row, layout.resetTimes, nowMs, { locale: language, timeFormat: layout.timeFormat });
  const goal = layout.usageGoal ? usageGoal(row, nowMs) : null;
  const balance = row.headline === "value";
  const label = row.label === "Session" ? `${t("Session")} (5h)` : metricLabel(row.label);
  return (
    <div className="mac-metric">
      <div className="mac-metric-heading">
        <span>{label}</span>
        <strong>{balance ? row.value : `${percent}%`}</strong>
      </div>
      <div
        className="mac-meter"
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent}
      >
        <span className="mac-meter-fill" data-severity={row.severity} style={{ width: `${percent}%` }} />
      </div>
      {reset || row.detail || (balance && percent > 0) ? (
        <div className="mac-metric-note">
          <span>{reset || row.detail}</span>
          {balance && percent > 0 ? <span>{percent}% {t("used")}</span> : null}
        </div>
      ) : null}
      {goal ? (
        <div className="mac-usage-goal">
          <div className="mac-goal-heading">
            <span>{t(goal.estimated ? "Estimated goal now" : "Goal now")}</span>
            <strong>{Math.round(goal.percent)}%</strong>
          </div>
          <div
            className="mac-goal-meter"
            role="progressbar"
            aria-label={`${label}: ${t(goal.estimated ? "Estimated goal now" : "Goal now")}`}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={Math.round(goal.percent)}
          >
            <span className="mac-goal-meter-fill" style={{ width: `${goal.percent}%` }} />
          </div>
        </div>
      ) : null}
    </div>
  );
}

function DetailRow({ row, layout, nowMs }: { row: Row; layout: Layout; nowMs: number }) {
  const { metricLabel, t } = useI18n();
  if (row.kind === "metric") return <Metric row={row} layout={layout} nowMs={nowMs} />;
  if (row.kind === "text") {
    return (
      <div className="mac-detail-row">
        <span>{metricLabel(row.label)}</span>
        <strong>{row.value}</strong>
      </div>
    );
  }
  if (row.kind === "resetCredits") {
    return (
      <div className="mac-detail-row">
        <span>{metricLabel(row.label)}</span>
        <strong>{row.available} {t(row.available === 1 ? "available singular" : "available")}</strong>
      </div>
    );
  }
  return (
    <div className="mac-detail-block">
      <strong>{metricLabel(row.label)}</strong>
      {row.body.map((line, index) => <span key={`${index}:${line}`}>{line}</span>)}
    </div>
  );
}

/** Pointer travel before a press on the tab row counts as a drag, not a click. */
const DRAG_THRESHOLD = 4;

/**
 * The tab row scrolls sideways with its scrollbar hidden, so a mouse gets two
 * ways in: drag the row, or turn the wheel. A drag never also selects a tab.
 */
function useDragScroll() {
  const ref = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; left: number; moved: boolean; id: number } | null>(null);
  const [dragging, setDragging] = useState(false);

  useEffect(() => {
    const row = ref.current;
    if (!row) return;
    // Native and non-passive: a vertical wheel over the row must scroll it
    // sideways instead of scrolling the panel underneath.
    const onWheel = (event: WheelEvent) => {
      if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
      const max = row.scrollWidth - row.clientWidth;
      const next = Math.min(max, Math.max(0, row.scrollLeft + event.deltaY));
      // At either end the row cannot move, so the panel keeps the scroll.
      if (next === row.scrollLeft) return;
      row.scrollLeft = next;
      event.preventDefault();
    };
    row.addEventListener("wheel", onWheel, { passive: false });
    return () => row.removeEventListener("wheel", onWheel);
  }, []);

  function onPointerDown(event: PointerEvent<HTMLDivElement>) {
    if (event.button !== 0 || event.pointerType !== "mouse") return;
    drag.current = { x: event.clientX, left: event.currentTarget.scrollLeft, moved: false, id: event.pointerId };
  }

  function onPointerMove(event: PointerEvent<HTMLDivElement>) {
    const state = drag.current;
    if (!state) return;
    const dx = event.clientX - state.x;
    if (!state.moved && Math.abs(dx) > DRAG_THRESHOLD) {
      state.moved = true;
      event.currentTarget.setPointerCapture(state.id);
      setDragging(true);
    }
    if (state.moved) event.currentTarget.scrollLeft = state.left - dx;
  }

  function onPointerEnd(event: PointerEvent<HTMLDivElement>) {
    const state = drag.current;
    if (state?.moved && event.currentTarget.hasPointerCapture(state.id)) {
      event.currentTarget.releasePointerCapture(state.id);
    }
    setDragging(false);
    if (!state?.moved || event.type === "pointercancel") {
      drag.current = null;
      return;
    }
    // Keep `moved` for the click that ends this press (dispatched in the same
    // task), then drop it: a press with no click must not swallow the next one.
    window.setTimeout(() => {
      if (drag.current === state) drag.current = null;
    }, 0);
  }

  function onClickCapture(event: MouseEvent<HTMLDivElement>) {
    if (drag.current?.moved) {
      event.preventDefault();
      event.stopPropagation();
    }
    drag.current = null;
  }

  return {
    ref,
    "data-dragging": dragging || undefined,
    onClickCapture,
    onPointerCancel: onPointerEnd,
    onPointerDown,
    onPointerMove,
    onPointerUp: onPointerEnd,
  };
}

/** A compact provider switcher for the macOS menu bar popover. */
export function MacDashboard({ cards, layout, nowMs, payload, onOpenCustomize }: MacDashboardProps) {
  const { language, t } = useI18n();
  const [selectedId, setSelectedId] = useState("");
  const tabRow = useDragScroll();
  const selected = cards.find((card) => card.id === selectedId)
    ?? cards.find((card) => card.id === payload.primary)
    ?? cards.find((card) => primaryMetric(card))
    ?? cards[0];
  const selectedEntry = payload.entries.find((entry) => entry.id === selected?.id);
  const updated = payload.generatedAt > 0
    ? Math.max(0, Math.floor((nowMs - payload.generatedAt) / 60_000))
    : null;

  return (
    <div className="mac-dashboard">
      {cards.length ? (
        <>
          <div className="mac-provider-tabs" role="group" aria-label={t("Providers")} {...tabRow}>
            {cards.map((card) => {
              const active = selected?.id === card.id;
              return (
                <button
                  key={card.id}
                  type="button"
                  aria-pressed={active}
                  className="mac-provider-tab"
                  data-active={active}
                  onClick={() => setSelectedId(card.id)}
                >
                  <ProviderIcon slug={providerIconId(card.id)} title={card.title} size={17} />
                  <span className="mac-tab-name">{card.title}</span>
                  <span className="mac-tab-value">{providerPreview(card)}</span>
                </button>
              );
            })}
          </div>

          {selected ? (
            <section className="mac-provider-card" aria-label={selected.title}>
              <div className="mac-provider-heading">
                <span className="mac-provider-mark"><ProviderIcon slug={providerIconId(selected.id)} title={selected.title} size={25} /></span>
                <span className="mac-provider-title">
                  <strong>{selected.title}</strong>
                  <small>{(layout.showPlan !== false && selected.plan) || (selectedEntry?.status === "ready" ? t("Current usage") : t("Usage unavailable"))}{selected.stale ? ` · ${t("Cached")}` : ""}</small>
                </span>
                <button type="button" className="mac-provider-refresh" title={`${t("Refresh")} ${selected.title}`} aria-label={`${t("Refresh")} ${selected.title}`} onClick={() => sendCommand("refresh-entry", { id: selected.id })}>
                  <MdiRefresh aria-hidden />
                </button>
              </div>

              {selected.rows.length ? (
                <div className="mac-usage-section">
                  <div className="mac-section-label">{t("USAGE & BALANCE")}</div>
                  {selected.rows.map((row, index) => <DetailRow key={row.key || `${row.kind}:${index}`} row={row} layout={layout} nowMs={nowMs} />)}
                </div>
              ) : selected.error ? (
                <div className="mac-empty-state">
                  <strong>{t(selected.errorTitle || "Usage unavailable")}</strong>
                  <p>{t(selected.errorHint || selected.error)}</p>
                </div>
              ) : (
                <div className="mac-empty-state">{t("No usage or balance data yet.")}</div>
              )}
              {selected.error && selected.rows.length ? <p className="mac-cached-note">{t(selected.errorTitle)}: {t(selected.errorHint)}</p> : null}
            </section>
          ) : null}
        </>
      ) : (
        <div className="mac-empty-state">
          <strong>{payload.hostError ? t("Couldn't load usage") : payload.entries.length ? t("No providers shown") : t("No providers detected")}</strong>
          <p>{payload.hostError || (payload.entries.length ? t("Turn on a provider in Customize.") : t("Refresh to check your installed providers."))}</p>
          <button type="button" className="mac-retry-button" onClick={payload.entries.length ? onOpenCustomize : () => sendCommand("detect")}>
            {t(payload.entries.length ? "Customize Providers" : "Detect Providers")}
          </button>
        </div>
      )}

      <div className="mac-dashboard-status">
        <span>{updated === null ? t("Waiting for update") : updated < 1 ? t("Updated just now") : language === "pt-BR" ? `Atualizado há ${updated} min` : `Updated ${updated}m ago`}</span>
        <span>{nextUpdateLabel(payload, nowMs, language)}</span>
      </div>
    </div>
  );
}
