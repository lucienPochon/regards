import { invoke } from "@tauri-apps/api/core";
import type { UndoInfo } from "./accounts";

/**
 * Notes about the people an account writes with (ADR-0020): one fact each,
 * by the person or an agent, kept per account and address on this computer.
 * Agents' notes with a link, an address or bank details wait as a `note` hold.
 */
export interface Note {
  id: number;
  account_id: string;
  /** The person it is about, lower case. */
  address: string;
  text: string;
  by: "person" | "agent";
  /** The message it was drawn from, when one was given. */
  from_message_id?: string;
  from_subject?: string;
  /** Who sent that message: worth showing when it is not `address`. */
  from_address?: string;
  created_at: number;
  updated_at: number;
}

/** Whether notes are on (the default), and how many there are. */
export interface NoteSettings {
  on: boolean;
  notes: number;
  people: number;
}

/** Most characters a note may have. */
export const NOTE_MAX = 500;

/** The notes about one person in one account, oldest first. */
export const listNotes = (accountId: string, address: string) =>
  invoke<{ notes: Note[] }>("note_list", { accountId, address }).then((result) => result.notes);

/** Keep a note; `from_message`: the message it was drawn from (of the same account). */
export const addNote = (note: { account_id: string; address: string; text: string; from_message?: number }) =>
  invoke<{ note: Note; undo?: UndoInfo }>("note_add", { note }).then((result) => result.note);

/** Change a note's text: it is the person's from then on. */
export const updateNote = (id: number, text: string) => invoke<Note>("note_update", { id, text });

/** Remove a note; `undo` puts it back for a while. */
export const removeNote = (id: number) => invoke<{ removed: boolean; undo?: UndoInfo }>("note_remove", { id });

export const noteSettings = () => invoke<NoteSettings>("note_settings");

/** Off keeps the notes, but nobody is shown or given them, and agents add none. */
export const setNotesEnabled = (on: boolean) => invoke<NoteSettings>("note_set_enabled", { on });

/** Remove every note, in every account. */
export const forgetAllNotes = () => invoke<{ forgotten: number }>("note_forget_all");
