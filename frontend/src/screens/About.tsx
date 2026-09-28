import type { Payload } from "@/lib/types";
import { useI18n } from "@/lib/i18n";

interface AboutProps {
  payload: Payload;
}

/** App name and the version this build was compiled as. */
export function About({ payload }: AboutProps) {
  const { t } = useI18n();
  const version = payload.version ? `${t("Version")} ${payload.version}` : t("Development build");

  return (
    <div className="flex flex-col gap-[var(--section-gap)]">
      <div className="card-surface">
        <div className="flex flex-col gap-1 px-[var(--card-pad)] py-[var(--pad-control)]">
          <span className="text-[length:var(--sz-header)] font-semibold">AI Usage</span>
          <span className="text-[length:var(--sz-support)] text-label-2">{version}</span>
        </div>
      </div>
    </div>
  );
}
