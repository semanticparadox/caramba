import type { UserSubscription } from '../context/AuthContext'

export function buildConfigUrl(sub: UserSubscription, opts?: {
  client?: string; nodeId?: number; variantId?: string; relayCountry?: string
}): string {
  const base = sub.subscription_url
  if (!opts) return base
  const params = new URLSearchParams()
  if (opts.client) params.set('client', opts.client)
  if (opts.nodeId) params.set('node_id', String(opts.nodeId))
  if (opts.client === 'singbox' && opts.variantId) params.set('variant', opts.variantId)
  if (opts.relayCountry && opts.relayCountry !== 'auto') params.set('relay_country', opts.relayCountry)
  const q = params.toString()
  if (!q) return base
  return `${base}${base.includes('?') ? '&' : '?'}${q}`
}
