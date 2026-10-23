import { useEffect, useState } from "react";

/**
 * Whether this computer has a network, as the system says: kept current.
 * Mail synced before still reads, and what is sent waits in the Outbox.
 */
export function useOnline(): boolean {
  const [online, setOnline] = useState(() => navigator.onLine);
  useEffect(() => {
    const update = () => setOnline(navigator.onLine);
    window.addEventListener("online", update);
    window.addEventListener("offline", update);
    return () => {
      window.removeEventListener("online", update);
      window.removeEventListener("offline", update);
    };
  }, []);
  return online;
}
