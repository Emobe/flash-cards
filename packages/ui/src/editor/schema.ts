import {
  type DOMOutputSpec,
  type MarkSpec,
  type MarkType,
  type NodeSpec,
  type NodeType,
  Schema,
} from "prosemirror-model";

/**
 * What a field may hold (ADR 0011 decision 1). The schema is the allowlist: anything it has no
 * rule for is dropped when HTML is loaded or pasted, so nothing else can reach the editor.
 */

/** A media name made by the core is `<stem>-<hash>.<ext>`. Anything with a path or URL in it is not one. */
export function isPlainMediaName(name: string): boolean {
  return name.length > 0 && !/[:/\\?#<>"[\]]/.test(name);
}

const bold: MarkSpec = {
  parseDOM: [
    { tag: "b", getAttrs: (node) => (node.style.fontWeight === "normal" ? false : null) },
    { tag: "strong" },
  ],
  toDOM: (): DOMOutputSpec => ["b", 0],
};

const italic: MarkSpec = {
  parseDOM: [{ tag: "i" }, { tag: "em" }],
  toDOM: (): DOMOutputSpec => ["i", 0],
};

const nodes: Record<string, NodeSpec> = {
  doc: { content: "(paragraph | bullet_list | ordered_list)+" },
  paragraph: {
    content: "inline*",
    group: "block",
    parseDOM: [{ tag: "p" }],
    toDOM: (): DOMOutputSpec => ["p", 0],
  },
  text: { group: "inline" },
  hard_break: {
    inline: true,
    group: "inline",
    selectable: false,
    parseDOM: [{ tag: "br" }],
    toDOM: (): DOMOutputSpec => ["br"],
  },
  bullet_list: {
    content: "list_item+",
    group: "block",
    parseDOM: [{ tag: "ul" }],
    toDOM: (): DOMOutputSpec => ["ul", 0],
  },
  ordered_list: {
    content: "list_item+",
    group: "block",
    parseDOM: [{ tag: "ol" }],
    toDOM: (): DOMOutputSpec => ["ol", 0],
  },
  list_item: {
    content: "paragraph (bullet_list | ordered_list)*",
    defining: true,
    parseDOM: [{ tag: "li" }],
    toDOM: (): DOMOutputSpec => ["li", 0],
  },
  // Until 2.4c gives them real views, media shows as a label. The field's own `src` is never put
  // in the page, so the editor cannot fetch anything.
  image: {
    inline: true,
    group: "inline",
    atom: true,
    attrs: { name: {} },
    parseDOM: [
      {
        tag: "img[src]",
        getAttrs: (node) => {
          const name = node.getAttribute("src") ?? "";
          return isPlainMediaName(name) ? { name } : false;
        },
      },
    ],
    toDOM: (node): DOMOutputSpec => [
      "span",
      { class: "pm-media", "data-media": String(node.attrs.name), title: String(node.attrs.name) },
      "Image",
    ],
  },
  sound: {
    inline: true,
    group: "inline",
    atom: true,
    attrs: { name: {} },
    parseDOM: [
      {
        tag: "span[data-sound]",
        getAttrs: (node) => {
          const name = node.getAttribute("data-sound") ?? "";
          return isPlainMediaName(name) ? { name } : false;
        },
      },
    ],
    toDOM: (node): DOMOutputSpec => [
      "span",
      { class: "pm-media", "data-media": String(node.attrs.name), title: String(node.attrs.name) },
      `Sound ${node.attrs.name}`,
    ],
  },
};

export const schema = new Schema({ nodes, marks: { bold, italic } });

function node(name: string): NodeType {
  const type = schema.nodes[name];
  if (!type) throw new Error(`The schema has no node ${name}`);
  return type;
}

function mark(name: string): MarkType {
  const type = schema.marks[name];
  if (!type) throw new Error(`The schema has no mark ${name}`);
  return type;
}

/** The schema's types, checked once, so no caller has to handle a missing one. */
export const types = {
  paragraph: node("paragraph"),
  hardBreak: node("hard_break"),
  bulletList: node("bullet_list"),
  orderedList: node("ordered_list"),
  listItem: node("list_item"),
  image: node("image"),
  sound: node("sound"),
  bold: mark("bold"),
  italic: mark("italic"),
};
