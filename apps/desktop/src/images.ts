import { invoke } from "@tauri-apps/api/core";
import type { Editor } from "@tiptap/core";
import type { Draft } from "./compose";

/**
 * Images in a message's text (plan §5.9). The daemon keeps each as an
 * attachment with a Content-ID, and the draft's HTML refers to it as
 * `<img src="cid:…">`; the message carries it with the HTML. The editor
 * shows the bytes (`data:`), marking each image with its Content-ID, and
 * the two forms are turned into each other on opening and saving.
 */

/** What the text can carry; other files go beside it. */
export const IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"];

/** Put an image (pasted, say) in a draft's text: its Content-ID, and the draft. */
export const attachImage = (draftId: number, image: { name: string; mime: string; data: string }) =>
  invoke<{ content_id: string; draft: Draft }>("draft_attach_image", { draftId, ...image });

export interface DraftImage {
  content_id: string;
  mime: string;
  /** The bytes, base64. */
  data: string;
}

/** The images in a draft's text, to show while writing. */
export const draftImages = (draftId: number) =>
  invoke<{ images: DraftImage[] }>("draft_images", { draftId }).then((result) => result.images);

/** Put an image in the text where the cursor is, shown from its bytes. */
export function insertImage(editor: Editor, image: DraftImage) {
  const src = `data:${image.mime};base64,${image.data}`;
  editor.chain().focus().insertContent({ type: "image", attrs: { src, cid: image.content_id, alt: "" } }).run();
}

/** A file's bytes as base64. */
export function fileBase64(file: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result).split(",", 2)[1] ?? "");
    reader.onerror = () => reject(reader.error ?? new Error("the image could not be read"));
    reader.readAsDataURL(file);
  });
}

const IMG = /<img\b[^>]*>/g;
const attribute = (tag: string, name: string) => new RegExp(`\\b${name}="([^"]*)"`).exec(tag)?.[1];

/** The editor's HTML as it is kept: its images by Content-ID, without their bytes. */
export function toStoredHtml(html: string): string {
  return html.replace(IMG, (tag) => {
    const cid = attribute(tag, "data-cid");
    // Not one of the message's own: sending leaves it out.
    if (!cid) return tag;
    return `<img src="cid:${cid}" alt="${attribute(tag, "alt") ?? ""}">`;
  });
}

/** Kept HTML for the editor: each of its images shown from its bytes. */
export function toEditorHtml(html: string, images: DraftImage[]): string {
  const bytes = new Map(images.map((image) => [image.content_id, `data:${image.mime};base64,${image.data}`]));
  return html.replace(IMG, (tag) => {
    const cid = /\bsrc="cid:([^"]+)"/.exec(tag)?.[1];
    if (!cid) return tag;
    return `<img src="${bytes.get(cid) ?? ""}" alt="${attribute(tag, "alt") ?? ""}" data-cid="${cid}">`;
  });
}
