import { type Editor, Extension } from "@tiptap/core";
import Image from "@tiptap/extension-image";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useRef, useState } from "react";
import { IMAGE_TYPES } from "./images";
// ProseMirror's base styles as files: the app's CSP allows no injected <style>.
import "prosemirror-view/style/prosemirror.css";
import "prosemirror-gapcursor/style/gapcursor.css";

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
}).configure({ inline: true, allowBase64: true });

/**
 * The message text, with formatting: bold, italic, underline, strikethrough,
 * links, lists, quotes and images (Tiptap, on ProseMirror). The daemon
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
      // Cmd/Ctrl+Enter sends (with Shift, and Done), before the line break that would otherwise take it.
      Extension.create({
        name: "sendKey",
        priority: 1000,
        addKeyboardShortcuts() {
          return {
            "Mod-Enter": () => {
              send.current(false);
              return true;
            },
            "Mod-Shift-Enter": () => {
              send.current(true);
              return true;
            },
          };
        },
      }),
    ],
    content: initial,
    injectCSS: false,
    editable: !disabled,
    autofocus: autofocus ? "start" : false,
    editorProps: {
      attributes: { class: "compose-body", "aria-label": "Message", role: "textbox", "aria-multiline": "true" },
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
    }),
  });
  const [linking, setLinking] = useState<string | null>(null);
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
      <div className="format-bar small" role="toolbar" aria-label="Formatting">
        {button("B", active?.bold ?? false, () => chain().toggleBold().run(), "Bold")}
        {button("I", active?.italic ?? false, () => chain().toggleItalic().run(), "Italic")}
        {button("U", active?.underline ?? false, () => chain().toggleUnderline().run(), "Underline")}
        {button("S", active?.strike ?? false, () => chain().toggleStrike().run(), "Strikethrough")}
        {button(
          "Link",
          active?.link ?? false,
          () => setLinking(editor.getAttributes("link").href ?? ""),
          "Add or change a link",
        )}
        {button("• List", active?.bullets ?? false, () => chain().toggleBulletList().run(), "Bulleted list")}
        {button("1. List", active?.numbers ?? false, () => chain().toggleOrderedList().run(), "Numbered list")}
        {button("Quote", active?.quote ?? false, () => chain().toggleBlockquote().run(), "Quote")}
        {linking !== null && (
          <span className="link-input">
            <input
              autoFocus
              value={linking}
              placeholder="https://…"
              aria-label="Link address"
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
              {linking.trim() ? "Apply" : "Remove"}
            </button>
          </span>
        )}
      </div>
      <EditorContent editor={editor} />
    </div>
  );
}
