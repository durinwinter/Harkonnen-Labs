import { useEffect, useState } from 'react';

function initialsForCard(card) {
  return (card?.name || '?')
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map(part => part[0])
    .join('')
    .toUpperCase();
}

export default function ActionCardTile({ card, variant = 'support' }) {
  if (!card) return null;

  const [imageFailed, setImageFailed] = useState(false);

  useEffect(() => {
    setImageFailed(false);
  }, [card?.id, card?.image]);

  const planned = card.status !== 'ready';
  const hasImage = Boolean(card.image) && !imageFailed;

  return (
    <article className={`act-card ${variant} ${planned ? 'planned' : 'ready'}`} style={{ '--act-accent': card.accent || '#c2a372' }}>
      <div className="act-card-art">
        {hasImage ? (
          <img src={card.image} alt={card.title || card.name} className="act-card-image" onError={() => setImageFailed(true)} />
        ) : (
          <div className="act-card-placeholder">
            <div className="act-card-initials">{initialsForCard(card)}</div>
            <div className="act-card-placeholder-text">Card incoming</div>
          </div>
        )}
        <div className="act-card-badge">{planned ? 'Planned' : 'Ready'}</div>
      </div>
      <div className="act-card-body">
        <div className="act-card-name">{card.name}</div>
        <div className="act-card-title">{card.title}</div>
        {card.subtitle && <div className="act-card-subtitle">{card.subtitle}</div>}
      </div>

      <style jsx>{`
        .act-card {
          display: flex;
          flex-direction: column;
          min-width: 0;
          border-radius: 16px;
          overflow: hidden;
          border: 1px solid rgba(255, 255, 255, 0.08);
          background: linear-gradient(180deg, rgba(255, 255, 255, 0.05), rgba(10, 14, 22, 0.82));
          box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.02);
        }
        .act-card.ready {
          box-shadow: 0 18px 40px rgba(0, 0, 0, 0.22), inset 0 0 0 1px rgba(255, 255, 255, 0.03);
        }
        .act-card.planned {
          border-style: dashed;
        }
        .act-card-art {
          position: relative;
          aspect-ratio: ${variant === 'hero' ? '4 / 5' : variant === 'mode' ? '4 / 5' : '2 / 3'};
          background:
            radial-gradient(circle at top, color-mix(in srgb, var(--act-accent) 32%, transparent), transparent 56%),
            linear-gradient(180deg, rgba(255, 255, 255, 0.04), rgba(0, 0, 0, 0.18));
        }
        .act-card-image {
          display: block;
          width: 100%;
          height: 100%;
          object-fit: cover;
          object-position: center top;
        }
        .act-card-placeholder {
          display: flex;
          flex-direction: column;
          align-items: center;
          justify-content: center;
          width: 100%;
          height: 100%;
          padding: 1rem;
          gap: 0.55rem;
          text-align: center;
        }
        .act-card-initials {
          width: 4rem;
          height: 4rem;
          border-radius: 999px;
          display: inline-flex;
          align-items: center;
          justify-content: center;
          font-size: 1.2rem;
          font-weight: 800;
          color: #fff;
          background: color-mix(in srgb, var(--act-accent) 80%, #111 20%);
          box-shadow: 0 0 30px color-mix(in srgb, var(--act-accent) 35%, transparent);
        }
        .act-card-placeholder-text {
          color: rgba(255, 255, 255, 0.72);
          font-size: 0.78rem;
          letter-spacing: 0.04em;
          text-transform: uppercase;
        }
        .act-card-badge {
          position: absolute;
          top: 0.65rem;
          right: 0.65rem;
          border-radius: 999px;
          padding: 0.25rem 0.55rem;
          font-size: 0.68rem;
          font-weight: 800;
          letter-spacing: 0.08em;
          text-transform: uppercase;
          color: #111;
          background: color-mix(in srgb, var(--act-accent) 72%, #fff 28%);
        }
        .act-card-body {
          display: flex;
          flex-direction: column;
          gap: 0.28rem;
          padding: 0.72rem 0.8rem 0.82rem;
        }
        .act-card-name {
          color: var(--act-accent);
          text-transform: uppercase;
          letter-spacing: 0.12em;
          font-size: 0.68rem;
          font-weight: 800;
        }
        .act-card-title {
          color: #fff;
          font-size: ${variant === 'hero' ? '1.02rem' : '0.86rem'};
          font-weight: 700;
          line-height: 1.35;
        }
        .act-card-subtitle {
          color: rgba(255, 255, 255, 0.72);
          font-size: 0.78rem;
          line-height: 1.45;
        }
      `}</style>
    </article>
  );
}
