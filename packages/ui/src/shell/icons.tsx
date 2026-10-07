import type { ReactNode } from "react";

/**
 * Icons drawn for the Paper direction (ADR 0010): 24 px grid, 1.75 stroke, round caps and joins,
 * `currentColor`. They are decoration: the navigation always shows a text label too.
 */
function Svg({ children }: { children: ReactNode }) {
  return (
    <svg
      className="icon"
      viewBox="0 0 24 24"
      width="24"
      height="24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.75"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {children}
    </svg>
  );
}

export function DecksIcon() {
  return (
    <Svg>
      <path d="M7 5h10" />
      <rect x="4" y="8" width="16" height="11" rx="2.5" />
    </Svg>
  );
}

export function AddIcon() {
  return (
    <Svg>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 8v8M8 12h8" />
    </Svg>
  );
}

export function BrowseIcon() {
  return (
    <Svg>
      <circle cx="11" cy="11" r="6" />
      <path d="M15.5 15.5 20 20" />
    </Svg>
  );
}

export function SettingsIcon() {
  return (
    <Svg>
      <path d="M4 7h8M18 7h2M4 17h2M12 17h8" />
      <circle cx="15" cy="7" r="2.5" />
      <circle cx="9" cy="17" r="2.5" />
    </Svg>
  );
}

export function BackIcon() {
  return (
    <Svg>
      <path d="m15 5-7 7 7 7" />
    </Svg>
  );
}

export function DeveloperIcon() {
  return (
    <Svg>
      <path d="m9 8-4 4 4 4M15 8l4 4-4 4" />
    </Svg>
  );
}

export function WarningIcon() {
  return (
    <Svg>
      <path d="M12 4 21 20H3z" />
      <path d="M12 10v4M12 17.25v.01" />
    </Svg>
  );
}

export function EmptyIcon() {
  return (
    <Svg>
      <rect x="4" y="6" width="16" height="12" rx="2.5" />
      <path d="M8 11h8M8 14h5" />
    </Svg>
  );
}
