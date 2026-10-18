import { useCallback, useEffect, useState } from "react";
import { useChanges } from "./changes";
import { type Draft, listDrafts } from "./compose";

/** Messages being written, to pick up again. Shown only when there are some. */
export function DraftsPanel({ onOpen, open }: { onOpen: (draftId: number) => void; open: number | null }) {
  const [drafts, setDrafts] = useState<Draft[]>([]);

  const refresh = useCallback(() => {
    listDrafts()
      .then((result) => setDrafts(result.drafts))
      .catch(() => {});
  }, []);
  useEffect(refresh, [refresh]);
  useChanges(refresh);

  if (drafts.length === 0) return null;
  return (
    <section className="card drafts" aria-labelledby="drafts-heading">
      <h2 id="drafts-heading">Drafts</h2>
      <ul>
        {drafts.map((draft) => (
          <li key={draft.id}>
            <button
              type="button"
              className={draft.id === open ? "draft current" : "draft"}
              onClick={() => onOpen(draft.id)}
            >
              <span className="subject">{draft.subject || "(no subject)"}</span>
              <span className="muted small">
                {draft.to.length > 0
                  ? `to ${draft.to[0].name ?? draft.to[0].address}${draft.to.length > 1 ? ` and ${draft.to.length - 1} more` : ""}`
                  : "no recipient yet"}
                {" · "}
                {new Date(draft.updated_at).toLocaleString()}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
