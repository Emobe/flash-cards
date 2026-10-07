import { useEffect, useState } from "react";

/**
 * How far the on-screen keyboard covers the bottom of the layout viewport, in px (ADR 0010
 * decision 4). The WebView (M139 and later), Chrome and Safari resize the visual viewport when
 * the keyboard opens and leave the layout viewport alone. Only the shell calls this.
 */
export function useKeyboardInset(): number {
  const [inset, setInset] = useState(0);

  useEffect(() => {
    const viewport = window.visualViewport;
    if (!viewport) return;
    const measure = () => {
      // Pinch zoom also shrinks the visual viewport. That is not a keyboard.
      if (viewport.scale > 1.01) return setInset(0);
      const covered = window.innerHeight - viewport.height - viewport.offsetTop;
      setInset(covered > 1 ? Math.round(covered) : 0);
    };
    measure();
    viewport.addEventListener("resize", measure);
    viewport.addEventListener("scroll", measure);
    return () => {
      viewport.removeEventListener("resize", measure);
      viewport.removeEventListener("scroll", measure);
    };
  }, []);

  return inset;
}
