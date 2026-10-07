import { describe, expect, test } from "vitest";
import {
  type Decoded,
  type ImageCodec,
  MAX_BYTES,
  MediaRefused,
  prepareImage,
  prepareSound,
} from "./media";

/** happy-dom has no canvas, so the codec is ours; the real one is run in a browser. */
function codec(width: number, height: number, decodes = true) {
  const encoded: { width: number; height: number; type: string; quality: number }[] = [];
  const fake: ImageCodec = {
    async decode(): Promise<Decoded | null> {
      return decodes ? { width, height, source: null } : null;
    },
    async encode(_picture, w, h, type, quality) {
      encoded.push({ width: w, height: h, type, quality });
      return new Blob([`encoded ${w}x${h} ${type}`], { type });
    },
  };
  return { fake, encoded };
}

const file = (name: string, type: string, content: string | number = "x") =>
  new File([typeof content === "number" ? new Uint8Array(content) : content], name, { type });

const text = (bytes: Uint8Array) => new TextDecoder().decode(bytes);

describe("prepareImage", () => {
  test("a JPEG is always re-encoded as a JPEG at 0.85, with the long side at most 1600 px", async () => {
    const { fake, encoded } = codec(4000, 3000);
    const out = await prepareImage(file("IMG_1.jpeg", "image/jpeg"), fake);
    expect(encoded).toEqual([{ width: 1600, height: 1200, type: "image/jpeg", quality: 0.85 }]);
    expect(out.name).toBe("IMG_1.jpg");
    expect(out.type).toBe("image/jpeg");
    expect(text(out.bytes)).toBe("encoded 1600x1200 image/jpeg");
  });

  test("a small JPEG is still re-encoded (that is what drops EXIF), without being enlarged", async () => {
    const { fake, encoded } = codec(40, 20);
    await prepareImage(file("a.jpg", "image/jpeg"), fake);
    expect(encoded).toEqual([{ width: 40, height: 20, type: "image/jpeg", quality: 0.85 }]);
  });

  test("a tall JPEG is limited by its height", async () => {
    const { fake, encoded } = codec(1000, 3200);
    await prepareImage(file("a.jpg", "image/jpeg"), fake);
    expect(encoded[0]).toMatchObject({ width: 500, height: 1600 });
  });

  test("a small PNG is stored as it is", async () => {
    const { fake, encoded } = codec(800, 600);
    const out = await prepareImage(file("map.png", "image/png", "png bytes"), fake);
    expect(encoded).toEqual([]);
    expect(out.name).toBe("map.png");
    expect(text(out.bytes)).toBe("png bytes");
  });

  test("a large PNG is resized and stays a PNG", async () => {
    const { fake, encoded } = codec(3200, 1600);
    const out = await prepareImage(file("map.png", "image/png"), fake);
    expect(encoded).toEqual([{ width: 1600, height: 800, type: "image/png", quality: 1 }]);
    expect(out.type).toBe("image/png");
  });

  test.each([
    ["anim.gif", "image/gif"],
    ["pic.webp", "image/webp"],
    ["logo.svg", "image/svg+xml"],
  ])("%s is stored as it is, without decoding it", async (name, type) => {
    const { fake, encoded } = codec(1, 1, false);
    const out = await prepareImage(file(name, type, "raw"), fake);
    expect(encoded).toEqual([]);
    expect(out.name).toBe(name);
    expect(text(out.bytes)).toBe("raw");
  });

  test("the extension decides when the picker gives no type", async () => {
    const { fake, encoded } = codec(5000, 100);
    const out = await prepareImage(file("camera.JPG", ""), fake);
    expect(encoded[0]).toMatchObject({ type: "image/jpeg", width: 1600 });
    expect(out.name).toBe("camera.jpg");
  });

  test("a file the browser cannot decode is refused with what to do", async () => {
    const { fake } = codec(1, 1, false);
    await expect(prepareImage(file("IMG.heic", "image/heic"), fake)).rejects.toThrow(
      "This picture's format can't be shown on cards. Try a JPEG or PNG.",
    );
  });

  test("another format the browser can decode is stored as a PNG", async () => {
    const { fake, encoded } = codec(300, 200);
    const out = await prepareImage(file("scan.bmp", "image/bmp"), fake);
    expect(encoded).toEqual([{ width: 300, height: 200, type: "image/png", quality: 1 }]);
    expect(out.name).toBe("scan.png");
  });

  test("over 20 MB is refused before anything is decoded, and 20 MB is allowed", async () => {
    let decoded = false;
    const watching: ImageCodec = {
      decode: async () => {
        decoded = true;
        return { width: 1, height: 1, source: null };
      },
      encode: async () => new Blob(["x"]),
    };
    const big = file("big.png", "image/png", MAX_BYTES + 1);
    await expect(prepareImage(big, watching)).rejects.toThrow(
      "This file is larger than 20 MB. Pick a smaller one.",
    );
    expect(decoded).toBe(false);
    await expect(
      prepareImage(file("ok.png", "image/png", MAX_BYTES), watching),
    ).resolves.toBeTruthy();
  });

  test("an empty file is refused", async () => {
    await expect(
      prepareImage(file("a.png", "image/png", ""), codec(1, 1).fake),
    ).rejects.toBeInstanceOf(MediaRefused);
  });

  test("a name with a path keeps only its stem", async () => {
    const out = await prepareImage(file("C:\\pics\\cat.png", "image/png"), codec(10, 10).fake);
    expect(out.name).toBe("cat.png");
  });
});

describe("prepareSound", () => {
  test("is stored as it is, under its own name", async () => {
    const out = await prepareSound(file("meow.mp3", "audio/mpeg", "sound"), () => true);
    expect(out).toMatchObject({ name: "meow.mp3", type: "audio/mpeg" });
    expect(text(out.bytes)).toBe("sound");
    expect(out.warning).toBeUndefined();
  });

  test("the extension decides when the picker gives no type", async () => {
    const out = await prepareSound(file("clip.M4A", ""), () => true);
    expect(out).toMatchObject({ name: "clip.m4a", type: "audio/mp4" });
  });

  test("one this device cannot play is kept, with a warning", async () => {
    const out = await prepareSound(file("a.flac", "audio/flac"), () => false);
    expect(out.warning).toContain("may not be able to play");
    expect(out.bytes.length).toBeGreaterThan(0);
  });

  test("a file that is not audio is refused", async () => {
    await expect(prepareSound(file("notes.txt", "text/plain"), () => true)).rejects.toThrow(
      "This does not look like a sound file",
    );
  });

  test("over 20 MB is refused", async () => {
    await expect(
      prepareSound(file("long.mp3", "audio/mpeg", MAX_BYTES + 1), () => true),
    ).rejects.toThrow("larger than 20 MB");
  });
});
