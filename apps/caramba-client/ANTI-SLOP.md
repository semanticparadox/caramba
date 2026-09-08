# Anti-slop rules (design + writing)

Standing quality bar for Caramba. Applies to every UI and every visible string.
Sources: `Leonxlnx/taste-skill` (design), `conorbronsdon/avoid-ai-writing` and
`jalaalrd/anti-ai-slop-writing` (text). Install the writing skill with
`npx skills add conorbronsdon/avoid-ai-writing`.

## Naming

- User-facing platform and app brand is "Caramba Connect" (neutral default look).
  Write it that way in UI copy and docs unless an operator brand overrides it.
- exarobot is tenant #1: a configured white-label that keeps the @exarobot bot
  deep link and the exarobot.top panel URL. Those exarobot strings belong only
  where exarobot is the tenant.
- Per-tenant brands are operator-configured at runtime through the connected
  panel. Brand is a setting, not a hardcoded value.
- Internal code identifiers stay caramba and never change for branding:
  Dart package `caramba_client`, class `CarambaApp`, channel `com.caramba/vpn`,
  plugin `caramba_vpn`, Go module `github.com/semanticparadox/caramba`, and
  `CARAMBA_*` env vars. These are not user-facing brand.
- `profile.CarambaSelector = "CARAMBA"` is the panel<->client contract. It must
  never change.
- Anti-slop applies to all brands, including operator white-labels.

## Design

- No AI-purple/violet/indigo, no neon gradients, no colored glows (THE LILA RULE).
  Neutral base + one high-contrast accent used with intent.
- Caramba direction: color carries connection status only (green connected,
  amber connecting, red error). Everything else is monochrome neutrals. Primary
  button is white-on-black.
- Flat solid surfaces. No glassmorphism on everything (a single subtle nav blur is fine).
- No emoji as icons anywhere (UI, copy, headings, alt text). Use one real icon
  family (Lucide or Phosphor official glyphs), consistent stroke width. Never
  hand-draw SVG paths. Countries use mono 2-letter codes, not flag emoji.
- Type: system SF Pro stack or Geist, not Inter as a default. Mono only for
  technical data (latency, byte counts, codes, prices).
- Motion is subtle: animate transform/opacity only, no infinite breathing glows.
  Exactly one ambient background is allowed, the atmosphere layer specified in
  `DESIGN.md` section 4.2 (Open Chart). It is a flat line drawing capped at 8
  percent field alpha, it never blurs or glows, it only animates while connecting
  or reconnecting, and its ticker is stopped in every settled state. Any other
  ambient background is out of spec.
- Ship full state cycles (loading, empty, error), WCAG AA contrast, single-line
  button labels, one label per intent, no centered-hero-over-mesh default.

## Writing

- Em-dash (`—` and `--`) banned everywhere including headings. Use period,
  comma, parentheses, or colon.
- No "it's not X, it's Y" reframes. No compulsive rule-of-three. No sentence
  openers "Furthermore/Moreover/However". No "in conclusion / in this article".
- Replace on sight (and inflections): delve, leverage, utilize, seamless, robust,
  comprehensive, cutting-edge, pivotal, testament to, landscape (metaphor), realm,
  paradigm, embark, elevate, unleash, unlock, harness, game-changer, transformative,
  groundbreaking, meticulous, showcasing, deep dive, intricate, vibrant, thriving,
  nestled.
- Cut hollow intensifiers and hedges: genuinely, truly, really, "it's worth
  noting", "it's important to note", "to be clear", perhaps, could potentially.
- No marketing fluff in product copy. Plain functional language, concrete verbs,
  realistic content (no Lorem Ipsum, Acme, John Doe).

## Files still to de-slop

- `demo/caramba-demo.html` (rebuilt to the rules; reference implementation).

`DESIGN.md` and the Flutter theme tokens under `lib/theme/` are done: the
"Electric Iris" purple is gone, the palette is neutral with status-only color,
and section 4.2 pins the one ambient layer the product is allowed.
