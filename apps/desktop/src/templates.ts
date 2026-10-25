import { invoke } from "@tauri-apps/api/core";
import type { Person } from "./accounts";
import type { Draft } from "./compose";

/** A message to start from (plan §5.9): subject, text, recipients and files, kept on this device for every account. */
export interface Template {
  id: number;
  name: string;
  subject: string;
  /** The text; with `html`, its plain-text version. */
  body: string;
  /** The formatted version, sanitised. Images in it are among `files`, by Content-ID. */
  html: string | null;
  to: Person[];
  cc: Person[];
  bcc: Person[];
  /** The files it keeps, as they were added. */
  files: TemplateFile[];
  updated_at: number;
}

export interface TemplateFile {
  id: number;
  name: string;
  mime: string;
  size: number;
  /** An image in its text, not a file beside it. */
  inline: boolean;
}

/** By name. */
export const listTemplates = () =>
  invoke<{ templates: Template[] }>("template_list").then((result) => result.templates);

/**
 * Add one (no `id`) or change one. With `html`, the daemon sanitises it and
 * derives the text. `filesOfDraft`: keep that draft's files with it (saving
 * a message as a template); `removeFiles`: files it keeps no longer.
 */
export const saveTemplate = (template: {
  id: number | null;
  name: string;
  subject: string;
  body: string;
  html: string | null;
  to?: Person[];
  cc?: Person[];
  bcc?: Person[];
  filesOfDraft?: number | null;
  removeFiles?: number[];
}) =>
  invoke<Template>("template_save", {
    template: {
      id: template.id ?? undefined,
      name: template.name,
      subject: template.subject,
      body: template.body,
      html: template.html ?? undefined,
      to: template.to ?? [],
      cc: template.cc ?? [],
      bcc: template.bcc ?? [],
      files_of_draft: template.filesOfDraft ?? undefined,
      remove_files: template.removeFiles ?? [],
    },
  });

export const removeTemplate = (id: number) => invoke("template_remove", { id });

/** Put a template's files (and the images in its text) on a draft, but those it has; the draft then. */
export const useTemplateOnDraft = (draftId: number, templateId: number) =>
  invoke<Draft>("draft_use_template", { draftId, templateId });

/** What a message has, to keep as a template: its subject, text (stored form), recipients, and the draft whose files go with it. */
export interface Written {
  subject: string;
  html: string;
  to: Person[];
  cc: Person[];
  bcc: Person[];
  draftId: number;
}

/** Keep what is written as a new template named `name`, its recipients and files with it. */
export const saveWritten = (name: string, written: Written) =>
  saveTemplate({
    id: null,
    name,
    subject: written.subject,
    body: "",
    html: written.html,
    to: written.to,
    cc: written.cc,
    bcc: written.bcc,
    filesOfDraft: written.draftId,
  });
