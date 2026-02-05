export const TOKEN_STORAGE_KEY = 'bg_tokens'
export const NAME_STORAGE_KEY = 'bg_names'

function loadMap(key: string): Record<string, string> {
    try {
        const raw = localStorage.getItem(key)
        if (!raw) return {}
        const parsed = JSON.parse(raw) as Record<string, string>
        return parsed ?? {}
    } catch {
        return {}
    }
}

function saveMap(key: string, value: Record<string, string>) {
    localStorage.setItem(key, JSON.stringify(value))
}

export function getStoredToken(roomId: string): string | null {
    if (!roomId) return null
    return loadMap(TOKEN_STORAGE_KEY)[roomId] ?? null
}

export function getStoredName(roomId: string): string | null {
    if (!roomId) return null
    return loadMap(NAME_STORAGE_KEY)[roomId] ?? null
}

export function persistAuth(roomId: string, token?: string, name?: string) {
    if (!roomId) return
    if (token) {
        const tokens = loadMap(TOKEN_STORAGE_KEY)
        tokens[roomId] = token
        saveMap(TOKEN_STORAGE_KEY, tokens)
    }
    if (name) {
        const names = loadMap(NAME_STORAGE_KEY)
        names[roomId] = name
        saveMap(NAME_STORAGE_KEY, names)
    }
}
