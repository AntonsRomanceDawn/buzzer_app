import { useEffect, useRef, useState } from 'react'
import { useCreateRoom, useJoinRoom, useRefreshToken } from './hooks/useRoomMutations'
import { type SoundSettings, useSoundBoard } from './hooks/useSoundBoard'
import { ApiError, roomsApi, type Role } from './lib/api'
import {
    clearActiveRoomId,
    getActiveRoomId,
    getStoredName,
    getStoredRole,
    getStoredToken,
    persistAuth,
} from './lib/storage'
import './App.css'

type ParticipantInfo = {
    name: string
    role: Role
    locked_out: boolean
}

type ServerMessage =
    | { type: 'accepted'; name: string }
    | { type: 'rejected' }
    | { type: 'timed_out'; name: string }
    | { type: 'round_started' }
    | { type: 'round_continued' }
    | { type: 'participants'; participants: ParticipantInfo[] }
    | { type: 'action_denied'; reason: string }
    | { type: 'kicked' }

type Notice = {
    id: string
    text: string
    tone?: 'ok' | 'warn' | 'bad'
}

const REFRESH_THRESHOLD_SECS = 60 * 60
const REFRESH_CHECK_INTERVAL_SECS = 15 * 60

function getJwtExpSecs(token: string): number | null {
    try {
        const [, payload] = token.split('.')
        if (!payload) return null
        const normalized = payload.replace(/-/g, '+').replace(/_/g, '/')
        const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=')
        const decoded = atob(padded)
        const data = JSON.parse(decoded) as { exp?: number }
        if (!data.exp) return null
        return data.exp
    } catch {
        return null
    }
}

function isTypingTarget(target: EventTarget | null): boolean {
    if (!target || !(target instanceof HTMLElement)) return false
    const tag = target.tagName.toLowerCase()
    if (tag === 'input' || tag === 'textarea' || tag === 'select' || target.isContentEditable) {
        return true
    }
    return false
}

function App() {
    const [roomId, setRoomId] = useState('')
    const [name, setName] = useState('')
    const [role, setRole] = useState<Role | null>(null)
    const [participants, setParticipants] = useState<ParticipantInfo[]>([])
    const [token, setToken] = useState<string | null>(null)
    const [view, setView] = useState<'landing' | 'room'>('landing')
    const [error, setError] = useState<string | null>(null)
    const [retryDeadline, setRetryDeadline] = useState<number | null>(null)
    const [wsState, setWsState] = useState<'disconnected' | 'connecting' | 'connected'>(
        'disconnected'
    )
    const [result, setResult] = useState<'idle' | 'won' | 'lost' | 'rejected'>('idle')
    const [winnerName, setWinnerName] = useState('')
    const [myName, setMyName] = useState('')
    const [roomMode, setRoomMode] = useState<'create' | 'join'>('create')
    const [answerWindowInMs, setAnswerWindowInMs] = useState('5000')
    const [hasBuzzedThisRound, setHasBuzzedThisRound] = useState(false)
    const [answeringPlayer, setAnsweringPlayer] = useState<string | null>(null)
    const [notice, setNotice] = useState<Notice | null>(null)
    const [flashTone, setFlashTone] = useState<'win' | 'lose' | null>(null)
    const [soundMenuOpen, setSoundMenuOpen] = useState(false)
    const [soundSettings, setSoundSettings] = useState<SoundSettings>({
        roundStart: true,
        roundContinued: true,
        win: true,
        lose: true,
        timeout: true,
    })
    const [roundLocked, setRoundLocked] = useState(false)
    const wsRef = useRef<WebSocket | null>(null)
    const noticeTimerRef = useRef<number | null>(null)
    const flashTimerRef = useRef<number | null>(null)
    const soundMenuRef = useRef<HTMLDivElement | null>(null)
    const soundSettingsBackupRef = useRef<SoundSettings | null>(null)
    const soundBoardRef = useRef<ReturnType<typeof useSoundBoard> | null>(null)
    const connectionFailuresRef = useRef(0)
    const createRoomMutation = useCreateRoom()
    const joinRoomMutation = useJoinRoom()
    const refreshTokenMutation = useRefreshToken()
    const isAuthPending = createRoomMutation.isPending || joinRoomMutation.isPending

    const soundEnabled =
        soundSettings.roundStart ||
        soundSettings.roundContinued ||
        soundSettings.win ||
        soundSettings.lose ||
        soundSettings.timeout
    const soundBoard = useSoundBoard(soundEnabled, soundSettings)

    useEffect(() => {
        const params = new URLSearchParams(window.location.search)
        const roomParam = params.get('room')
        const activeRoomId = getActiveRoomId()

        if (roomParam) {
            setRoomId(roomParam)
            setRoomMode('join')

            if (roomParam === activeRoomId) {
                const savedToken = getStoredToken(roomParam)
                const savedName = getStoredName(roomParam)
                const savedRole = getStoredRole(roomParam) as Role | null
                if (savedToken && savedName && savedRole) {
                    setToken(savedToken)
                    setName(savedName)
                    setMyName(savedName)
                    setRole(savedRole)
                    setView('room')
                }
            }
            return
        }

        if (!activeRoomId) return

        setRoomId(activeRoomId)
        const savedToken = getStoredToken(activeRoomId)
        if (savedToken) setToken(savedToken)

        const savedName = getStoredName(activeRoomId)
        if (savedName) {
            setName(savedName)
            setMyName(savedName)
        }

        const savedRole = getStoredRole(activeRoomId) as Role | null
        if (savedRole) {
            setRole(savedRole)
        }

        if (savedToken) {
            setView('room')
        }
    }, [])

    useEffect(() => {
        if (view !== 'landing' || isAuthPending) return
        if (!roomId) return
        const savedToken = getStoredToken(roomId)
        if (savedToken) {
            setToken(savedToken)
        }
        const savedName = getStoredName(roomId)
        if (savedName) {
            setName(savedName)
            setMyName(savedName)
        }

        const savedRole = getStoredRole(roomId) as Role | null
        if (savedRole) {
            setRole(savedRole)
        }
    }, [isAuthPending, roomId, view])

    useEffect(() => {
        soundBoardRef.current = soundBoard
    }, [soundBoard])

    useEffect(() => {
        if (!retryDeadline) return
        const timer = setInterval(() => {
            const now = Date.now()
            if (now >= retryDeadline) {
                setRetryDeadline(null)
                setError(null)
            } else {
                const remaining = Math.ceil((retryDeadline - now) / 1000)
                setError(`Too many requests. Wait for ${remaining}s`)
            }
        }, 1000)
        return () => clearInterval(timer)
    }, [retryDeadline])

    useEffect(() => {
        const prime = () => soundBoardRef.current?.prime()
        window.addEventListener('pointerdown', prime, { once: true })
        window.addEventListener('keydown', prime, { once: true })
        return () => {
            window.removeEventListener('pointerdown', prime)
            window.removeEventListener('keydown', prime)
        }
    }, [])

    const showNotice = (text: string, tone: 'ok' | 'warn' | 'bad' = 'ok', durationMs = 2800) => {
        setNotice({ id: crypto.randomUUID(), text, tone })
        if (noticeTimerRef.current) {
            window.clearTimeout(noticeTimerRef.current)
        }
        if (durationMs > 0) {
            noticeTimerRef.current = window.setTimeout(() => {
                setNotice(null)
            }, durationMs)
        }
    }

    const triggerFlash = (tone: 'win' | 'lose') => {
        setFlashTone(tone)
        if (flashTimerRef.current) {
            window.clearTimeout(flashTimerRef.current)
        }
        flashTimerRef.current = window.setTimeout(() => {
            setFlashTone(null)
        }, 800)
    }

    const toggleSoundSetting = (key: keyof SoundSettings) => {
        setSoundSettings((prev) => ({
            ...prev,
            [key]: !prev[key],
        }))
    }

    const refreshTokenIfNeeded = async (
        currentToken: string | null,
        roomId: string
    ): Promise<string | null> => {
        if (!currentToken || !roomId) return currentToken
        const expSecs = getJwtExpSecs(currentToken)
        let shouldRefresh = !expSecs || (expSecs - Date.now() / 1000 < REFRESH_THRESHOLD_SECS);

        if (!shouldRefresh) {
            return currentToken
        }

        try {
            const nextToken = await refreshTokenMutation.mutateAsync({ new_token: currentToken, roomId })
            if (nextToken !== currentToken) {
                setToken(nextToken)
                persistAuth(roomId, nextToken)
            }
            return nextToken
        } catch (error) {
            const errMsg = (error as Error).message || ''
            if (errMsg.includes('room_not_found')) {
                clearActiveRoomId()
            }
            return null
        }
    }

    const resetSession = () => {
        setRole(null)
        setParticipants([])
        setToken(null)
        setWsState('disconnected')
        setResult('idle')
        setHasBuzzedThisRound(false)
        setRoundLocked(false)
        if (wsRef.current) {
            wsRef.current.close()
            wsRef.current = null
        }
        setView('landing')
        // Clear query param if present
        if (window.location.search) {
            window.history.replaceState({}, '', window.location.pathname)
        }
    }

    const copyInviteLink = () => {
        const url = `${window.location.origin}?room=${roomId}`
        navigator.clipboard.writeText(url)
        showNotice('Invite link copied!', 'ok', 2000)
    }

    const createRoom = async () => {
        if (isAuthPending) return
        if (retryDeadline && Date.now() < retryDeadline) return

        setError(null)
        try {
            const data = await createRoomMutation.mutateAsync({
                name,
                answerWindowInMs,
            })
            setRoomId(data.room_id)
            setMyName(name)
            setRole('admin')
            setToken(data.token)
            setAnswerWindowInMs(String(data.answer_window_in_ms))
            setView('room')
            showNotice('Room created. You are the admin.', 'ok', 3000)
            persistAuth(data.room_id, data.token, name, 'admin')
        } catch (err) {
            if (err instanceof ApiError && err.status === 429 && err.retryAfter) {
                setRetryDeadline(Date.now() + err.retryAfter * 1000)
                setError(`Too many requests. Wait for ${err.retryAfter}s`)
            } else {
                setError((err as Error).message)
            }
        }
    }

    const joinRoom = async () => {
        if (isAuthPending) return
        if (retryDeadline && Date.now() < retryDeadline) return

        setError(null)
        try {
            const data = await joinRoomMutation.mutateAsync({ roomId, name })
            const nextRole = data.role
            setRole(nextRole)
            setToken(data.token)
            setAnswerWindowInMs(String(data.answer_window_in_ms))
            setView('room')
            if (name.trim()) {
                setMyName(name.trim())
            }
            persistAuth(roomId, data.token ?? undefined, name, nextRole)
        } catch (err) {
            if (err instanceof ApiError && err.status === 429 && err.retryAfter) {
                setRetryDeadline(Date.now() + err.retryAfter * 1000)
                setError(`Too many requests. Wait for ${err.retryAfter}s`)
            } else {
                setError((err as Error).message)
            }
        }
    }

    const connectWs = async () => {
        if (!token || !roomId) {
            setError('missing_token_or_room')
            return
        }
        if (wsRef.current) {
            wsRef.current.close()
        }
        setWsState('connecting')
        const freshToken = await refreshTokenIfNeeded(token, roomId)
        const wsUrl = `${window.location.origin.replace('http', 'ws')}/ws/${roomId}?token=${freshToken}`
        const ws = new WebSocket(wsUrl)
        wsRef.current = ws

        ws.onopen = () => {
            connectionFailuresRef.current = 0
            setWsState('connected')
            setResult('idle')
            setWinnerName('')
            setHasBuzzedThisRound(false)
            setRoundLocked(false)
        }
        ws.onmessage = (event) => {
            try {
                const msg = JSON.parse(event.data) as ServerMessage
                switch (msg.type) {
                    case 'accepted':
                        setRoundLocked(true)
                        setWinnerName(msg.name)
                        setAnsweringPlayer(msg.name)
                        if (msg.name === myName) {
                            setResult('won')
                            triggerFlash('win')
                            soundBoardRef.current?.playWin()
                            setHasBuzzedThisRound(true)
                        } else {
                            setResult('lost')
                            triggerFlash('lose')
                            soundBoardRef.current?.playLose()
                        }
                        break
                    case 'rejected':
                        setResult('rejected')
                        break
                    case 'timed_out':
                        setResult('idle')
                        setWinnerName('')
                        setRoundLocked(false)
                        setAnsweringPlayer(null)
                        setParticipants((prev) =>
                            prev.map((p) => (p.name === msg.name ? { ...p, locked_out: true } : p))
                        )
                        soundBoardRef.current?.playTimeout()
                        if (msg.name === myName) {
                            showNotice('You timed out!', 'warn', 2200)
                        } else {
                            showNotice(`Buzzer open! ${msg.name} timed out`, 'ok', 2200)
                        }
                        break
                    case 'round_continued':
                        setResult('idle')
                        setWinnerName('')
                        setRoundLocked(false)
                        if (answeringPlayer) {
                            setParticipants((prev) =>
                                prev.map((p) =>
                                    p.name === answeringPlayer ? { ...p, locked_out: true } : p
                                )
                            )
                            setAnsweringPlayer(null)
                        }
                        showNotice('Buzzer open!', 'ok', 2200)
                        soundBoardRef.current?.playRoundContinued()
                        break
                    case 'round_started':
                        setResult('idle')
                        setWinnerName('')
                        setHasBuzzedThisRound(false)
                        setRoundLocked(false)
                        setAnsweringPlayer(null)
                        setParticipants((prev) => prev.map((p) => ({ ...p, locked_out: false })))
                        showNotice('Round started. Buzz now!', 'ok', 2200)
                        soundBoardRef.current?.playRoundStart()
                        break
                    case 'participants': {
                        setParticipants(msg.participants)
                        break
                    }
                    case 'action_denied':
                        break
                    case 'kicked':
                        showNotice('You were kicked from the room.', 'bad', 6000)
                        setResult('idle')
                        resetSession()
                        break
                    default:
                }
            } catch {
                // ignore
            }
        }
        ws.onclose = async () => {
            setWsState('disconnected')

            connectionFailuresRef.current += 1
            if (connectionFailuresRef.current > 3 && view === 'room' && token && roomId) {
                try {
                    await roomsApi.refreshToken({ roomId, new_token: token })
                } catch (e) {
                    const msg = (e as Error).message
                    if (msg.includes('room_not_found') || msg.includes('user_not_in_room')) {
                        if (msg.includes('room_not_found')) {
                            clearActiveRoomId()
                        }
                        resetSession()
                        return
                    }
                }

                resetSession()
            } else {
            }
        }
        ws.onerror = () => {
            setWsState('disconnected')
        }
    }

    const sendBuzz = () => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) {
            return
        }
        if (roundLocked) {
            return
        }
        if (hasBuzzedThisRound) {
            return
        }
        wsRef.current.send(JSON.stringify({ type: 'buzz' }))
    }

    const startRound = () => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) {
            return
        }
        wsRef.current.send(JSON.stringify({ type: 'start_round' }))
    }

    const continueRound = () => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) {
            return
        }
        wsRef.current.send(JSON.stringify({ type: 'continue_round' }))
    }

    const kickPlayer = (targetName: string) => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) return
        if (!confirm(`Kick ${targetName}?`)) return
        wsRef.current.send(JSON.stringify({ type: 'kick', name: targetName }))
    }

    useEffect(() => {
        if (view === 'room' && roomId) {
            const url = new URL(window.location.href)
            if (url.searchParams.get('room') !== roomId) {
                url.searchParams.set('room', roomId)
                window.history.replaceState({}, '', url.toString())
            }
        }
    }, [view, roomId])

    useEffect(() => {
        if (view !== 'room') return
        if (wsState === 'connected' || wsState === 'connecting') return
        void connectWs()
    }, [view, wsState])

    useEffect(() => {
        if (view !== 'room' || !token || !roomId) return
        const interval = setInterval(() => {
            void refreshTokenIfNeeded(token, roomId)
        }, REFRESH_CHECK_INTERVAL_SECS)
        return () => clearInterval(interval)
    }, [view, token, roomId])

    useEffect(() => {
        return () => {
            if (noticeTimerRef.current) {
                window.clearTimeout(noticeTimerRef.current)
            }
            if (flashTimerRef.current) {
                window.clearTimeout(flashTimerRef.current)
            }
        }
    }, [])

    useEffect(() => {
        const soundEnabled =
            soundSettings.roundStart ||
            soundSettings.roundContinued ||
            soundSettings.win ||
            soundSettings.lose ||
            soundSettings.timeout
        if (soundEnabled) {
            soundSettingsBackupRef.current = soundSettings
        }
    }, [soundSettings])

    const toggleSoundMaster = () => {
        if (soundEnabled) {
            soundSettingsBackupRef.current = soundSettings
            setSoundSettings({
                roundStart: false,
                roundContinued: false,
                win: false,
                lose: false,
                timeout: false,
            })
            return
        }
        const backup = soundSettingsBackupRef.current
        if (backup) {
            setSoundSettings(backup)
            return
        }
        setSoundSettings({
            roundStart: true,
            roundContinued: true,
            win: true,
            lose: true,
            timeout: true,
        })
    }

    useEffect(() => {
        if (!soundMenuOpen) return
        const onPointerDown = (event: PointerEvent) => {
            const target = event.target as Node | null
            if (!soundMenuRef.current || !target) return
            if (!soundMenuRef.current.contains(target)) {
                setSoundMenuOpen(false)
            }
        }
        window.addEventListener('pointerdown', onPointerDown)
        return () => window.removeEventListener('pointerdown', onPointerDown)
    }, [soundMenuOpen])

    const iAmLockedOut = participants.find((p) => p.name === myName)?.locked_out ?? false

    const buzzDisabled =
        wsState !== 'connected' || hasBuzzedThisRound || roundLocked || iAmLockedOut
    const isWinning = winnerName === myName
    const buzzStatus = roundLocked
        ? isWinning
            ? ''
            : 'Someone is answering.'
        : iAmLockedOut
            ? 'Locked out this round.'
            : hasBuzzedThisRound
                ? ''
                : wsState === 'connected'
                    ? 'Tap once per round or press space.'
                    : 'Connect to buzz.'

    const buzzLabel =
        winnerName ||
        (iAmLockedOut ? 'Locked Out' : hasBuzzedThisRound ? 'Buzzed already' : 'Buzz')

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key !== ' ' && event.key !== 'Spacebar' && event.code !== 'Space') return
            if (view !== 'room') return
            if (wsState !== 'connected') return
            if (hasBuzzedThisRound || iAmLockedOut) return
            if (isTypingTarget(event.target)) return
            event.preventDefault()
            sendBuzz()
        }

        window.addEventListener('keydown', onKeyDown)
        return () => window.removeEventListener('keydown', onKeyDown)
    }, [view, wsState, hasBuzzedThisRound, sendBuzz, iAmLockedOut])

    return (
        <div className={`app ${result !== 'idle' ? `status-${result}` : ''}`}>
            <header className="hero">
                <div>
                    <p className="eyebrow">buzzer_game</p>
                    <h1>Fastest finger wins.</h1>
                    <p className="subhead">
                        Create a room, share the code, and run lightning rounds in your browser.
                    </p>
                </div>
                <div className="hero-meta">
                    <div>
                        <span className="label">Connection</span>
                        <span className={`pill ${wsState}`}>{wsState}</span>
                    </div>
                    <div>
                        <span className="label">Room ID</span>
                        <span className="pill">{roomId || '—'}</span>
                    </div>
                    <div>
                        <span className="label">Role</span>
                        <span className="pill">{role ?? '—'}</span>
                    </div>
                    <div>
                        <span className="label">Answer window</span>
                        <span className="pill">
                            {answerWindowInMs ? `${answerWindowInMs} ms` : '—'}
                        </span>
                    </div>
                    <div>
                        <span className="label">Sound</span>
                        <div className="sound-control" ref={soundMenuRef}>
                            <button
                                className={`sound-toggle ${soundEnabled ? 'on' : 'off'}`}
                                onClick={toggleSoundMaster}
                                aria-label={soundEnabled ? 'Sound on' : 'Sound off'}
                                title={soundEnabled ? 'Sound on' : 'Sound off'}
                            >
                                <svg viewBox="0 0 24 24" aria-hidden="true">
                                    <path d="M3.5 9.5h4.1L13 5.2v13.6l-5.4-4.3H3.5z" />
                                    <path
                                        className="wave"
                                        d="M16.2 8.3a6 6 0 010 7.4"
                                        fill="none"
                                        strokeWidth="1.8"
                                        strokeLinecap="round"
                                    />
                                    <path
                                        className="wave"
                                        d="M18.6 6a9 9 0 010 12"
                                        fill="none"
                                        strokeWidth="1.8"
                                        strokeLinecap="round"
                                    />
                                    <path className="slash" d="M4 4l16 16" />
                                </svg>
                            </button>
                            <button
                                className="sound-menu-toggle"
                                onClick={() => setSoundMenuOpen((prev) => !prev)}
                                aria-label="Sound options"
                                title="Sound options"
                            >
                                Options
                            </button>
                            {soundMenuOpen && (
                                <div className="sound-menu" role="menu">
                                    <label className="sound-item">
                                        <input
                                            type="checkbox"
                                            checked={soundSettings.roundStart}
                                            onChange={() => toggleSoundSetting('roundStart')}
                                        />
                                        Round start
                                    </label>
                                    <label className="sound-item">
                                        <input
                                            type="checkbox"
                                            checked={soundSettings.roundContinued}
                                            onChange={() => toggleSoundSetting('roundContinued')}
                                        />
                                        Round continued
                                    </label>
                                    <label className="sound-item">
                                        <input
                                            type="checkbox"
                                            checked={soundSettings.win}
                                            onChange={() => toggleSoundSetting('win')}
                                        />
                                        Win
                                    </label>
                                    <label className="sound-item">
                                        <input
                                            type="checkbox"
                                            checked={soundSettings.lose}
                                            onChange={() => toggleSoundSetting('lose')}
                                        />
                                        Lose
                                    </label>
                                    <label className="sound-item">
                                        <input
                                            type="checkbox"
                                            checked={soundSettings.timeout}
                                            onChange={() => toggleSoundSetting('timeout')}
                                        />
                                        Timeout
                                    </label>
                                </div>
                            )}
                        </div>
                    </div>
                </div>
            </header>

            {view === 'landing' && (
                <main className="layout">
                    <section className="panel wide">
                        <div className="panel-row">
                            <div>
                                <h2>Start or join</h2>
                                <p className="panel-sub">
                                    Choose your path. Saved identity is used automatically if available.
                                </p>
                            </div>
                            <div className="panel-actions">
                                <button
                                    className={roomMode === 'create' ? 'secondary' : 'ghost'}
                                    onClick={() => setRoomMode('create')}
                                >
                                    Create room
                                </button>
                                <button
                                    className={roomMode === 'join' ? 'secondary' : 'ghost'}
                                    onClick={() => setRoomMode('join')}
                                >
                                    Join room
                                </button>
                            </div>
                        </div>

                        {roomMode === 'create' ? (
                            <div className="split">
                                <label>
                                    Display name
                                    <input
                                        value={name}
                                        onChange={(event) => setName(event.target.value)}
                                        placeholder="e.g. Ada"
                                    />
                                </label>
                                <label>
                                    Answer window (ms)
                                    <input
                                        type="number"
                                        value={answerWindowInMs}
                                        onChange={(event) => setAnswerWindowInMs(event.target.value)}
                                        placeholder="5000"
                                    />
                                </label>
                                <button disabled={isAuthPending || !name.trim()} onClick={createRoom}>
                                    Create room
                                </button>
                            </div>
                        ) : (
                            <div className="split">
                                <label>
                                    Display name
                                    <input
                                        value={name}
                                        onChange={(event) => setName(event.target.value)}
                                        placeholder="e.g. Ada"
                                    />
                                </label>
                                <label>
                                    Room id
                                    <input
                                        value={roomId}
                                        onChange={(event) => setRoomId(event.target.value.trim())}
                                        placeholder="room id"
                                    />
                                </label>
                                <button
                                    disabled={isAuthPending || !roomId || !name.trim()}
                                    onClick={joinRoom}
                                >
                                    Join room
                                </button>
                            </div>
                        )}

                        {error && <div className="error">Error: {error}</div>}
                    </section>
                </main>
            )}

            {view === 'room' && (
                <main className="layout">
                    <section className="panel wide">
                        <div className="panel-row">
                            <div>
                                <h2>Room console</h2>
                                <p className="panel-sub">Manage the round and send buzzes.</p>
                            </div>
                            <div className="panel-actions">
                                <button className="ghost" onClick={copyInviteLink}>
                                    Copy invite
                                </button>
                                <button className="ghost" onClick={resetSession}>
                                    Leave room
                                </button>
                            </div>
                        </div>

                        <div className="buzzer-area">
                            <button
                                className={`buzzer-button ${hasBuzzedThisRound ? 'pressed' : ''} ${iAmLockedOut ? 'locked-out' : ''}`}
                                onPointerDown={(e) => {
                                    if (e.target instanceof HTMLButtonElement && e.target.disabled) return;
                                    sendBuzz()
                                }}
                                disabled={buzzDisabled}
                            >
                                <span className="buzzer-label">{buzzLabel}</span>
                                <span className="buzzer-sub">{buzzStatus}</span>
                            </button>
                        </div>

                        <div className="room-grid">
                            <div className="room-card">
                                <h3>Participants</h3>
                                <ul>
                                    {participants.length === 0 && <li className="muted">No players yet.</li>}
                                    {participants.map((participant) => (
                                        <li key={participant.name}>
                                            <div className="participant-info">
                                                <span>{participant.name}</span>
                                                {participant.locked_out && (
                                                    <span className="badge">Locked</span>
                                                )}
                                                <span className={`badge ${participant.role}`}>
                                                    {participant.role}
                                                </span>
                                            </div>
                                            {role === 'admin' && participant.role !== 'admin' && (
                                                <button
                                                    className="icon-button danger"
                                                    onClick={() => kickPlayer(participant.name)}
                                                    title="Kick player"
                                                    aria-label={`Kick ${participant.name}`}
                                                >
                                                    <svg
                                                        viewBox="0 0 24 24"
                                                        aria-hidden="true"
                                                        fill="currentColor"
                                                    >
                                                        <path d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z" />
                                                    </svg>
                                                </button>
                                            )}
                                        </li>
                                    ))}
                                </ul>
                            </div>
                            {role === 'admin' && (
                                <div className="room-card">
                                    <h3>Controls</h3>
                                    <div className="controls">
                                        <button
                                            className="secondary"
                                            onClick={startRound}
                                            disabled={wsState !== 'connected'}
                                        >
                                            Start round
                                        </button>
                                        <button
                                            className="ghost"
                                            onClick={continueRound}
                                            disabled={wsState !== 'connected'}
                                            title="Continue round (reject answer or skip)"
                                        >
                                            Continue Round
                                        </button>
                                    </div>
                                </div>
                            )}
                        </div>

                        {error && <div className="error">Error: {error}</div>}
                    </section>
                </main>
            )}

            {notice && (
                <div className={`notice ${notice.tone ?? 'ok'}`} role="alert">
                    <div>
                        <p>{notice.text}</p>
                    </div>
                    <button className="notice-close" onClick={() => setNotice(null)}>
                        Dismiss
                    </button>
                </div>
            )}

            <div className={`screen-flash ${flashTone ?? ''}`} aria-hidden="true" />
        </div>
    )
}

export default App
