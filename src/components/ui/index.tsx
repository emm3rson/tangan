import type { ButtonHTMLAttributes, ReactNode } from 'react'

export const cx = (...parts: Array<string | false | null | undefined>) =>
  parts.filter(Boolean).join(' ')

interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: 'primary' | 'secondary' | 'ghost' | 'danger'
  size?: 'sm' | 'md'
  icon?: ReactNode
}

export function Button({
  variant = 'secondary',
  size = 'md',
  icon,
  className,
  children,
  ...rest
}: ButtonProps) {
  const variants = {
    primary:
      'bg-primary text-primary-foreground hover:opacity-90 active:opacity-95 shadow-sm',
    secondary:
      'bg-elevated text-foreground border border-border-strong hover:bg-muted active:bg-muted/80',
    ghost:
      'text-muted-foreground hover:text-foreground hover:bg-muted active:bg-muted/80',
    danger:
      'bg-danger-surface text-danger border border-danger/25 hover:brightness-[.97] active:brightness-95',
  }

  return (
    <button
      className={cx(
        'inline-flex items-center justify-center gap-2 font-medium rounded-[var(--radius-sm)] transition-all duration-150 select-none outline-none focus-visible:ring-2 focus-visible:ring-ring/55 focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:opacity-40 disabled:pointer-events-none active:translate-y-px',
        size === 'sm' ? 'h-8 px-3 text-[12px]' : 'h-9.5 px-4 text-[13px]',
        variants[variant],
        className
      )}
      {...rest}
    >
      {icon}
      {children}
    </button>
  )
}

export function Segmented<T extends string>({
  value,
  onChange,
  options,
  ariaLabel,
}: {
  value: T
  onChange: (value: T) => void
  options: { value: T; label: string }[]
  ariaLabel?: string
}) {
  return (
    <div
      role={ariaLabel ? 'group' : undefined}
      aria-label={ariaLabel}
      className="inline-flex rounded-[var(--radius-sm)] bg-muted p-0.5 gap-0.5"
    >
      {options.map((option) => (
        <button
          key={option.value}
          type="button"
          aria-pressed={option.value === value}
          onClick={() => onChange(option.value)}
          className={cx(
            'h-8 px-3.5 text-[12px] font-medium rounded-[5px] transition-colors outline-none focus-visible:ring-2 focus-visible:ring-ring/50',
            option.value === value
              ? 'bg-elevated text-foreground shadow-sm'
              : 'text-muted-foreground hover:text-foreground'
          )}
        >
          {option.label}
        </button>
      ))}
    </div>
  )
}

export function Slider({
  value,
  onChange,
  min = 0,
  max = 100,
}: {
  value: number
  onChange: (value: number) => void
  min?: number
  max?: number
}) {
  const pct = ((value - min) / (max - min)) * 100

  return (
    <input
      aria-label="Quality"
      type="range"
      min={min}
      max={max}
      value={value}
      onChange={(event) => onChange(Number(event.target.value))}
      className="w-full h-5 appearance-none bg-transparent cursor-pointer [&::-webkit-slider-runnable-track]:h-1.5 [&::-webkit-slider-runnable-track]:rounded-full [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:-mt-[5px] [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-elevated [&::-webkit-slider-thumb]:border [&::-webkit-slider-thumb]:border-border-strong [&::-webkit-slider-thumb]:shadow-sm"
      style={{
        WebkitAppearance: 'none',
        background: `linear-gradient(to right, var(--foreground) ${pct}%, var(--muted) ${pct}%) center/100% 6px no-repeat`,
        borderRadius: 999,
      }}
    />
  )
}

export function StepSlider<T extends string>({
  value,
  onChange,
  options,
  ariaLabel = 'Level',
}: {
  value: T
  onChange: (value: T) => void
  options: Array<{ value: T; label: string }>
  ariaLabel?: string
}) {
  const currentIndex = options.findIndex((opt) => opt.value === value)
  const safeIndex = currentIndex === -1 ? 0 : currentIndex
  const max = Math.max(0, options.length - 1)
  const pct = max > 0 ? (safeIndex / max) * 100 : 0

  return (
    <input
      aria-label={ariaLabel}
      type="range"
      min={0}
      max={max}
      step={1}
      value={safeIndex}
      onChange={(event) => {
        const nextIndex = Number(event.target.value)
        const nextOption = options[nextIndex]
        if (nextOption) {
          onChange(nextOption.value)
        }
      }}
      className="w-full h-5 appearance-none bg-transparent cursor-pointer [&::-webkit-slider-runnable-track]:h-1.5 [&::-webkit-slider-runnable-track]:rounded-full [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:-mt-[5px] [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-elevated [&::-webkit-slider-thumb]:border [&::-webkit-slider-thumb]:border-border-strong [&::-webkit-slider-thumb]:shadow-sm"
      style={{
        WebkitAppearance: 'none',
        background: `linear-gradient(to right, var(--foreground) ${pct}%, var(--muted) ${pct}%) center/100% 6px no-repeat`,
        borderRadius: 999,
      }}
    />
  )
}

export function Checkbox({
  checked,
  onChange,
}: {
  checked: boolean
  onChange: (checked: boolean) => void
}) {
  return (
    <button
      type="button"
      role="checkbox"
      aria-checked={checked}
      onClick={() => onChange(!checked)}
      className={cx(
        'h-[18px] w-[18px] shrink-0 rounded-[5px] border transition-colors grid place-items-center outline-none focus-visible:ring-2 focus-visible:ring-ring/50',
        checked
          ? 'bg-primary border-primary text-primary-foreground'
          : 'bg-elevated border-border-strong hover:border-foreground/40'
      )}
    >
      {checked && (
        <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
          <path
            d="M2.5 6.2 4.8 8.5 9.5 3.5"
            stroke="currentColor"
            strokeWidth="1.8"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
        </svg>
      )}
    </button>
  )
}

export function ProgressBar({ value }: { value: number }) {
  return (
    <div className="h-1.5 w-full rounded-full bg-muted overflow-hidden">
      <div
        className="h-full rounded-full bg-foreground transition-[width] duration-300 ease-out"
        style={{ width: `${Math.max(0, Math.min(100, value))}%` }}
      />
    </div>
  )
}
