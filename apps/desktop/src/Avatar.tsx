import type { Person } from "./accounts";

/** How many avatar colours the stylesheet defines (`.avatar.hue-0` …). */
const HUES = 8;

/** Up to two letters for a person: from the name's first and last words, else the address. */
export function initials(person: Person | null): string {
  const words = (person?.name ?? "")
    .replace(/["'()[\]<>]/g, " ")
    .split(/\s+/)
    .filter((word) => /\p{L}|\p{N}/u.test(word));
  if (words.length > 0) {
    const first = Array.from(words[0])[0] ?? "";
    const last = words.length > 1 ? (Array.from(words[words.length - 1])[0] ?? "") : "";
    return (first + last).toUpperCase();
  }
  const local = (person?.address ?? "").split("@")[0].replace(/[^\p{L}\p{N}]/gu, "");
  return Array.from(local).slice(0, 1).join("").toUpperCase() || "?";
}

/** The same person always gets the same colour. */
export function hue(person: Person | null): number {
  const key = (person?.address ?? person?.name ?? "").toLowerCase();
  let hash = 2166136261;
  for (let index = 0; index < key.length; index++) {
    hash ^= key.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0) % HUES;
}

/** A coloured circle with initials, optionally ringed; Regards does not fetch pictures of people. */
export function Avatar({
  person,
  size = "md",
  ring = false,
}: {
  person: Person | null;
  size?: "xs" | "sm" | "md" | "lg";
  ring?: boolean;
}) {
  return (
    <span className={`avatar ${size} hue-${hue(person)}${ring ? " ring" : ""}`} aria-hidden="true">
      {initials(person)}
    </span>
  );
}
