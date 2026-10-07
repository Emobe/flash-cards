/// <reference types="bun" />
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "vitest";

// Vitest empties CSS imports (even `?raw`), so read the file directly.
const css = readFileSync(join(import.meta.dirname, "tokens.css"), "utf8");

type Palette = Record<string, string>;

function colours(block: string): Palette {
  const palette: Palette = {};
  for (const match of block.matchAll(/--([a-z-]+):\s*(#[0-9a-f]{6});/g)) {
    palette[match[1] as string] = match[2] as string;
  }
  return palette;
}

const darkMedia = css.indexOf("@media (prefers-color-scheme: dark)");
const darkOverride = css.indexOf(':root[data-theme="dark"]');
const motion = css.indexOf("@media (prefers-reduced-motion");

const light = colours(css.slice(0, darkMedia));
const darkSystem = colours(css.slice(darkMedia, darkOverride));
const dark = colours(css.slice(darkOverride, motion));

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((i) => {
    const value = Number.parseInt(hex.slice(i, i + 2), 16) / 255;
    return value <= 0.03928 ? value / 12.92 : ((value + 0.055) / 1.055) ** 2.4;
  });
  return (
    0.2126 * (channels[0] as number) +
    0.7152 * (channels[1] as number) +
    0.0722 * (channels[2] as number)
  );
}

function ratio(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

/** Foreground, background, minimum ratio. Text needs 4.5:1, controls and focus rings 3:1. */
const pairs: [string, string, number][] = [
  ["text", "bg", 4.5],
  ["text", "surface", 4.5],
  ["muted", "bg", 4.5],
  ["muted", "surface", 4.5],
  ["accent", "bg", 4.5],
  ["accent", "surface", 4.5],
  ["danger", "bg", 4.5],
  ["danger", "surface", 4.5],
  ["on-accent", "accent", 4.5],
  ["control", "bg", 3],
  ["control", "surface", 3],
];

for (const [name, palette] of [
  ["light", light],
  ["dark", dark],
] as const) {
  test(`${name} palette meets WCAG contrast`, () => {
    const failures: string[] = [];
    for (const [fg, bg, min] of pairs) {
      const a = palette[fg];
      const b = palette[bg];
      if (!a || !b) {
        failures.push(`${fg} or ${bg} is missing`);
        continue;
      }
      const value = ratio(a, b);
      if (value < min)
        failures.push(`${fg} ${a} on ${bg} ${b} is ${value.toFixed(2)}:1, needs ${min}:1`);
    }
    expect(failures).toEqual([]);
  });
}

test("the dark palette for the system setting matches the explicit dark override", () => {
  expect(darkSystem).toEqual(dark);
});

test("every light token has a dark value", () => {
  expect(Object.keys(dark).sort()).toEqual(Object.keys(light).sort());
});
