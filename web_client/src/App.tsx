import { useEffect, useRef, useState } from 'react'
import { useCreateRoom, useJoinRoom, useRefreshToken } from './hooks/useRoomMutations'
import { type SoundSettings, useSoundBoard } from './hooks/useSoundBoard'
import { type Role } from './lib/api'
import { getStoredName, getStoredToken, persistAuth } from './lib/storage'
import './App.css'

type ParticipantInfo = {
    name: string
    role: Role
}

type ServerMessage =
    | { type: 'accepted'; name: string; deadline_in_ms: number; ts_ms: number }
    | { type: 'rejected'; ts_ms: number }
    | { type: 'timed_out'; name: string; ts_ms: number }
    | { type: 'round_started'; ts_ms: number }
    | { type: 'participants'; participants: ParticipantInfo[]; ts_ms: number }
    | { type: 'action_denied'; reason: string; ts_ms: number }
    | { type: 'kicked'; ts_ms: number }

type LogEntry = {
    id: string
    ts: string
    text: string
    tone?: 'ok' | 'warn' | 'bad'
}

type Notice = {
    id: string
    text: string
    tone?: LogEntry['tone']
}

const REFRESH_THRESHOLD_MS = 60 * 60 * 1000
const REFRESH_CHECK_INTERVAL_MS = 15 * 60 * 1000

function getJwtExpMs(token: string): number | null {
    try {
        const [, payload] = token.split('.')
        if (!payload) return null
        const normalized = payload.replace(/-/g, '+').replace(/_/g, '/')
        const padded = normalized.padEnd(Math.ceil(normalized.length / 4) * 4, '=')
        const decoded = atob(padded)
        const data = JSON.parse(decoded) as { exp?: number }
        if (!data.exp) return null
        return data.exp * 1000
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
    const [wsState, setWsState] = useState<'disconnected' | 'connecting' | 'connected'>(
        'disconnected'
    )
    const [logs, setLogs] = useState<LogEntry[]>([])
    const [adminTarget, setAdminTarget] = useState('')
    const [kickTarget, setKickTarget] = useState('')
    const [result, setResult] = useState<'idle' | 'won' | 'lost' | 'rejected'>('idle')
    const [myName, setMyName] = useState('')
    const [roomMode, setRoomMode] = useState<'create' | 'join'>('create')
    const [answerWindowInMs, setAnswerWindowInMs] = useState('5000')
    const [hasBuzzedThisRound, setHasBuzzedThisRound] = useState(false)
    const [notice, setNotice] = useState<Notice | null>(null)
    const [flashTone, setFlashTone] = useState<'win' | 'lose' | null>(null)
    const [soundMenuOpen, setSoundMenuOpen] = useState(false)
    const [soundSettings, setSoundSettings] = useState<SoundSettings>({
        roundStart: true,
        win: true,
        lose: true,
        timeout: true,
    })
    const [roundLocked, setRoundLocked] = useState(false)
    const wsRef = useRef<WebSocket | null>(null)
    const noticeTimerRef = useRef<number | null>(null)
    const flashTimerRef = useRef<number | null>(null)
    const roleRef = useRef<Role | null>(null)
    const soundMenuRef = useRef<HTMLDivElement | null>(null)
    const soundSettingsBackupRef = useRef<SoundSettings | null>(null)
    const soundBoardRef = useRef<ReturnType<typeof useSoundBoard> | null>(null)
    const createRoomMutation = useCreateRoom()
    const joinRoomMutation = useJoinRoom()
    const refreshTokenMutation = useRefreshToken()
    const isAuthPending = createRoomMutation.isPending || joinRoomMutation.isPending

    const soundEnabled =
        soundSettings.roundStart ||
        soundSettings.win ||
        soundSettings.lose ||
        soundSettings.timeout
    const soundBoard = useSoundBoard(soundEnabled, soundSettings)

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
    }, [isAuthPending, roomId, view])

    useEffect(() => {
        soundBoardRef.current = soundBoard
    }, [soundBoard])

    useEffect(() => {
        const prime = () => soundBoardRef.current?.prime()
        window.addEventListener('pointerdown', prime, { once: true })
        window.addEventListener('keydown', prime, { once: true })
        return () => {
            window.removeEventListener('pointerdown', prime)
            window.removeEventListener('keydown', prime)
        }
    }, [])

    const appendLog = (text: string, tone: LogEntry['tone'] = 'ok', tsMs?: number) => {
        const ts = new Date(tsMs ?? Date.now()).toLocaleTimeString([], {
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
            fractionalSecondDigits: 3,
        })
        setLogs((prev) => [
            { id: crypto.randomUUID(), ts, text, tone },
            ...prev.slice(0, 200),
        ])
    }

    const showNotice = (text: string, tone: LogEntry['tone'] = 'ok', durationMs = 2800) => {
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
        const expMs = getJwtExpMs(currentToken)
        if (!expMs) return currentToken
        if (expMs - Date.now() > REFRESH_THRESHOLD_MS) {
            return currentToken
        }
        try {
            const nextToken = await refreshTokenMutation.mutateAsync({ new_token: currentToken, roomId })
            if (nextToken !== currentToken) {
                setToken(nextToken)
                persistAuth(roomId, nextToken)
            }
            return nextToken
        } catch {
            return currentToken
        }
    }

    const resetSession = () => {
        setRole(null)
        roleRef.current = null
        setParticipants([])
        setToken(null)
        setWsState('disconnected')
        setLogs([])
        setResult('idle')
        setHasBuzzedThisRound(false)
        setRoundLocked(false)
        if (wsRef.current) {
            wsRef.current.close()
            wsRef.current = null
        }
        setView('landing')
    }

    const createRoom = async () => {
        if (isAuthPending) return
        setError(null)
        try {
            const data = await createRoomMutation.mutateAsync({
                name,
                answerWindowInMs,
            })
            setRoomId(data.room_id)
            setMyName(name)
            setRole('admin')
            roleRef.current = 'admin'
            setToken(data.token)
            setAnswerWindowInMs(String(data.answer_window_in_ms))
            setView('room')
            appendLog('Room created. You are the admin.', 'ok')
            showNotice('Room created. You are the admin.', 'ok', 3000)
            persistAuth(data.room_id, data.token, name)
        } catch (err) {
            setError((err as Error).message)
        }
    }

    const joinRoom = async () => {
        if (isAuthPending) return
        setError(null)
        try {
            const hadStoredToken = Boolean(getStoredToken(roomId))
            const data = await joinRoomMutation.mutateAsync({ roomId, name })
            const nextRole = hadStoredToken ? role ?? 'player' : 'player'
            setRole(nextRole)
            roleRef.current = nextRole
            setToken(data.token)
            setAnswerWindowInMs(String(data.answer_window_in_ms))
            setView('room')
            appendLog('Joined room.', 'ok')
            if (name.trim()) {
                setMyName(name.trim())
            }
            persistAuth(roomId, data.token ?? undefined, name)
        } catch (err) {
            setError((err as Error).message)
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
        if (!freshToken) {
            setError('missing_token_or_room')
            setWsState('disconnected')
            return
        }
        const wsUrl = `${window.location.origin.replace('http', 'ws')}/ws/${roomId}?token=${freshToken}`
        const ws = new WebSocket(wsUrl)
        wsRef.current = ws

        ws.onopen = () => {
            setWsState('connected')
            appendLog('WebSocket connected.', 'ok')
            setResult('idle')
            setHasBuzzedThisRound(false)
            setRoundLocked(false)
        }
        ws.onmessage = (event) => {
            try {
                const msg = JSON.parse(event.data) as ServerMessage
                switch (msg.type) {
                    case 'accepted':
                        appendLog(`${msg.name} buzzed first.`, 'ok', msg.ts_ms)
                        setRoundLocked(true)
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
                        appendLog('Buzz rejected.', 'warn', msg.ts_ms)
                        setResult('rejected')
                        break
                    case 'timed_out':
                        appendLog(`${msg.name} timed out.`, 'bad', msg.ts_ms)
                        setResult('idle')
                        setRoundLocked(false)
                        soundBoardRef.current?.playTimeout()
                        break
                    case 'round_started':
                        appendLog('Round started.', 'ok', msg.ts_ms)
                        setResult('idle')
                        setHasBuzzedThisRound(false)
                        setRoundLocked(false)
                        showNotice('Round started. Buzz now!', 'ok', 2200)
                        soundBoardRef.current?.playRoundStart()
                        break
                    case 'participants': {
                        setParticipants(msg.participants)
                        const identity = myName || name
                        if (identity) {
                            const me = msg.participants.find((participant) => participant.name === identity)
                            if (me) {
                                const prevRole = roleRef.current
                                roleRef.current = me.role
                                setRole(me.role)
                                if (prevRole !== 'admin' && me.role === 'admin') {
                                    showNotice('You are now the admin.', 'ok', 3000)
                                } else if (prevRole === 'admin' && me.role !== 'admin') {
                                    showNotice('Admin role transferred.', 'warn', 3000)
                                }
                            } else if (view === 'room') {
                                appendLog('You are no longer in the room.', 'bad')
                                resetSession()
                                return
                            }
                        }
                        appendLog('Participants updated.', 'ok', msg.ts_ms)
                        break
                    }
                    case 'action_denied':
                        appendLog(`Action denied: ${msg.reason}`, 'warn', msg.ts_ms)
                        break
                    case 'kicked':
                        appendLog('You were removed from the room.', 'bad', msg.ts_ms)
                        showNotice('You were kicked from the room.', 'bad', 6000)
                        setResult('idle')
                        resetSession()
                        break
                    default:
                        appendLog('Unknown server message.', 'warn')
                }
            } catch {
                appendLog('Invalid message from server.', 'warn')
            }
        }
        ws.onclose = () => {
            setWsState('disconnected')
            appendLog('WebSocket disconnected.', 'warn')
        }
        ws.onerror = () => {
            setWsState('disconnected')
            appendLog('WebSocket error.', 'bad')
        }
    }

    const sendBuzz = () => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) {
            appendLog('WebSocket not connected.', 'warn')
            return
        }
        if (roundLocked) {
            appendLog('Round is in progress.', 'warn')
            return
        }
        if (hasBuzzedThisRound) {
            appendLog('Already buzzed this round.', 'warn')
            return
        }
        wsRef.current.send(JSON.stringify({ type: 'buzz' }))
    }

    const startRound = () => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) {
            appendLog('WebSocket not connected.', 'warn')
            return
        }
        wsRef.current.send(JSON.stringify({ type: 'start_round' }))
    }

    const setAdmin = () => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) {
            appendLog('WebSocket not connected.', 'warn')
            return
        }
        if (!adminTarget.trim()) {
            appendLog('Admin target required.', 'warn')
            return
        }
        wsRef.current.send(JSON.stringify({ type: 'set_admin', name: adminTarget.trim() }))
        setAdminTarget('')
    }

    const kickPlayer = () => {
        if (wsRef.current?.readyState !== WebSocket.OPEN) {
            appendLog('WebSocket not connected.', 'warn')
            return
        }
        if (!kickTarget.trim()) {
            appendLog('Kick target required.', 'warn')
            return
        }
        wsRef.current.send(JSON.stringify({ type: 'kick', name: kickTarget.trim() }))
        setKickTarget('')
    }

    useEffect(() => {
        if (view !== 'room') return
        if (wsState === 'connected' || wsState === 'connecting') return
        void connectWs()
    }, [view, wsState])

    useEffect(() => {
        if (view !== 'room' || !token || !roomId) return
        const interval = setInterval(() => {
            void refreshTokenIfNeeded(token, roomId)
        }, REFRESH_CHECK_INTERVAL_MS)
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

    const buzzDisabled = wsState !== 'connected' || hasBuzzedThisRound || roundLocked
    const buzzStatus = hasBuzzedThisRound
        ? 'Buzzed this round.'
        : roundLocked
          ? 'Someone is answering.'
          : wsState === 'connected'
            ? 'Tap once per round or press space.'
            : 'Connect to buzz.'

    useEffect(() => {
        const onKeyDown = (event: KeyboardEvent) => {
            if (event.key !== ' ' && event.key !== 'Spacebar' && event.code !== 'Space') return
            if (view !== 'room') return
            if (wsState !== 'connected') return
            if (hasBuzzedThisRound) return
            if (isTypingTarget(event.target)) return
            event.preventDefault()
            sendBuzz()
        }

        window.addEventListener('keydown', onKeyDown)
        return () => window.removeEventListener('keydown', onKeyDown)
    }, [view, wsState, hasBuzzedThisRound, sendBuzz])

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
                                <button className="ghost" onClick={resetSession}>
                                    Leave room
                                </button>
                            </div>
                        </div>

                        <div className="buzzer-area">
                            <button
                                className={`buzzer-button ${hasBuzzedThisRound ? 'pressed' : ''}`}
                                onClick={sendBuzz}
                                disabled={buzzDisabled}
                            >
                                <span className="buzzer-label">Buzz</span>
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
                                            <span>{participant.name}</span>
                                            <span className={`badge ${participant.role}`}>{participant.role}</span>
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
                                    </div>
                                    <div className="controls admin-tools">
                                        <label>
                                            Set admin
                                            <input
                                                value={adminTarget}
                                                onChange={(event) => setAdminTarget(event.target.value)}
                                                placeholder="player name"
                                                disabled={wsState !== 'connected'}
                                            />
                                        </label>
                                        <button
                                            className="ghost"
                                            onClick={setAdmin}
                                            disabled={wsState !== 'connected'}
                                        >
                                            Assign admin
                                        </button>
                                    </div>
                                    <div className="controls admin-tools">
                                        <label>
                                            Kick player
                                            <input
                                                value={kickTarget}
                                                onChange={(event) => setKickTarget(event.target.value)}
                                                placeholder="player name"
                                                disabled={wsState !== 'connected'}
                                            />
                                        </label>
                                        <button
                                            className="ghost"
                                            onClick={kickPlayer}
                                            disabled={wsState !== 'connected'}
                                        >
                                            Kick
                                        </button>
                                    </div>
                                </div>
                            )}
                            <div className="room-card logs">
                                <h3>Event log</h3>
                                <div className="log-list">
                                    {logs.length === 0 && <p className="muted">No events yet.</p>}
                                    {logs.map((entry) => (
                                        <div key={entry.id} className={`log ${entry.tone ?? 'ok'}`}>
                                            <span>{entry.ts}</span>
                                            <span>{entry.text}</span>
                                        </div>
                                    ))}
                                </div>
                            </div>
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
