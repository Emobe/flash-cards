import { expect, test } from "vitest";
import source from "./frame.html?raw";

// frame.html is one trusted inline script, so its pure CSS rewriter is cut out between markers and
// run here. The rest of the frame needs a real sandboxed iframe and is checked in browsers.
const block = /\/\/ <css-rewrite>([\s\S]*?)\/\/ <\/css-rewrite>/.exec(source);
if (!block?.[1]) throw new Error("frame.html has no css-rewrite block");
const rewriteCssUrls = new Function(`${block[1]}\nreturn rewriteCssUrls;`)() as (
  css: string,
  urlFor: (name: string) => string | undefined,
) => string;

const files: Record<string, string> = { "bg-1.png": "blob:A", "f-2.woff2": "blob:B" };
const urlFor = (name: string) => (Object.hasOwn(files, name) ? files[name] : undefined);
const rewrite = (css: string) => rewriteCssUrls(css, urlFor);

test("the three forms of url() are rewritten to the blob URL", () => {
  expect(rewrite("a{background:url(bg-1.png)}")).toBe('a{background:url("blob:A")}');
  expect(rewrite("a{background:url('bg-1.png')}")).toBe('a{background:url("blob:A")}');
  expect(rewrite('a{background:url("bg-1.png")}')).toBe('a{background:url("blob:A")}');
});

test("case, spaces and several urls are handled", () => {
  expect(rewrite("a{b:URL(  bg-1.png  ) Url( 'f-2.woff2' )}")).toBe(
    'a{b:url("blob:A") url("blob:B")}',
  );
});

test("a name the card was not given is left alone", () => {
  const css =
    "a{background:url(other.png) url(https://example.com/x.png) url(data:image/png;base64,AA==)}";
  expect(rewrite(css)).toBe(css);
});

test("names that are properties of every object are not files", () => {
  const css = "a{b:url(__proto__) url(constructor) url(toString) url(hasOwnProperty)}";
  expect(rewrite(css)).toBe(css);
});

test("unterminated, empty and odd input is copied and never throws", () => {
  for (const css of [
    "",
    "url(",
    'url("bg-1.png',
    "url('bg-1.png'",
    "url(bg-1.png",
    "url",
    "u",
    "url()",
    "url('')",
  ]) {
    expect(rewrite(css)).toBe(css);
  }
  // A quoted name must be closed by `)` too.
  expect(rewrite('url("bg-1.png" x)')).toBe('url("bg-1.png" x)');
});

test("a name does not run past the end of its url()", () => {
  expect(rewrite("url(bg-1.png) url(nope.png)")).toBe('url("blob:A") url(nope.png)');
  expect(rewrite("url(a) url(bg-1.png)")).toBe('url(a) url("blob:A")');
});

test("hostile CSS stays fast", () => {
  const css = "url(".repeat(200_000) + 'url("'.repeat(100_000);
  const start = performance.now();
  rewrite(css);
  expect(performance.now() - start).toBeLessThan(1500);
});
