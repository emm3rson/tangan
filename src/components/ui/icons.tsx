import type { ReactNode, SVGProps } from 'react'

type IconProps = SVGProps<SVGSVGElement> & { size?: number }

function Base({ size = 18, children, ...props }: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      {...props}
    >
      {children}
    </svg>
  )
}

export const ConvertIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M4 8h11M4 8l3-3M4 8l3 3M20 16H9M20 16l-3-3M20 16l-3 3" />
  </Base>
)

export const TraceIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M4 19 9 5l4 10 3-7 4 11" />
    <circle cx="4" cy="19" r="1" fill="currentColor" />
    <circle cx="9" cy="5" r="1" fill="currentColor" />
    <circle cx="13" cy="15" r="1" fill="currentColor" />
    <circle cx="16" cy="8" r="1" fill="currentColor" />
    <circle cx="20" cy="19" r="1" fill="currentColor" />
  </Base>
)

export const CompressIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M12 3v6M12 9l-2.5-2.5M12 9l2.5-2.5M12 21v-6M12 15l-2.5 2.5M12 15l2.5 2.5M4 12h16" />
  </Base>
)

export const LogoPackIcon = (p: IconProps) => (
  <Base {...p}>
    <rect x="3" y="3" width="8" height="8" rx="1.5" />
    <rect x="14" y="3" width="7" height="7" rx="3.5" />
    <rect x="3" y="14" width="7" height="7" rx="1.5" />
    <path d="M14 14.5h7M14 18h7M14 21h4" />
  </Base>
)

export const DocumentIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
    <path d="M14 2v6h6" />
    <path d="M16 13H8M16 17H8M10 9H8" />
  </Base>
)


export const UploadIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M12 15V4M12 4 8 8M12 4l4 4M4 15v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3" />
  </Base>
)

export const FolderIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" />
  </Base>
)

export const ImageIcon = (p: IconProps) => (
  <Base {...p}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <circle cx="8.5" cy="9.5" r="1.5" />
    <path d="m4 17 4.5-4.5a2 2 0 0 1 2.8 0L20 21" />
  </Base>
)

export const VideoIcon = (p: IconProps) => (
  <Base {...p}>
    <rect x="2.5" y="5" width="19" height="14" rx="2.5" />
    <path d="m10 9 5 3-5 3z" />
  </Base>
)

export const PaletteIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M12 2a9.6 9.6 0 0 0 0 19.2c.9 0 1.4-.5 1.4-1.2 0-.8-.6-1.3-.6-2.1 0-.8.7-1.5 1.6-1.5h2a3 3 0 0 0 3-3A10.7 10.7 0 0 0 12 2Z" />
    <circle cx="7.2" cy="10" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="10.6" cy="6.6" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="15.6" cy="7.2" r="1.1" fill="currentColor" stroke="none" />
    <circle cx="17.4" cy="12" r="1.1" fill="currentColor" stroke="none" />
  </Base>
)

export const ArrowUp = (p: IconProps) => (
  <Base {...p}>
    <path d="M12 19V5M12 5l-6 6M12 5l6 6" />
  </Base>
)

export const ArrowDown = (p: IconProps) => (
  <Base {...p}>
    <path d="M12 5v14M12 19l-6-6M12 19l6-6" />
  </Base>
)

export const XIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M6 6l12 12M18 6 6 18" />
  </Base>
)

export const LockIcon = (p: IconProps) => (
  <Base {...p}>
    <rect x="3" y="11" width="18" height="11" rx="2" />
    <path d="M7 11V7a5 5 0 0 1 10 0v4" />
  </Base>
)

export const UnlockIcon = (p: IconProps) => (
  <Base {...p}>
    <rect x="3" y="11" width="18" height="11" rx="2" />
    <path d="M7 11V7a5 5 0 0 1 9.9-1" />
  </Base>
)

export const ChevronDown = (p: IconProps) => (
  <Base {...p}>
    <path d="m6 9 6 6 6-6" />
  </Base>
)

export const ArrowLeft = (p: IconProps) => (
  <Base {...p}>
    <path d="M19 12H5M5 12l6-6M5 12l6 6" />
  </Base>
)

export const CheckIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M4 12.5 9 17.5 20 6.5" />
  </Base>
)

export const AlertIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M12 3.5 22 20H2L12 3.5ZM12 10v4.5M12 17.5h.01" />
  </Base>
)

export const CopyIcon = (p: IconProps) => (
  <Base {...p}>
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M5 15V6a2 2 0 0 1 2-2h9" />
  </Base>
)

export const GearIcon = (p: IconProps) => (
  <Base {...p}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 13a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 4.6 9a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 0 4h-.09A1.65 1.65 0 0 0 19.4 13Z" />
  </Base>
)

export const SunIcon = (p: IconProps) => (
  <Base {...p}>
    <circle cx="12" cy="12" r="4" />
    <path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M19.1 4.9l-1.4 1.4M6.3 17.7l-1.4 1.4" />
  </Base>
)

export const MoonIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M20 13.5A8 8 0 1 1 10.5 4a6.5 6.5 0 0 0 9.5 9.5Z" />
  </Base>
)

export const MonitorIcon = (p: IconProps) => (
  <Base {...p}>
    <rect x="3" y="4" width="18" height="12" rx="2" />
    <path d="M8 20h8M12 16v4" />
  </Base>
)

export const RotateCcwIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
    <path d="M3 3v5h5" />
  </Base>
)

export const PlusIcon = (p: IconProps) => (
  <Base {...p}>
    <path d="M12 5v14M5 12h14" />
  </Base>
)

export const SpinnerIcon = (p: IconProps) => (
  <svg
    width={p.size ?? 16}
    height={p.size ?? 16}
    viewBox="0 0 24 24"
    fill="none"
    className={p.className}
  >
    <circle cx="12" cy="12" r="9" stroke="currentColor" strokeWidth="2.2" opacity=".2" />
    <path d="M21 12a9 9 0 0 0-9-9" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" />
  </svg>
)
