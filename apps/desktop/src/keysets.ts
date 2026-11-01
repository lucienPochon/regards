import type { KeySet } from "./prefs";

/** The sets with a full list of their own (shortcuts.ts for commands, actions.ts for message actions). */
export type BaseSet = "standard" | "classic";

/** The full list a key set starts from: the others differ from the standard one. */
export const baseOf = (set: KeySet): BaseSet => (set === "classic" ? "classic" : "standard");

/**
 * What another mail app's keys change in the standard set, by action or
 * command; everything else keeps its standard keys. An empty list is no key.
 *
 * Regards reads one key at a time, so two-key moves (Gmail's G then I) are not
 * here, and G stays free so that they do nothing rather than set mail aside.
 * Enter always opens the highlighted message, and combinations of Ctrl with
 * Command, or with Alt, cannot be keys: Apple Mail's Archive, Move and sidebar
 * keys keep the standard ones, and Superhuman's Enter for reply all is A.
 *
 * Gmail: support.google.com/mail/answer/6594. Apple Mail, with Mod for
 * Command: support.apple.com/guide/mail/mlhlb94f262b/mac. Superhuman:
 * pie-menu.com/shortcuts/superhuman, a third party's list, as Superhuman's own
 * help pages could not be read.
 */
export const OTHER_APP_KEYS: Partial<Record<KeySet, Record<string, string[]>>> = {
  gmail: {
    reply: ["r"],
    replyAll: ["a"],
    read: ["Shift+I"],
    unread: ["Shift+U"],
    flag: ["s"],
    snooze: ["b"],
    aside: [],
    move: ["v"],
    junk: ["!"],
    delete: ["#", "Delete"],
    mute: ["m"],
    priority: ["+", "="],
    search: ["/", "Mod+f"],
    sidebar: ["Mod+/"],
    openAll: [";"],
    foldOlder: [":"],
    print: ["Mod+p"],
    sync: ["Shift+N"],
  },
  apple: {
    reply: ["Mod+r"],
    replyAll: ["Mod+Shift+R"],
    forward: ["Mod+Shift+F"],
    read: ["Mod+Shift+U"],
    unread: ["Mod+Shift+U"],
    junk: ["Mod+Shift+J"],
    again: [],
    compose: ["Mod+n"],
    sync: ["Mod+Shift+N"],
    send: ["Mod+Shift+D", "Mod+Enter"],
    // Theirs are Apple's Forward and Mark unread above.
    find: ["Mod+Shift+L"],
    newFolder: ["Mod+Shift+O"],
  },
  superhuman: {
    reply: ["r"],
    replyAll: ["a"],
    flag: ["s"],
    snooze: ["h"],
    aside: [],
    move: ["v"],
    junk: ["!"],
    delete: ["#", "Delete"],
    mute: ["Shift+M"],
    unsubscribe: ["Mod+u"],
    search: ["/", "Mod+f"],
    sidebar: ["Mod+/"],
  },
};
