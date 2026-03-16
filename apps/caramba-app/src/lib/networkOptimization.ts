import { apiUrl } from '../config'

export type RecommendedNode = {
  id: number
  name: string
  country_code: string
  score: number
  distance_km: number
  load_pct: number
  latency_ms: number
}

export type RecommendedNodesResponse = {
  user_location: {
    lat: number
    lon: number
  }
  nodes: RecommendedNode[]
}

export const fetchRecommendedNodes = async (token: string) => {
  const response = await fetch(apiUrl('/api/v2/client/recommended'), {
    headers: {
      Authorization: `Bearer ${token}`,
    },
  })

  if (!response.ok) {
    throw new Error('Failed to fetch recommended nodes')
  }

  return (await response.json()) as RecommendedNodesResponse
}

export const pinSubscriptionNode = async (
  token: string,
  subscriptionId: number,
  nodeId: number,
) => {
  const response = await fetch(
    apiUrl(`/api/client/subscription/${subscriptionId}/server`),
    {
      method: 'POST',
      headers: {
        Authorization: `Bearer ${token}`,
        'Content-Type': 'application/json',
      },
      body: JSON.stringify({ node_id: nodeId }),
    },
  )

  if (!response.ok) {
    throw new Error('Failed to pin recommended node')
  }

  return response
}
