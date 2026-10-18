import type { ReactNode } from "react";

/** Line icons drawn for Regards on a 24-unit grid, stroked in the current text colour. */
const PATHS = {
  inbox: (
    <>
      <path d="M3 13h5l1.5 3h5l1.5-3h5" />
      <path d="M5.6 5h12.8L21 13v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-5z" />
    </>
  ),
  send: (
    <>
      <path d="M21 3 10.5 13.5" />
      <path d="M21 3 14.5 21l-4-7.5L3 9.5z" />
    </>
  ),
  draft: (
    <>
      <path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" />
      <path d="M14 3v5h5" />
      <path d="M9 13h6M9 17h4" />
    </>
  ),
  outbox: (
    <>
      <path d="M3 14h5l1.5 3h5l1.5-3h5" />
      <path d="M3 14v4a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-4" />
      <path d="M12 3v9M8.5 6.5 12 3l3.5 3.5" />
    </>
  ),
  archive: (
    <>
      <rect x="3" y="4" width="18" height="5" rx="1.2" />
      <path d="M5 9v9a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V9" />
      <path d="M10 13h4" />
    </>
  ),
  trash: (
    <>
      <path d="M4 7h16" />
      <path d="M9 7V4.8A1.8 1.8 0 0 1 10.8 3h2.4A1.8 1.8 0 0 1 15 4.8V7" />
      <path d="M6 7l.9 12.2A2 2 0 0 0 8.9 21h6.2a2 2 0 0 0 2-1.8L18 7" />
      <path d="M10 11v6M14 11v6" />
    </>
  ),
  junk: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M5.7 5.7l12.6 12.6" />
    </>
  ),
  folder: <path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 16.5z" />,
  move: (
    <>
      <path d="M3 7.5A2.5 2.5 0 0 1 5.5 5H9l2 2h7.5A2.5 2.5 0 0 1 21 9.5v7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 16.5z" />
      <path d="M9 13h6M13 10.5l2.5 2.5-2.5 2.5" />
    </>
  ),
  flag: (
    <>
      <path d="M5 21V4" />
      <path d="M5 4h12l-2.5 4.5L17 13H5" />
    </>
  ),
  flagFilled: (
    <>
      <path d="M5 21V4" />
      <path d="M5 4h12l-2.5 4.5L17 13H5z" fill="currentColor" />
    </>
  ),
  mail: (
    <>
      <rect x="3" y="5" width="18" height="14" rx="2.5" />
      <path d="M3.8 6.8 12 13l8.2-6.2" />
    </>
  ),
  mailOpen: (
    <>
      <path d="M3 10.2 12 4l9 6.2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
      <path d="M3.6 10.6 12 16l8.4-5.4" />
    </>
  ),
  reply: (
    <>
      <path d="M9.5 6.5 4 12l5.5 5.5" />
      <path d="M4 12h10a6 6 0 0 1 6 6v1" />
    </>
  ),
  replyAll: (
    <>
      <path d="M8 6.5 2.5 12 8 17.5" />
      <path d="M13 6.5 7.5 12l5.5 5.5" />
      <path d="M7.5 12H15a6 6 0 0 1 6 6v1" />
    </>
  ),
  forward: (
    <>
      <path d="M14.5 6.5 20 12l-5.5 5.5" />
      <path d="M20 12H10a6 6 0 0 0-6 6v1" />
    </>
  ),
  more: (
    <>
      <circle cx="5.5" cy="12" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="12" cy="12" r="1.3" fill="currentColor" stroke="none" />
      <circle cx="18.5" cy="12" r="1.3" fill="currentColor" stroke="none" />
    </>
  ),
  compose: (
    <>
      <path d="M4 20h4.2L19 9.2a2.9 2.9 0 0 0-4.2-4.2L4 15.8z" />
      <path d="M13.5 6.5l4 4" />
    </>
  ),
  close: <path d="M6.5 6.5l11 11M17.5 6.5l-11 11" />,
  check: <path d="M5 12.5l4.5 4.5L19 7.5" />,
  chevronDown: <path d="M6.5 9.5 12 15l5.5-5.5" />,
  chevronRight: <path d="M9.5 6.5 15 12l-5.5 5.5" />,
  sidebar: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2.5" />
      <path d="M9.5 4v16" />
    </>
  ),
  settings: (
    <>
      <path d="M4 7h9M18 7h2M4 17h3M12 17h8" />
      <circle cx="15.5" cy="7" r="2.3" />
      <circle cx="9.5" cy="17" r="2.3" />
    </>
  ),
  refresh: (
    <>
      <path d="M19.5 10A7.8 7.8 0 0 0 5.6 7.4L4 9" />
      <path d="M4 4.5V9h4.5" />
      <path d="M4.5 14a7.8 7.8 0 0 0 13.9 2.6L20 15" />
      <path d="M20 19.5V15h-4.5" />
    </>
  ),
  paperclip: (
    <path d="M20 11.5l-8.2 8.2a5 5 0 0 1-7.1-7.1l8.6-8.6a3.3 3.3 0 0 1 4.7 4.7l-8.5 8.5a1.7 1.7 0 0 1-2.4-2.4l7.8-7.8" />
  ),
  keyboard: (
    <>
      <rect x="2.5" y="6" width="19" height="12" rx="2.5" />
      <path d="M6.5 10h.01M10 10h.01M14 10h.01M17.5 10h.01M7.5 14h9" />
    </>
  ),
  help: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M9.6 9.4a2.5 2.5 0 1 1 3.4 2.4c-.6.3-1 .8-1 1.5v.5" />
      <path d="M12 17h.01" />
    </>
  ),
  shield: (
    <>
      <path d="M12 3l7 3v5.2c0 4.3-2.9 8.1-7 9.8-4.1-1.7-7-5.5-7-9.8V6z" />
      <path d="M9 12l2 2 4-4" />
    </>
  ),
  image: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2.5" />
      <circle cx="9" cy="10" r="1.6" />
      <path d="M21 16l-5-5-9 9" />
    </>
  ),
  text: <path d="M4 6h16M4 11h16M4 16h10" />,
  plus: <path d="M12 5v14M5 12h14" />,
  user: (
    <>
      <circle cx="12" cy="8.5" r="4" />
      <path d="M4.5 20a7.5 7.5 0 0 1 15 0" />
    </>
  ),
  alert: (
    <>
      <path d="M10.3 4.2 2.8 17.5A2 2 0 0 0 4.5 20.5h15a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z" />
      <path d="M12 10v4M12 17h.01" />
    </>
  ),
  sun: (
    <>
      <circle cx="12" cy="12" r="4" />
      <path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5.5M12 7.5h.01" />
    </>
  ),
  pin: (
    <>
      <path d="M9 3.5h6l-.9 5.2 3.4 3.3v1.5h-11V12l3.4-3.3z" />
      <path d="M12 13.5v7" />
    </>
  ),
  pinFilled: (
    <>
      <path d="M9 3.5h6l-.9 5.2 3.4 3.3v1.5h-11V12l3.4-3.3z" fill="currentColor" />
      <path d="M12 13.5v7" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 2" />
    </>
  ),
  aside: (
    <>
      <rect x="3.5" y="4" width="17" height="16" rx="2.5" />
      <path d="M14.5 4v16" />
      <path d="M8 10.5h3M8 14h3" />
    </>
  ),
  logo: (
    <>
      <rect x="4" y="6" width="16" height="12" rx="2.2" />
      <path d="M4.8 7.2 12 12.6l7.2-5.4" />
    </>
  ),
} satisfies Record<string, ReactNode>;

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 18, className }: { name: IconName; size?: number; className?: string }) {
  return (
    <svg
      className={className ? `icon ${className}` : "icon"}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.75}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
