import { type Node, DOMParser as ProseMirrorParser } from "prosemirror-model";
import { isPlainMediaName, schema, types } from "./schema";

/**
 * Field HTML in and out of the editor (ADR 0011 decision 1). Field HTML is parsed with the
 * browser's `DOMParser`, which makes an inert document (scripts and handlers do not run), and then
 * with the schema's parser, which drops everything the schema has no rule for. Nothing from a field
 * is ever put into the page as HTML.
 */

const SOUND = /\[sound:([^\][<>]+)\]/g;
const parser = ProseMirrorParser.fromSchema(schema);

function escapeText(text: string): string {
  return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** `[sound:name]` is text in a field. Turn it into an element the schema can parse. */
function withSoundElements(html: string): string {
  return html.replace(SOUND, (whole, raw: string) => {
    const name = decodeEntities(raw);
    return isPlainMediaName(name) ? `<span data-sound="${escapeText(name)}"></span>` : whole;
  });
}

function decodeEntities(text: string): string {
  return text
    .replace(/&nbsp;/g, " ")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#0*39;|&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

/** The editor's document for a field's HTML. */
export function loadField(html: string): Node {
  const parsed = new DOMParser().parseFromString(withSoundElements(html), "text/html");
  return parser.parse(parsed.body, { preserveWhitespace: true });
}

function inline(node: Node): string {
  let out = "";
  node.forEach((child) => {
    if (child.type === types.hardBreak) {
      out += "<br>";
    } else if (child.type === types.image) {
      out += `<img src="${escapeText(String(child.attrs.name)).replace(/"/g, "&quot;")}">`;
    } else if (child.type === types.sound) {
      out += `[sound:${escapeText(String(child.attrs.name))}]`;
    } else {
      let text = escapeText(child.text ?? "");
      // Bold outside italic, always, so the same text always gives the same HTML.
      if (child.marks.some((m) => m.type === types.italic)) text = `<i>${text}</i>`;
      if (child.marks.some((m) => m.type === types.bold)) text = `<b>${text}</b>`;
      out += text;
    }
  });
  return out;
}

function list(node: Node): string {
  const tag = node.type === types.orderedList ? "ol" : "ul";
  let items = "";
  node.forEach((item) => {
    let content = "";
    item.forEach((part) => {
      content += part.type === types.paragraph ? inline(part) : list(part);
    });
    items += `<li>${content}</li>`;
  });
  return `<${tag}>${items}</${tag}>`;
}

/**
 * The HTML to store. A field with no list is its inline HTML with no wrapper, so typing `kot`
 * stores `kot`. Paragraphs (from a paste) are joined with `<br>`. A line break left at the end
 * is dropped.
 */
export function saveField(doc: Node): string {
  let out = "";
  let previousWasParagraph = false;
  doc.forEach((block) => {
    if (block.type === types.paragraph) {
      if (previousWasParagraph) out += "<br>";
      out += inline(block);
      previousWasParagraph = true;
    } else {
      out += list(block);
      previousWasParagraph = false;
    }
  });
  return out.replace(/(<br>)+$/, "");
}

/** What is compared when asking whether the editor can show a field without changing it. */
function normalise(html: string): string {
  let text = decodeEntities(html)
    .replace(/<(\/?)strong>/gi, "<$1b>")
    .replace(/<(\/?)em>/gi, "<$1i>")
    .replace(/<br\s*\/?>/gi, "<br>")
    .replace(/\s+/g, " ")
    .trim();
  // Two bold runs side by side are one run.
  for (let previous = ""; previous !== text; ) {
    previous = text;
    text = text.replace(/<\/b><b>/g, "").replace(/<\/i><i>/g, "");
  }
  return text.replace(/(<br>)+$/, "");
}

/**
 * Whether the editor shows `html` without changing it. When it does not, opening the field in the
 * editor would lose formatting, so Phase 3.2 shows a plain HTML box instead (ADR 0011).
 */
export function roundTrips(html: string): boolean {
  return normalise(saveField(loadField(html))) === normalise(html);
}
