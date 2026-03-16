const trimTrailingSlash = (value: string) => value.replace(/\/+$/, '')

const rawBase = import.meta.env.VITE_API_BASE_URL?.trim() || ''

export const config = {
  API_BASE_URL: rawBase ? trimTrailingSlash(rawBase) : '',
}

export const apiUrl = (path: string) => {
  if (!path.startsWith('/')) {
    throw new Error(`apiUrl expects an absolute path, got: ${path}`)
  }

  if (!config.API_BASE_URL) return path
  return `${config.API_BASE_URL}${path}`
}
