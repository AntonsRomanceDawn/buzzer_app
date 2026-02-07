export const TOKEN_STORAGE_KEY = 'bg_tokens'
export const NAME_STORAGE_KEY = 'bg_names'
export const ACTIVE_ROOM_STORAGE_KEY = 'bg_active_room'
export const ROLE_STORAGE_KEY = 'bg_roles'

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

export function getStoredRole(roomId: string): string | null {
    if (!roomId) return null
    return loadMap(ROLE_STORAGE_KEY)[roomId] ?? null
}

export function getActiveRoomId(): string | null {
    return localStorage.getItem(ACTIVE_ROOM_STORAGE_KEY)
}

export function setActiveRoomId(roomId: string) {
    localStorage.setItem(ACTIVE_ROOM_STORAGE_KEY, roomId)
}

export function clearActiveRoomId() {
    localStorage.removeItem(ACTIVE_ROOM_STORAGE_KEY)
}

export function persistAuth(roomId: string, token?: string, name?: string, role?: string) {
    if (!roomId) return
    setActiveRoomId(roomId)

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

    if (role) {
        const roles = loadMap(ROLE_STORAGE_KEY)
        roles[roomId] = role
        saveMap('bg_roles', roles)
    }
}
