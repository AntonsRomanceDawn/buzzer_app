import { useMutation } from '@tanstack/react-query'
import { roomsApi } from '../lib/api'
import { qk } from '../lib/queryKeys'
import { getStoredToken } from '../lib/storage'

export function useCreateRoom() {
    return useMutation({
        mutationKey: qk.rooms.create,
        mutationFn: roomsApi.createRoom,
    })
}

export function useJoinRoom() {
    return useMutation({
        mutationKey: qk.rooms.joinMutation,
        mutationFn: async (payload: { roomId: string; name: string }) => {
            const token = getStoredToken(payload.roomId)
            return roomsApi.joinRoom({
                roomId: payload.roomId,
                name: payload.name,
                token,
            })
        },
    })
}

export function useRefreshToken() {
    return useMutation({
        mutationKey: qk.rooms.refreshMutation,
        mutationFn: roomsApi.refreshToken,
    })
}
