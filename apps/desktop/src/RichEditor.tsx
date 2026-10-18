import { Extension } from "@tiptap/core";
import { EditorContent, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useRef, useState } from "react";
// ProseMirror's base styles as files: the app's CSP allows no injected <style>.
import "prosemirror-view/style/prosemirror.css";
import "prosemirror-gapcursor/style/gapcursor.css";

/**
 * The message text, with formatting: bold, italic, underline, strikethrough,
 * links, lists and quotes (Tiptap, on ProseMirror). The daemon sanitises what
 * it receives, so this is about writing comfortably, not about safety.
 */
export function RichEditor({
  initial,
  onChange,
  onSend,
  disabled,
  autofocus,
}: {
  /** HTML to start from. */
  initial: string;
  onChange: (html: string) => void;
  onSend: () => void;
  disabled: boolean;
  autofocus: boolean;
}) {
  // The latest callbacks, for the editor's long-lived keyboard handler.
  const send = useRef(onSend);
  send.current = onSend;
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
      // Cmd/Ctrl+Enter sends, before the line break that would otherwise take it.
      Extension.create({
        name: "sendKey",
        priority: 1000,
        addKeyboardShortcuts() {
          return {
            "Mod-Enter": () => {
              send.current();
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
    editorProps: { attributes: { class: "compose-body", "aria-label": "Message", role: "textbox" } },
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

  if (!editor) return null;
  const chain = () => editor.chain().focus();
  const button = (label: string, pressed: boolean, run: () => void, title: string) => (
    <button type="button" aria-pressed={pressed} title={title} onClick={run} disabled={disabled}>
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
