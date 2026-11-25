import { useEffect, useRef, useState } from "react";
import {
  accountErrorMessage,
  fetchAuditEvents,
  type AuditPage,
  type AuditScope,
} from "../../lib/account";
import { formatClock } from "../../lib/format";
import {
  AUDIT_ACTION_LABELS,
  auditDetail,
  AuditIcon,
} from "../settings-constants";

export interface AuditSettingsSectionProps {
  revision: number;
}

export function AuditSettingsSection({ revision }: AuditSettingsSectionProps) {
  const [auditScope, setAuditScope] = useState<AuditScope>("account");
  const [auditPage, setAuditPage] = useState<AuditPage>();
  const [auditError, setAuditError] = useState<string>();
  const [auditLoading, setAuditLoading] = useState(false);
  const requestSeqRef = useRef(0);

  useEffect(() => {
    const seq = ++requestSeqRef.current;
    setAuditLoading(true);
    setAuditError(undefined);
    void fetchAuditEvents(auditScope)
      .then((page) => {
        if (requestSeqRef.current === seq) setAuditPage(page);
      })
      .catch((cause) => {
        if (requestSeqRef.current === seq) {
          setAuditError(accountErrorMessage(cause));
        }
      })
      .finally(() => {
        if (requestSeqRef.current === seq) setAuditLoading(false);
      });
    return () => {
      requestSeqRef.current += 1;
    };
  }, [revision, auditScope]);

  async function loadMoreAudit() {
    const before = auditPage?.next_before;
    if (before === undefined || auditLoading) return;
    const seq = ++requestSeqRef.current;
    setAuditLoading(true);
    setAuditError(undefined);
    try {
      const next = await fetchAuditEvents(auditScope, before);
      if (requestSeqRef.current !== seq) return;
      setAuditPage((current) => ({
        ...next,
        events: [...(current?.events ?? []), ...next.events],
      }));
    } catch (cause) {
      if (requestSeqRef.current === seq) {
        setAuditError(accountErrorMessage(cause));
      }
    } finally {
      if (requestSeqRef.current === seq) {
        setAuditLoading(false);
      }
    }
  }

  return (
    <section className="settings-section" aria-labelledby="audit-settings">
      <div className="settings-section-heading audit-section-heading">
        <span className="settings-section-icon" aria-hidden="true">
          <AuditIcon size={18} />
        </span>
        <div>
          <h3 id="audit-settings">审计记录</h3>
          <p>追踪账户安全、订阅和当前工作区的管理动作。</p>
        </div>
        <div className="audit-scope-switch" aria-label="审计范围">
          {(["account", "workspace"] as const).map((scope) => (
            <button
              aria-pressed={auditScope === scope}
              key={scope}
              onClick={() => {
                setAuditPage(undefined);
                setAuditScope(scope);
              }}
              type="button"
            >
              {scope === "account" ? "账户" : "当前工作区"}
            </button>
          ))}
        </div>
      </div>
      {auditPage === undefined && auditLoading ? (
        <p className="settings-hint">正在读取审计记录…</p>
      ) : auditPage?.events.length === 0 ? (
        <p className="audit-empty">当前范围还没有审计记录。</p>
      ) : (
        <div className="audit-event-list">
          {auditPage?.events.map((event) => {
            const detail = auditDetail(event);
            return (
              <div className="audit-event-row" key={event.event_id}>
                <span
                  className="audit-event-marker"
                  data-scope={event.scope}
                  aria-hidden="true"
                />
                <span className="audit-event-copy">
                  <strong>{AUDIT_ACTION_LABELS[event.action]}</strong>
                  {detail.length > 0 && <small>{detail}</small>}
                </span>
                <time dateTime={event.created_at}>
                  {formatClock(event.created_at)}
                </time>
              </div>
            );
          })}
        </div>
      )}
      {auditPage?.next_before !== undefined && (
        <button
          className="secondary audit-load-more"
          disabled={auditLoading}
          onClick={() => void loadMoreAudit()}
          type="button"
        >
          {auditLoading ? "读取中…" : "加载更多"}
        </button>
      )}
      {auditError !== undefined && (
        <p className="error-text" role="alert">
          {auditError}
        </p>
      )}
    </section>
  );
}
