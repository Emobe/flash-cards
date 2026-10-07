import { describe, expect, test } from "vitest";
import { highestCloze } from "./commands";
import { loadField, roundTrips, saveField } from "./html";

const through = (html: string) => saveField(loadField(html));

describe("loadField and saveField", () => {
  test.each([
    ["", ""],
    ["kot", "kot"],
    ["a <b>bold</b> and <i>italic</i> word", "a <b>bold</b> and <i>italic</i> word"],
    ["<b><i>both</i></b>", "<b><i>both</i></b>"],
    ["line one<br>line two", "line one<br>line two"],
    ["Tom &amp; Jerry &lt;3", "Tom &amp; Jerry &lt;3"],
    ["{{c1::Warszawa::city}} is big", "{{c1::Warszawa::city}} is big"],
    [
      'see <img src="cat-0123456789abcdef.jpg"> here',
      'see <img src="cat-0123456789abcdef.jpg"> here',
    ],
    ["hear [sound:meow-0123456789abcdef.mp3]", "hear [sound:meow-0123456789abcdef.mp3]"],
    ["<ul><li>a</li><li>b</li></ul>", "<ul><li>a</li><li>b</li></ul>"],
    ["<ol><li>one</li></ol>", "<ol><li>one</li></ol>"],
    ["intro<ul><li>a <b>b</b></li></ul>", "intro<ul><li>a <b>b</b></li></ul>"],
    ["<ul><li>a<ul><li>nested</li></ul></li></ul>", "<ul><li>a<ul><li>nested</li></ul></li></ul>"],
  ])("%s", (html, expected) => {
    expect(through(html)).toBe(expected);
  });

  test("strong and em are read as b and i", () => {
    expect(through("<strong>a</strong> <em>b</em>")).toBe("<b>a</b> <i>b</i>");
  });

  test("list items lose the paragraph the schema puts inside them", () => {
    expect(through("<ul><li><p>a</p></li></ul>")).toBe("<ul><li>a</li></ul>");
  });

  test("two paragraphs are joined with a line break, and a trailing break is dropped", () => {
    expect(through("<p>one</p><p>two</p>")).toBe("one<br>two");
    expect(through("one<br>")).toBe("one");
  });

  test("whitespace between tags in a list does not make text", () => {
    expect(through("<ul>\n  <li>a</li>\n  <li>b</li>\n</ul>")).toBe(
      "<ul><li>a</li><li>b</li></ul>",
    );
  });

  test("what the schema has no rule for is dropped", () => {
    expect(through("a<script>alert(1)</script>b")).toBe("ab");
    expect(through('<iframe src="about:blank"></iframe>text')).toBe("text");
    expect(through("<style>p{color:red}</style>text")).toBe("text");
    expect(through('<span style="color:red" onclick="x()">red</span>')).toBe("red");
    expect(through("<u>under</u>")).toBe("under");
    expect(through('<a href="https://x">link</a>')).toBe("link");
  });

  test("an image whose source is not a plain media name is dropped", () => {
    expect(through('<img src="https://example.com/a.png">')).toBe("");
    expect(through('<img src="data:image/png;base64,AAAA">')).toBe("");
    expect(through('<img src="../a.png">')).toBe("");
    expect(through('<img src="a.png?x=1">')).toBe("");
    expect(through('<img src="javascript:alert(1)">')).toBe("");
  });

  test("a handler on an image does not survive", () => {
    expect(through('<img src="a.png" onerror="alert(1)">')).toBe('<img src="a.png">');
  });

  test("a sound with a path in it stays as plain text", () => {
    expect(through("[sound:../x.mp3]")).toBe("[sound:../x.mp3]");
  });

  test("loading never runs anything", () => {
    (window as unknown as { hit?: boolean }).hit = false;
    loadField('<img src="x" onerror="window.hit=true"><script>window.hit=true</script>');
    expect((window as unknown as { hit?: boolean }).hit).toBe(false);
  });

  test("an unknown tag keeps its text", () => {
    expect(through("<custom-tag>text</custom-tag>")).toBe("text");
  });
});

describe("roundTrips", () => {
  test.each([
    "",
    "kot",
    "<b>a</b> and <i>b</i>",
    "<strong>a</strong>",
    "one<br>two",
    "one<br />two",
    "<ul><li>a</li></ul>",
    "[sound:a-0123456789abcdef.mp3]",
    '<img src="a-0123456789abcdef.jpg">',
    "{{c1::x}}",
    "a<b>b</b><b>c</b>",
    "Tom &amp; Jerry",
    "a&nbsp;b",
    "  spaced   out ",
  ])("%j can be shown as it is", (html) => {
    expect(roundTrips(html)).toBe(true);
  });

  test.each([
    "<u>underlined</u>",
    '<span style="color:red">red</span>',
    '<a href="https://x">link</a>',
    "<table><tr><td>x</td></tr></table>",
    '<img src="https://example.com/a.png">',
    "<script>x</script>",
    "<sub>2</sub>",
  ])("%j cannot", (html) => {
    expect(roundTrips(html)).toBe(false);
  });
});

describe("highestCloze", () => {
  test("is 0 with no cloze", () => {
    expect(highestCloze([])).toBe(0);
    expect(highestCloze(["plain", "{{not a cloze}}"])).toBe(0);
  });

  test("finds the highest across every field", () => {
    expect(highestCloze(["{{c1::a}} {{c3::b}}", "{{c2::c}}"])).toBe(3);
    expect(highestCloze(["x", "{{c12::y::hint}}", "{{c2::z}}"])).toBe(12);
  });

  test("sees a cloze inside formatting", () => {
    expect(highestCloze(["<b>{{c4::bold}}</b>"])).toBe(4);
  });
});
