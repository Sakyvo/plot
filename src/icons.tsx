// Hand-drawn 24px stroke icons — no icon library, keeps the exe lean.
import type { ReactNode } from "react";

const stroke = {
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.7,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

function Svg({ d, children }: { d?: string; children?: ReactNode }) {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" {...stroke} aria-hidden="true">
      {d && <path d={d} />}
      {children}
    </svg>
  );
}

export function GlobeIcon() {
  return (
    <Svg d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18">
      <circle cx="12" cy="12" r="9" />
    </Svg>
  );
}

export function FolderIcon() {
  return <Svg d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />;
}

export function RefreshIcon() {
  return <Svg d="M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6" />;
}

export function LocateIcon() {
  return (
    <Svg d="M12 2v3M12 19v3M2 12h3M19 12h3">
      <circle cx="12" cy="12" r="6.5" />
      <circle cx="12" cy="12" r="1" />
    </Svg>
  );
}

export function MoonIcon() {
  return <Svg d="M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5z" />;
}
