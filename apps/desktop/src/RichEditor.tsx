import { type Editor, Extension } from "@tiptap/core";
import Image from "@tiptap/extension-image";
import { Table, TableCell, TableHeader, TableRow } from "@tiptap/extension-table";
import { TextStyle } from "@tiptap/extension-text-style";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useRef, useState } from "react";
import { IMAGE_TYPES } from "./images";
import { commandOf } from "./shortcuts";
// ProseMirror's base styles as files: the app's CSP allows no injected <style>.
import "prosemirror-view/style/prosemirror.css";
import "prosemirror-gapcursor/style/gapcursor.css";
import { t } from "./i18n";

/** An image in the text, shown from its bytes, marked with its Content-ID (images.ts). */
const InlineImage = Image.extend({
  addAttributes() {
    return {
      ...this.parent?.(),
      cid: {
        default: null,
        parseHTML: (element) => element.getAttribute("data-cid"),
        renderHTML: (attributes) => (attributes.cid ? { "data-cid": attributes.cid } : {}),
      },
    };
  },
}).configure({
  inline: true,
  allowBase64: true,
  // Made smaller or larger from a corner, keeping its proportions.
  resize: {
    enabled: true,
    directions: ["top-left", "top-right", "bottom-left", "bottom-right"],
    minWidth: 32,
    minHeight: 32,
    alwaysPreserveAspectRatio: true,
  },
});

/**
 * Colour, highlight, font and size on a span of text. The app's CSP allows
 * no inline styles, so the editor marks them with `data-colour`,
 * `data-highlight`, `data-font` and `data-size` (shown by styles.css, for
 * those offered below); the HTML kept and sent has them as styles
 * (`toStoredHtml`).
 */
const Colours = TextStyle.extend({
  addAttributes() {
    const kept = (name: string) => ({
      default: null,
      parseHTML: (element: HTMLElement) => element.getAttribute(`data-${name}`),
      renderHTML: (attributes: Record<string, unknown>) =>
        attributes[name] ? { [`data-${name}`]: attributes[name] as string } : {},
    });
    return { ...this.parent?.(), colour: kept("colour"), highlight: kept("highlight"), font: kept("font"), size: kept("size") };
  },
});

declare module "@tiptap/core" {
  interface Commands<ReturnType> {
    indent: {
      /** One step further in (a list item: one level down). */
      indent: () => ReturnType;
      /** One step back out. */
      outdent: () => ReturnType;
    };
  }
}

/** Steps a paragraph or heading can be indented: each is 40 pixels in mail (`toStoredHtml`). */
export const MAX_INDENT = 4;

/** A paragraph's or heading's indent, marked `data-indent` (1 to 4) as colours are. */
const Indent = Extension.create({
  name: "indent",
  addGlobalAttributes() {
    return [
      {
        types: ["paragraph", "heading"],
        attributes: {
          indent: {
            default: 0,
            parseHTML: (element: HTMLElement) => Math.min(MAX_INDENT, Number(element.getAttribute("data-indent")) || 0),
            renderHTML: (attributes: Record<string, unknown>) =>
              attributes.indent ? { "data-indent": String(attributes.indent) } : {},
          },
        },
      },
    ];
  },
  addCommands() {
    const shift =
      (by: 1 | -1) =>
      () =>
      ({ state, tr, dispatch }: { state: Editor["state"]; tr: Editor["state"]["tr"]; dispatch?: unknown }) => {
        let changed = false;
        state.doc.nodesBetween(state.selection.from, state.selection.to, (node, pos) => {
          if (node.type.name !== "paragraph" && node.type.name !== "heading") return;
          // In a list, items go a level down or up instead.
          if (state.doc.resolve(pos).parent.type.name === "listItem") return;
          const indent = Math.max(0, Math.min(MAX_INDENT, (Number(node.attrs.indent) || 0) + by));
          if (indent === node.attrs.indent) return;
          if (dispatch) tr.setNodeMarkup(pos, undefined, { ...node.attrs, indent });
          changed = true;
        });
        return changed;
      };
    return { indent: shift(1), outdent: shift(-1) };
  },
});

/** Fonts and sizes offered, as the daemon lets them through (`sanitize_outgoing`); none is the reader's own. */
const FONTS = [
  { value: "sans", label: t("Sans serif") },
  { value: "serif", label: t("Serif") },
  { value: "mono", label: t("Fixed width") },
];
const SIZES = [
  { value: "small", label: t("Small") },
  { value: "large", label: t("Large") },
  { value: "huge", label: t("Huge") },
];

/** Text colours and highlights offered, as the daemon lets them through (`sanitize_outgoing`). */
const TEXT_COLOURS = [
  { name: "Red", value: "#c62828" },
  { name: "Orange", value: "#e65100" },
  { name: "Green", value: "#2e7d32" },
  { name: "Blue", value: "#1565c0" },
  { name: "Purple", value: "#6a1b9a" },
  { name: "Grey", value: "#616161" },
];
const HIGHLIGHTS = [
  { name: "Yellow", value: "#fff59d" },
  { name: "Green", value: "#c8e6c9" },
  { name: "Blue", value: "#bbdefb" },
  { name: "Pink", value: "#f8bbd0" },
  { name: "Orange", value: "#ffe0b2" },
];

/**
 * The message text, with formatting: text styles and headings, bold,
 * italic, underline, strikethrough, colour and highlight, links, lists,
 * quotes and images that can be resized (Tiptap, on ProseMirror). The daemon
 * sanitises what it receives, so this is about writing comfortably, not
 * about safety.
 */
export function RichEditor({
  initial,
  onChange,
  onSend,
  onImages,
  onEditor,
  disabled,
  autofocus,
}: {
  /** HTML to start from. */
  initial: string;
  onChange: (html: string) => void;
  /** Ctrl+Enter sends; Ctrl+Shift+Enter sends and files the conversation as Done. */
  onSend: (done: boolean) => void;
  /** Images pasted: to store, then put in the text (`insertImage`). */
  onImages?: (files: File[]) => void;
  /** The editor, once there, for what the composer puts in it. */
  onEditor?: (editor: Editor) => void;
  disabled: boolean;
  autofocus: boolean;
}) {
  // The latest callbacks, for the editor's long-lived handlers.
  const send = useRef(onSend);
  send.current = onSend;
  const images = useRef(onImages);
  images.current = onImages;
  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        heading: { levels: [1, 2, 3] },
        codeBlock: false,
        link: {
          openOnClick: false,
          autolink: true,
          defaultProtocol: "https",
          protocols: ["http", "https", "mailto"],
        },
      }),
      InlineImage,
      Colours,
      Indent,
      // Simple tables: a header row, then rows; columns keep their own widths.
      Table.configure({ resizable: false }),
      TableRow,
      TableHeader,
      TableCell,
    ],
    content: initial,
    injectCSS: false,
    editable: !disabled,
    autofocus: autofocus ? "start" : false,
    editorProps: {
      // Send's keys (Cmd/Ctrl+Enter, with Shift and Done, unless changed) before the
      // editor's own: a line break would otherwise take Cmd/Ctrl+Enter.
      handleKeyDown: (_view, event) => {
        const command = commandOf(event, ["send", "sendDone"]);
        if (command === null) return false;
        send.current(command === "sendDone");
        return true;
      },
      attributes: { class: "compose-body", "aria-label": t("Message"), role: "textbox", "aria-multiline": "true" },
      // Pasted images are stored first: the editor shows them once they are.
      handlePaste: (_view, event) => {
        const files = [...(event.clipboardData?.files ?? [])].filter((file) => IMAGE_TYPES.includes(file.type));
        if (files.length === 0 || !images.current) return false;
        images.current(files);
        return true;
      },
    },
    onUpdate: ({ editor }) => onChange(editor.getHTML()),
  });
  const active = useEditorState({
    editor,
    selector: ({ editor }) => ({
      bold: editor?.isActive("bold") ?? false,
      italic: editor?.isActive("italic") ?? false,
      underline: editor?.isActive("underline") ?? false,
      strike: editor?.isActive("strike") ?? false,
      link: editor?.isActive("link") ?? false,
      bullets: editor?.isActive("bulletList") ?? false,
      numbers: editor?.isActive("orderedList") ?? false,
      quote: editor?.isActive("blockquote") ?? false,
      heading: ([1, 2, 3] as const).find((level) => editor?.isActive("heading", { level })) ?? 0,
      colour: (editor?.getAttributes("textStyle").colour as string | undefined) ?? null,
      highlight: (editor?.getAttributes("textStyle").highlight as string | undefined) ?? null,
      font: (editor?.getAttributes("textStyle").font as string | undefined) ?? "",
      size: (editor?.getAttributes("textStyle").size as string | undefined) ?? "",
      table: editor?.isActive("table") ?? false,
      listItem: editor?.isActive("listItem") ?? false,
    }),
  });
  const [linking, setLinking] = useState<string | null>(null);
  // Which row of swatches is open, if one is.
  const [swatches, setSwatches] = useState<"colour" | "highlight" | null>(null);
  useEffect(() => {
    if (editor) onEditor?.(editor);
  }, [editor, onEditor]);

  if (!editor) return null;
  const chain = () => editor.chain().focus();
  const button = (label: string, pressed: boolean, run: () => void, title: string) => (
    <button type="button" aria-pressed={pressed} aria-label={title} title={title} onClick={run} disabled={disabled}>
      {label}
    </button>
  );
  const applyLink = () => {
    const url = (linking ?? "").trim();
    if (url) {
      chain().extendMarkRange("link").setLink({ href: /^[a-z]+:/i.test(url) ? url : `https://${url}` }).run();
    } else {
      chain().extendMarkRange("link").unsetLink().run();
    }
    setLinking(null);
  };

  return (
    <div className="rich-editor">
      <div className="format-bar small" role="toolbar" aria-label={t("Formatting")}>
        <select
          className="text-style"
          aria-label={t("Text style")}
          title={t("Text style")}
          disabled={disabled}
          value={active?.heading ?? 0}
          onChange={(event) => {
            const level = Number(event.target.value);
            if (level === 1 || level === 2 || level === 3) chain().setHeading({ level }).run();
            else chain().setParagraph().run();
          }}
        >
          <option value={0}>{t("Text")}</option>
          <option value={1}>{t("Title")}</option>
          <option value={2}>{t("Heading")}</option>
          <option value={3}>{t("Subheading")}</option>
        </select>
        <select
          className="text-font"
          aria-label={t("Font")}
          title={t("Font")}
          disabled={disabled}
          value={active?.font ?? ""}
          onChange={(event) => chain().setMark("textStyle", { font: event.target.value || null }).removeEmptyTextStyle().run()}
        >
          <option value="">{t("Font")}</option>
          {FONTS.map((font) => (
            <option key={font.value} value={font.value}>
              {font.label}
            </option>
          ))}
        </select>
        <select
          className="text-size"
          aria-label={t("Text size")}
          title={t("Text size")}
          disabled={disabled}
          value={active?.size ?? ""}
          onChange={(event) => chain().setMark("textStyle", { size: event.target.value || null }).removeEmptyTextStyle().run()}
        >
          <option value="">{t("Normal")}</option>
          {SIZES.map((size) => (
            <option key={size.value} value={size.value}>
              {size.label}
            </option>
          ))}
        </select>
        {button("B", active?.bold ?? false, () => chain().toggleBold().run(), t("Bold"))}
        {button("I", active?.italic ?? false, () => chain().toggleItalic().run(), t("Italic"))}
        {button("U", active?.underline ?? false, () => chain().toggleUnderline().run(), t("Underline"))}
        {button("S", active?.strike ?? false, () => chain().toggleStrike().run(), t("Strikethrough"))}
        <button
          type="button"
          className="colour-button"
          aria-pressed={swatches === "colour"}
          aria-label={t("Text colour")}
          title={t("Text colour")}
          disabled={disabled}
          onClick={() => setSwatches(swatches === "colour" ? null : "colour")}
        >
          {t("A")}<span className="colour-bar" style={{ background: active?.colour ?? "currentColor" }} />
        </button>
        <button
          type="button"
          className="colour-button"
          aria-pressed={swatches === "highlight"}
          aria-label={t("Highlight")}
          title={t("Highlight")}
          disabled={disabled}
          onClick={() => setSwatches(swatches === "highlight" ? null : "highlight")}
        >
          <span className="highlight-mark" style={active?.highlight ? { background: active.highlight } : undefined}>
            ab
          </span>
        </button>
        {swatches !== null && (
          // One row of colours; picking one (or none) closes it.
          <span
            className="swatches"
            role="group"
            aria-label={swatches === "colour" ? t("Text colours") : t("Highlights")}
            onKeyDown={(event) => {
              if (event.key === "Escape") {
                event.preventDefault();
                event.stopPropagation();
                setSwatches(null);
              }
            }}
          >
            {(swatches === "colour" ? TEXT_COLOURS : HIGHLIGHTS).map((swatch) => (
              <button
                key={swatch.value}
                type="button"
                className="swatch"
                aria-label={swatch.name}
                title={swatch.name}
                aria-pressed={(swatches === "colour" ? active?.colour : active?.highlight) === swatch.value}
                style={{ background: swatch.value }}
                onClick={() => {
                  chain().setMark("textStyle", { [swatches]: swatch.value }).run();
                  setSwatches(null);
                }}
              />
            ))}
            <button
              type="button"
              onClick={() => {
                chain().setMark("textStyle", { [swatches]: null }).removeEmptyTextStyle().run();
                setSwatches(null);
              }}
            >
              {swatches === "colour" ? t("Default") : t("None")}
            </button>
          </span>
        )}
        {button(
          t("Link"),
          active?.link ?? false,
          () => setLinking(editor.getAttributes("link").href ?? ""),
          t("Add or change a link"),
        )}
        {button(t("• List"), active?.bullets ?? false, () => chain().toggleBulletList().run(), t("Bulleted list"))}
        {button(t("1. List"), active?.numbers ?? false, () => chain().toggleOrderedList().run(), t("Numbered list"))}
        {button(t("Quote"), active?.quote ?? false, () => chain().toggleBlockquote().run(), t("Quote"))}
        {button("⇤", false, () => (active?.listItem ? chain().liftListItem("listItem").run() : chain().outdent().run()), t("Indent less"))}
        {button("⇥", false, () => (active?.listItem ? chain().sinkListItem("listItem").run() : chain().indent().run()), t("Indent more"))}
        {button(t("Table"), active?.table ?? false, () => chain().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(), t("Insert a table"))}
        {button(t("Clear"), false, () => chain().unsetAllMarks().clearNodes().run(), t("Clear formatting"))}
        {active?.table && (
          <span className="table-tools" role="group" aria-label={t("Table")}>
            {button(t("+ Row"), false, () => chain().addRowAfter().run(), t("Add a row below"))}
            {button(t("+ Column"), false, () => chain().addColumnAfter().run(), t("Add a column to the right"))}
            {button(t("− Row"), false, () => chain().deleteRow().run(), t("Delete this row"))}
            {button(t("− Column"), false, () => chain().deleteColumn().run(), t("Delete this column"))}
            {button(t("No table"), false, () => chain().deleteTable().run(), t("Delete the table"))}
          </span>
        )}
        {linking !== null && (
          <span className="link-input">
            <input
              autoFocus
              value={linking}
              placeholder="https://…"
              aria-label={t("Link address")}
              onChange={(event) => setLinking(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  applyLink();
                } else if (event.key === "Escape") {
                  event.preventDefault();
                  event.stopPropagation();
                  setLinking(null);
                }
              }}
            />
            <button type="button" onClick={applyLink}>
              {linking.trim() ? t("Apply") : t("Remove")}
            </button>
          </span>
        )}
      </div>
      <EditorContent editor={editor} />
    </div>
  );
}
