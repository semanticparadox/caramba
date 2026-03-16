import type { AppDirectoryEntry } from '../data/appDirectory'

type AppDirectoryCardProps = {
  entry: AppDirectoryEntry
  onGuide: (entry: AppDirectoryEntry) => void
}

export default function AppDirectoryCard({
  entry,
  onGuide,
}: AppDirectoryCardProps) {
  return (
    <article className="guide-app-card glass-card">
      <div className="guide-app-head">
        <div>
          <h4>{entry.name}</h4>
          <p>{entry.description}</p>
        </div>
        {entry.badge && <span className="badge badge-proto">{entry.badge}</span>}
      </div>

      <div className="guide-app-meta">
        <span className={`guide-confidence ${entry.confidence}`}>{entry.confidence}</span>
      </div>

      <div className="guide-app-actions">
        <a
          href={entry.officialUrl}
          target="_blank"
          rel="noreferrer"
          className="btn-primary guide-action"
        >
          Скачать
        </a>
        <button
          type="button"
          className="btn-secondary guide-action"
          onClick={() => onGuide(entry)}
        >
          Гид
        </button>
      </div>
    </article>
  )
}
