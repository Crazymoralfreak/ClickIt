import { useApp } from "../../app/context";
import { useT } from "../../i18n";
import { Field, PageFrame, Section } from "../../ui";
import "./help.css";

const APP_NAME = "ClickIt";
/** Shown as plain text on purpose: the application opens no links. */
const REPOSITORY = "github.com/Crazymoralfreak/ClickIt";

export function AboutPage() {
  const { t } = useT();
  const { info } = useApp();

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
      <Section>
        <p className="help-text">{t("about.offline")}</p>
      </Section>
    </PageFrame>
  );
}
