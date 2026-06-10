import type { ReactElement, SVGProps } from 'react'

// Лёгкий inline-SVG Icon без сторонних библиотек. Заменяет текстовые
// плейсхолдеры («X», «>», «<», «v»/«^») на аккуратные векторные иконки.
// Декоративные по умолчанию (aria-hidden) — добавьте title/aria-label у кнопки.
export type IconName = 'close' | 'chevron-right' | 'chevron-left' | 'chevron-down' | 'check'

type IconProps = Omit<SVGProps<SVGSVGElement>, 'name'> & {
  name: IconName
  size?: number
}

const PATHS: Record<IconName, ReactElement> = {
  close: <path d="M5 5l10 10M15 5L5 15" />,
  'chevron-right': <path d="M7 4l6 6-6 6" />,
  'chevron-left': <path d="M13 4l-6 6 6 6" />,
  'chevron-down': <path d="M4 7l6 6 6-6" />,
  check: <path d="M4 10l4 4 8-9" />,
}

export default function Icon({ name, size = 16, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth={2}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      {PATHS[name]}
    </svg>
  )
}
