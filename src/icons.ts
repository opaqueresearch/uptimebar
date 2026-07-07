// Shared inline SVG icons (Lucide, MIT) + an icon-button helper, used by both the
// settings and popover webviews. Inlined so there's no dependency or network fetch;
// 16×16, currentColor stroke — they inherit the button's text color. The
// `.btn-icon` CSS is global (single styles.css), so classes work across both pages.

const svg = (paths: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="16" height="16" viewBox="0 0 24 24" ` +
  `fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" ` +
  `stroke-linejoin="round" aria-hidden="true">${paths}</svg>`;

export const ICONS = {
  // refresh-cw
  test: svg(
    `<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/>` +
      `<path d="M21 3v5h-5"/>` +
      `<path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/>` +
      `<path d="M8 16H3v5"/>`,
  ),
  // pencil
  edit: svg(
    `<path d="M21.174 6.812a1 1 0 0 0-3.986-3.986L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>` +
      `<path d="m15 5 4 4"/>`,
  ),
  // trash-2
  remove: svg(
    `<path d="M3 6h18"/>` +
      `<path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/>` +
      `<path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>` +
      `<line x1="10" x2="10" y1="11" y2="17"/>` +
      `<line x1="14" x2="14" y1="11" y2="17"/>`,
  ),
  // pause
  pause: svg(`<rect x="14" y="4" width="4" height="16" rx="1"/><rect x="6" y="4" width="4" height="16" rx="1"/>`),
  // play (resume)
  play: svg(`<path d="M6 3v18l15-9L6 3z"/>`),
  // bell-off (mute)
  mute: svg(
    `<path d="M8.7 3A6 6 0 0 1 18 8c0 2.5.5 4 1 5"/>` +
      `<path d="M6 8a6 6 0 0 0-.4 2.1c-.2 2.3-1 4-2.6 5.9h13"/>` +
      `<path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>` +
      `<line x1="2" x2="22" y1="2" y2="22"/>`,
  ),
  // bell (unmute)
  unmute: svg(
    `<path d="M10.268 21a2 2 0 0 0 3.464 0"/>` +
      `<path d="M3.262 15.326A1 1 0 0 0 4 17h16a1 1 0 0 0 .74-1.673C19.41 13.956 18 12.499 18 8A6 6 0 0 0 6 8c0 4.499-1.411 5.956-2.738 7.326"/>`,
  ),
};

/// Icon-only button with a hover tooltip (the SVG carries no text, so the title +
/// aria-label keep it labeled/accessible).
export function mkIcon(icon: string, title: string, cls = ""): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = `btn btn-icon btn-sm ${cls}`.trim();
  b.innerHTML = icon;
  b.title = title;
  b.setAttribute("aria-label", title);
  return b;
}
