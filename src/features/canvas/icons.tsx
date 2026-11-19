// Small line icons of the blocks, drawn in the colour of the text around them.

import type { ReactNode } from "react";
import type { NodeType } from "../../types/scenario";

const SHAPES: Record<NodeType, ReactNode> = {
  start: <path d="M5 3.5v9l7-4.5z" />,
  intercept: <path d="M9 1.5 3.5 9H8l-1 5.5L12.5 7H8z" />,
  click: (
    <>
      <path d="M4 2.5v9l2.6-2.2 1.7 3.9 1.6-.7-1.7-3.8 3.3-.3z" />
    </>
  ),
  move: (
    <>
      <path d="M8 1.5v13M1.5 8h13" />
      <path d="m6 3.5 2-2 2 2M6 12.5l2 2 2-2M3.5 6l-2 2 2 2M12.5 6l2 2-2 2" />
    </>
  ),
  drag: (
    <>
      <path d="M2.5 12.5h3v-3" />
      <path d="M2.5 12.5 9 6" />
      <rect x="9" y="2.5" width="4.5" height="4.5" rx="1" />
    </>
  ),
  scroll: (
    <>
      <rect x="4.5" y="1.5" width="7" height="13" rx="3.5" />
      <path d="M8 4.5v3" />
    </>
  ),
  key: (
    <>
      <rect x="1.5" y="3.5" width="13" height="9" rx="1.5" />
      <path d="M4.5 6.5h1M7.5 6.5h1M10.5 6.5h1M5 9.5h6" />
    </>
  ),
  text: <path d="M3 4V3h10v1M8 3v10M6 13h4" />,
  pause: <path d="M5.5 3v10M10.5 3v10" />,
  stop: <rect x="3.5" y="3.5" width="9" height="9" rx="1" />,
  wait: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 4.5V8l2.5 1.5" />
    </>
  ),
  branch: (
    <>
      <path d="M8 1.5 14.5 8 8 14.5 1.5 8z" />
      <path d="M6.5 6.5a1.6 1.6 0 1 1 2.3 1.4c-.5.3-.8.6-.8 1.1M8 11v.2" />
    </>
  ),
  counter: (
    <>
      <path d="M13 8a5 5 0 1 1-1.6-3.7" />
      <path d="M13.5 2v3h-3" />
    </>
  ),
  call: (
    <>
      <rect x="2" y="2.5" width="8" height="8" rx="1" />
      <path d="M6 13.5h8v-8M8.5 11 14 5.5" />
    </>
  ),
};

export function BlockIcon({ type }: { type: NodeType }) {
  return (
    <svg
      className="block-icon"
      viewBox="0 0 16 16"
      width="16"
      height="16"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.4"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {SHAPES[type]}
    </svg>
  );
}
