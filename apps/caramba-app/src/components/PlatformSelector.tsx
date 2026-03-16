import type { PlatformKey } from '../data/appDirectory'
import { platformTabs } from '../lib/connectionGuides'

type PlatformSelectorProps = {
  value: PlatformKey
  onChange: (platform: PlatformKey) => void
}

export default function PlatformSelector({
  value,
  onChange,
}: PlatformSelectorProps) {
  return (
    <div className="platform-selector" role="tablist" aria-label="Выбор платформы">
      {platformTabs.map((platform) => (
        <button
          key={platform.id}
          type="button"
          role="tab"
          aria-selected={value === platform.id}
          className={`platform-chip${value === platform.id ? ' active' : ''}`}
          onClick={() => onChange(platform.id)}
        >
          {platform.label}
        </button>
      ))}
    </div>
  )
}
