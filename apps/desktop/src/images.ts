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
const SPAN = /<span\b[^>]*>/g;
const attribute = (tag: string, name: string) => new RegExp(`(?:^|\\s)${name}="([^"]*)"`).exec(tag)?.[1];
const HEX = /^#[0-9a-f]{6}$/i;

/** A resized image's width and height, as whole pixels (the editor may give fractions). */
function size(tag: string): string {
  return ["width", "height"]
    .map((name) => [name, Math.round(Number(attribute(tag, name)))] as const)
    .filter(([, value]) => Number.isFinite(value) && value > 0)
    .map(([name, value]) => ` ${name}="${value}"`)
    .join("");
}

/**
 * The editor's HTML as it is kept: its images by Content-ID, without their
 * bytes, and its colours as styles (the editor marks them with `data-colour`
 * and `data-highlight`, its CSP allowing no inline styles).
 */
export function toStoredHtml(html: string): string {
  return coloursAsStyles(
    html.replace(IMG, (tag) => {
      const cid = attribute(tag, "data-cid");
      // Not one of the message's own: sending leaves it out.
      if (!cid) return tag;
      return `<img src="cid:${cid}" alt="${attribute(tag, "alt") ?? ""}"${size(tag)}>`;
    }),
  );
}

/** Kept HTML for the editor: each of its images shown from its bytes, its colours as the editor marks them. */
export function toEditorHtml(html: string, images: DraftImage[]): string {
  const bytes = new Map(images.map((image) => [image.content_id, `data:${image.mime};base64,${image.data}`]));
  return coloursAsMarks(
    html.replace(IMG, (tag) => {
      const cid = /\bsrc="cid:([^"]+)"/.exec(tag)?.[1];
      if (!cid) return tag;
      return `<img src="${bytes.get(cid) ?? ""}" alt="${attribute(tag, "alt") ?? ""}" data-cid="${cid}"${size(tag)}>`;
    }),
  );
}

/** The editor's colour marks (`data-colour`, `data-highlight`) as the styles mail carries. */
export function coloursAsStyles(html: string): string {
  return html.replace(SPAN, (tag) => {
    const styles = [
      ["color", attribute(tag, "data-colour")],
      ["background-color", attribute(tag, "data-highlight")],
    ].filter(([, value]) => value && HEX.test(value));
    return styles.length > 0 ? `<span style="${styles.map(([name, value]) => `${name}: ${value}`).join("; ")}">` : tag;
  });
}

/** Colour styles as the editor marks them. */
export function coloursAsMarks(html: string): string {
  return html.replace(SPAN, (tag) => {
    const style = attribute(tag, "style") ?? "";
    const colour = /(?:^|;)\s*color:\s*(#[0-9a-f]{6})/i.exec(style)?.[1];
    const highlight = /(?:^|;)\s*background-color:\s*(#[0-9a-f]{6})/i.exec(style)?.[1];
    if (!colour && !highlight) return tag;
    return `<span${colour ? ` data-colour="${colour}"` : ""}${highlight ? ` data-highlight="${highlight}"` : ""}>`;
  });
}

/**
 * A signature's pictures (embedded as `data:`) made the draft's own images,
 * as pasted ones are: each attached with a Content-ID and marked with it.
 */
export async function attachPictures(draftId: number, html: string): Promise<string> {
  let out = html;
  for (const [at, tag] of (html.match(IMG) ?? []).entries()) {
    if (attribute(tag, "data-cid")) continue;
    const found = /^data:(image\/(png|jpeg|gif|webp));base64,([A-Za-z0-9+/=]+)$/.exec(attribute(tag, "src") ?? "");
    if (!found) continue;
    const [, mime, kind, data] = found;
    const { content_id } = await attachImage(draftId, { name: `signature-${at + 1}.${kind}`, mime, data });
    const marked = `<img src="data:${mime};base64,${data}" alt="${attribute(tag, "alt") ?? ""}" data-cid="${content_id}"${size(tag)}>`;
    out = out.replace(tag, () => marked);
  }
  return out;
}
