import { useEffect, useState } from "react";

/** The width where the layout stops being compact, the same value as `shell.css`. */
export const WIDE_QUERY = "(min-width: 600px)";

function matches(query: string): boolean {
  return typeof window.matchMedia === "function" && window.matchMedia(query).matches;
}

/** Whether a media query matches, kept live. Layout follows the window width, never the platform. */
export function useMediaQuery(query: string): boolean {
  const [value, setValue] = useState(() => matches(query));
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const list = window.matchMedia(query);
    const onChange = () => setValue(list.matches);
    onChange();
    list.addEventListener("change", onChange);
    return () => list.removeEventListener("change", onChange);
  }, [query]);
  return value;
}

/** 600 px and over: the rail, and screens put their controls above the content (ADR 0010). */
export function useWideLayout(): boolean {
  return useMediaQuery(WIDE_QUERY);
}
