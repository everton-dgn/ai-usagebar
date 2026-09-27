import {
  closestCorners,
  DndContext,
  DragOverlay,
  type DragEndEvent,
  type DragOverEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import { restrictToVerticalAxis } from "@dnd-kit/modifiers";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { DragHandle } from "@/components/DragHandle";
import {
  handleRowDragEnd,
  handleRowDragOver,
  LIST_ALWAYS,
  LIST_DEMAND,
  SortableColumn,
  SortableItem,
  useTraySensors,
  type RowLists,
} from "@/components/dnd";
import MdiStar from "~icons/mdi/star";
import MdiStarOutline from "~icons/mdi/star-outline";
import { Switch } from "@/components/ui/switch";
import type { Card, Layout, ProviderView, Row } from "@/lib/types";
import { useI18n } from "@/lib/i18n";
import { isStarred, prefsForCard, providerLinks, rowKey } from "../model.js";

interface ProviderDetailProps {
  card?: Card;
  layout: Layout;
  view?: ProviderView;
  onView?: (view: ProviderView) => void;
  onToggleCollapse?: (expanded: boolean) => void;
  starError?: string;
  onReorderRows: (lists: RowLists) => void;
  onToggleRow: (key: string, on: boolean) => void;
  onToggleStar: (key: string) => void;
  onRename?: (name: string) => void;
}

/**
 * CustomizeProviderDetailView (L2): Always Visible and On Demand grouped cards. Rows drag within
 * a card or across the divider; an empty card shows the dashed "Drag metrics here" target.
 */
export function ProviderDetail({ card, layout, view = "overview", onView, onToggleCollapse, starError, onReorderRows, onToggleRow, onToggleStar, onRename }: ProviderDetailProps) {
  const { t } = useI18n();
  const sensors = useTraySensors();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [draft, setDraft] = useState<RowLists | null>(null);
  const listsRef = useRef<RowLists>({ always: [], demand: [] });
  if (!card) return null;
  const prefs = prefsForCard(card, layout);
  const byKey = new Map<string, Row>((card.rows || []).map((row: Row) => [rowKey(row), row]));
  const lists: RowLists = draft || { always: prefs.always, demand: prefs.demand };
  listsRef.current = lists;

  function onDragStart(event: DragStartEvent) {
    const initial = { always: prefs.always.slice(), demand: prefs.demand.slice() };
    listsRef.current = initial;
    setActiveId(String(event.active.id));
    setDraft(initial);
  }

  function onDragOver(event: DragOverEvent) {
    const next = handleRowDragOver(event, listsRef.current);
    if (!next) return;
    listsRef.current = next;
    setDraft(next);
  }

  function onDragEnd(event: DragEndEvent) {
    const next = handleRowDragEnd(event, listsRef.current);
    setActiveId(null);
    setDraft(null);
    onReorderRows(next);
  }

  const overlayRow = activeId ? byKey.get(activeId) : undefined;
  const views: Array<[ProviderView, string]> = [["overview", "Full list"], ["individual", "Individual"]];

  function changeView(next: ProviderView) {
    setActiveId(null);
    setDraft(null);
    onView?.(next);
  }

  function onViewKeyDown(event: KeyboardEvent<HTMLButtonElement>, current: ProviderView) {
    let next: ProviderView;
    if (event.key === "ArrowRight" || event.key === "ArrowLeft") next = current === "overview" ? "individual" : "overview";
    else if (event.key === "Home") next = "overview";
    else if (event.key === "End") next = "individual";
    else return;
    event.preventDefault();
    changeView(next);
    document.getElementById(`provider-view-${next}`)?.focus();
  }

  return (
    <DndContext
      sensors={sensors}
      collisionDetection={closestCorners}
      modifiers={[restrictToVerticalAxis]}
      onDragStart={onDragStart}
      onDragOver={onDragOver}
      onDragEnd={onDragEnd}
      onDragCancel={() => {
        setActiveId(null);
        setDraft(null);
      }}
    >
      <div className="flex flex-col gap-[var(--section-gap)]">
        {onRename ? <NameField card={card} onRename={onRename} /> : null}
        {onView ? (
          <div className="flex flex-col gap-2">
            <div className="settings-tabs" role="tablist" aria-label={t("Customize view")}>
              {views.map(([id, label]) => (
                <button key={id} type="button" id={`provider-view-${id}`} className="settings-tab"
                  role="tab" aria-selected={view === id} aria-controls="provider-view-panel"
                  tabIndex={view === id ? 0 : -1} onClick={() => changeView(id)}
                  onKeyDown={(event) => onViewKeyDown(event, id)}>
                  {t(label)}
                </button>
              ))}
            </div>
            <p className="px-1 text-[length:var(--sz-badge)] text-label-2">
              {t(view === "individual" ? "Changes below apply only to this provider's individual dropdown." : "Changes below apply only to this provider in the full list.")}
            </p>
          </div>
        ) : null}
        <div id={onView ? "provider-view-panel" : undefined} role={onView ? "tabpanel" : undefined}
          aria-labelledby={onView ? `provider-view-${view}` : undefined}
          className="flex flex-col gap-[var(--section-gap)]">
        {onToggleCollapse ? (
          <div className="card-surface flex items-center gap-[10px] p-[var(--pad-control)]">
            <span className="min-w-0 flex-1">{t("Show details when opened")}</span>
            <Switch checked={!layout.collapsed[card.id]} aria-label={t("Show details when opened")}
              onCheckedChange={(on) => onToggleCollapse(on === true)} />
          </div>
        ) : null}
        <MetricSection
          byKey={byKey}
          cardId={card.id}
          id={LIST_ALWAYS}
          keys={lists.always}
          layout={layout}
          prefs={prefs}
          title={t("Always Visible")}
          onToggleRow={onToggleRow}
          onToggleStar={onToggleStar}
        />
        {providerLinks(card.id).length ? (
          <div className="flex flex-col gap-[var(--header-card-gap)]">
            <div className="section-title">{t("Links")}</div>
            <div className="card-surface">
              {providerLinks(card.id).map((link) => (
                <div key={link.label} className="flex items-center gap-[10px] px-[var(--pad-control)] py-[var(--pad-control)]">
                  <span className="min-w-0 flex-1">{t(link.label)}</span>
                  <Switch
                    checked={!prefs.off[`link:${link.label}`]}
                    aria-label={`${t("Show")} ${t(link.label)}`}
                    onCheckedChange={(on) => onToggleRow(`link:${link.label}`, on === true)}
                  />
                </div>
              ))}
            </div>
          </div>
        ) : null}
        <MetricSection
          byKey={byKey}
          cardId={card.id}
          id={LIST_DEMAND}
          keys={lists.demand}
          layout={layout}
          prefs={prefs}
          title={t("On Demand")}
          onToggleRow={onToggleRow}
          onToggleStar={onToggleStar}
        />
        {starError ? (
          <div className="px-1 text-[length:var(--sz-badge)] text-meter-red">{t(starError)}</div>
        ) : null}
        </div>
      </div>
      <DragOverlay dropAnimation={null}>
        {overlayRow ? (
          <div className="lifted-surface">
            <MetricTuneRow enabled row={overlayRow} />
          </div>
        ) : null}
      </DragOverlay>
    </DndContext>
  );
}

/** The card's own name; empty or the report's name goes back to the report's name. */
function NameField({ card, onRename }: { card: Card; onRename: (name: string) => void }) {
  const { t } = useI18n();
  const original = card.defaultTitle || card.title;
  const [draft, setDraft] = useState(card.title);
  useEffect(() => setDraft(card.title), [card.title]);

  function save() {
    const name = draft.trim();
    if (name === card.title) return;
    onRename(name === original ? "" : name);
  }

  return (
    <div className="flex flex-col gap-[var(--header-card-gap)]">
      <div className="section-title">{t("Name")}</div>
      <div className="card-surface flex flex-wrap items-center gap-2 p-2">
        <input
          type="text"
          maxLength={80}
          className="h-7 min-w-0 flex-1 rounded-[6px] border border-[var(--border)] bg-[var(--control-fill)] px-1.5"
          aria-label={t("Name")}
          placeholder={original}
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          onBlur={save}
          onKeyDown={(event) => {
            if (event.key === "Enter") event.currentTarget.blur();
            if (event.key === "Escape") {
              // Cancel the edit only; the screen's own Escape must not also go back.
              event.stopPropagation();
              event.nativeEvent.stopImmediatePropagation();
              setDraft(card.title);
            }
          }}
        />
        {card.defaultTitle ? (
          <button type="button" className="plain-btn text-label-2" onClick={() => onRename("")}>
            {t("Restore original name")}
          </button>
        ) : null}
      </div>
    </div>
  );
}

interface MetricSectionProps {
  byKey: Map<string, Row>;
  cardId: string;
  id: string;
  keys: string[];
  layout: Layout;
  prefs: { off: Record<string, boolean> };
  title: string;
  onToggleRow: (key: string, on: boolean) => void;
  onToggleStar: (key: string) => void;
}

function MetricSection({
  byKey,
  cardId,
  id,
  keys,
  layout,
  prefs,
  title,
  onToggleRow,
  onToggleStar,
}: MetricSectionProps) {
  const { t } = useI18n();
  return (
    <div className="flex flex-col gap-[var(--header-card-gap)]">
      <div className="section-title">{title}</div>
      <SortableColumn id={id} items={keys} className="card-surface">
        {keys.length === 0 ? <div className="drop-zone">{t("Drag metrics here")}</div> : null}
        {keys.map((key) => {
          const row = byKey.get(key);
          if (!row) return null;
          return (
            <SortableItem key={key} id={key}>
              {({ attributes, listeners }) => (
                <MetricTuneRow
                  enabled={!prefs.off[key]}
                  handle={{ attributes, listeners }}
                  row={row}
                  starred={isStarred(layout.stars, cardId, key)}
                  onStar={() => onToggleStar(key)}
                  onToggle={(on) => onToggleRow(key, on)}
                />
              )}
            </SortableItem>
          );
        })}
      </SortableColumn>
    </div>
  );
}

interface MetricTuneRowProps {
  enabled: boolean;
  handle?: Parameters<typeof DragHandle>[0];
  row: Row;
  starred?: boolean;
  onStar?: () => void;
  onToggle?: (on: boolean) => void;
}

/** CustomizeMetricRow: grip, metric title, star, on/off switch. */
function MetricTuneRow({ enabled, handle, row, starred, onStar, onToggle }: MetricTuneRowProps) {
  const { metricLabel, t } = useI18n();
  const title = metricLabel(String(row.label || row.kind));
  return (
    <div data-row-key={rowKey(row)} className="flex items-center gap-[10px] px-[var(--pad-control)] py-[var(--pad-control)]">
      <DragHandle attributes={handle?.attributes} listeners={handle?.listeners} />
      <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">{title}</span>
      {onStar ? (
        <button
          type="button"
          aria-label={starred ? `${t("Unstar")} ${title}` : `${t("Star")} ${title} ${t("for menu bar")}`}
          aria-pressed={starred === true}
          className="inline-flex size-[18px] items-center justify-center text-label-2 hover:text-foreground"
          onClick={onStar}
        >
          {starred ? <MdiStar className="size-[14px] text-primary" /> : <MdiStarOutline className="size-[14px]" />}
        </button>
      ) : null}
      <Switch checked={enabled} aria-label={`${t("Show")} ${title}`} onCheckedChange={(on) => onToggle?.(on === true)} />
    </div>
  );
}
