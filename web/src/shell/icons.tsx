/*
 * A small local icon set. Paths are copied from Lucide (https://lucide.dev), used under the
 * ISC licence:
 *
 *   ISC License. Copyright (c) for portions of Lucide are held by Cole Bemis 2013-2022 as part
 *   of Feather (MIT). All other copyright (c) for Lucide are held by Lucide Contributors 2022.
 *   Permission to use, copy, modify, and/or distribute this software for any purpose with or
 *   without fee is hereby granted, provided that the above copyright notice and this
 *   permission notice appear in all copies. THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR
 *   DISCLAIMS ALL WARRANTIES WITH REGARD TO THIS SOFTWARE.
 *
 * `logo` and `codex` are simple shapes drawn here, not Lucide icons. Only the icons the UI
 * uses are included; add a new one by copying its paths from Lucide.
 */
import { PATHS_A } from './iconPathsA';
import { PATHS_B } from './iconPathsB';

const PATHS = { ...PATHS_A, ...PATHS_B };
export type IconName = keyof typeof PATHS;

/** An inline SVG icon. Decorative: the control around it carries the accessible name. */
export function Icon({ name, size = 16, className, strokeWidth = 2 }: { name: IconName; size?: number; className?: string; strokeWidth?: number }) {
  return (
    <svg
      className={`icon${className ? ` ${className}` : ''}`}
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      {PATHS[name]}
    </svg>
  );
}
