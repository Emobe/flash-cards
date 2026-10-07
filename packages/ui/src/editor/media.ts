/**
 * Getting a picked image or sound ready to store (ADR 0011 decision 3). Nothing here talks to the
 * core: it turns a `File` into a name and bytes, or refuses it with a sentence the person can act
 * on.
 */

export const MAX_BYTES = 20 * 1024 * 1024;
export const MAX_SIDE = 1600;
const JPEG_QUALITY = 0.85;

/** A file that cannot be used. `message` is for the person. */
export class MediaRefused extends Error {}

export type Prepared = {
  /** The name to give `addMedia`: the original stem with the extension of what is stored. */
  name: string;
  bytes: Uint8Array;
  /** A content type for showing it. */
  type: string;
  /** Something the person should know, when the file was stored anyway. */
  warning?: string;
};

/** A decoded picture, opaque to everything but the codec that made it. */
export type Decoded = { width: number; height: number; source: unknown };

/** What `prepareImage` needs from the browser, so a test can supply its own. */
export type ImageCodec = {
  /** Null when the browser cannot decode the file. Applies the EXIF orientation. */
  decode(file: Blob): Promise<Decoded | null>;
  encode(
    picture: Decoded,
    width: number,
    height: number,
    type: "image/jpeg" | "image/png",
    quality: number,
  ): Promise<Blob>;
};

export const browserCodec: ImageCodec = {
  async decode(file) {
    try {
      const bitmap = await createImageBitmap(file, { imageOrientation: "from-image" });
      return { width: bitmap.width, height: bitmap.height, source: bitmap };
    } catch {
      return null;
    }
  },
  async encode(picture, width, height, type, quality) {
    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;
    const context = canvas.getContext("2d");
    if (!context) throw new MediaRefused("This picture could not be prepared. Try another one.");
    context.imageSmoothingQuality = "high";
    context.drawImage(picture.source as ImageBitmap, 0, 0, width, height);
    (picture.source as ImageBitmap).close?.();
    const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, type, quality));
    if (!blob) throw new MediaRefused("This picture could not be prepared. Try another one.");
    return blob;
  },
};

const TOO_BIG = "This file is larger than 20 MB. Pick a smaller one.";
const CANNOT_SHOW = "This picture's format can't be shown on cards. Try a JPEG or PNG.";

function split(name: string): { stem: string; extension: string } {
  const base = name.split(/[\\/]/).pop() ?? name;
  const dot = base.lastIndexOf(".");
  return dot > 0
    ? { stem: base.slice(0, dot), extension: base.slice(dot + 1).toLowerCase() }
    : { stem: base, extension: "" };
}

async function bytesOf(blob: Blob): Promise<Uint8Array> {
  return new Uint8Array(await blob.arrayBuffer());
}

const IMAGE_TYPES: Record<string, string> = {
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  png: "image/png",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
};

function imageType(file: File): string {
  const { extension } = split(file.name);
  // A picker may give no type, or a wrong one, so the extension counts when the type is not an image.
  return file.type.startsWith("image/") ? file.type : (IMAGE_TYPES[extension] ?? file.type);
}

/**
 * JPEG (every camera photo) is always re-encoded, at most 1600 px on the longest side, which also
 * drops EXIF data including the location. PNG is resized only when it is larger than that. GIF,
 * WebP and SVG are stored as they are. Anything else the browser can decode is stored as a PNG;
 * anything it cannot is refused.
 */
export async function prepareImage(
  file: File,
  codec: ImageCodec = browserCodec,
): Promise<Prepared> {
  if (file.size > MAX_BYTES) throw new MediaRefused(TOO_BIG);
  if (file.size === 0) throw new MediaRefused("The file is empty. Pick a different one.");
  const { stem } = split(file.name);
  const type = imageType(file);

  if (type === "image/gif" || type === "image/webp" || type === "image/svg+xml") {
    const { extension } = split(file.name);
    const known = Object.entries(IMAGE_TYPES).find(([, t]) => t === type)?.[0] ?? extension;
    return { name: `${stem}.${known}`, bytes: await bytesOf(file), type };
  }

  const picture = await codec.decode(file);
  if (!picture) throw new MediaRefused(CANNOT_SHOW);
  const scale = Math.min(1, MAX_SIDE / Math.max(picture.width, picture.height));
  const width = Math.max(1, Math.round(picture.width * scale));
  const height = Math.max(1, Math.round(picture.height * scale));

  if (type === "image/jpeg") {
    const out = await codec.encode(picture, width, height, "image/jpeg", JPEG_QUALITY);
    return { name: `${stem}.jpg`, bytes: await bytesOf(out), type: "image/jpeg" };
  }
  if (type === "image/png" && scale === 1) {
    return { name: `${stem}.png`, bytes: await bytesOf(file), type: "image/png" };
  }
  const out = await codec.encode(picture, width, height, "image/png", 1);
  return { name: `${stem}.png`, bytes: await bytesOf(out), type: "image/png" };
}

const SOUND_TYPES: Record<string, string> = {
  mp3: "audio/mpeg",
  wav: "audio/wav",
  ogg: "audio/ogg",
  oga: "audio/ogg",
  opus: "audio/ogg",
  m4a: "audio/mp4",
  aac: "audio/aac",
  flac: "audio/flac",
};

/** Whether this browser says it can play the type (`""` means no). */
export function browserCanPlay(type: string): boolean {
  return document.createElement("audio").canPlayType(type) !== "";
}

/**
 * A sound is stored as it is. One this browser cannot play is still stored, with a warning,
 * because the card frame's engine may differ from the editor's.
 */
export async function prepareSound(
  file: File,
  canPlay: (type: string) => boolean = browserCanPlay,
): Promise<Prepared> {
  if (file.size > MAX_BYTES) throw new MediaRefused(TOO_BIG);
  if (file.size === 0) throw new MediaRefused("The file is empty. Pick a different one.");
  const { stem, extension } = split(file.name);
  const type = file.type.startsWith("audio/") ? file.type : (SOUND_TYPES[extension] ?? "");
  if (!type) {
    throw new MediaRefused(
      "This does not look like a sound file. Try an MP3, M4A, OGG or WAV file.",
    );
  }
  const prepared: Prepared = {
    name: extension ? `${stem}.${extension}` : stem,
    bytes: await bytesOf(file),
    type,
  };
  if (!canPlay(type)) {
    prepared.warning =
      "This sound was added, but this device may not be able to play it. Check it on a card.";
  }
  return prepared;
}
