import { useCallback, useEffect, useRef, useState } from "react";
import { type MessageLabels, messageLabels } from "./accounts";
import { useChanges } from "./changes";

/** Most ids asked about at once (the daemon's limit). */
const AT_ONCE = 1000;

/**
 * The Gmail labels of these messages (label folder ids by message id), kept
 * current. `update` puts in what `setLabels` answered, before the next look.
 */
export function useLabels(ids: number[]): {
  labels: ReadonlyMap<number, number[]>;
  update: (changed: MessageLabels[]) => void;
} {
  const [labels, setLabels] = useState<Map<number, number[]>>(() => new Map());
  const key = ids.join(",");
  const generation = useRef(0);
  const load = useCallback(() => {
    const run = ++generation.current;
    const wanted = key ? key.split(",").map(Number) : [];
    if (wanted.length === 0) {
      setLabels(new Map());
      return;
    }
    const chunks: number[][] = [];
    for (let at = 0; at < wanted.length; at += AT_ONCE) chunks.push(wanted.slice(at, at + AT_ONCE));
    Promise.all(chunks.map(messageLabels)).then(
      (pages) => {
        if (run === generation.current) setLabels(new Map(pages.flat().map((message) => [message.id, message.labels])));
      },
      () => {},
    );
  }, [key]);
  useEffect(load, [load]);
  useChanges(load);
  const update = useCallback((changed: MessageLabels[]) => {
    setLabels((now) => {
      const next = new Map(now);
      for (const message of changed) next.set(message.id, message.labels);
      return next;
    });
  }, []);
  return { labels, update };
}
