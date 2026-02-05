export const qk = {
    rooms: {
        create: ['rooms', 'create'] as const,
        join: (roomId: string) => ['rooms', 'join', roomId] as const,
        refreshToken: (roomId: string) => ['rooms', 'refresh_token', roomId] as const,
        joinMutation: ['rooms', 'join_mutation'] as const,
        refreshMutation: ['rooms', 'refresh_mutation'] as const,
    },
}
