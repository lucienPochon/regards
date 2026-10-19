import { invoke } from "@tauri-apps/api/core";

/** Text to start a message with (plan §5.9), kept on this device for every account. */
export interface Template {
  id: number;
  name: string;
  subject: string;
  /** The text; with `html`, its plain-text version. */
  body: string;
  /** The formatted version, sanitised. */
  html: string | null;
  updated_at: number;
}

/** By name. */
export const listTemplates = () =>
  invoke<{ templates: Template[] }>("template_list").then((result) => result.templates);

/** Add one (no `id`) or change one. With `html`, the daemon sanitises it and derives the text. */
export const saveTemplate = (template: { id: number | null; name: string; subject: string; body: string; html: string | null }) =>
  invoke<Template>("template_save", {
    template: { ...template, id: template.id ?? undefined, html: template.html ?? undefined },
  });

export const removeTemplate = (id: number) => invoke("template_remove", { id });
