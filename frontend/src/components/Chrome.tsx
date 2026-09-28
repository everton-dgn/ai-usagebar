import type { ReactNode } from "react";
import MdiChevronDown from "~icons/mdi/chevron-down";
import MdiChevronLeft from "~icons/mdi/chevron-left";
import MdiCogOutline from "~icons/mdi/cog-outline";
import MdiMagnifyScan from "~icons/mdi/magnify-scan";
import MdiApple from "~icons/mdi/apple";
import MdiInformationOutline from "~icons/mdi/information-outline";
import MdiPower from "~icons/mdi/power";
import MdiRefresh from "~icons/mdi/refresh";
import MdiRestore from "~icons/mdi/restore";
import MdiResize from "~icons/mdi/resize";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { Payload } from "@/lib/types";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { sendCommand } from "../model.js";

interface TopBarProps {
  onBack: () => void;
  onReset?: () => void;
  resetArmed?: boolean;
  resetLabel?: string;
  title: string;
}

/** PopoverTopBar: compact bar, centered headline, Back on the left, Reset on the right. */
export function TopBar({ onBack, onReset, resetArmed, resetLabel, title }: TopBarProps) {
  const { t } = useI18n();
  return (
    <div className="bar-glass grid shrink-0 grid-cols-[28px_1fr_28px] items-center p-[var(--panel-pad)]">
      <button type="button" aria-label={t("Back")} className="circle-btn" title={t("Back")} onClick={onBack}>
        <MdiChevronLeft className="size-4" />
      </button>
      <h1 className="m-0 [overflow-wrap:anywhere] text-center text-[13px] font-semibold">{title}</h1>
      {onReset ? (
        <button
          type="button"
          aria-label={resetArmed ? t("Click again to confirm") : resetLabel}
          className={cn("circle-btn", resetArmed && "bg-destructive text-white hover:bg-destructive")}
          title={resetArmed ? t("Click again to confirm") : resetLabel}
          onClick={onReset}
        >
          <MdiRestore className="size-[15px]" />
        </button>
      ) : (
        <span />
      )}
    </div>
  );
}

interface FooterProps {
  optionsOpen: boolean;
  payload: Payload;
  onOpenAbout: () => void;
  onOpenSettings: () => void;
  onOptionsOpenChange: (open: boolean) => void;
}

/** PopoverFooter: the panel size reset on the left, the Options ▾ capsule on the right. */
export function Footer({
  optionsOpen,
  payload,
  onOpenAbout,
  onOpenSettings,
  onOptionsOpenChange,
}: FooterProps) {
  const { t } = useI18n();
  return (
    <footer className="bar-glass flex shrink-0 items-center gap-2 p-[var(--panel-pad)]">
      {/* The version lives in About; the footer carries the size reset instead. */}
      <button
        type="button"
        className="capsule-btn min-w-0"
        aria-label={t("Reset Panel Size")}
        title={t("Reset Panel Size")}
        onClick={() => sendCommand("reset-panel-size")}
      >
        <MdiResize className="size-[13px] shrink-0" />
        <span className="truncate">{t("Reset Size")}</span>
      </button>
      <span className="min-w-2 flex-1" />
      <DropdownMenu modal={false} open={optionsOpen} onOpenChange={onOptionsOpenChange}>
        <DropdownMenuTrigger asChild>
          <button type="button" className="capsule-btn">
            {t("Options")}
            <MdiChevronDown className="size-[13px]" />
          </button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" side="top" sideOffset={6} className="min-w-[184px] rounded-[10px] border-0 p-[5px] shadow-lg">
          <MenuItem icon={<MdiCogOutline />} label={t("Settings")} onSelect={onOpenSettings} />
          <DropdownMenuSeparator />
          <MenuItem icon={<MdiRefresh />} label={t("Refresh")} onSelect={() => sendCommand("refresh")} />
          <MenuItem icon={<MdiMagnifyScan />} label={t("Detect Providers")} onSelect={() => sendCommand("detect")} />
          <DropdownMenuSeparator />
          <MenuItem
            checked={payload.startupEnabled}
            icon={<MdiApple />}
            label={t("Start at Login")}
            onSelect={() => sendCommand("toggle-startup")}
          />
          <DropdownMenuSeparator />
          <MenuItem icon={<MdiInformationOutline />} label={t("About")} onSelect={onOpenAbout} />
          <MenuItem destructive icon={<MdiPower />} label={t("Quit")} onSelect={() => sendCommand("quit")} />
        </DropdownMenuContent>
      </DropdownMenu>
    </footer>
  );
}

interface MenuItemProps {
  checked?: boolean;
  destructive?: boolean;
  icon: ReactNode;
  label: string;
  onSelect: () => void;
}

function MenuItem({ checked, destructive, icon, label, onSelect }: MenuItemProps) {
  const { t } = useI18n();
  return (
    <DropdownMenuItem
      className={cn(
        "gap-2 rounded-[var(--radius-sm)] px-2 py-[5px] text-[13px] focus:bg-[var(--card)] focus:text-label-1 [&_svg]:size-[15px] [&_svg]:text-label-2 focus:[&_svg]:text-label-2",
        destructive && "text-destructive",
      )}
      variant={destructive ? "destructive" : "default"}
      onSelect={onSelect}
    >
      {icon}
      <span className="flex-1">{label}</span>
      {checked ? <span aria-label={t("On")}>✓</span> : null}
    </DropdownMenuItem>
  );
}
