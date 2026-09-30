import { useEffect, useState } from "react";
import { claudeCodeSetHooks, claudeCodeStatus, type ClaudeCodeStatus } from "../api";
import { formatDuration } from "../format";
import type { Translate } from "../i18n";

// Eingebautes Fenster „Claude Code wartet“ (Issue #10): Empfang, Hooks,
// wartende Sessions. Die Hooks werden nur nach Rückfrage eingetragen.
export default function ClaudeCodeCard({ t }: { t: Translate }) {
  const [status, setStatus] = useState<ClaudeCodeStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    let active = true;
    const load = () => claudeCodeStatus()
      .then((next) => { if (active) { setStatus(next); setNow(Date.now()); } })
      .catch((e) => { if (active) setError(String(e)); });
    load();
    const timer = window.setInterval(load, 5000);
    return () => { active = false; window.clearInterval(timer); };
  }, []);

  const setHooks = async (install: boolean) => {
    const path = status?.settingsPath ?? "~/.claude/settings.json";
    const question = install
      ? t("cc.install.confirm", { path })
      : t("cc.remove.confirm", { path });
    if (!window.confirm(question)) return;
    setBusy(true);
    setError(null);
    try { setStatus(await claudeCodeSetHooks(install)); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  const hooks = status?.hooks ?? "missing";
  return (
    <div className="card">
      <h3>{t("cc.title")}</h3>
      <p className="muted small">{t("cc.intro")}</p>
      {status && (status.listener === "failed"
        ? <p className="notice-bad" role="alert">{t("cc.listener.failed", { port: status.port, error: status.error ?? "" })}</p>
        : <p className="muted small">{t(status.listener === "running" ? "cc.listener.running" : "cc.listener.starting", { port: status.port })}</p>)}
      <p className={hooks === "installed" ? "muted small" : "notice-warn"}>
        {t(`cc.hooks.${hooks}`)}{status?.settingsPath ? ` · ${status.settingsPath}` : ""}
      </p>
      <div className="field-row">
        {hooks !== "installed" && <button type="button" className="btn btn-primary" disabled={busy} onClick={() => setHooks(true)}>{t("cc.install")}</button>}
        {hooks !== "missing" && <button type="button" className="btn" disabled={busy} onClick={() => setHooks(false)}>{t("cc.remove")}</button>}
      </div>
      {status && status.waiting.length > 0 && (
        <>
          <h4>{t("cc.waiting")}</h4>
          {status.waiting.map((session, index) => (
            <p key={index} className="small">
              {session.project || t("cc.title")} · {t(`cc.state.${session.state}`)} · {formatDuration(t, Math.max(0, now / 1000 - session.since))}
            </p>
          ))}
        </>
      )}
      <p className="muted small">{t("cc.window.hint")}</p>
      {error && <p className="notice-bad" role="alert">{error}</p>}
    </div>
  );
}
