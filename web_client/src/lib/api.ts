export type Role = 'admin' | 'player'

export type CreateRoomResponse = {
    room_id: string
    token: string
    answer_window_in_ms: number
}

export type JoinRoomResponse = {
    token: string | null
    answer_window_in_ms: number
}

export type RefreshTokenResponse = {
    new_token: string
}

async function apiRequest<T>(url: string, init?: RequestInit): Promise<T> {
    const resp = await fetch(url, init)
    if (!resp.ok) {
        const text = await resp.text()
        throw new Error(text || `request_failed_${resp.status}`)
    }
    return (await resp.json()) as T
}

export const roomsApi = {
    createRoom: (payload: { name: string; answerWindowInMs: string }) =>
        apiRequest<CreateRoomResponse>('/api/rooms', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                name: payload.name,
                answer_window_in_ms: Number.isFinite(Number(payload.answerWindowInMs))
                    ? Number(payload.answerWindowInMs)
                    : null,
            }),
        }),

    joinRoom: (payload: { roomId: string; name: string; token?: string | null }) => {
        const headers: Record<string, string> = { 'Content-Type': 'application/json' }
        if (payload.token) {
            headers.Authorization = `Bearer ${payload.token}`
        }

        return apiRequest<JoinRoomResponse>(`/api/rooms/${payload.roomId}/join`, {
            method: 'POST',
            headers,
            body: JSON.stringify({ name: payload.name.trim() }),
        })
    },

    refreshToken: async (payload: { roomId: string; new_token: string }) => {
        try {
            const data = await apiRequest<RefreshTokenResponse>(
                `/api/rooms/${payload.roomId}/refresh_token`,
                {
                    method: 'POST',
                    headers: { Authorization: `Bearer ${payload.new_token}` },
                }
            )
            return data.new_token || payload.new_token
        } catch {
            return payload.new_token
        }
    },
}
