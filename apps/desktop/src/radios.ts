import type { KeyboardEvent } from "react";

const NEXT = ["ArrowRight", "ArrowDown"];
const PREVIOUS = ["ArrowLeft", "ArrowUp"];

/**
 * Keys for an element with role="radiogroup": the arrows move to the next or
 * previous radio (round the ends), Home and End to the first and last. With
 * `choose` (the default) the radio moved to is chosen, as the pattern
 * expects; without, only focus moves, for groups where choosing closes them.
 * Give the radios tabIndex from `radioTab`, so Tab stops on the group once.
 */
export function radioKeys(event: KeyboardEvent<HTMLElement>, choose = true) {
  if (![...NEXT, ...PREVIOUS, "Home", "End"].includes(event.key)) return;
  const radios = [...event.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]:not(:disabled)')];
  const at = radios.indexOf(document.activeElement as HTMLElement);
  if (at === -1 || radios.length === 0) return;
  event.preventDefault();
  const next =
    event.key === "Home"
      ? 0
      : event.key === "End"
        ? radios.length - 1
        : (at + (NEXT.includes(event.key) ? 1 : -1) + radios.length) % radios.length;
  radios[next].focus();
  if (choose) radios[next].click();
}

/** Tab reaches the chosen radio, or the first when none is chosen. */
export const radioTab = (checked: boolean, index: number, anyChecked: boolean) =>
  checked || (!anyChecked && index === 0) ? 0 : -1;
