import type { SVGProps } from "react";

export type IconName =
  | "archive"
  | "chevronDown"
  | "close"
  | "copy"
  | "library"
  | "menu"
  | "pause"
  | "play"
  | "plus"
  | "remove"
  | "recheck"
  | "restore"
  | "settings"
  | "transfers"
  | "workbench";

export interface IconProps extends Omit<SVGProps<SVGSVGElement>, "children"> {
  readonly name: IconName;
}

export function Icon({ name, ...properties }: IconProps) {
  return (
    <svg
      viewBox="0 0 20 20"
      width="1em"
      height="1em"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      focusable="false"
      aria-hidden="true"
      {...properties}
    >
      <IconPaths name={name} />
    </svg>
  );
}

function IconPaths({ name }: { readonly name: IconName }) {
  switch (name) {
    case "archive":
      return (
        <>
          <path d="M3.25 6.5h13.5v9.75H3.25z" />
          <path d="M2.5 3.75h15v2.75h-15zM7.5 10h5" />
        </>
      );
    case "chevronDown":
      return <path d="m5.5 7.75 4.5 4.5 4.5-4.5" />;
    case "close":
      return <path d="m5 5 10 10M15 5 5 15" />;
    case "copy":
      return (
        <>
          <rect x="6.25" y="6.25" width="9.5" height="9.5" rx="1.25" />
          <path d="M13.75 6.25v-1.5a.5.5 0 0 0-.5-.5h-8.5a.5.5 0 0 0-.5.5v8.5a.5.5 0 0 0 .5.5h1.5" />
        </>
      );
    case "library":
      return (
        <>
          <path d="M3.5 4.25h4v11.5h-4zM8.5 4.25h3.25v11.5H8.5z" />
          <path d="m13 5 3.5-.75 1.75 10.75-3.5.75z" />
        </>
      );
    case "menu":
      return <path d="M3.5 5.25h13M3.5 10h13M3.5 14.75h13" />;
    case "pause":
      return <path d="M7 5v10M13 5v10" />;
    case "play":
      return <path d="m7 4.75 7 5.25-7 5.25z" />;
    case "plus":
      return <path d="M10 4v12M4 10h12" />;
    case "remove":
      return (
        <>
          <path d="M4.75 6.25h10.5M8 3.75h4M6 6.25l.75 10h6.5l.75-10" />
          <path d="M8.25 9v4.5M11.75 9v4.5" />
        </>
      );
    case "recheck":
      return (
        <>
          <path d="M4.25 9.25a5.75 5.75 0 0 1 10.45-2.7" />
          <path d="M14.75 3.75v3.5h-3.5M15.75 10.75A5.75 5.75 0 0 1 5.3 13.45" />
          <path d="M5.25 16.25v-3.5h3.5" />
        </>
      );
    case "restore":
      return (
        <>
          <path d="M4.1 9a6 6 0 1 1 1.7 4.9" />
          <path d="M4 4.75V9h4.25" />
        </>
      );
    case "settings":
      return (
        <>
          <circle cx="10" cy="10" r="2.4" />
          <path d="M8.85 2.9h2.3l.45 1.75c.45.17.87.41 1.25.72l1.72-.5 1.16 2-1.29 1.25c.04.29.06.58.06.88s-.02.59-.06.88l1.29 1.25-1.16 2-1.72-.5c-.38.31-.8.55-1.25.72l-.45 1.75h-2.3l-.45-1.75a5.4 5.4 0 0 1-1.25-.72l-1.72.5-1.16-2 1.29-1.25A6 6 0 0 1 5.5 9c0-.3.02-.59.06-.88L4.27 6.87l1.16-2 1.72.5c.38-.31.8-.55 1.25-.72z" />
        </>
      );
    case "transfers":
      return (
        <>
          <path d="M6.25 3.5v12.25M3.5 13l2.75 2.75L9 13" />
          <path d="M13.75 16.5V4.25M11 7l2.75-2.75L16.5 7" />
        </>
      );
    case "workbench":
      return (
        <>
          <path d="M3.25 3.5h13.5v13H3.25zM3.25 8h13.5M9 8v8.5" />
        </>
      );
  }
}
