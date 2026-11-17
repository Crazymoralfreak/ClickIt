import { api } from "../../api";
import { useApp } from "../../app/context";
import { useT, type Translator } from "../../i18n";
import type { UpdateStatus } from "../../types/runtime";
import { Button, Field, PageFrame, Section } from "../../ui";
import "./help.css";

const APP_NAME = "ClickIt";
/** Shown as plain text on purpose: the application opens no links. */
const REPOSITORY = "github.com/Crazymoralfreak/ClickIt";
const RELEASES = `${REPOSITORY}/releases/latest`;

function updateText(update: UpdateStatus, t: Translator["t"]): string {
  const version = update.version ?? "";
  switch (update.state) {
    case "idle":
      return t("update.idle");
    case "checking":
      return t("update.checking");
    case "upToDate":
      return t("update.upToDate");
    case "available":
      return t("update.available", { version, url: RELEASES });
    case "downloading":
      return t("update.downloading", { version });
    case "ready":
      return t("update.ready", { version });
    case "installing":
      return t("update.installing", { version });
    case "failed":
      // Without a version the check itself failed; with one, the download or the installation did.
      return update.version === null ? t("update.failed") : t("update.failedVersion", { version });
    default: {
      const unreachable: never = update.state;
      throw new Error(`Unknown update state: ${String(unreachable)}`);
    }
  }
}

/** A check makes no sense while one is running or while a found update is on its way. */
function canCheck(update: UpdateStatus): boolean {
  switch (update.state) {
    case "idle":
    case "upToDate":
    case "available":
    case "failed":
      return true;
    case "checking":
    case "downloading":
    case "ready":
    case "installing":
      return false;
    default: {
      const unreachable: never = update.state;
      throw new Error(`Unknown update state: ${String(unreachable)}`);
    }
  }
}

export function AboutPage() {
  const { t } = useT();
  const { info, update, showError } = useApp();

  return (
    <PageFrame title={t("nav.about")}>
      <Section>
        <div className="about-name">{APP_NAME}</div>
        <p className="about-description">{t("about.description")}</p>
        <Field label={t("about.version")}>
          <span className="about-value">{info.version}</span>
        </Field>
        <Field label={t("about.dataDir")}>
          <span className="about-value">{info.dataDir}</span>
        </Field>
        <Field label={t("about.repository")}>
          <span className="about-value">{REPOSITORY}</span>
        </Field>
      </Section>
      <Section title={t("update.title")}>
        <p className="help-text about-update">{updateText(update, t)}</p>
        <Button disabled={!canCheck(update)} onClick={() => void api.updateCheck().catch(showError)}>
          {t("update.check")}
        </Button>
      </Section>
      <Section>
        <p className="help-text">{t("about.network")}</p>
      </Section>
    </PageFrame>
  );
}
